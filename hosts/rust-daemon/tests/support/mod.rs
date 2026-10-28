use anima_daemon::app as daemon_app;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;

fn workspace_root_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn test_app() -> Router {
    daemon_app()
}

pub(crate) async fn configured_loopback_test_app(
    config: anima_daemon::DaemonConfig,
) -> std::io::Result<Router> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    anima_daemon::app_with_configured_persistence_for_listener(config, &listener).await
}

/// Read the deterministic owner credential supplied by the Rust test runner.
pub(crate) fn test_owner_token() -> String {
    let token = std::env::var("ANIMAOS_TEST_OWNER_TOKEN")
        .unwrap_or_else(|_| panic!("test runner must set ANIMAOS_TEST_OWNER_TOKEN"));
    assert!(
        !token.is_empty(),
        "test runner must set a non-empty ANIMAOS_TEST_OWNER_TOKEN"
    );
    token
}

/// Attach owner credentials explicitly so raw requests still test rejection paths.
pub(crate) fn owner_request(builder: axum::http::request::Builder) -> axum::http::request::Builder {
    let token = test_owner_token();
    builder
        .header("host", "127.0.0.1:8080")
        .header("authorization", format!("Bearer {token}"))
}

pub(crate) struct WorkspaceRootGuard {
    previous: Option<OsString>,
    previous_document_edits: Option<OsString>,
    path: PathBuf,
    profile: OnceLock<PrivateProfile>,
    _lock: MutexGuard<'static, ()>,
}

struct PrivateProfile {
    path: PathBuf,
    canonical: PathBuf,
    parent: PathBuf,
    identity: (u64, u64),
    held: Option<cap_std::fs::Dir>,
}

impl PrivateProfile {
    fn create(workspace: &Path) -> std::io::Result<Self> {
        use cap_fs_ext::{DirExt, MetadataExt};
        let parent = workspace.parent().expect("workspace has a parent");
        let mut name = workspace
            .file_name()
            .expect("workspace has a name")
            .to_os_string();
        name.push(".profile");
        let path = parent.join(&name);
        #[cfg(windows)]
        {
            use win_custody::{current_user_sid_string, SecurityDescriptor};
            let user = current_user_sid_string()?;
            SecurityDescriptor::from_sddl(&format!(
                "O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)"
            ))?
            .create_directory(&path)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&path)?;
        }
        #[cfg(not(any(windows, unix)))]
        fs::create_dir(&path)?;
        let parent = parent.canonicalize()?;
        let canonical = path.canonicalize()?;
        let workspace = workspace.canonicalize()?;
        if canonical.parent() != Some(parent.as_path())
            || canonical.starts_with(&workspace)
            || workspace.starts_with(&canonical)
        {
            return Err(std::io::Error::other(
                "private test profile is not a separate sibling",
            ));
        }
        let parent_handle =
            cap_std::fs::Dir::open_ambient_dir(&parent, cap_std::ambient_authority())?;
        let named = parent_handle.symlink_metadata(&name)?;
        if !regular_directory(&named) {
            return Err(std::io::Error::other("private test profile is linked"));
        }
        let held = parent_handle.open_dir_nofollow(&name)?;
        let metadata = held.dir_metadata()?;
        if !regular_directory(&metadata)
            || (named.dev(), named.ino()) != (metadata.dev(), metadata.ino())
        {
            return Err(std::io::Error::other(
                "private test profile identity changed",
            ));
        }
        Ok(Self {
            path,
            canonical,
            parent,
            identity: (metadata.dev(), metadata.ino()),
            held: Some(held),
        })
    }

    fn cleanup(&mut self) -> std::io::Result<()> {
        use cap_fs_ext::{DirExt, MetadataExt};
        let canonical = self.path.canonicalize()?;
        if canonical != self.canonical || canonical.parent() != Some(self.parent.as_path()) {
            return Err(std::io::Error::other("private test profile moved"));
        }
        let parent =
            cap_std::fs::Dir::open_ambient_dir(&self.parent, cap_std::ambient_authority())?;
        let name = self.path.file_name().expect("private profile has a name");
        let named = parent.symlink_metadata(name)?;
        if !regular_directory(&named) || (named.dev(), named.ino()) != self.identity {
            return Err(std::io::Error::other("private test profile name changed"));
        }
        let fresh = parent.open_dir_nofollow(name)?;
        let fresh_metadata = fresh.dir_metadata()?;
        let held = self
            .held
            .as_ref()
            .expect("private profile retains its directory");
        let held_metadata = held.dir_metadata()?;
        if !regular_directory(&fresh_metadata)
            || !regular_directory(&held_metadata)
            || (fresh_metadata.dev(), fresh_metadata.ino()) != self.identity
            || (held_metadata.dev(), held_metadata.ino()) != self.identity
        {
            return Err(std::io::Error::other(
                "private test profile custody changed",
            ));
        }
        drop(fresh);
        self.held
            .take()
            .expect("verified private profile handle")
            .remove_open_dir_all()
    }
}

