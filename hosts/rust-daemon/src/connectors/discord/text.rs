//! Adapted eight-message cap from upstream DiscordAdapter._cap_split_chunks; see HERMES-NOTICE.
use sha2::{Digest, Sha256};
pub(crate) const DISCORD_MESSAGE_CHARACTER_LIMIT: usize = 2000;
pub(crate) const MAX_SPLIT_MESSAGES: usize = 8;
pub(crate) fn valid_chunk(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= DISCORD_MESSAGE_CHARACTER_LIMIT
        && text.encode_utf16().count() <= DISCORD_MESSAGE_CHARACTER_LIMIT
}

pub(crate) fn split_reply(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut units = 0;
    for character in text.chars() {
        let width = character.len_utf16();
        if units + width > DISCORD_MESSAGE_CHARACTER_LIMIT {
            chunks.push(std::mem::take(&mut current));
            units = 0;
            if chunks.len() == MAX_SPLIT_MESSAGES {
                chunks.truncate(MAX_SPLIT_MESSAGES - 1);
                chunks.push("Anima shortened this reply to eight Discord messages. The complete reply is available in session history.".into());
                return chunks;
            }
        }
        current.push(character);
        units += width;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
/// Recent provider nonce dedup improves resistance; it never authorizes replay of Unknown.
pub(crate) fn stable_nonce(attempt_id: &str) -> String {
    hex::encode(Sha256::digest(attempt_id.as_bytes()))[..24].to_owned()
}
