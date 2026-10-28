//! Lifetime authority for a production profile's durable resources.
//!
//! Lock sidecars are stable objects: never unlink them and never infer authority
//! from a PID, port, timestamp, or a connection that can reconnect itself.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use cap_fs_ext::MetadataExt;
use sqlx::{Connection, PgConnection, PgPool};
use tokio::sync::{Mutex, OwnedMutexGuard};

mod registry;

// Two positive int4 keys use PostgreSQL's database-scoped advisory namespace.
const POSTGRES_LOCK_CLASS: i32 = 0x414e494d;
const POSTGRES_LOCK_PROFILE: i32 = 0x50524f46;
const POSTGRES_AUTHORITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Clone, Debug)]
pub(crate) enum ProfileResource {
    File(PathBuf),
    Sqlite(PathBuf),
    Directory(PathBuf),
}
impl ProfileResource {
    fn path(&self) -> &Path {
        match self {
            Self::File(path) | Self::Sqlite(path) | Self::Directory(path) => path,
        }
    }
    fn is_directory(&self) -> bool {
        matches!(self, Self::Directory(_))
    }
    fn is_sqlite(&self) -> bool {
        matches!(self, Self::Sqlite(_))
    }
    fn kind(&self) -> u8 {
        match self {
            Self::File(_) => 0,
            Self::Sqlite(_) => 1,
            Self::Directory(_) => 2,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ProfileOwner(Arc<OwnerInner>);

struct OwnerInner {
    files: Vec<FileLease>,
    payloads: std::sync::Mutex<Vec<PinnedPayload>>,
    lost: AtomicBool,
    lost_signal: anima_core::CancelSignal,
    postgres: OnceLock<Arc<Mutex<PgConnection>>>,
    registry: Option<registry::Claim>,
}

struct FileLease {
    resource: PathBuf,
    sidecar: PathBuf,
    file: File,
    identity: (u64, u64),
    directory: bool,
    sqlite: bool,
}

struct PinnedPayload {
    resource: PathBuf,
    directory: bool,
    // Close the real object before releasing its self-keyed OS identity lease.
    pin: File,
    lease: Arc<registry::PayloadLease>,
    identity: (u64, u64),
}

impl std::fmt::Debug for ProfileOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileOwner")
            .field("resources", &self.0.files.len())
            .field("lost", &self.0.lost.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct OwnershipLost;
impl std::fmt::Display for OwnershipLost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "profile ownership is unavailable or uncertain; restart after the active owner exits",
        )
    }
}
impl std::error::Error for OwnershipLost {}

pub(crate) fn ownership_lost_error() -> io::Error {
    io::Error::other(OwnershipLost)
}

impl ProfileOwner {
    /// Canonical profile payloads excluded from read-only project knowledge grants.
    pub(crate) fn protected_paths(&self) -> Vec<PathBuf> {
        self.0
            .files
            .iter()
            .map(|lease| lease.resource.clone())
            .collect()
    }

    pub(crate) fn acquire(resources: impl IntoIterator<Item = PathBuf>) -> io::Result<Self> {
        Self::acquire_resources(resources.into_iter().map(ProfileResource::File))
    }

    pub(crate) fn resolve_path(path: &Path) -> io::Result<PathBuf> {
        if reserved_payload_name(path) {
            return Err(ownership_lost_error());
        }
        canonical_resource(path)
    }