fn regular_directory(metadata: &cap_std::fs::Metadata) -> bool {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

impl WorkspaceRootGuard {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn control_plane_path(&self) -> PathBuf {
        self.profile
            .get_or_init(|| {
                PrivateProfile::create(&self.path).expect("private test profile creates atomically")
            })
            .path
            .join("control-plane.json")
    }
}

impl Drop for WorkspaceRootGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var("ANIMAOS_WORKSPACE_ROOT", previous);
        } else {
            std::env::remove_var("ANIMAOS_WORKSPACE_ROOT");
        }
        if let Some(previous) = &self.previous_document_edits {
            std::env::set_var("ANIMAOS_RS_DOCUMENT_EDITS_DIR", previous);
        } else {
            std::env::remove_var("ANIMAOS_RS_DOCUMENT_EDITS_DIR");
        }
        if let Some(profile) = self.profile.get_mut() {
            let _ = profile.cleanup();
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn use_temp_workspace_root(prefix: &str) -> WorkspaceRootGuard {
    let lock = workspace_root_lock()
        .lock()
        .expect("workspace root lock should not be poisoned");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("anima-daemon-{prefix}-{unique}"));
    fs::create_dir_all(&path).expect("workspace dir should be created");

    let previous = std::env::var_os("ANIMAOS_WORKSPACE_ROOT");
    let previous_document_edits = std::env::var_os("ANIMAOS_RS_DOCUMENT_EDITS_DIR");
    std::env::set_var("ANIMAOS_WORKSPACE_ROOT", &path);
    std::env::remove_var("ANIMAOS_RS_DOCUMENT_EDITS_DIR");

    WorkspaceRootGuard {
        previous,
        previous_document_edits,
        path,
        profile: OnceLock::new(),
        _lock: lock,
    }
}

pub(crate) async fn send_request(app: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app.clone().oneshot(request).await.expect("app responds");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body reads");
    (
        status,
        std::str::from_utf8(&body)
            .expect("body is utf-8")
            .to_string(),
    )
}

pub(crate) async fn send_json_request(
    app: &Router,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, String) {
    send_request(
        app,
        owner_request(Request::builder())
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .expect("request builds"),
    )
    .await
}

pub(crate) async fn send_empty_request(
    app: &Router,
    method: &str,
    uri: &str,
) -> (StatusCode, String) {
    send_request(
        app,
        owner_request(Request::builder())
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request builds"),
    )
    .await
}

/// Build a GET URI with one query parameter, percent-encoding the value
/// (Windows paths contain `:` and `\`, which must be encoded).
pub(crate) fn query_uri(path: &str, param: &str, value: &str) -> String {
    let encoded: String = value
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            other => format!("%{:02X}", other as u32),
        })
        .collect();
    format!("{path}?{param}={encoded}")
}

pub(crate) fn extract_json_string_field(response: &str, field: &str) -> String {
    let needle = format!("\"{field}\":\"");
    let start = response
        .find(&needle)
        .map(|index| index + needle.len())
        .expect("field should exist");
    let rest = &response[start..];
    let end = rest.find('"').expect("field should terminate");
    rest[..end].to_string()
}

#[allow(dead_code)]
pub(crate) fn extract_json_u64_field(response: &str, field: &str) -> u64 {
    let needle = format!("\"{field}\":");
    let start = response
        .find(&needle)
        .map(|index| index + needle.len())
        .expect("field should exist");
    let rest = &response[start..];
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end]
        .parse::<u64>()
        .expect("field should be an unsigned integer")
}

#[allow(dead_code)]
pub(crate) fn extract_sse_event_data<'a>(stream: &'a str, event_name: &str) -> Option<&'a str> {
    let marker = format!("event: {event_name}\n");
    let start = stream.find(&marker)? + marker.len();
    let rest = &stream[start..];
    let data_marker = "data: ";
    let data_start = rest.find(data_marker)? + data_marker.len();
    let data = &rest[data_start..];
    let end = data.find("\n\n").unwrap_or(data.len());
    Some(&data[..end])
}

/// End app workers and blocking persistence before restart or profile cleanup.
#[allow(dead_code)]
pub(crate) async fn run_in_joined_runtime<F, Fut, T>(make_work: F) -> T
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T>,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test boot runtime should start");
        let result = runtime.block_on(make_work());
        drop(runtime);
        result
    })
    .await
    .expect("test boot runtime should fully shut down")
}
