//! Bounded, host-independent skill learning helpers.

use serde::{Deserialize, Serialize};

pub const MAX_SKILL_EXPERIENCES: usize = 64;
const MAX_PATCHES: usize = 16;
const MAX_PATCHED_BYTES: usize = 32 * 1_024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextPatch {
    pub old_text: String,
    pub new_text: String,
}

/// Apply small exact replacements without selecting an arbitrary repeated match.
pub fn apply_patches(text: &str, patches: &[TextPatch]) -> Result<String, String> {
    if patches.is_empty() || patches.len() > MAX_PATCHES {
        return Err(format!("expected between 1 and {MAX_PATCHES} patches"));
    }
    let mut result = text.to_owned();
    for (index, patch) in patches.iter().enumerate() {
        let number = index + 1;
        if patch.old_text.is_empty() {
            return Err(format!("patch {number} old_text must not be empty"));
        }
        if patch.old_text == patch.new_text {
            return Err(format!("patch {number} makes no change"));
        }
        let start = result
            .find(&patch.old_text)
            .ok_or_else(|| format!("patch {number} old_text was not found"))?;
        // find/rfind also detect overlapping matches, unlike match_indices.
        if result.rfind(&patch.old_text) != Some(start) {
            return Err(format!("patch {number} old_text is ambiguous"));
        }
        let output_bytes = (result.len() - patch.old_text.len())
            .checked_add(patch.new_text.len())
            .ok_or_else(|| format!("patch {number} output exceeds {MAX_PATCHED_BYTES} bytes"))?;
        if output_bytes > MAX_PATCHED_BYTES {
            return Err(format!(
                "patch {number} output exceeds {MAX_PATCHED_BYTES} bytes"
            ));
        }
        result.replace_range(start..start + patch.old_text.len(), &patch.new_text);
    }
    if result == text {
        return Err("patches make no overall change".into());
    }
    Ok(result)
}

/// Produce a bounded excerpt with conservative, heuristic line redaction.
///
/// This is not an exhaustive secret detector. Hosts must select appropriate
/// evidence before passing it here and must not treat the result as trusted.
pub fn safe_learning_excerpt(text: &str, max_bytes: usize) -> String {
    let mut excerpt = String::new();
    let mut private_key_block = false;
    for line in text.split_inclusive('\n') {
        if excerpt.len() == max_bytes {
            break;
        }
        let (body, ending) = if let Some(body) = line.strip_suffix("\r\n") {
            (body, "\r\n")
        } else if let Some(body) = line.strip_suffix('\n') {
            (body, "\n")
        } else {
            (line, "")
        };
        let private_key_label = contains_ascii_case(body, "private key-----")
            || contains_ascii_case(body, "pgp private key block-----");
        let begins_key = contains_ascii_case(body, "-----begin") && private_key_label;
        let ends_key = contains_ascii_case(body, "-----end") && private_key_label;
        let redact = private_key_block || begins_key || sensitive_learning_line(body);
        private_key_block = (private_key_block || begins_key) && !ends_key;
        let visible = if redact {
            "[redacted sensitive line]"
        } else {
            body
        };
        if !append_bounded(&mut excerpt, visible, max_bytes)
            || !append_bounded(&mut excerpt, ending, max_bytes)
        {
            break;
        }
    }
    excerpt
}

fn contains_ascii_case(text: &str, needle: &str) -> bool {
    text.as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn sensitive_learning_line(line: &str) -> bool {
    const SENSITIVE_MARKERS: &[&str] = &[
        "credential",
        "password",
        "passwd",
        "api_key",
        "api-key",
        "api key",
        "apikey",
        "authorization",
        "bearer",
        "cookie",
        "private key",
        "private_key",
        "private-key",
        "token",
        "secret",
        "access_key",
        "access-key",
        "access key",
    ];
    line.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '\u{00ad}' | '\u{034f}' | '\u{061c}' | '\u{115f}'..='\u{1160}'
                    | '\u{17b4}'..='\u{17b5}' | '\u{180b}'..='\u{180f}'
                    | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}'
                    | '\u{2060}'..='\u{206f}' | '\u{2800}' | '\u{3164}'
                    | '\u{fe00}'..='\u{fe0f}' | '\u{feff}' | '\u{ffa0}'
                    | '\u{e0000}'..='\u{e007f}' | '\u{e0100}'..='\u{e01ef}'
            )
    }) || SENSITIVE_MARKERS
        .iter()
        .any(|marker| contains_ascii_case(line, marker))
}