    pub(crate) fn acquire_resources(
        resources: impl IntoIterator<Item = ProfileResource>,
    ) -> io::Result<Self> {
        let mut resources = resources
            .into_iter()
            .map(|resource| {
                if reserved_payload_name(resource.path()) {
                    return Err(ownership_lost_error());
                }
                let path = canonical_resource(resource.path())?;
                if reserved_payload_name(&path) {
                    return Err(ownership_lost_error());
                }
                check_typed_resource(&path, resource.is_directory())?;
                Ok(match resource {
                    ProfileResource::File(_) => ProfileResource::File(path),
                    ProfileResource::Sqlite(_) => ProfileResource::Sqlite(path),
                    ProfileResource::Directory(_) => ProfileResource::Directory(path),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        resources.sort_by(|a, b| a.path().cmp(b.path()));
        for adjacent in resources.windows(2) {
            if adjacent[0].path() == adjacent[1].path() && adjacent[0].kind() != adjacent[1].kind()
            {
                return Err(ownership_lost_error());
            }
        }
        resources.dedup_by(|a, b| a.path() == b.path());
        // The registry excludes every overlap BEFORE creating any configured
        // resource directory or its exact-path sidecar.
        let (registry, (files, payloads)) =
            registry::Claim::acquire(&resources, |root| prepare_file_leases(&resources, root))?;
        let owner = Self(Arc::new(OwnerInner {
            files,
            payloads: std::sync::Mutex::new(payloads),
            lost: AtomicBool::new(false),
            lost_signal: anima_core::CancelSignal::new(),
            postgres: OnceLock::new(),
            registry,
        }));
        owner.ensure_owned()?;
        Ok(owner)
    }

    /// Checks local authority without blocking an async runtime on PostgreSQL.
    /// Effect admission must additionally use verify_owned().
    pub(crate) fn ensure_owned(&self) -> io::Result<()> {
        if self.0.lost.load(Ordering::Acquire) {
            return Err(ownership_lost_error());
        }
        if self
            .0
            .registry
            .as_ref()
            .is_some_and(|claim| claim.ensure_owned().is_err())
        {
            self.mark_lost();
            return Err(ownership_lost_error());
        }
        let payloads = self.0.payloads.lock().map_err(|_| {
            self.mark_lost();
            ownership_lost_error()
        })?;
        for payload in payloads.iter() {
            if payload.ensure_owned().is_err() {
                self.mark_lost();
                return Err(ownership_lost_error());
            }
        }
        for lease in &self.0.files {
            let valid = (|| -> io::Result<bool> {
                let retained = file_metadata(&lease.file)?;
                let named = metadata_only(&lease.sidecar)?;
                for candidate in lease.resource.ancestors() {
                    match metadata_only(candidate) {
                        Ok(payload)
                            if !payloads
                                .iter()
                                .any(|pin| pin.identity == (payload.dev(), payload.ino())) =>
                        {
                            if let Some(claim) = &self.0.registry {
                                claim.probe_payload(&payload)?;
                            }
                        }
                        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                        _ => {}
                    }
                }
                Ok((retained.dev(), retained.ino()) == lease.identity
                    && (named.dev(), named.ino()) == lease.identity
                    && retained.nlink() == 1
                    && named.nlink() == 1
                    && canonical_resource(&lease.resource)? == lease.resource
                    && check_typed_resource(&lease.resource, lease.directory).is_ok())
            })();
            if !matches!(valid, Ok(true)) {
                self.mark_lost();
                return Err(ownership_lost_error());
            }
        }
        Ok(())
    }

    pub(crate) fn owned_sqlite_path(&self, path: &Path) -> io::Result<PathBuf> {
        let canonical = self.canonical_path(path)?;
        if self
            .0
            .files
            .iter()
            .any(|lease| lease.resource == canonical && lease.sqlite)
        {
            return Ok(canonical);
        }
        self.mark_lost();
        Err(ownership_lost_error())
    }

    /// Only directory initialization belongs in this callback. The gate is
    /// released before actual store jobs; existing ACL provisioning stays here.
    pub(crate) fn with_directory<T, E>(
        &self,
        path: &Path,
        initialize: impl FnOnce() -> Result<T, E>,
    ) -> io::Result<Result<T, E>> {
        let canonical = self.canonical_path(path)?;
        if !self
            .0
            .files
            .iter()
            .any(|lease| lease.resource == canonical && lease.directory)
        {
            self.mark_lost();
            return Err(ownership_lost_error());
        }
        let claim = self.0.registry.as_ref().ok_or_else(ownership_lost_error)?;
        let mut outcome = None;
        let result = claim.with_gate(|root| {
            let mut payloads = self.0.payloads.lock().map_err(|_| ownership_lost_error())?;
            probe_resource_identities(&canonical, root, &payloads)?;
            if !payloads.iter().any(|pin| pin.resource == canonical)
                && metadata_only(&canonical).is_ok()
            {
                let pin = PinnedPayload::open(&canonical, true, false, root, &payloads)?;
                payloads.push(pin);
            }
            outcome = Some(initialize());
            if outcome.as_ref().is_some_and(Result::is_ok)
                && !payloads.iter().any(|pin| pin.resource == canonical)
            {
                let pin = PinnedPayload::open(&canonical, true, false, root, &payloads)?;
                payloads.push(pin);
            }
            Ok(())
        });
        if result.is_err() {
            self.mark_lost();
        }
        result?;
        self.ensure_owned()?;
        Ok(outcome.expect("directory initialization ran under the gate"))
    }

    pub(crate) fn mark_lost(&self) {
        self.0.lost.store(true, Ordering::Release);
        self.0.lost_signal.cancel();
    }

    pub(crate) fn lost_signal(&self) -> anima_core::CancelSignal {
        self.0.lost_signal.clone()
    }

    pub(crate) fn canonical_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.ensure_owned()?;
        let canonical = canonical_resource(path)?;
        for lease in &self.0.files {
            if lease.resource == canonical {
                return Ok(canonical);
            }
            if let Ok(metadata) = metadata_only(&lock_path(&canonical)) {
                if (metadata.dev(), metadata.ino()) == lease.identity {
                    return Ok(lease.resource.clone());
                }
            }
        }
        self.mark_lost();
        Err(ownership_lost_error())
    }

    /// Separate from the pool so max_connections=1 cannot deadlock. This raw
    /// session is never returned to a pool or transparently reconnected.
    pub(crate) async fn attach_postgres(&self, pool: &PgPool) -> io::Result<()> {
        self.ensure_owned()?;
        if self.0.postgres.get().is_some() {
            return Err(ownership_lost_error());
        }
        let mut attempt = OwnershipAttempt {
            owner: self.clone(),
            complete: false,
        };
        let options = pool.connect_options();
        let mut connection = tokio::time::timeout(
            POSTGRES_AUTHORITY_TIMEOUT,
            PgConnection::connect_with(options.as_ref()),
        )
        .await
        .map_err(|_| ownership_lost_error())?
        .map_err(|_| ownership_lost_error())?;
        let acquired = tokio::time::timeout(
            POSTGRES_AUTHORITY_TIMEOUT,
            sqlx::query_scalar::<_, bool>("SELECT pg_catalog.pg_try_advisory_lock($1, $2)")
                .bind(POSTGRES_LOCK_CLASS)
                .bind(POSTGRES_LOCK_PROFILE)
                .fetch_one(&mut connection),
        )
        .await
        .map_err(|_| ownership_lost_error())?
        .map_err(|_| ownership_lost_error())?;
        if !acquired {
            return Err(ownership_lost_error());
        }
        self.0
            .postgres
            .set(Arc::new(Mutex::new(connection)))
            .map_err(|_| ownership_lost_error())?;
        self.verify_owned().await?;
        attempt.complete = true;
        self.start_postgres_health_monitor();
        Ok(())
    }

    fn start_postgres_health_monitor(&self) {
        let weak = Arc::downgrade(&self.0);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let owner = ProfileOwner(inner);
                // Retain this verification to completion. A cancelled partial
                // SQL query must never be reused as proof of authority.
                if owner.verify_owned().await.is_err() {
                    return;
                }
                drop(owner);
            }
        });
    }

    pub(crate) async fn verify_owned(&self) -> io::Result<()> {
        self.ensure_owned()?;
        if self.0.postgres.get().is_some() {
            let mut writer = self.postgres_writer().await?;
            writer.complete()?;
        }
        Ok(())
    }

    pub(crate) async fn postgres_writer(&self) -> io::Result<OwnedPostgresWriter> {
        self.ensure_owned()?;
        let connection = self.0.postgres.get().ok_or_else(ownership_lost_error)?;
        let guard = match tokio::time::timeout(
            POSTGRES_AUTHORITY_TIMEOUT,
            Arc::clone(connection).lock_owned(),
        )
        .await
        {
            Ok(guard) => guard,
            Err(_) => {
                self.mark_lost();
                return Err(ownership_lost_error());
            }
        };
        // Install the revoking guard BEFORE the first awaited SQL operation.
        // Cancellation or timeout during lock proof permanently loses authority.
        let mut writer = OwnedPostgresWriter {
            connection: guard,
            owner: self.clone(),
            completed: false,
        };
        self.ensure_owned()?;
        let held = tokio::time::timeout(POSTGRES_AUTHORITY_TIMEOUT,
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_locks WHERE locktype = 'advisory' \
                 AND pid = pg_catalog.pg_backend_pid() AND classid::bigint = $1 AND objid::bigint = $2 \
                 AND objsubid = 2 AND granted)",
            )
            .bind(i64::from(POSTGRES_LOCK_CLASS))
            .bind(i64::from(POSTGRES_LOCK_PROFILE))
            .fetch_one(&mut *writer),
        ).await;
        if !matches!(held, Ok(Ok(true))) {
            return Err(ownership_lost_error());
        }
        Ok(writer)
    }
}

