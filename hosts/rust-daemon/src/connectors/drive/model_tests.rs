use super::model::*;
use zeroize::Zeroizing;

#[test]
fn drive_file_ids_cannot_be_paths_queries_or_links_and_round_trip_as_strings() {
    for value in [
        "",
        "../file",
        "https://drive.google.com/file/d/id/view",
        "a?alt=media",
        "a%2Fb",
        "file name",
        "é",
    ] {
        assert_eq!(
            DriveFileId::parse(value).unwrap_err(),
            DriveError::InvalidInput
        );
    }
    assert!(DriveFileId::parse("a".repeat(257)).is_err());
    let id = DriveFileId::parse("opaque_A-z-09").unwrap();
    assert_eq!(serde_json::to_string(&id).unwrap(), r#""opaque_A-z-09""#);
    assert_eq!(
        serde_json::from_str::<DriveFileId>(r#""opaque_A-z-09""#).unwrap(),
        id
    );
    assert!(serde_json::from_str::<DriveFileId>(r#""../bad""#).is_err());
}
#[test]
fn drive_consent_has_only_readonly_scope_offline_and_pkce_without_secrets() {
    let config = DriveOAuthConfig::new(
        "client".into(),
        Zeroizing::new("secret-sentinel".into()),
        "http://127.0.0.1:8080/api/connectors/gdrive/callback".into(),
        8,
    )
    .unwrap();
    let consent = config.consent_url("opaque-state", "pkce-challenge");
    let url = reqwest::Url::parse(&consent).unwrap();
    let pairs = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(url.host_str(), Some("accounts.google.com"));
    assert_eq!(pairs["scope"], DRIVE_SCOPE);
    assert_eq!(pairs["access_type"], "offline");
    assert_eq!(pairs["code_challenge_method"], "S256");
    assert_eq!(pairs["code_challenge"], "pkce-challenge");
    assert!(!consent.contains("secret-sentinel"));
    assert!(!format!("{config:?}").contains("secret-sentinel"));
}