fn append_bounded(output: &mut String, text: &str, max_bytes: usize) -> bool {
    let mut end = text.len().min(max_bytes.saturating_sub(output.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&text[..end]);
    end == text.len()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillUseStatus {
    #[default]
    Loaded,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExperience {
    pub agent_id: String,
    pub run_id: String,
    pub session_id: String,
    pub version: String,
    pub loaded_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub status: SkillUseStatus,
    pub verified_evidence: Option<String>,
    pub corrected: bool,
}

/// Keep the first load for each agent/run/version and retain the newest records.
pub fn record_experience(entries: &mut Vec<SkillExperience>, experience: SkillExperience) {
    if !entries.iter().any(|entry| {
        entry.agent_id == experience.agent_id
            && entry.run_id == experience.run_id
            && entry.version == experience.version
    }) {
        entries.push(experience);
    }
    while entries.len() > MAX_SKILL_EXPERIENCES {
        let oldest = entries
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.loaded_at_ms)
            .map(|(index, _)| index)
            .expect("over-capacity experience list is nonempty");
        entries.remove(oldest);
    }
}

/// Settle outstanding uses in the run; completion does not imply verification.
pub fn settle_experience(
    entries: &mut [SkillExperience],
    agent_id: &str,
    run_id: &str,
    status: SkillUseStatus,
    at_ms: u64,
) {
    if status == SkillUseStatus::Loaded {
        return;
    }
    for entry in entries {
        if entry.agent_id == agent_id
            && entry.run_id == run_id
            && entry.status == SkillUseStatus::Loaded
        {
            entry.status = status;
            entry.finished_at_ms = Some(at_ms.max(entry.loaded_at_ms));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(old_text: &str, new_text: &str) -> TextPatch {
        TextPatch {
            old_text: old_text.into(),
            new_text: new_text.into(),
        }
    }

    fn experience(agent_id: &str, run_id: &str, version: &str, at_ms: u64) -> SkillExperience {
        SkillExperience {
            agent_id: agent_id.into(),
            run_id: run_id.into(),
            session_id: "session".into(),
            version: version.into(),
            loaded_at_ms: at_ms,
            finished_at_ms: None,
            status: SkillUseStatus::Loaded,
            verified_evidence: None,
            corrected: false,
        }
    }

    #[test]
    fn patches_apply_sequentially_and_preserve_utf8() {
        let source = "First: café\nSecond: original\n";
        assert_eq!(
            apply_patches(
                source,
                &[patch("original", "更新"), patch("更新", "complete ✓")]
            )
            .unwrap(),
            "First: café\nSecond: complete ✓\n"
        );
        assert_eq!(
            apply_patches("remove me", &[patch(" me", "")]).unwrap(),
            "remove"
        );
    }

    #[test]
    fn patches_reject_missing_ambiguous_and_overlapping_matches() {
        for (source, old) in [("abc", "z"), ("abc abc", "abc"), ("aaa", "aa")] {
            assert!(apply_patches(source, &[patch(old, "new")]).is_err());
        }
    }

    #[test]
    fn patches_reject_no_ops_and_enforce_bounds() {
        assert!(apply_patches("abc", &[]).is_err());
        assert!(apply_patches("abc", &[patch("", "x")]).is_err());
        assert!(apply_patches("abc", &[patch("abc", "abc")]).is_err());
        assert!(apply_patches("abc", &[patch("abc", "xyz"), patch("xyz", "abc")]).is_err());
        assert!(apply_patches("a", &vec![patch("a", "aa"); 17]).is_err());
        assert!(apply_patches("a", &[patch("a", &"é".repeat(16_385))]).is_err());
        assert!(apply_patches("a", &[patch("a", &"x".repeat(32_769)), patch("x", "a")]).is_err());
        assert_eq!(
            apply_patches("a", &[patch("a", &"é".repeat(16_384))])
                .unwrap()
                .len(),
            32_768
        );
    }

    #[test]
    fn patch_json_rejects_unexpected_fields() {
        assert!(serde_json::from_value::<TextPatch>(serde_json::json!({
            "old_text": "old", "new_text": "new", "path": "elsewhere"
        }))
        .is_err());
    }

    #[test]
    fn excerpt_redacts_sensitive_lines_and_private_key_bodies() {
        let text = "Verified tests passed\nPASSWORD=hunter2\napi_key: abc123\nAuthorization: Bearer xyz\nCookie: session=secret\ncredentials: owner\n-----BEGIN RSA PRIVATE KEY-----\nopaque-key-material\n-----END RSA PRIVATE KEY-----\nReused the procedure successfully\n";
        let excerpt = safe_learning_excerpt(text, 4_096);
        assert!(excerpt.contains("Verified tests passed"));
        assert!(excerpt.contains("Reused the procedure successfully"));
        for secret in [
            "hunter2",
            "abc123",
            "xyz",
            "session=secret",
            "owner",
            "opaque-key-material",
        ] {
            assert!(!excerpt.contains(secret), "retained {secret}");
        }
        assert!(excerpt.contains("[redacted sensitive line]"));
    }

    #[test]
    fn excerpt_redacts_pgp_private_key_block_through_its_end_marker() {
        let text = "Verified before\n-----BEGIN PGP PRIVATE KEY BLOCK-----\nVersion: GnuPG\n\nlQOYBGSyntheticArmoredPayload\n=checksum\n-----END PGP PRIVATE KEY BLOCK-----\nVerified after\n";
        let excerpt = safe_learning_excerpt(text, 4_096);
        assert!(excerpt.starts_with("Verified before\n"));
        assert!(excerpt.ends_with("Verified after\n"));
        for private_material in ["GnuPG", "lQOYBGSyntheticArmoredPayload", "=checksum"] {
            assert!(
                !excerpt.contains(private_material),
                "retained {private_material}"
            );
        }
    }

    #[test]
    fn excerpt_redacts_hidden_unicode_and_controls_and_preserves_crlf() {
        let text = "first\r\nhidden\u{202e}instruction\nzero\u{200b}width\ncontrol\u{0007}value\nplain café\r\n";
        let excerpt = safe_learning_excerpt(text, 1_024);
        assert!(excerpt.starts_with("first\r\n"));
        assert!(excerpt.ends_with("plain café\r\n"));
        for hidden in ["instruction", "width", "value"] {
            assert!(!excerpt.contains(hidden));
        }
    }

    #[test]
    fn excerpt_enforces_byte_limits_at_utf8_boundaries() {
        assert_eq!(safe_learning_excerpt("café ✓", 4), "caf");
        assert_eq!(safe_learning_excerpt("café ✓", 5), "café");
        assert_eq!(safe_learning_excerpt("ordinary", 0), "");
        for bound in 0..40 {
            assert!(safe_learning_excerpt("password=secret\nVerified ✓", bound).len() <= bound);
        }
    }

    #[test]
    fn experiences_deduplicate_per_agent_run_and_version() {
        let first = experience("a", "run", "v1", 10);
        let mut entries = vec![];
        record_experience(&mut entries, first.clone());
        record_experience(&mut entries, experience("a", "run", "v1", 20));
        record_experience(&mut entries, experience("a", "run", "v2", 30));
        record_experience(&mut entries, experience("b", "run", "v1", 40));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], first);
    }

    #[test]
    fn experiences_drop_oldest_records_to_stay_bounded() {
        let mut entries = vec![];
        for at_ms in (0..65).rev() {
            record_experience(
                &mut entries,
                experience("a", &at_ms.to_string(), "v1", at_ms),
            );
        }
        assert_eq!(entries.len(), MAX_SKILL_EXPERIENCES);
        assert!(!entries.iter().any(|entry| entry.loaded_at_ms == 0));
        assert!(entries.iter().any(|entry| entry.loaded_at_ms == 64));
    }

    #[test]
    fn settlement_updates_only_matching_loaded_experiences() {
        let mut entries = vec![
            experience("a", "run", "v1", 10),
            experience("a", "run", "v2", 20),
            experience("b", "run", "v1", 30),
            experience("a", "other", "v1", 40),
        ];
        entries[1].status = SkillUseStatus::Failed;
        entries[1].finished_at_ms = Some(25);
        settle_experience(&mut entries, "a", "run", SkillUseStatus::Completed, 50);
        assert_eq!(entries[0].status, SkillUseStatus::Completed);
        assert_eq!(entries[0].finished_at_ms, Some(50));
        assert_eq!(entries[1].status, SkillUseStatus::Failed);
        assert_eq!(entries[1].finished_at_ms, Some(25));
        assert_eq!(entries[2].status, SkillUseStatus::Loaded);
        assert_eq!(entries[3].status, SkillUseStatus::Loaded);
    }

    #[test]
    fn loaded_is_not_a_settlement_and_finish_time_cannot_precede_load() {
        let mut entries = vec![experience("a", "run", "v1", 20)];
        settle_experience(&mut entries, "a", "run", SkillUseStatus::Loaded, 30);
        assert_eq!(entries[0].finished_at_ms, None);
        settle_experience(&mut entries, "a", "run", SkillUseStatus::Cancelled, 10);
        assert_eq!(entries[0].finished_at_ms, Some(20));
    }

    #[test]
    fn experience_json_uses_camel_case_and_round_trips() {
        let entry = experience("a", "run", "v1", 10);
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["agentId"], "a");
        assert_eq!(json["loadedAtMs"], 10);
        assert_eq!(json["status"], "loaded");
        assert_eq!(
            serde_json::from_value::<SkillExperience>(json).unwrap(),
            entry
        );
        assert_eq!(SkillUseStatus::default(), SkillUseStatus::Loaded);
    }
}
