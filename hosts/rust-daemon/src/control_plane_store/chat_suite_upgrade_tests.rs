use super::*;

#[tokio::test]
async fn chat_suite_upgrade_preserves_exact_v28_backup_and_round_trips_chat_state() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.json");
    let original = b"{\n  \"version\":28\n}\n";
    fs::write(&path, original).unwrap();
    let config = ControlPlaneStoreConfig::Json(path.clone());
    let mut saved = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert_eq!(saved.version, 28);
    let backup = write_pre_upgrade_backup(&config, saved.version)
        .await
        .unwrap();
    assert_eq!(
        PathBuf::from(backup),
        path.with_file_name("state.json.pre-chat-suite.bak")
    );
    assert_eq!(
        fs::read(path.with_file_name("state.json.pre-chat-suite.bak")).unwrap(),
        original
    );
    let mut session = crate::sessions::SessionRecord::new(
        "a",
        "chat:one",
        crate::sessions::SessionKind::Chat,
        crate::sessions::SessionOrigin::Web,
        "Chat".into(),
        crate::sessions::TitleSource::System,
        1,
    );
    session.model_preferences.primary = crate::chat_models::ModelPreference::Override {
        provider: "chatgpt".into(),
        model: "gpt-6-astra".into(),
        reasoning_effort: Some("high".into()),
    };
    saved.sessions.push(session);
    saved.version = CONTROL_PLANE_STORE_VERSION;
    save_control_plane_snapshot(Some(&config), &saved)
        .await
        .unwrap();
    let restored = load_control_plane_snapshot(&config).await.unwrap().unwrap();
    assert_eq!(restored.version, CONTROL_PLANE_STORE_VERSION);
    assert_eq!(
        restored.sessions[0].model_preferences,
        saved.sessions[0].model_preferences
    );
    assert!(restored.sessions[0].conversation_shares.is_empty());
    assert_eq!(
        fs::read(path.with_file_name("state.json.pre-chat-suite.bak")).unwrap(),
        original
    );
}
