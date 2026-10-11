use super::*;
use std::sync::Mutex;

#[tokio::test]
async fn discord_credentials_fence_generation_bot_and_connector_without_token_formats() {
    let store = InMemoryDiscordCredentialStore::default();
    let token = DiscordBotToken::parse("discord-secret-sentinel").unwrap();
    store.put("native", 1, "42", &token).await.unwrap();
    store
        .put(
            "native",
            2,
            "42",
            &DiscordBotToken::parse("replacement").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        store.load("native", 1, "42").await.unwrap(),
        Some(token.clone())
    );
    assert_eq!(
        store.load("native", 1, "43").await.unwrap_err(),
        DiscordCredentialError::InvalidPayload
    );
    assert_eq!(store.load("other", 1, "42").await.unwrap(), None);
    store.delete("native", 1).await.unwrap();
    assert_eq!(store.load("native", 1, "42").await.unwrap(), None);
    assert_eq!(
        store
            .load("native", 2, "42")
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "replacement"
    );
    assert!(!format!("{token:?} {token}").contains("sentinel"));
    for invalid in ["", "Bot abc", "a\nb", "a/b"] {
        assert!(DiscordBotToken::parse(invalid).is_err());
    }
    for (id, generation, bot) in [
        ("x", 0, "42"),
        ("../bad", 1, "42"),
        ("x", 1, "0"),
        ("x", 1, "042"),
    ] {
        assert_eq!(
            store.put(id, generation, bot, &token).await.unwrap_err(),
            DiscordCredentialError::InvalidInput
        );
    }
}
#[derive(Default)]
struct FakeBackend {
    value: Mutex<Option<Zeroizing<String>>>,
    fail_after_put: Mutex<bool>,
    ignore_delete: Mutex<bool>,
    fail_reads: Mutex<bool>,
}
impl Backend for FakeBackend {
    fn load(&self, _: &str) -> Result<Option<Zeroizing<String>>, DiscordCredentialError> {
        if *self.fail_reads.lock().unwrap() {
            return Err(DiscordCredentialError::Unavailable);
        }
        Ok(self.value.lock().unwrap().clone())
    }
    fn put(&self, _: &str, value: &str) -> Result<(), DiscordCredentialError> {
        *self.value.lock().unwrap() = Some(Zeroizing::new(value.into()));
        let mut fail = self.fail_after_put.lock().unwrap();
        if *fail {
            *fail = false;
            Err(DiscordCredentialError::Unavailable)
        } else {
            Ok(())
        }
    }
    fn delete(&self, _: &str) -> Result<(), DiscordCredentialError> {
        if !*self.ignore_delete.lock().unwrap() {
            *self.value.lock().unwrap() = None;
        }
        Ok(())
    }
}
#[tokio::test]
async fn discord_vault_compensates_failed_put_and_reports_unconfirmed_delete() {
    let backend = Arc::new(FakeBackend::default());
    let store = OsKeyringDiscordCredentialStore {
        backend: backend.clone(),
    };
    let token = DiscordBotToken::parse("old").unwrap();
    store.put("native", 1, "42", &token).await.unwrap();
    *backend.fail_after_put.lock().unwrap() = true;
    assert_eq!(
        store
            .put("native", 1, "42", &DiscordBotToken::parse("new").unwrap())
            .await
            .unwrap_err(),
        DiscordCredentialError::Unavailable
    );
    assert_eq!(store.load("native", 1, "42").await.unwrap(), Some(token));
    *backend.ignore_delete.lock().unwrap() = true;
    assert_eq!(
        store.delete("native", 1).await.unwrap_err(),
        DiscordCredentialError::Uncertain
    );
    for replacement in [
        r#"{"version":99,"connectorId":"native","generation":1,"botId":"42","token":"secret"}"#,
        r#"{"version":1,"connectorId":"other","generation":1,"botId":"42","token":"secret"}"#,
    ] {
        *backend.value.lock().unwrap() = Some(Zeroizing::new(replacement.into()));
        assert_eq!(
            store.load("native", 1, "42").await.unwrap_err(),
            DiscordCredentialError::InvalidPayload
        );
    }
}
