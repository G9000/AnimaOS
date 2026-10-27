//! Dedicated owner profiles. Disk metadata contains no credentials; the worker
//! seals bounded origin state using a per-profile key held in the OS keyring.
use super::{records::valid_id, BrowserServiceError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use utoipa::ToSchema;
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserProfileView {
    #[serde(default)]
    pub(crate) login_active: bool,
    pub(crate) id: String,
    pub(crate) agent_id: String,
    pub(crate) policy_id: String,
    pub(crate) origin: String,
    pub(crate) revision: u64,
    pub(crate) digest: String,
    pub(crate) ready: bool,
    pub(crate) revoked: bool,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateBrowserProfile {
    pub(crate) policy_id: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrowserProfileMutation {
    #[serde(deserialize_with = "super::records::positive")]
    pub(crate) expected_revision: u64,
}
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartBrowserLogin {
    pub(crate) expected_revision: u64,
    pub(crate) route_id: String,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserProfileEnvelope {
    pub(crate) profile: BrowserProfileView,
}
#[derive(Serialize, ToSchema)]
pub(crate) struct BrowserProfilesEnvelope {
    pub(crate) profiles: Vec<BrowserProfileView>,
}

trait ProfileKeys: Send + Sync {
    fn load(&self, id: &str) -> Result<String, BrowserServiceError>;
    fn put(&self, id: &str, key: &str) -> Result<(), BrowserServiceError>;
    fn delete(&self, id: &str) -> Result<(), BrowserServiceError>;
}
struct OsKeys;
impl OsKeys {
    fn entry(id: &str) -> Result<keyring::Entry, BrowserServiceError> {
        keyring::Entry::new(
            &crate::keychain::service_name("animaos.browser.profiles"),
            id,
        )
        .map_err(|_| BrowserServiceError::Unavailable)
    }
}
impl ProfileKeys for OsKeys {
    fn load(&self, id: &str) -> Result<String, BrowserServiceError> {
        Self::entry(id)?
            .get_password()
            .map_err(|_| BrowserServiceError::Unavailable)
    }
    fn put(&self, id: &str, key: &str) -> Result<(), BrowserServiceError> {
        Self::entry(id)?
            .set_password(key)
            .map_err(|_| BrowserServiceError::Unavailable)?;
        if self.load(id)? != key {
            return Err(BrowserServiceError::Unavailable);
        }
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), BrowserServiceError> {
        let entry = Self::entry(id)?;
        let _ = entry.delete_credential();
        match entry.get_password() {
            Err(keyring::Error::NoEntry) => Ok(()),
            _ => Err(BrowserServiceError::Unavailable),
        }
    }
}
#[derive(Clone)]
pub(super) struct ProfileStore {
    root: Option<PathBuf>,
    keys: Arc<dyn ProfileKeys>,
}
impl Default for ProfileStore {
    fn default() -> Self {
        Self {
            root: std::env::var_os("LOCALAPPDATA")
                .map(|p| PathBuf::from(p).join("AnimaOS").join("browser-profiles")),
            keys: Arc::new(OsKeys),
        }
    }
}
pub(crate) struct ProfileSession {
    pub(super) view: BrowserProfileView,
    key: Zeroizing<String>,
    sealed_path: PathBuf,
    lease: Mutex<Option<File>>,
    pub(super) owner_login: bool,
    pub(super) login_route_id: String,
}
impl ProfileSession {
    pub(super) fn bootstrap(&self) -> serde_json::Value {
        serde_json::json!({"id":self.view.id,"revision":self.view.revision,"digest":self.view.digest,"origin":self.view.origin,"sealedPath":self.sealed_path,"ownerLogin":self.owner_login,"loginRouteId":self.login_route_id})
    }
    pub(super) fn expose_key(&self) -> &str {
        self.key.as_str()
    }
    pub(super) fn release(&self) {
        self.lease.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}
impl ProfileStore {
    #[cfg(test)]
    pub(super) fn cleanup_test_key(&self, id: &str) {
        let _ = self.keys.delete(id);
    }
    #[cfg(test)]
    pub(super) fn with_system_keys_for_test(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            keys: Arc::new(OsKeys),
        }
    }
    #[cfg(test)]
    pub(super) fn for_test(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            keys: Arc::new(tests::Keys::default()),
        }
    }
    fn root(&self, workspace: &Path) -> Result<PathBuf, BrowserServiceError> {
        let root = self
            .root
            .as_ref()
            .filter(|p| p.is_absolute())
            .ok_or(BrowserServiceError::Unavailable)?;
        if root.starts_with(workspace) || workspace.starts_with(root) {
            return Err(BrowserServiceError::Invalid);
        }
        fs::create_dir_all(root).map_err(|_| BrowserServiceError::Unavailable)?;
        let canonical = fs::canonicalize(root).map_err(|_| BrowserServiceError::Unavailable)?;
        let workspace =
            fs::canonicalize(workspace).map_err(|_| BrowserServiceError::Unavailable)?;
        if canonical.starts_with(&workspace) || workspace.starts_with(&canonical) {
            return Err(BrowserServiceError::Invalid);
        }
        for ancestor in root.ancestors() {
            let meta =
                fs::symlink_metadata(ancestor).map_err(|_| BrowserServiceError::Unavailable)?;
            if linked(&meta) {
                return Err(BrowserServiceError::Invalid);
            }
        }
        Ok(root.clone())
    }
    pub(super) fn create(
        &self,
        workspace: &Path,
        agent: &str,
        policy: &str,
        origin: &str,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        let root = self.root(workspace)?;
        if fs::read_dir(&root)
            .map_err(|_| BrowserServiceError::Unavailable)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|x| x == "json"))
            .count()
            >= 64
        {
            return Err(BrowserServiceError::Busy);
        }
        let id = format!("profile-{}", uuid::Uuid::new_v4());
        let key = Zeroizing::new(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        if let Err(error) = self.keys.put(&id, &key) {
            let _ = self.keys.delete(&id);
            return Err(error);
        }
        let mut view = BrowserProfileView {
            login_active: false,
            id,
            agent_id: agent.into(),
            policy_id: policy.into(),
            origin: origin.into(),
            revision: 1,
            digest: String::new(),
            ready: false,
            revoked: false,
        };
        Self::digest(&mut view)?;
        if let Err(error) = self.save(&root, &view) {
            let _ = self.keys.delete(&view.id);
            return Err(error);
        }
        Ok(view)
    }
    fn digest(view: &mut BrowserProfileView) -> Result<(), BrowserServiceError> {
        view.digest = hex::encode(Sha256::digest(
            serde_json::to_vec(&(
                &view.id,
                &view.agent_id,
                &view.policy_id,
                &view.origin,
                view.revision,
            ))
            .map_err(|_| BrowserServiceError::Unavailable)?,
        ));
        Ok(())
    }
    fn save(&self, root: &Path, view: &BrowserProfileView) -> Result<(), BrowserServiceError> {
        let bytes = serde_json::to_vec(view).map_err(|_| BrowserServiceError::Unavailable)?;
        atomicwrites::AtomicFile::new(
            root.join(format!("{}.json", view.id)),
            atomicwrites::AllowOverwrite,
        )
        .write(|file| file.write_all(&bytes))
        .map_err(|_| BrowserServiceError::Unavailable)
    }
    pub(super) fn get(
        &self,
        workspace: &Path,
        agent: &str,
        id: &str,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        if !valid_id(id) {
            return Err(BrowserServiceError::Invalid);
        }
        let root = self.root(workspace)?;
        let path = root.join(format!("{id}.json"));
        let meta = fs::symlink_metadata(&path).map_err(|_| BrowserServiceError::NotFound)?;
        if !meta.is_file() || linked(&meta) || meta.len() > 8192 {
            return Err(BrowserServiceError::Invalid);
        }
        let view: BrowserProfileView =
            serde_json::from_slice(&fs::read(path).map_err(|_| BrowserServiceError::Unavailable)?)
                .map_err(|_| BrowserServiceError::Invalid)?;
        let mut expected = view.clone();
        Self::digest(&mut expected)?;
        if view.id != id || view.agent_id != agent || view.digest != expected.digest {
            return Err(BrowserServiceError::NotFound);
        }
        Ok(view)
    }
    pub(super) fn list(
        &self,
        workspace: &Path,
        agent: &str,
    ) -> Result<Vec<BrowserProfileView>, BrowserServiceError> {
        let root = self.root(workspace)?;
        let mut result = Vec::new();
        for entry in fs::read_dir(root)
            .map_err(|_| BrowserServiceError::Unavailable)?
            .take(256)
        {
            let path = entry.map_err(|_| BrowserServiceError::Unavailable)?.path();
            if path.extension().is_some_and(|x| x == "json") {
                if let Some(id) = path.file_stem().and_then(|x| x.to_str()) {
                    if let Ok(view) = self.get(workspace, agent, id) {
                        result.push(view)
                    }
                }
            }
        }
        Ok(result)
    }
    pub(super) fn begin_login(
        &self,
        workspace: &Path,
        view: &BrowserProfileView,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        let root = self.root(workspace)?;
        let current = self.get(workspace, &view.agent_id, &view.id)?;
        if current != *view || view.revoked {
            return Err(BrowserServiceError::Conflict);
        }
        let mut next = view.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .filter(|x| *x <= anima_service_adapters::browser::MAX_BROWSER_SAFE_INTEGER)
            .ok_or(BrowserServiceError::Conflict)?;
        next.ready = false;
        Self::digest(&mut next)?;
        let key = Zeroizing::new(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        self.save(&root, &next)?;
        self.keys.put(&view.id, &key)?;
        match fs::remove_file(root.join(format!("{}.sealed", view.id))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(BrowserServiceError::Unavailable),
        }
        Ok(next)
    }
    pub(super) fn acquire(
        &self,
        workspace: &Path,
        view: &BrowserProfileView,
        owner_login: bool,
        login_route_id: String,
    ) -> Result<Arc<ProfileSession>, BrowserServiceError> {
        let current = self.get(workspace, &view.agent_id, &view.id)?;
        if current != *view || view.revoked || (!owner_login && !view.ready) {
            return Err(BrowserServiceError::Conflict);
        }
        let root = self.root(workspace)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let lease = options
            .open(root.join(format!("{}.lease", view.id)))
            .map_err(|_| BrowserServiceError::Busy)?;
        let key = Zeroizing::new(self.keys.load(&view.id)?);
        if !super::records::digest_valid(&key) {
            return Err(BrowserServiceError::Unavailable);
        }
        Ok(Arc::new(ProfileSession {
            view: view.clone(),
            key,
            sealed_path: root.join(format!("{}.sealed", view.id)),
            lease: Mutex::new(Some(lease)),
            owner_login,
            login_route_id,
        }))
    }
    pub(super) fn finish(
        &self,
        workspace: &Path,
        view: &BrowserProfileView,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        let root = self.root(workspace)?;
        let current = self.get(workspace, &view.agent_id, &view.id)?;
        if current != *view || view.revoked {
            return Err(BrowserServiceError::Conflict);
        }
        let path = root.join(format!("{}.sealed", view.id));
        let meta = fs::symlink_metadata(path).map_err(|_| BrowserServiceError::Conflict)?;
        if linked(&meta) || !meta.is_file() || meta.len() == 0 || meta.len() > 65536 {
            return Err(BrowserServiceError::Conflict);
        }
        let mut next = view.clone();
        next.ready = true;
        self.save(&root, &next)?;
        Ok(next)
    }
    pub(super) fn purge_sealed(
        &self,
        workspace: &Path,
        id: &str,
    ) -> Result<(), BrowserServiceError> {
        if !valid_id(id) {
            return Err(BrowserServiceError::Invalid);
        }
        let root = self.root(workspace)?;
        match fs::remove_file(root.join(format!("{id}.sealed"))) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(BrowserServiceError::Unavailable),
        }
    }
    pub(super) fn revoke(
        &self,
        workspace: &Path,
        view: &BrowserProfileView,
    ) -> Result<BrowserProfileView, BrowserServiceError> {
        let root = self.root(workspace)?;
        let current = self.get(workspace, &view.agent_id, &view.id)?;
        if current != *view {
            return Err(BrowserServiceError::Conflict);
        }
        let mut next = view.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .filter(|x| *x <= anima_service_adapters::browser::MAX_BROWSER_SAFE_INTEGER)
            .ok_or(BrowserServiceError::Conflict)?;
        next.revoked = true;
        next.ready = false;
        Self::digest(&mut next)?;
        self.save(&root, &next)?;
        self.keys.delete(&view.id)?;
        match fs::remove_file(root.join(format!("{}.sealed", view.id))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(BrowserServiceError::Unavailable),
        }
        Ok(next)
    }
}
fn linked(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(Default)]
    pub(super) struct Keys(Mutex<BTreeMap<String, String>>);
    impl ProfileKeys for Keys {
        fn load(&self, id: &str) -> Result<String, BrowserServiceError> {
            self.0
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or(BrowserServiceError::Unavailable)
        }
        fn put(&self, id: &str, key: &str) -> Result<(), BrowserServiceError> {
            self.0.lock().unwrap().insert(id.into(), key.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), BrowserServiceError> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, ProfileStore) {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let store = ProfileStore {
            root: Some(directory.path().join("private-profiles")),
            keys: Arc::new(Keys::default()),
        };
        (directory, workspace, store)
    }
    #[test]
    fn owner_profile_custody_is_outside_workspace_and_disk_metadata_contains_no_key() {
        let (_root, workspace, store) = fixture();
        let view = store
            .create(
                &workspace,
                "owner",
                "authenticated-site",
                "https://owner.example",
            )
            .unwrap();
        let metadata = fs::read_to_string(
            store
                .root
                .as_ref()
                .unwrap()
                .join(format!("{}.json", view.id)),
        )
        .unwrap();
        assert!(!metadata.contains(&store.keys.load(&view.id).unwrap()));
        assert_eq!(
            store.get(&workspace, "foreign", &view.id),
            Err(BrowserServiceError::NotFound)
        );
        let invalid = ProfileStore {
            root: Some(workspace.join("profiles")),
            keys: Arc::new(Keys::default()),
        };
        assert_eq!(
            invalid.create(&workspace, "owner", "site", "https://owner.example"),
            Err(BrowserServiceError::Invalid)
        );
    }
    #[test]
    fn setup_must_finish_before_dispatch_and_restart_recovers_owner_profile() {
        let (_root, workspace, store) = fixture();
        let created = store
            .create(&workspace, "owner", "site", "https://owner.example")
            .unwrap();
        assert!(store
            .acquire(&workspace, &created, false, "login".into())
            .is_err());
        let setup = store.begin_login(&workspace, &created).unwrap();
        assert!(setup.revision > created.revision);
        assert_ne!(setup.digest, created.digest);
        assert!(store.finish(&workspace, &setup).is_err());
        fs::write(
            store
                .root
                .as_ref()
                .unwrap()
                .join(format!("{}.sealed", setup.id)),
            br#"{"version":1,"data":"encrypted"}"#,
        )
        .unwrap();
        let finished = store.finish(&workspace, &setup).unwrap();
        assert!(finished.ready);
        let fresh = ProfileStore {
            root: store.root.clone(),
            keys: store.keys.clone(),
        };
        assert_eq!(
            fresh.get(&workspace, "owner", &finished.id).unwrap(),
            finished
        );
        assert!(fresh
            .acquire(&workspace, &finished, false, "login".into())
            .is_ok());
    }
    #[test]
    fn revocation_changes_dispatch_witness_deletes_custody_and_refuses_reuse() {
        let (_root, workspace, store) = fixture();
        let view = store
            .create(&workspace, "owner", "site", "https://owner.example")
            .unwrap();
        let before = store.begin_login(&workspace, &view).unwrap();
        let revoked = store.revoke(&workspace, &before).unwrap();
        assert!(revoked.revoked);
        assert!(!revoked.ready);
        assert_ne!(revoked.digest, before.digest);
        assert!(store
            .acquire(&workspace, &revoked, true, "login".into())
            .is_err());
        assert!(store.keys.load(&view.id).is_err());
        assert!(store.begin_login(&workspace, &before).is_err());
    }
    #[test]
    #[cfg(windows)]
    fn physical_profile_lease_is_exclusive_until_cleanup_releases_it() {
        let (_root, workspace, store) = fixture();
        let view = store
            .create(&workspace, "owner", "site", "http://127.0.0.1:12345")
            .unwrap();
        let lease = store
            .acquire(&workspace, &view, true, "login".into())
            .unwrap();
        assert!(store
            .acquire(&workspace, &view, true, "login".into())
            .is_err());
        lease.release();
        assert!(store
            .acquire(&workspace, &view, true, "login".into())
            .is_ok());
    }
}