fn prepare_file_leases(
    resources: &[ProfileResource],
    registry_root: &Path,
) -> io::Result<(Vec<FileLease>, Vec<PinnedPayload>)> {
    // Probe all existing identities before any configured parent is created.
    for resource in resources {
        probe_resource_identities(resource.path(), registry_root, &[])?;
    }
    let mut files = Vec::new();
    let mut names = HashSet::new();
    for resource in resources {
        let path = resource.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        check_typed_resource(path, resource.is_directory())?;
        let sidecar = lock_path(path);
        if std::fs::symlink_metadata(&sidecar)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(ownership_lost_error());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .share_mode(0x00000001 | 0x00000002)
                .custom_flags(0x00200000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            );
        }
        let file = options.open(&sidecar)?;
        let metadata = file_metadata(&file)?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(ownership_lost_error());
        }
        if !names.insert(std::fs::canonicalize(&sidecar)?) {
            continue;
        }
        file.try_lock().map_err(|_| ownership_lost_error())?;
        files.push(FileLease {
            resource: path.to_path_buf(),
            sidecar,
            file,
            identity: (metadata.dev(), metadata.ino()),
            directory: resource.is_directory(),
            sqlite: resource.is_sqlite(),
        });
    }
    let mut payloads = Vec::new();
    for lease in &files {
        if lease.sqlite || (lease.directory && metadata_only(&lease.resource).is_ok()) {
            payloads.push(PinnedPayload::open(
                &lease.resource,
                lease.directory,
                lease.sqlite,
                registry_root,
                &payloads,
            )?);
        }
    }
    Ok((files, payloads))
}

