use super::text::*;

#[test]
fn discord_chunks_preserve_scalar_boundaries_and_cap_eight_with_history_notice() {
    for text in ["x".repeat(16000), "🦀".repeat(8000), "a🦀b\n".repeat(1200)] {
        let chunks = split_reply(&text);
        assert!(chunks.len() <= 8);
        assert!(chunks.iter().all(|c| valid_chunk(c)));
        assert_eq!(chunks.concat(), text);
    }
    for text in ["x".repeat(16001), "🦀".repeat(8001)] {
        let chunks = split_reply(&text);
        assert_eq!(chunks.len(), 8);
        assert!(chunks.iter().all(|c| valid_chunk(c)));
        assert!(text.starts_with(&chunks[..7].concat()));
        assert!(chunks[7].contains("Anima"));
        assert!(chunks[7].contains("session history"));
    }
    assert!(split_reply("").is_empty());
    assert!(!valid_chunk(""));
    assert!(!valid_chunk(&"🦀".repeat(1001)));
}
#[test]
fn discord_nonce_is_stable_short_and_chunk_specific() {
    let first = stable_nonce("outbound:42:chunk:0");
    assert_eq!(first, stable_nonce("outbound:42:chunk:0"));
    assert!(first.len() <= 25);
    assert_ne!(first, stable_nonce("outbound:42:chunk:1"));
}
