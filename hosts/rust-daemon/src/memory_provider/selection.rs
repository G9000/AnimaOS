//! Bounded model-authored search inputs. Natural-language interpretation belongs to the model.
use anima_memory::ExternalMemoryError;
use serde_json::Value;
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

pub(super) type Rank = (u8, usize, u8);

pub(crate) struct RecallPlan {
    pub(super) query: String,
    pub(super) mode: String,
    pub(super) kind: Option<String>,
    exact_names: Vec<String>,
    terms: BTreeSet<String>,
}

fn normalized(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).collect()
}

fn words(text: &str) -> BTreeSet<String> {
    normalized(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(128)
        .map(str::to_owned)
        .collect()
}

fn continuous_script(c: char) -> bool {
    matches!(c as u32, 0x2e80..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0x20000..=0x323af)
}

fn contains_name(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(start, value)| {
        let before = text[..start].chars().next_back();
        let after = text[start + value.len()..].chars().next();
        (name.chars().next().is_some_and(continuous_script)
            || before.is_none_or(|c| !c.is_alphanumeric() || continuous_script(c)))
            && (name.chars().next_back().is_some_and(continuous_script)
                || after.is_none_or(|c| !c.is_alphanumeric() || continuous_script(c)))
    })
}

pub(super) fn bounded_query(text: &str) -> String {
    let mut units = 0;
    text.trim()
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 200
        })
        .collect()
}

impl RecallPlan {
    /// Compatibility for the provider trait: preserve the query, never guess its intent.
    pub(crate) fn new(text: &str) -> Self {
        let query = bounded_query(text);
        Self {
            terms: words(&query),
            query,
            mode: "hybrid".into(),
            kind: None,
            exact_names: vec![],
        }
    }

    pub(crate) fn search(
        query: &str,
        mode: &str,
        kind: Option<&str>,
        names: &[String],
    ) -> Result<Self, ExternalMemoryError> {
        let valid_text = |text: &str| {
            !text.trim().is_empty()
                && text.encode_utf16().count() <= 200
                && !crate::memory_text::has_hidden_text(text)
        };
        if !valid_text(query)
            || !matches!(mode, "literal" | "hybrid" | "semantic")
            || kind.is_some_and(|k| {
                !matches!(
                    k,
                    "memory" | "decision" | "task" | "checkpoint" | "artifact" | "action"
                )
            })
            || names.len() > 2
            || names.iter().any(|n| !valid_text(n))
        {
            return Err(ExternalMemoryError::new("Invalid shared search: use a 1–200 UTF-16-unit query, supported mode/category and at most two exact names"));
        }
        let mut plan = Self::new(query);
        plan.mode = mode.into();
        plan.kind = kind.map(str::to_owned);
        plan.exact_names = names.iter().map(|n| normalized(n.trim())).collect();
        Ok(plan)
    }

    pub(super) fn inventory_only(&self) -> bool {
        self.query.is_empty()
    }

    pub(super) fn rank(&self, record: &Value, _literal_hit: bool) -> Rank {
        if self.mode != "literal" {
            return (0, 0, 0);
        }
        let title = record["title"].as_str().unwrap_or_default();
        let body = record["content"]
            .as_str()
            .or_else(|| record["excerpt"].as_str())
            .unwrap_or_default();
        let title_names = self
            .exact_names
            .iter()
            .any(|n| contains_name(&normalized(title), n));
        let body_names = self
            .exact_names
            .iter()
            .any(|n| contains_name(&normalized(body), n));
        let terms = words(&format!("{title} {body}"));
        // Preserve learned/provider ordering across languages. Only an explicitly
        // literal search uses token overlap to order substring distractors.
        (
            u8::from(title_names || body_names),
            if self.mode == "literal" {
                self.terms.intersection(&terms).count()
            } else {
                0
            },
            u8::from(title_names),
        )
    }

    pub(super) fn matches_current(&self, record: &Value) -> bool {
        self.exact_names.is_empty()
            || self.exact_names.iter().any(|name| {
                ["title", "content"].iter().any(|key| {
                    contains_name(&normalized(record[key].as_str().unwrap_or_default()), name)
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn multilingual_recall_preserves_model_queries_without_english_rules() {
        for q in [
            "my cat ID",
            "我的猫叫什么？ cite revision",
            "猫の名前を教えて",
            "nama kucing saya",
            "API MCP JSON",
            "the current record",
        ] {
            let p = RecallPlan::search(q, "semantic", Some("memory"), &[]).unwrap();
            assert_eq!(p.query, q);
            assert_eq!(p.mode, "semantic");
            assert!(
                p.matches_current(&json!({"title":"Household pets","content":"My cat is Vega."})),
                "semantic results need not share query words"
            );
        }
    }

    #[test]
    fn multilingual_recall_exact_names_normalize_and_recheck_complete_records() {
        for (name, right, wrong) in [
            ("AsterOS", "ASTEROS-kit", "AsterOS2"),
            ("ÄsterOS", "A\u{308}STEROS", "AsterOS"),
            ("寧寧", "我家的寧寧喜歡曬太陽", "我家的花花"),
            ("ミケ", "猫のミケは眠い", "猫のタマ"),
        ] {
            let p = RecallPlan::search("mixed search", "hybrid", None, &[name.into()]).unwrap();
            assert!(
                p.matches_current(&json!({"content":right})),
                "{name}: {right}"
            );
            assert!(
                !p.matches_current(&json!({"content":wrong})),
                "{name}: {wrong}"
            );
        }
    }

    #[test]
    fn multilingual_recall_rejects_invalid_model_searches_without_truncating() {
        assert_eq!(bounded_query(&"🪴".repeat(110)).encode_utf16().count(), 200);
        for q in [
            String::new(),
            " ".into(),
            "🪴".repeat(101),
            "hidden\u{202e}".into(),
        ] {
            assert!(RecallPlan::search(&q, "hybrid", None, &[]).is_err());
        }
        assert!(RecallPlan::search("x", "invented", None, &[]).is_err());
        assert!(RecallPlan::search("x", "literal", Some("private"), &[]).is_err());
        assert!(
            RecallPlan::search("x", "literal", None, &["a".into(), "b".into(), "c".into()])
                .is_err()
        );
    }
}