fn probe_resource_identities(path: &Path, root: &Path, owned: &[PinnedPayload]) -> io::Result<()> {
    for candidate in path.ancestors() {
        match metadata_only(candidate) {
            Ok(metadata)
                if !owned
                    .iter()
                    .any(|pin| pin.identity == (metadata.dev(), metadata.ino())) =>
            {
                registry::probe_payload(root, &metadata)?
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

impl PinnedPayload {
    fn open(
        resource: &Path,
        directory: bool,
        create_sqlite: bool,
        root: &Path,
        owned: &[Self],
    ) -> io::Result<Self> {
        check_typed_resource(resource, directory)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .access_mode(0)
                .share_mode(0x00000001 | 0x00000002)
                .custom_flags(0x02000000 | 0x00200000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            );
        }
        let pin = match options.open(resource) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound && create_sqlite => {
                let mut create = OpenOptions::new();
                create.read(true).write(true).create_new(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    create
                        .share_mode(0x00000001 | 0x00000002)
                        .custom_flags(0x00200000);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    create.mode(0o600).custom_flags(
                        (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
                    );
                }
                create.open(resource)?
            }
            Err(error) => return Err(error),
        };
        let metadata = file_metadata(&pin)?;
        if metadata.is_dir() != directory
            || (!directory && (!metadata.is_file() || metadata.nlink() != 1))
        {
            return Err(ownership_lost_error());
        }
        let identity = (metadata.dev(), metadata.ino());
        let lease = match owned.iter().find(|pin| pin.identity == identity) {
            Some(pin) => {
                pin.ensure_owned()?;
                Arc::clone(&pin.lease)
            }
            None => Arc::new(registry::PayloadLease::acquire(root, &metadata)?),
        };
        let pinned = Self {
            resource: resource.to_path_buf(),
            directory,
            pin,
            lease,
            identity,
        };
        pinned.ensure_owned()?;
        Ok(pinned)
    }
    fn ensure_owned(&self) -> io::Result<()> {
        self.lease.ensure_owned()?;
        let held = file_metadata(&self.pin)?;
        let named = metadata_only(&self.resource)?;
        if (held.dev(), held.ino()) != self.identity
            || (named.dev(), named.ino()) != self.identity
            || held.is_dir() != self.directory
            || named.is_dir() != self.directory
            || (!self.directory
                && (!held.is_file() || !named.is_file() || held.nlink() != 1 || named.nlink() != 1))
        {
            return Err(ownership_lost_error());
        }
        Ok(())
    }
}

struct OwnershipAttempt {
    owner: ProfileOwner,
    complete: bool,
}
impl Drop for OwnershipAttempt {
    fn drop(&mut self) {
        if !self.complete {
            self.owner.mark_lost();
        }
    }
}

/// Any cancelled/failed mutation permanently invalidates admission. Successful
/// callers explicitly complete after their transaction/statement is acknowledged.
pub(crate) struct OwnedPostgresWriter {
    connection: OwnedMutexGuard<PgConnection>,
    owner: ProfileOwner,
    completed: bool,
}
impl OwnedPostgresWriter {
    pub(crate) fn complete(&mut self) -> io::Result<()> {
        self.owner.ensure_owned()?;
        self.completed = true;
        Ok(())
    }
}
impl Deref for OwnedPostgresWriter {
    type Target = PgConnection;
    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}
impl DerefMut for OwnedPostgresWriter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}
impl Drop for OwnedPostgresWriter {
    fn drop(&mut self) {
        if !self.completed {
            self.owner.mark_lost();
        }
    }
}

struct MetadataFacts {
    device: u64,
    inode: u64,
    links: u64,
    regular: bool,
    directory: bool,
}
impl MetadataFacts {
    fn dev(&self) -> u64 {
        self.device
    }
    fn ino(&self) -> u64 {
        self.inode
    }
    fn nlink(&self) -> u64 {
        self.links
    }
    fn is_file(&self) -> bool {
        self.regular
    }
    fn is_dir(&self) -> bool {
        self.directory
    }
}

#[cfg(unix)]
fn unix_metadata_facts(metadata: std::fs::Metadata) -> MetadataFacts {
    MetadataFacts {
        device: metadata.dev(),
        inode: metadata.ino(),
        links: metadata.nlink(),
        regular: metadata.is_file(),
        directory: metadata.is_dir(),
    }
}

fn file_metadata(file: &File) -> io::Result<MetadataFacts> {
    #[cfg(unix)]
    {
        Ok(unix_metadata_facts(file.metadata()?))
    }
    #[cfg(not(unix))]
    {
        let metadata = cap_std::fs::File::from_std(file.try_clone()?).metadata()?;
        Ok(MetadataFacts {
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
            regular: metadata.is_file(),
            directory: metadata.is_dir(),
        })
    }
}

fn metadata_only(path: &Path) -> io::Result<MetadataFacts> {
    // Unix stat/lstat never opens a FIFO/device, so checking a special file or
    // a raced replacement cannot block waiting for another endpoint.
    #[cfg(unix)]
    {
        Ok(unix_metadata_facts(std::fs::symlink_metadata(path)?))
    }
    #[cfg(not(unix))]
    {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.access_mode(0).custom_flags(0x02000000 | 0x00200000);
        }
        file_metadata(&options.open(path)?)
    }
}

fn check_typed_resource(resource: &Path, directory: bool) -> io::Result<()> {
    match metadata_only(resource) {
        Ok(metadata) if directory && metadata.is_dir() => Ok(()),
        Ok(metadata) if !directory && metadata.is_file() && metadata.nlink() == 1 => Ok(()),
        Ok(_) => Err(ownership_lost_error()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Resolve existing aliases and lexical components without creating payload
/// directories. The private registry claim must precede those mutations.
fn canonical_resource(path: &Path) -> io::Result<PathBuf> {
    let mut current = std::path::absolute(path)?;
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&current) {
            Ok(mut resolved) => {
                for component in missing.into_iter().rev() {
                    match component {
                        MissingComponent::Name(name) => resolved.push(name),
                        MissingComponent::Parent => {
                            if !resolved.pop() {
                                return Err(ownership_lost_error());
                            }
                        }
                    }
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let component = current
                    .components()
                    .next_back()
                    .ok_or_else(ownership_lost_error)?;
                missing.push(match component {
                    std::path::Component::Normal(name) => {
                        MissingComponent::Name(name.to_os_string())
                    }
                    std::path::Component::ParentDir => MissingComponent::Parent,
                    _ => return Err(ownership_lost_error()),
                });
                if !current.pop() {
                    return Err(ownership_lost_error());
                }
            }
            Err(error) => return Err(error),
        }
    }
}
enum MissingComponent {
    Name(std::ffi::OsString),
    Parent,
}

#[cfg(test)]
pub(crate) fn test_registry_path() -> io::Result<PathBuf> {
    registry::test_registry_path()
}

fn reserved_payload_name(resource: &Path) -> bool {
    if resource.file_name().is_none() {
        return true;
    }
    resource.components().any(|component| match component {
        std::path::Component::Normal(name) => reserved_payload_component(name),
        #[cfg(windows)]
        std::path::Component::Prefix(prefix) => matches!(
            prefix.kind(),
            std::path::Prefix::DeviceNS(_) | std::path::Prefix::Verbatim(_)
        ),
        _ => false,
    })
}

fn reserved_payload_component(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    #[cfg(windows)]
    let name = name.to_ascii_lowercase();
    #[cfg(windows)]
    {
        // Win32 and extended paths must agree on locator meaning. A missing
        // component ending in dot/space otherwise has separately named leases.
        if name.ends_with([' ', '.'])
            || name
                .chars()
                .any(|character| character <= '\u{1f}' || "<>:\"|?*".contains(character))
        {
            return true;
        }
        let base = name.split('.').next().unwrap_or("").trim_end_matches(' ');
        if matches!(
            base,
            "con" | "prn" | "aux" | "nul" | "conin$" | "conout$" | "clock$"
        ) || ["com", "lpt"].iter().any(|prefix| {
            base.strip_prefix(*prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }) {
            return true;
        }
    }
    name.ends_with(".owner.lock")
        || name.ends_with("-wal")
        || name.ends_with("-shm")
        || name.ends_with("-journal")
        || (name.ends_with(".bak") && name.rsplit_once(".pre-").is_some())
}

fn lock_path(resource: &Path) -> PathBuf {
    let mut path = resource.as_os_str().to_os_string();
    path.push(".owner.lock");
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    struct OwnerChild(Child);

    impl Drop for OwnerChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn child_owner(resource: &std::path::Path, ready: &std::path::Path) -> OwnerChild {
        child_owner_typed(resource, ready, false)
    }

    fn child_owner_typed(resource: &Path, ready: &Path, directory: bool) -> OwnerChild {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "profile_owner::tests::ownership_child",
                "--nocapture",
            ])
            .env("ANIMA_TEST_OWNERSHIP_RESOURCE", resource)
            .env(
                "ANIMA_TEST_PROFILE_OWNER_REGISTRY_DIR",
                test_registry_path().unwrap(),
            )
            .env("ANIMA_TEST_OWNERSHIP_READY", ready)
            .env_remove("ANIMA_TEST_OWNERSHIP_DIRECTORY")
            .env_remove("ANIMA_TEST_OWNERSHIP_REFUSED")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if directory {
            command.env("ANIMA_TEST_OWNERSHIP_DIRECTORY", "1");
        }
        let mut child = OwnerChild(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "ownership child exited before acquiring its resource"
            );
            assert!(
                Instant::now() < deadline,
                "ownership child did not acquire its resource"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        child
    }

    #[test]
    fn ownership_child() {
        if let Some(output) = std::env::var_os("ANIMA_TEST_OWNERSHIP_REPORT_ROOT") {
            let location = registry::production_registry_path()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "unavailable".into());
            std::fs::write(output, location).unwrap();
            return;
        }
        let Some(resource) = std::env::var_os("ANIMA_TEST_OWNERSHIP_RESOURCE") else {
            return;
        };
        let resource = PathBuf::from(resource);
        let owner = if std::env::var_os("ANIMA_TEST_OWNERSHIP_DIRECTORY").is_some() {
            ProfileOwner::acquire_resources([ProfileResource::Directory(resource)])
        } else {
            ProfileOwner::acquire([resource])
        };
        if std::env::var_os("ANIMA_TEST_OWNERSHIP_REFUSED").is_some() {
            assert!(
                owner.is_err(),
                "reserved lock sidecar was admitted as another profile payload"
            );
            return;
        }
        let _owner = owner.unwrap();
        std::fs::write(
            std::env::var_os("ANIMA_TEST_OWNERSHIP_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn profile_owner_second_process_is_refused_before_payload_is_created() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("control-plane.json");
        let _child = child_owner(&resource, &temp.path().join("ready"));
        let contender = ProfileOwner::acquire([resource.clone()]);
        assert!(
            contender.is_err(),
            "a second process obtained the same production profile"
        );
        assert!(
            !resource.exists(),
            "ownership discovery must not create the payload file"
        );
    }

    #[test]
    fn profile_owner_separate_profiles_can_start_concurrently() {
        let temp = tempfile::tempdir().unwrap();
        let _first = ProfileOwner::acquire([temp.path().join("one/state.json")]).unwrap();
        let _second = ProfileOwner::acquire([temp.path().join("two/state.json")]).unwrap();
    }

    #[test]
    fn profile_owner_overlapping_history_or_embedding_resources_refuse_contender() {
        for shared in ["history.sqlite", "memory.sqlite", "embeddings.sqlite"] {
            let temp = tempfile::tempdir().unwrap();
            let resource = temp.path().join(shared);
            let _first =
                ProfileOwner::acquire([temp.path().join("one.json"), resource.clone()]).unwrap();
            assert!(
                ProfileOwner::acquire([temp.path().join("two.json"), resource]).is_err(),
                "different control planes shared {shared}"
            );
        }
    }

    #[test]
    fn profile_owner_aliases_are_deduplicated_and_still_exclude_contenders() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("nested")).unwrap();
        let resource = temp.path().join("memory.sqlite");
        let alias = temp.path().join("nested/../memory.sqlite");
        let _first = ProfileOwner::acquire([resource.clone(), alias.clone()]).unwrap();
        assert!(
            ProfileOwner::acquire([alias]).is_err(),
            "a lexical path alias bypassed ownership"
        );
    }

    #[test]
    fn profile_owner_existing_hardlinked_resource_is_identified_or_refused() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("memory.sqlite");
        let alias = temp.path().join("different-name.sqlite");
        std::fs::write(&resource, b"payload remains untouched").unwrap();
        std::fs::hard_link(&resource, &alias).unwrap();
        if let Ok(_first) = ProfileOwner::acquire([resource.clone()]) {
            assert!(
                ProfileOwner::acquire([alias]).is_err(),
                "hardlinked resources obtained independent owners"
            );
        }
        assert_eq!(
            std::fs::read(&resource).unwrap(),
            b"payload remains untouched"
        );
    }

    #[test]
    fn profile_owner_clones_retain_authority_until_last_detached_job_finishes() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("memory.sqlite");
        let owner = ProfileOwner::acquire([resource.clone()]).unwrap();
        let detached = owner.clone();
        drop(owner);
        assert!(
            ProfileOwner::acquire([resource.clone()]).is_err(),
            "dropping State released a detached save's lease"
        );
        detached.ensure_owned().unwrap();
        drop(detached);
        ProfileOwner::acquire([resource]).unwrap();
    }

    #[test]
    fn profile_owner_crashed_process_releases_for_takeover_without_removing_sidecars() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("control-plane.json");
        let child = child_owner(&resource, &temp.path().join("ready"));
        assert!(ProfileOwner::acquire([resource.clone()]).is_err());
        drop(child);
        let _successor = ProfileOwner::acquire([resource]).unwrap();
    }

    #[test]
    fn profile_owner_uncertainty_permanently_revokes_every_clone() {
        let temp = tempfile::tempdir().unwrap();
        let owner = ProfileOwner::acquire([temp.path().join("memory.sqlite")]).unwrap();
        let detached = owner.clone();
        owner.mark_lost();
        assert!(
            owner.ensure_owned().is_err(),
            "uncertain authority remained writable"
        );
        assert!(
            detached.ensure_owned().is_err(),
            "a stale detached clone revived lost authority"
        );
    }

    #[test]
    fn profile_owner_sidecar_identity_survives_owner_drop_and_takeover() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("state.json");
        let owner = ProfileOwner::acquire([resource.clone()]).unwrap();
        let sidecar = lock_path(&resource);
        assert!(sidecar.exists(), "ownership needs a stable lock sidecar");
        drop(owner);
        assert!(
            sidecar.exists(),
            "a lock sidecar must never be unlinked on release"
        );
        let _successor = ProfileOwner::acquire([resource]).unwrap();
        assert!(sidecar.exists());
    }

    #[cfg(windows)]
    #[test]
    fn profile_owner_windows_sidecar_cannot_be_deleted_while_locked() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("state.json");
        let _owner = ProfileOwner::acquire([resource.clone()]).unwrap();
        let sidecar = lock_path(&resource);
        assert!(sidecar.exists());
        assert!(
            std::fs::remove_file(sidecar).is_err(),
            "Windows allowed deletion of the retained lock handle"
        );
    }

    #[cfg(unix)]
    #[test]
    fn profile_owner_unlinked_sidecar_permanently_loses_mutation_authority() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("state.json");
        let owner = ProfileOwner::acquire([resource.clone()]).unwrap();
        let sidecar = lock_path(&resource);
        let moved = temp.path().join("retained-sidecar");
        std::fs::rename(&sidecar, &moved).unwrap();
        std::fs::write(&sidecar, b"").unwrap();
        assert!(
            owner.ensure_owned().is_err(),
            "the old owner wrote under a replaced Unix lock identity"
        );
        std::fs::rename(moved, sidecar).unwrap();
        assert!(
            owner.ensure_owned().is_err(),
            "lost ownership must stay lost"
        );
        assert!(
            ProfileOwner::acquire([resource.clone()]).is_err(),
            "the retained registry lease allowed takeover before the old owner retired"
        );
        drop(owner);
        ProfileOwner::acquire([resource]).unwrap();
    }

    #[test]
    fn profile_owner_payload_cannot_use_another_resources_reserved_sidecar() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("state.json");
        let reserved = lock_path(&resource);
        assert!(ProfileOwner::acquire([resource.clone(), reserved.clone()]).is_err());
        assert!(!resource.exists());
        assert!(
            !reserved.exists(),
            "discovery created a configured payload as a lock file"
        );
    }

    #[tokio::test]
    async fn profile_owner_new_hardlink_revokes_actual_snapshot_writes_permanently() {
        use crate::control_plane_store::{
            save_control_plane_snapshot, ControlPlaneSnapshot, ControlPlaneStoreConfig,
        };
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("state.json");
        let owner = ProfileOwner::acquire([resource.clone()]).unwrap();
        let config = ControlPlaneStoreConfig::Json(resource.clone())
            .with_owner(owner.clone())
            .unwrap();
        let snapshot = ControlPlaneSnapshot::default();
        save_control_plane_snapshot(Some(&config), &snapshot)
            .await
            .unwrap();
        let original = std::fs::read(&resource).unwrap();
        let alias = temp.path().join("unexpected-hardlink.json");
        std::fs::hard_link(&resource, &alias).unwrap();
        assert!(save_control_plane_snapshot(Some(&config), &snapshot)
            .await
            .is_err());
        std::fs::remove_file(alias).unwrap();
        assert!(
            save_control_plane_snapshot(Some(&config), &snapshot)
                .await
                .is_err(),
            "removing the suspicious alias revived authority"
        );
        assert_eq!(std::fs::read(&resource).unwrap(), original);
        assert!(owner.lost_signal().is_cancelled());
    }

    /// This test only accepts an explicitly provided disposable test database.
    /// It never falls back to DATABASE_URL or loads the operator's .env file.
    #[tokio::test]
    #[ignore = "requires ANIMA_TEST_PROFILE_OWNER_POSTGRES_URL pointing to an isolated disposable database"]
    async fn profile_owner_postgres_terminated_session_cannot_write_after_takeover() {
        use crate::control_plane_store::{
            save_control_plane_snapshot, write_pre_upgrade_backup, ControlPlaneSnapshot,
            ControlPlaneStoreConfig, WorkspaceConfig,
        };
        use crate::history::{HistoryStore, PostgresHistoryStore};
        use crate::memory_store::{save_memory_manager, MemoryStoreConfig};
        use anima_core::persistence::{DatabaseAdapter, Step, StepStatus};
        let url = std::env::var("ANIMA_TEST_PROFILE_OWNER_POSTGRES_URL")
            .expect("set only the dedicated isolated test database URL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        let old = ProfileOwner::acquire(Vec::<PathBuf>::new()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), old.attach_postgres(&pool))
            .await
            .unwrap()
            .unwrap();
        let before: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations')::text")
                .fetch_one(&pool)
                .await
                .unwrap();
        let refused = ProfileOwner::acquire(Vec::<PathBuf>::new()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), refused.attach_postgres(&pool))
                .await
                .unwrap()
                .is_err()
        );
        let after: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations')::text")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(before, after, "contending ownership touched migrations");
        let mut migration = old.postgres_writer().await.unwrap();
        sqlx::migrate!("./migrations")
            .run_direct(&mut *migration)
            .await
            .unwrap();
        migration.complete().unwrap();
        drop(migration);
        let old_control = ControlPlaneStoreConfig::Postgres(pool.clone())
            .with_owner(old.clone())
            .unwrap();
        let old_memory = MemoryStoreConfig::Postgres(pool.clone())
            .with_owner(old.clone())
            .unwrap();
        let old_history = PostgresHistoryStore::new_owned(pool.clone(), old.clone());
        let old_steps = crate::postgres::SqlxPostgresAdapter::new_owned(pool.clone(), old.clone());
        let mut stale_writer = old.postgres_writer().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *stale_writer)
            .await
            .unwrap();
        assert!(
            sqlx::query_scalar::<_, bool>("SELECT pg_terminate_backend($1)")
                .bind(pid)
                .fetch_one(&pool)
                .await
                .unwrap()
        );
        let successor = ProfileOwner::acquire(Vec::<PathBuf>::new()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), successor.attach_postgres(&pool))
            .await
            .unwrap()
            .unwrap();
        let successor_control = ControlPlaneStoreConfig::Postgres(pool.clone())
            .with_owner(successor.clone())
            .unwrap();
        let mut snapshot = ControlPlaneSnapshot::default();
        snapshot.workspace = Some(WorkspaceConfig {
            root_path: PathBuf::from("isolated-ownership-probe"),
            company_name: "test".into(),
            mission: "successor".into(),
            values: Vec::new(),
        });
        save_control_plane_snapshot(Some(&successor_control), &snapshot)
            .await
            .unwrap();
        let probe_id = format!("profile-owner-{}", uuid::Uuid::new_v4());
        let mut seed = successor.postgres_writer().await.unwrap();
        sqlx::query("INSERT INTO history_messages (id, agent_id, session_id, role, text, hidden, created_at_ms, ordinal, record) VALUES ($1,$1,$1,'user','ownership sentinel',false,1,1,'{}')")
            .bind(&probe_id).execute(&mut *seed).await.unwrap();
        seed.complete().unwrap();
        drop(seed);
        let memory_before: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT payload FROM host_snapshots WHERE key = 'memory'")
                .fetch_optional(&pool)
                .await
                .unwrap();
        // Even a guard admitted before loss is tied to the dead session; it
        // cannot use the pool's still-live connection after the successor wins.
        assert!(
            sqlx::query("DELETE FROM host_snapshots WHERE key = 'control_plane'")
                .execute(&mut *stale_writer)
                .await
                .is_err()
        );
        drop(stale_writer);
        assert!(old.verify_owned().await.is_err());
        assert!(
            save_control_plane_snapshot(Some(&old_control), &ControlPlaneSnapshot::default())
                .await
                .is_err()
        );
        assert!(write_pre_upgrade_backup(&old_control, 1).await.is_err());
        assert!(
            save_memory_manager(Some(&old_memory), &anima_memory::MemoryManager::new())
                .await
                .is_err()
        );
        assert!(old_history.delete_agent(&probe_id).await.is_err());
        let step = Step {
            id: probe_id.clone(),
            agent_id: probe_id.clone(),
            step_index: 0,
            idempotency_key: probe_id.clone(),
            step_type: "ownership-probe".into(),
            status: StepStatus::Done,
            input: None,
            output: None,
        };
        assert!(old_steps.write_step(&step).await.is_err());
        let saved: serde_json::Value =
            sqlx::query_scalar("SELECT payload FROM host_snapshots WHERE key = 'control_plane'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(saved["workspace"]["mission"], "successor");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM history_messages WHERE id = $1")
                .bind(&probe_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM step_log WHERE id = $1")
                .bind(&probe_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT payload FROM host_snapshots WHERE key = 'memory'"
            )
            .fetch_optional(&pool)
            .await
            .unwrap(),
            memory_before
        );
        successor.verify_owned().await.unwrap();
        let mut monitor_probe = successor.postgres_writer().await.unwrap();
        let successor_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *monitor_probe)
            .await
            .unwrap();
        monitor_probe.complete().unwrap();
        drop(monitor_probe);
        assert!(
            sqlx::query_scalar::<_, bool>("SELECT pg_terminate_backend($1)")
                .bind(successor_pid)
                .fetch_one(&pool)
                .await
                .unwrap()
        );
        tokio::time::timeout(Duration::from_secs(4), successor.lost_signal().cancelled())
            .await
            .expect("the PG health monitor must wake waiting gates after session death");
        assert!(successor.ensure_owned().is_err());

        // Leave a real SQL request pending, then cancel the next proof while
        // it owns the raw connection. This exercises cancellation before a
        // successful proof could ever construct/return a mutation writer.
        let cancelled = ProfileOwner::acquire(Vec::<PathBuf>::new()).unwrap();
        cancelled.attach_postgres(&pool).await.unwrap();
        let mut raw = Arc::clone(cancelled.0.postgres.get().unwrap())
            .lock_owned()
            .await;
        let pending_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *raw)
            .await
            .unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            sqlx::query("SELECT pg_sleep(30)").execute(&mut *raw)
        )
        .await
        .is_err());
        drop(raw);
        let checking = cancelled.clone();
        let proof = tokio::spawn(async move { checking.verify_owned().await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(cancelled.0.postgres.get().unwrap().try_lock().is_err());
        proof.abort();
        let _ = proof.await;
        assert!(
            cancelled.ensure_owned().is_err(),
            "a cancelled lock-proof query left the session reusable"
        );
        assert!(cancelled.lost_signal().is_cancelled());
        assert!(
            sqlx::query_scalar::<_, bool>("SELECT pg_terminate_backend($1)")
                .bind(pending_pid)
                .fetch_one(&pool)
                .await
                .unwrap()
        );

        // A monitor blocked behind an unresponsive holder must revoke within
        // its conservative deadline, while keeping the session out of pools.
        let stalled = ProfileOwner::acquire(Vec::<PathBuf>::new()).unwrap();
        stalled.attach_postgres(&pool).await.unwrap();
        let stalled_raw = Arc::clone(stalled.0.postgres.get().unwrap())
            .lock_owned()
            .await;
        tokio::time::timeout(Duration::from_secs(18), stalled.lost_signal().cancelled())
            .await
            .expect("blocked PostgreSQL verification never revoked uncertain ownership");
        assert!(stalled.ensure_owned().is_err());
        drop(stalled_raw);
    }

    #[test]
    fn profile_owner_another_process_cannot_claim_a_retained_sidecar_as_payload() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("state.json");
        let first = ProfileOwner::acquire([resource.clone()]).unwrap();
        let reserved = lock_path(&resource);
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "profile_owner::tests::ownership_child",
                "--nocapture",
            ])
            .env("ANIMA_TEST_OWNERSHIP_RESOURCE", &reserved)
            .env(
                "ANIMA_TEST_PROFILE_OWNER_REGISTRY_DIR",
                test_registry_path().unwrap(),
            )
            .env("ANIMA_TEST_OWNERSHIP_REFUSED", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "a second process accepted the first profile's reserved sidecar as a payload"
        );
        assert!(!lock_path(&reserved).exists());
        first.ensure_owned().unwrap();
        assert!(!resource.exists());
    }

    #[cfg(unix)]
    #[test]
    fn profile_owner_fifo_resource_is_refused_without_opening_payload() {
        let temp = tempfile::tempdir().unwrap();
        let fifo = temp.path().join("state.json");
        let status = Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("Unix ownership test requires mkfifo");
        assert!(status.success());
        let (finished, result) = std::sync::mpsc::channel();
        let path = fifo.clone();
        std::thread::spawn(move || {
            let _ = finished.send(ProfileOwner::acquire([path]).is_err());
        });
        assert!(result
            .recv_timeout(Duration::from_secs(2))
            .expect("FIFO identity check blocked instead of refusing the special file"));
        assert!(!lock_path(&fifo).exists());
    }

    #[test]
    fn profile_owner_upgrade_backup_namespace_cannot_be_another_profiles_payload() {
        let temp = tempfile::tempdir().unwrap();
        let resource = temp.path().join("state.json");
        let owner = ProfileOwner::acquire([resource.clone()]).unwrap();
        for version in 0..crate::control_plane_store::CONTROL_PLANE_STORE_VERSION {
            let backup = crate::control_plane_store::pre_upgrade_backup_path(&resource, version);
            assert!(
                ProfileOwner::acquire([backup.clone()]).is_err(),
                "another profile admitted reserved backup {}",
                backup.display()
            );
            assert!(!backup.exists());
            assert!(!lock_path(&backup).exists());
        }
        owner.ensure_owned().unwrap();
    }

    fn refused_child_resource(resource: &Path, directory: bool) -> bool {
        refused_child_command(resource, directory)
            .status()
            .unwrap()
            .success()
    }

    fn refused_child_command(resource: &Path, directory: bool) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "profile_owner::tests::ownership_child",
                "--nocapture",
            ])
            .env("ANIMA_TEST_OWNERSHIP_RESOURCE", resource)
            .env(
                "ANIMA_TEST_PROFILE_OWNER_REGISTRY_DIR",
                test_registry_path().unwrap(),
            )
            .env("ANIMA_TEST_OWNERSHIP_REFUSED", "1")
            .env_remove("ANIMA_TEST_OWNERSHIP_DIRECTORY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if directory {
            command.env("ANIMA_TEST_OWNERSHIP_DIRECTORY", "1");
        }
        command
    }

    #[test]
    fn profile_owner_directory_claim_refuses_a_second_process_nested_file_claim() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("retained-results");
        let owner =
            ProfileOwner::acquire_resources([ProfileResource::Directory(root.clone())]).unwrap();
        assert!(
            refused_child_resource(&root.join("nested/history.sqlite"), false),
            "a second process claimed a file inside another profile's directory resource"
        );
        assert!(
            !root.exists(),
            "a refused descendant created directories inside the active directory claim"
        );
        owner.ensure_owned().unwrap();
    }

    #[test]
    fn profile_owner_nested_file_claim_refuses_a_second_process_ancestor_directory_claim() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("retained-results");
        let owner = ProfileOwner::acquire([root.join("nested/memory.sqlite")]).unwrap();
        assert!(
            refused_child_resource(&root, true),
            "a second process claimed an ancestor of another profile's file resource"
        );
        owner.ensure_owned().unwrap();
    }

    #[test]
    fn profile_owner_nested_directory_claims_conflict_in_both_orders() {
        for reverse in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path().join("retained");
            let nested = root.join("nested");
            let (first, second) = if reverse {
                (&nested, &root)
            } else {
                (&root, &nested)
            };
            let owner =
                ProfileOwner::acquire_resources([ProfileResource::Directory(first.clone())])
                    .unwrap();
            assert!(
                refused_child_resource(second, true),
                "nested directory claims bypassed hierarchy exclusion"
            );
            owner.ensure_owned().unwrap();
        }
    }

    #[test]
    fn profile_owner_future_file_ancestor_refuses_descendants_before_mkdir() {
        for directory in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let future = fixture.path().join("future-payload");
            let owner = ProfileOwner::acquire([future.clone()]).unwrap();
            assert!(refused_child_resource(&future.join("nested"), directory));
            assert!(
                !future.exists(),
                "a refused descendant changed the active owner's future file into a directory"
            );
            owner.ensure_owned().unwrap();
        }
    }

    #[test]
    fn profile_owner_one_profile_cannot_configure_a_future_file_as_another_resource_parent() {
        let fixture = tempfile::tempdir().unwrap();
        let future = fixture.path().join("future-payload");
        assert!(ProfileOwner::acquire([future.clone(), future.join("nested")]).is_err());
        assert!(!future.exists());
    }

    #[test]
    fn profile_owner_crashed_directory_claim_releases_for_nested_file_takeover() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("retained");
        let child = child_owner_typed(&root, &fixture.path().join("ready"), true);
        assert!(ProfileOwner::acquire([root.join("nested/history.sqlite")]).is_err());
        assert!(!root.exists());
        drop(child);
        let successor = ProfileOwner::acquire([root.join("nested/history.sqlite")]).unwrap();
        successor.ensure_owned().unwrap();
    }

    #[test]
    fn profile_owner_ambient_directory_variables_cannot_bypass_hierarchy_claims() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("retained");
        let owner =
            ProfileOwner::acquire_resources([ProfileResource::Directory(root.clone())]).unwrap();
        let mut command = refused_child_command(&root.join("nested/state.json"), false);
        for variable in [
            "TEMP",
            "TMP",
            "TMPDIR",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "XDG_CACHE_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
        ] {
            command.env(variable, fixture.path().join("alternate-environment"));
        }
        assert!(command.status().unwrap().success());
        assert!(!root.exists());
        owner.ensure_owned().unwrap();
    }

    #[test]
    fn profile_owner_production_registry_location_ignores_ambient_directory_variables() {
        let fixture = tempfile::tempdir().unwrap();
        let output = fixture.path().join("location.txt");
        let expected = registry::production_registry_path().unwrap();
        let alternate = fixture.path().join("alternate-environment");
        #[cfg(windows)]
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            // Known-folder flags0 can first fail existence verification, then
            // successfully redirect when the alternate profile tree exists.
            if let Ok(relative) = expected
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .strip_prefix(PathBuf::from(profile))
            {
                std::fs::create_dir_all(alternate.join(relative)).unwrap();
            }
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "profile_owner::tests::ownership_child",
                "--nocapture",
            ])
            .env("ANIMA_TEST_OWNERSHIP_REPORT_ROOT", &output)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        for variable in [
            "TEMP",
            "TMP",
            "TMPDIR",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "XDG_CACHE_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
            "APPX_PACKAGE_FAMILY_NAME",
            "PACKAGE_FAMILY_NAME",
            "APPLICATION_USER_MODEL_ID",
        ] {
            command.env(variable, &alternate);
        }
        assert!(command.status().unwrap().success());
        let observed = std::fs::read_to_string(output).unwrap();
        assert!(
            observed == expected.to_string_lossy() || observed == "unavailable",
            "process environment selected a different successful ownership registry: {observed}"
        );
    }

    #[test]
    fn profile_owner_sqlite_companion_namespace_is_not_an_independent_payload() {
        let fixture = tempfile::tempdir().unwrap();
        let database = fixture.path().join("memory.sqlite");
        let owner = ProfileOwner::acquire([database.clone()]).unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            let companion = PathBuf::from(format!("{}{suffix}", database.display()));
            assert!(ProfileOwner::acquire([companion.clone()]).is_err());
            assert!(!companion.exists());
            assert!(!lock_path(&companion).exists());
        }
        owner.ensure_owned().unwrap();
    }

    #[test]
    fn profile_owner_reserved_storage_namespace_cannot_be_a_payload_ancestor() {
        let fixture = tempfile::tempdir().unwrap();
        for name in [
            "state.json.owner.lock",
            "state.json.pre-sessions.bak",
            "memory.sqlite-wal",
            "memory.sqlite-shm",
            "memory.sqlite-journal",
        ] {
            let reserved = fixture.path().join(name);
            assert!(refused_child_resource(
                &reserved.join("nested/payload.json"),
                false
            ));
            assert!(
                !reserved.exists(),
                "a refused profile created another store's reserved filename as a directory"
            );
        }
    }

    #[test]
    fn profile_owner_stable_sqlite_identity_excludes_a_renamed_payload_in_another_process() {
        let fixture = tempfile::tempdir().unwrap();
        let original = fixture.path().join("history.sqlite");
        let renamed = fixture.path().join("relocated.sqlite");
        std::fs::write(&original, b"").unwrap();
        let owner =
            ProfileOwner::acquire_resources([ProfileResource::Sqlite(original.clone())]).unwrap();
        match std::fs::rename(&original, &renamed) {
            Ok(()) => {
                // A contender declaring the retained database as JSON still
                // cannot evade the original stable SQLite inode authority.
                assert!(refused_child_resource(&renamed, false), "a renamed retained SQLite inode acquired a second writer under another locator");
                assert!(owner.ensure_owned().is_err());
                drop(owner);
                ProfileOwner::acquire([renamed]).unwrap();
            }
            Err(error) => {
                assert!(cfg!(windows), "unexpected payload rename failure: {error}");
                assert!(original.exists());
                assert!(!renamed.exists());
                owner.ensure_owned().unwrap();
                assert!(refused_child_resource(&original, false));
            }
        }
    }

    #[test]
    fn profile_owner_existing_directory_identity_excludes_renamed_roots_and_descendants() {
        let fixture = tempfile::tempdir().unwrap();
        let original = fixture.path().join("retained-results");
        let renamed = fixture.path().join("relocated-results");
        std::fs::create_dir(&original).unwrap();
        let owner = ProfileOwner::acquire_resources([ProfileResource::Directory(original.clone())])
            .unwrap();
        match std::fs::rename(&original, &renamed) {
            Ok(()) => {
                assert!(refused_child_resource(&renamed, true));
                assert!(refused_child_resource(
                    &renamed.join("nested/state.json"),
                    false
                ));
                assert!(!renamed.join("nested").exists());
                assert!(owner.ensure_owned().is_err());
                drop(owner);
                ProfileOwner::acquire([renamed.join("nested/state.json")]).unwrap();
            }
            Err(error) => {
                assert!(
                    cfg!(windows),
                    "unexpected directory rename failure: {error}"
                );
                assert!(original.exists());
                owner.ensure_owned().unwrap();
            }
        }
    }

    #[test]
    fn profile_owner_directory_activation_pins_the_created_root_before_store_jobs() {
        let fixture = tempfile::tempdir().unwrap();
        let original = fixture.path().join("retained-results");
        let renamed = fixture.path().join("relocated-results");
        let owner = ProfileOwner::acquire_resources([ProfileResource::Directory(original.clone())])
            .unwrap();
        assert!(!original.exists());
        owner
            .with_directory(&original, || std::fs::create_dir(&original))
            .unwrap()
            .unwrap();
        assert!(refused_child_resource(
            &original.join("nested/state.json"),
            false
        ));
        match std::fs::rename(&original, &renamed) {
            Ok(()) => {
                assert!(refused_child_resource(
                    &renamed.join("nested/state.json"),
                    false
                ));
                assert!(!renamed.join("nested").exists());
                assert!(owner.ensure_owned().is_err());
            }
            Err(error) => {
                assert!(
                    cfg!(windows),
                    "unexpected activated directory rename failure: {error}"
                );
                owner.ensure_owned().unwrap();
            }
        }
    }

    #[tokio::test]
    async fn profile_owner_future_replaceable_file_cannot_overwrite_a_later_renamed_sqlite_inode() {
        use crate::control_plane_store::{
            save_control_plane_snapshot, ControlPlaneSnapshot, ControlPlaneStoreConfig,
        };
        let fixture = tempfile::tempdir().unwrap();
        let original = fixture.path().join("history.sqlite");
        let future = fixture.path().join("future.json");
        let sqlite =
            ProfileOwner::acquire_resources([ProfileResource::Sqlite(original.clone())]).unwrap();
        let replaceable = ProfileOwner::acquire([future.clone()]).unwrap();
        let config = ControlPlaneStoreConfig::Json(future.clone())
            .with_owner(replaceable.clone())
            .unwrap();
        match std::fs::rename(&original, &future) {
            Ok(()) => {
                let bytes = std::fs::read(&future).unwrap();
                assert!(save_control_plane_snapshot(
                    Some(&config),
                    &ControlPlaneSnapshot::default()
                )
                .await
                .is_err());
                assert_eq!(std::fs::read(&future).unwrap(), bytes);
                assert!(replaceable.lost_signal().is_cancelled());
                drop(sqlite);
                assert!(save_control_plane_snapshot(
                    Some(&config),
                    &ControlPlaneSnapshot::default()
                )
                .await
                .is_err());
            }
            Err(error) => {
                assert!(
                    cfg!(windows),
                    "unexpected SQLite payload rename failure: {error}"
                );
                assert!(!future.exists());
                sqlite.ensure_owned().unwrap();
                replaceable.ensure_owned().unwrap();
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn profile_owner_windows_ambiguous_components_are_refused_before_creating_ancestors() {
        let fixture = tempfile::tempdir().unwrap();
        for name in [
            "future.",
            "future ",
            "NUL",
            "NuL.txt",
            "CON",
            "con.data.json",
            "PRN",
            "AUX",
            "COM1",
            "COM9.log",
            "COM¹.txt",
            "LPT2",
            "LPT³.txt",
            "CONIN$",
            "CONOUT$",
            "payload:stream",
        ] {
            let root = fixture
                .path()
                .join(format!("probe-{}", uuid::Uuid::new_v4()));
            assert!(
                refused_child_resource(&root.join(name).join("payload.json"), false),
                "unsafe Windows ancestor was accepted: {name}"
            );
            assert!(!root.exists());
            assert!(
                refused_child_resource(&root.join(name), false),
                "unsafe Windows final component was accepted: {name}"
            );
            assert!(!root.exists());
        }
        assert!(refused_child_resource(Path::new(r"\\.\NUL"), false));
        assert!(refused_child_resource(
            Path::new(r"\\?\GLOBALROOT\Device\Null"),
            false
        ));
    }
}
