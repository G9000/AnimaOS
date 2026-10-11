use super::super::model::DRIVE_SCOPE;
use super::*;
use std::sync::Mutex;
fn grant(access: &str) -> DriveTokenGrant {
    DriveTokenGrant {
        tokens: GoogleOAuthTokens::new(
            Zeroizing::new(access.into()),
            Zeroizing::new("refresh-sentinel".into()),
            123,
        ),
        granted_scopes: vec![DRIVE_SCOPE.into()],
    }
}
#[tokio::test]
async fn drive_vault_fences_generations_validates_scope_and_redacts_secrets() {
    let store = InMemoryDriveCredentialStore::default();
    store
        .put("selected", "generation-one", grant("access-sentinel"))
        .await
        .unwrap();
    store
        .put("selected", "generation-two", grant("new"))
        .await
        .unwrap();
    assert_eq!(
        store
            .load("selected", "generation-one")
            .await
            .unwrap()
            .unwrap()
            .tokens
            .access_token(),
        "access-sentinel"
    );
    store.delete("selected", "generation-one").await.unwrap();
    assert!(store
        .load("selected", "generation-one")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .load("selected", "generation-two")
            .await
            .unwrap()
            .unwrap()
            .tokens
            .access_token(),
        "new"
    );
    let mut invalid = grant("access");
    invalid.granted_scopes = vec!["openid".into()];
    assert_eq!(
        store
            .put("selected", "generation-three", invalid)
            .await
            .unwrap_err(),
        DriveError::ReauthRequired
    );
    assert!(!format!("{:?}", grant("access-sentinel")).contains("sentinel"));
    for (id, generation) in [
        ("../bad", "generation"),
        ("selected", ""),
        ("selected", "../../bad"),
    ] {
        assert_eq!(
            store.load(id, generation).await.unwrap_err(),
            DriveError::InvalidInput
        );
    }
}
#[derive(Default)]
struct FakeBackend {
    value: Mutex<Option<Zeroizing<String>>>,
    fail_after_write: Mutex<bool>,
    ignore_delete: Mutex<bool>,
}
impl Backend for FakeBackend {
    fn load(&self, _: &str) -> Result<Option<Zeroizing<String>>, DriveError> {
        Ok(self.value.lock().unwrap().clone())
    }
    fn put(&self, _: &str, value: &str) -> Result<(), DriveError> {
        *self.value.lock().unwrap() = Some(Zeroizing::new(value.into()));
        let mut fail = self.fail_after_write.lock().unwrap();
        if *fail {
            *fail = false;
            Err(DriveError::Credential)
        } else {
            Ok(())
        }
    }
    fn delete(&self, _: &str) -> Result<(), DriveError> {
        if !*self.ignore_delete.lock().unwrap() {
            *self.value.lock().unwrap() = None;
        }
        Ok(())
    }
}
#[tokio::test]
async fn drive_store_compensates_after_mutation_errors_and_rejects_payload_binding_changes() {
    let backend = Arc::new(FakeBackend::default());
    let store = OsKeyringDriveCredentialStore {
        backend: backend.clone(),
    };
    store
        .put("selected", "generation", grant("old"))
        .await
        .unwrap();
    *backend.fail_after_write.lock().unwrap() = true;
    assert_eq!(
        store
            .put("selected", "generation", grant("replacement"))
            .await
            .unwrap_err(),
        DriveError::Credential
    );
    assert_eq!(
        store
            .load("selected", "generation")
            .await
            .unwrap()
            .unwrap()
            .tokens
            .access_token(),
        "old"
    );
    *backend.ignore_delete.lock().unwrap() = true;
    assert_eq!(
        store.delete("selected", "generation").await.unwrap_err(),
        DriveError::Credential
    );
    let payload = backend.value.lock().unwrap().clone().unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    value["generation"] = serde_json::json!("other");
    *backend.value.lock().unwrap() = Some(Zeroizing::new(value.to_string()));
    assert_eq!(
        store.load("selected", "generation").await.unwrap_err(),
        DriveError::Credential
    );
}
