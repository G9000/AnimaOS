//! Endpoint-bound OAuth payloads in the same OS vault boundary as MCP bearer tokens.
use super::*;

fn oauth_account(connection: &str, reference: &str) -> Result<String, McpCredentialError> {
    Ok(format!("oauth:{}", account(connection, reference)?))
}
fn anchor_key(id: &str) -> Result<String, McpCredentialError> {
    if !valid_reference(id) {
        return Err(McpCredentialError::Invalid);
    }
    Ok(format!("oauth:connection:{id}:pending"))
}
fn pending_reference(
    backend: &dyn VaultBackend,
    id: &str,
) -> Result<Option<String>, McpCredentialError> {
    backend
        .load(&anchor_key(id)?)?
        .map(|value| {
            if !valid_reference(&value) {
                Err(McpCredentialError::Invalid)
            } else {
                Ok(value.to_string())
            }
        })
        .transpose()
}
fn clear_anchor(
    backend: &dyn VaultBackend,
    id: &str,
    reference: &str,
) -> Result<(), McpCredentialError> {
    if pending_reference(backend, id)?.as_deref() == Some(reference) {
        let key = anchor_key(id)?;
        backend.delete(&key)?;
        if backend.load(&key)?.is_some() {
            return Err(McpCredentialError::Uncertain);
        }
    }
    Ok(())
}
pub(super) async fn vault_commit(
    store: &OsKeyringMcpCredentialStore,
    id: &str,
    reference: &str,
) -> Result<(), McpCredentialError> {
    let backend = store.backend.clone();
    let (id, reference) = (id.to_owned(), reference.to_owned());
    tokio::task::spawn_blocking(move || clear_anchor(backend.as_ref(), &id, &reference))
        .await
        .map_err(|_| McpCredentialError::Uncertain)?
}
pub(super) async fn vault_recover(
    store: &OsKeyringMcpCredentialStore,
    id: &str,
    keep_reference: Option<&str>,
) -> Result<(), McpCredentialError> {
    let backend = store.backend.clone();
    let (id, keep_reference) = (id.to_owned(), keep_reference.map(str::to_owned));
    tokio::task::spawn_blocking(move || {
        if let Some(reference) = pending_reference(backend.as_ref(), &id)? {
            if keep_reference.as_deref() != Some(reference.as_str()) {
                delete_chunks(backend.as_ref(), &oauth_account(&id, &reference)?)?;
            }
            clear_anchor(backend.as_ref(), &id, &reference)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| McpCredentialError::Uncertain)?
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OAuthPayload {
    version: u8,
    connection: String,
    endpoint: String,
    reference: String,
    payload: String,
}

fn validate_payload(endpoint: &str, payload: &str) -> Result<(), McpCredentialError> {
    if !endpoint_valid(endpoint) || payload.is_empty() || payload.len() > 64 * 1024 {
        Err(McpCredentialError::Invalid)
    } else {
        Ok(())
    }
}

pub(super) fn memory_load(
    store: &InMemoryMcpCredentialStore,
    id: &str,
    endpoint: &str,
    reference: &str,
) -> Result<Option<Zeroizing<String>>, McpCredentialError> {
    let key = oauth_account(id, reference)?;
    if !endpoint_valid(endpoint) {
        return Err(McpCredentialError::Invalid);
    }
    store
        .oauth
        .read()
        .map_err(|_| McpCredentialError::Unavailable)?
        .get(&key)
        .map(|(bound, payload)| {
            if bound != endpoint {
                Err(McpCredentialError::Invalid)
            } else {
                Ok(payload.clone())
            }
        })
        .transpose()
}

pub(super) fn memory_put(
    store: &InMemoryMcpCredentialStore,
    id: &str,
    endpoint: &str,
    reference: &str,
    payload: Zeroizing<String>,
    replace: bool,
) -> Result<(), McpCredentialError> {
    validate_payload(endpoint, &payload)?;
    let key = oauth_account(id, reference)?;
    let mut values = store
        .oauth
        .write()
        .map_err(|_| McpCredentialError::Unavailable)?;
    match values.get(&key) {
        Some((bound, _)) if replace && bound == endpoint => {}
        None if !replace => {}
        _ => return Err(McpCredentialError::Invalid),
    }
    values.insert(key, (endpoint.into(), payload));
    Ok(())
}

pub(super) fn memory_delete(
    store: &InMemoryMcpCredentialStore,
    id: &str,
    reference: &str,
) -> Result<(), McpCredentialError> {
    store
        .oauth
        .write()
        .map_err(|_| McpCredentialError::Unavailable)?
        .remove(&oauth_account(id, reference)?);
    Ok(())
}

fn decode(
    value: &str,
    id: &str,
    endpoint: &str,
    reference: &str,
) -> Result<Zeroizing<String>, McpCredentialError> {
    let mut payload: OAuthPayload =
        serde_json::from_str(value).map_err(|_| McpCredentialError::Invalid)?;
    let secret = Zeroizing::new(std::mem::take(&mut payload.payload));
    if payload.version != 1
        || payload.connection != id
        || payload.endpoint != endpoint
        || payload.reference != reference
    {
        return Err(McpCredentialError::Invalid);
    }
    validate_payload(endpoint, &secret)?;
    Ok(secret)
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    generation: String,
    chunks: usize,
}
impl Manifest {
    fn key(&self, slot: &str, index: usize) -> String {
        format!("{slot}:{}:{index}", self.generation)
    }
    fn valid(&self) -> bool {
        self.version == 1 && valid_reference(&self.generation) && (1..=140).contains(&self.chunks)
    }
}
fn manifest(backend: &dyn VaultBackend, key: &str) -> Result<Option<Manifest>, McpCredentialError> {
    backend
        .load(key)?
        .map(|value| {
            let parsed: Manifest =
                serde_json::from_str(&value).map_err(|_| McpCredentialError::Invalid)?;
            if !parsed.valid() {
                return Err(McpCredentialError::Invalid);
            }
            Ok(parsed)
        })
        .transpose()
}
fn load_chunks(
    backend: &dyn VaultBackend,
    key: &str,
) -> Result<Option<Zeroizing<String>>, McpCredentialError> {
    let Some(active) = manifest(backend, key)? else {
        return Ok(None);
    };
    let mut payload = Zeroizing::new(String::new());
    for index in 0..active.chunks {
        let chunk = backend
            .load(&active.key(key, index))?
            .ok_or(McpCredentialError::Invalid)?;
        if chunk.encode_utf16().count() * 2 > 2000 || payload.len() + chunk.len() > 70 * 1024 {
            return Err(McpCredentialError::Invalid);
        }
        payload.push_str(&chunk);
    }
    Ok(Some(payload))
}
fn clean_chunks(
    backend: &dyn VaultBackend,
    key: &str,
    generation: &Manifest,
) -> Result<(), McpCredentialError> {
    for index in 0..generation.chunks {
        let chunk = generation.key(key, index);
        backend.delete(&chunk)?;
        if backend.load(&chunk)?.is_some() {
            return Err(McpCredentialError::Uncertain);
        }
    }
    Ok(())
}
fn recover_chunks(backend: &dyn VaultBackend, key: &str) -> Result<(), McpCredentialError> {
    let journal_key = format!("{key}:cleanup");
    let Some(journal) = backend.load(&journal_key)? else {
        return Ok(());
    };
    let entries: Vec<Manifest> =
        serde_json::from_str(&journal).map_err(|_| McpCredentialError::Invalid)?;
    if entries.len() > 2 || entries.iter().any(|entry| !entry.valid()) {
        return Err(McpCredentialError::Invalid);
    }
    let active = manifest(backend, key)?;
    for entry in entries
        .iter()
        .filter(|entry| Some(*entry) != active.as_ref())
    {
        clean_chunks(backend, key, entry)?;
    }
    backend.delete(&journal_key)?;
    if backend.load(&journal_key)?.is_some() {
        return Err(McpCredentialError::Uncertain);
    }
    Ok(())
}
fn save_chunks(
    backend: &dyn VaultBackend,
    key: &str,
    value: &str,
) -> Result<(), McpCredentialError> {
    recover_chunks(backend, key)?;
    let old = manifest(backend, key)?;
    // 500 Unicode scalars are at most 2000 UTF-16 bytes, below Windows' 2560 byte limit.
    let chars = Zeroizing::new(value.chars().collect::<Vec<_>>());
    let chunks = chars
        .chunks(500)
        .map(|part| Zeroizing::new(part.iter().collect::<String>()))
        .collect::<Vec<_>>();
    let fresh = Manifest {
        version: 1,
        generation: uuid::Uuid::new_v4().to_string(),
        chunks: chunks.len(),
    };
    if !fresh.valid() {
        return Err(McpCredentialError::Invalid);
    }
    let mut journal = old.into_iter().collect::<Vec<_>>();
    journal.push(fresh.clone());
    let journal_key = format!("{key}:cleanup");
    let intent = serde_json::to_string(&journal).map_err(|_| McpCredentialError::Invalid)?;
    backend.put(&journal_key, &intent)?;
    if backend
        .load(&journal_key)?
        .as_ref()
        .map(|value| value.as_str())
        != Some(intent.as_str())
    {
        return Err(McpCredentialError::Uncertain);
    }
    for (index, chunk) in chunks.iter().enumerate() {
        let chunk_key = fresh.key(key, index);
        backend.put(&chunk_key, chunk)?;
        if backend.load(&chunk_key)?.as_ref().map(|s| s.as_str()) != Some(chunk.as_str()) {
            return Err(McpCredentialError::Uncertain);
        }
    }
    backend.put(
        key,
        &serde_json::to_string(&fresh).map_err(|_| McpCredentialError::Invalid)?,
    )?;
    if manifest(backend, key)?.as_ref() != Some(&fresh) {
        return Err(McpCredentialError::Uncertain);
    }
    recover_chunks(backend, key)
}
fn delete_chunks(backend: &dyn VaultBackend, key: &str) -> Result<(), McpCredentialError> {
    recover_chunks(backend, key)?;
    if let Some(active) = manifest(backend, key)? {
        clean_chunks(backend, key, &active)?;
        backend.delete(key)?;
        if backend.load(key)?.is_some() {
            return Err(McpCredentialError::Uncertain);
        }
    }
    Ok(())
}

pub(super) async fn vault_load(
    store: &OsKeyringMcpCredentialStore,
    id: &str,
    endpoint: &str,
    reference: &str,
) -> Result<Option<Zeroizing<String>>, McpCredentialError> {
    let key = oauth_account(id, reference)?;
    if !endpoint_valid(endpoint) {
        return Err(McpCredentialError::Invalid);
    }
    let backend = store.backend.clone();
    let value = tokio::task::spawn_blocking(move || load_chunks(backend.as_ref(), &key))
        .await
        .map_err(|_| McpCredentialError::Unavailable)??;
    value
        .map(|value| decode(&value, id, endpoint, reference))
        .transpose()
}

pub(super) async fn vault_put(
    store: &OsKeyringMcpCredentialStore,
    id: &str,
    endpoint: &str,
    reference: &str,
    payload: Zeroizing<String>,
    replace: bool,
) -> Result<(), McpCredentialError> {
    validate_payload(endpoint, &payload)?;
    let key = oauth_account(id, reference)?;
    let mut envelope = OAuthPayload {
        version: 1,
        connection: id.into(),
        endpoint: endpoint.into(),
        reference: reference.into(),
        payload: payload.to_string(),
    };
    let value =
        Zeroizing::new(serde_json::to_string(&envelope).map_err(|_| McpCredentialError::Invalid)?);
    zeroize::Zeroize::zeroize(&mut envelope.payload);
    let backend = store.backend.clone();
    let (id, endpoint, reference) = (id.to_owned(), endpoint.to_owned(), reference.to_owned());
    tokio::task::spawn_blocking(move || {
        match load_chunks(backend.as_ref(), &key)? {
            Some(old) if replace => {
                decode(&old, &id, &endpoint, &reference)?;
            }
            None if !replace => {}
            _ => return Err(McpCredentialError::Invalid),
        }
        if !replace {
            if pending_reference(backend.as_ref(), &id)?.is_some() {
                return Err(McpCredentialError::Uncertain);
            }
            let anchor = anchor_key(&id)?;
            backend.put(&anchor, &reference)?;
            if pending_reference(backend.as_ref(), &id)?.as_deref() != Some(reference.as_str()) {
                return Err(McpCredentialError::Uncertain);
            }
        }
        let written = save_chunks(backend.as_ref(), &key, &value);
        let observed =
            load_chunks(backend.as_ref(), &key).map_err(|_| McpCredentialError::Uncertain)?;
        if written.is_ok() && observed.as_ref().map(|v| v.as_str()) == Some(value.as_str()) {
            return Ok(());
        }
        // A rotation may have consumed the provider's old refresh token. Its
        // uncertain result must be surfaced; never send the unsaved access token.
        if replace {
            return Err(McpCredentialError::Uncertain);
        }
        delete_chunks(backend.as_ref(), &key).map_err(|_| McpCredentialError::Uncertain)?;
        clear_anchor(backend.as_ref(), &id, &reference)?;
        if backend
            .load(&key)
            .map_err(|_| McpCredentialError::Uncertain)?
            .is_some()
        {
            return Err(McpCredentialError::Uncertain);
        }
        Err(McpCredentialError::Unavailable)
    })
    .await
    .map_err(|_| McpCredentialError::Uncertain)?
}

pub(super) async fn vault_delete(
    store: &OsKeyringMcpCredentialStore,
    id: &str,
    reference: &str,
) -> Result<(), McpCredentialError> {
    let key = oauth_account(id, reference)?;
    let backend = store.backend.clone();
    let (id, reference) = (id.to_owned(), reference.to_owned());
    tokio::task::spawn_blocking(move || {
        delete_chunks(backend.as_ref(), &key)?;
        if backend.load(&key)?.is_some() || backend.load(&format!("{key}:cleanup"))?.is_some() {
            return Err(McpCredentialError::Uncertain);
        }
        clear_anchor(backend.as_ref(), &id, &reference)
    })
    .await
    .map_err(|_| McpCredentialError::Uncertain)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct WindowsSizedVault {
        values: std::sync::Mutex<HashMap<String, Zeroizing<String>>>,
        fail_put: std::sync::atomic::AtomicUsize,
        puts: std::sync::atomic::AtomicUsize,
        fail_delete: std::sync::atomic::AtomicBool,
        unobserved_journal: std::sync::atomic::AtomicBool,
    }
    impl VaultBackend for WindowsSizedVault {
        fn load(&self, key: &str) -> Result<Option<Zeroizing<String>>, McpCredentialError> {
            Ok(self.values.lock().unwrap().get(key).cloned())
        }
        fn put(&self, key: &str, value: &str) -> Result<(), McpCredentialError> {
            use std::sync::atomic::Ordering::SeqCst;
            assert!(value.encode_utf16().count() * 2 <= 2560);
            let count = self.puts.fetch_add(1, SeqCst) + 1;
            if self.fail_put.load(SeqCst) == count {
                return Err(McpCredentialError::Unavailable);
            }
            if key.ends_with(":cleanup") && self.unobserved_journal.load(SeqCst) {
                return Ok(());
            }
            self.values
                .lock()
                .unwrap()
                .insert(key.into(), Zeroizing::new(value.into()));
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), McpCredentialError> {
            if self.fail_delete.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(McpCredentialError::Unavailable);
            }
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
    #[tokio::test]
    async fn windows_oauth_vault_chunks_rotate_restore_and_retry_partial_cleanup() {
        use std::sync::atomic::Ordering::SeqCst;
        let backend = Arc::new(WindowsSizedVault::default());
        let store = OsKeyringMcpCredentialStore {
            backend: backend.clone(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        let reference = uuid::Uuid::new_v4().to_string();
        let endpoint = "https://oauth.invalid/mcp";
        let large = "🦀".repeat(3000);
        store
            .put_oauth(
                &id,
                endpoint,
                &reference,
                Zeroizing::new(large.clone()),
                false,
            )
            .await
            .unwrap();
        let before = backend.values.lock().unwrap().clone();
        backend.unobserved_journal.store(true, SeqCst);
        assert!(store
            .put_oauth(
                &id,
                endpoint,
                &reference,
                Zeroizing::new("unobserved-journal-rotation".repeat(100)),
                true
            )
            .await
            .is_err());
        assert_eq!(*backend.values.lock().unwrap(), before);
        backend.unobserved_journal.store(false, SeqCst);
        assert_eq!(
            store
                .load_oauth(&id, endpoint, &reference)
                .await
                .unwrap()
                .unwrap()
                .as_str(),
            large
        );
        // A partial immutable-generation write keeps the prior manifest readable
        // and journals every chunk for a later cleanup after restart.
        backend
            .fail_put
            .store(backend.puts.load(SeqCst) + 3, SeqCst);
        assert!(store
            .put_oauth(
                &id,
                endpoint,
                &reference,
                Zeroizing::new("rotation".repeat(1000)),
                true
            )
            .await
            .is_err());
        assert_eq!(
            store
                .load_oauth(&id, endpoint, &reference)
                .await
                .unwrap()
                .unwrap()
                .as_str(),
            large
        );
        backend.fail_put.store(0, SeqCst);
        let restored = OsKeyringMcpCredentialStore {
            backend: backend.clone(),
        };
        restored
            .put_oauth(
                &id,
                endpoint,
                &reference,
                Zeroizing::new("rotated".repeat(1000)),
                true,
            )
            .await
            .unwrap();
        assert_eq!(
            restored
                .load_oauth(&id, endpoint, &reference)
                .await
                .unwrap()
                .unwrap()
                .as_str(),
            "rotated".repeat(1000)
        );
        backend.fail_delete.store(true, SeqCst);
        assert!(restored.delete_oauth(&id, &reference).await.is_err());
        backend.fail_delete.store(false, SeqCst);
        restored.delete_oauth(&id, &reference).await.unwrap();
        assert!(backend.values.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn oauth_vault_is_bound_and_replace_cannot_resurrect_a_deleted_slot() {
        let store = InMemoryMcpCredentialStore::default();
        let id = uuid::Uuid::new_v4().to_string();
        let reference = uuid::Uuid::new_v4().to_string();
        let endpoint = "https://oauth.invalid/mcp";
        store
            .put_oauth(
                &id,
                endpoint,
                &reference,
                Zeroizing::new("first".into()),
                false,
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .load_oauth(&id, endpoint, &reference)
                .await
                .unwrap()
                .unwrap()
                .as_str(),
            "first"
        );
        assert!(store
            .load_oauth(&id, "https://other.invalid/mcp", &reference)
            .await
            .is_err());
        store
            .put_oauth(
                &id,
                endpoint,
                &reference,
                Zeroizing::new("rotated".into()),
                true,
            )
            .await
            .unwrap();
        store.delete_oauth(&id, &reference).await.unwrap();
        assert!(store
            .put_oauth(
                &id,
                endpoint,
                &reference,
                Zeroizing::new("late".into()),
                true
            )
            .await
            .is_err());
        assert!(store
            .load_oauth(&id, endpoint, &reference)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn unpublished_oauth_anchor_survives_failed_cleanup_and_recovers_after_restart() {
        use std::sync::atomic::Ordering::SeqCst;
        let backend = Arc::new(WindowsSizedVault::default());
        let store = OsKeyringMcpCredentialStore {
            backend: backend.clone(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        let reference = uuid::Uuid::new_v4().to_string();
        // Anchor, journal and first chunk are durable; fail before publication.
        backend.fail_put.store(4, SeqCst);
        backend.fail_delete.store(true, SeqCst);
        assert!(store
            .put_oauth(
                &id,
                "https://oauth.invalid/mcp",
                &reference,
                Zeroizing::new("new-login-secret".repeat(1000)),
                false
            )
            .await
            .is_err());
        assert!(store.delete_oauth(&id, &reference).await.is_err());
        assert_eq!(
            pending_reference(backend.as_ref(), &id).unwrap().as_deref(),
            Some(reference.as_str())
        );
        assert!(
            manifest(backend.as_ref(), &oauth_account(&id, &reference).unwrap())
                .unwrap()
                .is_none()
        );
        let restored = OsKeyringMcpCredentialStore {
            backend: backend.clone(),
        };
        assert!(restored.recover_oauth(&id, None).await.is_err());
        backend.fail_delete.store(false, SeqCst);
        backend.fail_put.store(0, SeqCst);
        restored.recover_oauth(&id, None).await.unwrap();
        assert!(backend.values.lock().unwrap().is_empty());
    }
}
