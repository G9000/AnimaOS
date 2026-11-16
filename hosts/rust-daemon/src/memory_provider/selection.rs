//! Bounded query planning and topic checks owned by the harness, not the garden.
use serde_json::Value;
use std::collections::BTreeSet;

/// Inspect enough of the request to find its subject before bounding wire queries.
const MAX_INPUT_CHARS: usize = 4_096;
const MAX_TERMS: usize = 128;
const MAX_ANCHORS: usize = 2;

pub(super) type Rank = (u8, usize, u8);

pub(super) struct RecallPlan {
    pub(super) literal_queries: Vec<String>,
    pub(super) hybrid_query: String,
    anchors: Vec<Vec<String>>,
    terms: BTreeSet<String>,
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn topic_word(word: &str) -> bool {
    // Grammar and recall-protocol words do not identify a topic.
    !matches!(
        word,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "can"
            | "could"
            | "do"
            | "does"
            | "for"
            | "from"
            | "how"
            | "i"
            | "in"
            | "is"
            | "it"
            | "me"
            | "my"
            | "of"
            | "on"
            | "or"
            | "please"
            | "tell"
            | "that"
            | "the"
            | "this"
            | "to"
            | "was"
            | "what"
            | "when"
            | "where"
            | "which"
            | "who"
            | "why"
            | "will"
            | "with"
            | "would"
            | "you"
            | "your"
            | "read"
            | "only"
            | "recall"
            | "diagnostic"
            | "fresh"
            | "chat"
            | "find"
            | "appropriate"
            | "tool"
            | "tools"
            | "yourself"
            | "using"
            | "ordinary"
            | "discovery"
            | "check"
            | "shared"
            | "memory"
            | "memories"
            | "about"
            | "report"
            | "record"
            | "records"
            | "checkpoint"
            | "current"
            | "complete"
            | "actual"
            | "actually"
            | "result"
            | "mother"
            | "use"
            | "search"
            | "evidence"
    )
}

fn mixed_identifier(word: &str) -> bool {
    let mut lower = false;
    let mut internal_upper = false;
    for c in word.chars() {
        if c.is_uppercase() {
            internal_upper |= lower;
        }
        lower |= c.is_lowercase();
    }
    internal_upper
}

fn acronym(word: &str) -> bool {
    !word.chars().any(char::is_lowercase) && word.chars().filter(|c| c.is_uppercase()).count() >= 2
}

fn contains(text: &[String], anchor: &[String]) -> bool {
    !anchor.is_empty() && text.windows(anchor.len()).any(|window| window == anchor)
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
    pub(super) fn new(text: &str) -> Self {
        let input: String = text.chars().take(MAX_INPUT_CHARS).collect();
        let tokens: Vec<_> = input
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.chars().count() >= 2)
            .collect();
        let mut names: Vec<String> = input
            .split('"')
            .enumerate()
            .filter(|(index, _)| index % 2 == 1)
            .map(|(_, value)| value.trim())
            .filter(|value| {
                let words = words(value);
                !words.is_empty() && words.len() <= 4
            })
            .map(str::to_owned)
            .collect();
        names.extend(
            tokens
                .iter()
                .filter(|word| topic_word(&word.to_lowercase()) && mixed_identifier(word))
                .map(|word| (*word).to_owned()),
        );
        if names.is_empty() {
            // Bare acronyms may be protocol hints rather than the requested topic.
            names.extend(
                tokens
                    .iter()
                    .filter(|word| topic_word(&word.to_lowercase()) && acronym(word))
                    .map(|word| (*word).to_owned()),
            );
        }
        let mut seen = BTreeSet::new();
        let literal_queries: Vec<_> = names
            .into_iter()
            .filter(|name| name.encode_utf16().count() <= 200)
            .filter(|name| seen.insert(words(name)))
            .take(MAX_ANCHORS)
            .collect();
        let anchors = literal_queries.iter().map(|name| words(name)).collect();
        let mut terms = BTreeSet::new();
        for word in words(&input).into_iter().filter(|word| topic_word(word)) {
            terms.insert(word);
            if terms.len() == MAX_TERMS {
                break;
            }
        }
        let mut plan = Self {
            literal_queries,
            hybrid_query: bounded_query(&input),
            anchors,
            terms,
        };
        if plan.literal_queries.is_empty() {
            // Sentence capitalization alone is only a query hint, not a hard
            // entity constraint that could reject otherwise relevant evidence.
            if let Some(name) = tokens.iter().find(|word| {
                word.chars().next().is_some_and(char::is_uppercase)
                    && topic_word(&word.to_lowercase())
            }) {
                plan.literal_queries.push(bounded_query(name));
            }
        }
        if plan.literal_queries.is_empty() {
            let topics: Vec<_> = tokens
                .iter()
                .filter(|word| topic_word(&word.to_lowercase()))
                .collect();
            if topics.len() <= 4 {
                if let Some(topic) = topics.first() {
                    plan.literal_queries.push(bounded_query(topic));
                }
            }
        }
        plan
    }

    pub(super) fn rank(&self, record: &Value, literal_hit: bool) -> Rank {
        let title = words(record["title"].as_str().unwrap_or_default());
        let body = words(
            record["content"]
                .as_str()
                .or_else(|| record["excerpt"].as_str())
                .unwrap_or_default(),
        );
        let title_anchor = self.anchors.iter().any(|anchor| contains(&title, anchor));
        let body_anchor = self.anchors.iter().any(|anchor| contains(&body, anchor));
        let evidence = if title_anchor || body_anchor {
            2
        } else {
            u8::from(literal_hit)
        };
        let matches: BTreeSet<_> = title
            .iter()
            .chain(&body)
            .filter(|word| self.terms.contains(*word))
            .collect();
        (evidence, matches.len(), u8::from(title_anchor))
    }

    pub(super) fn matches_current(&self, record: &Value) -> bool {
        if self.anchors.is_empty() {
            return true;
        }
        let title = words(record["title"].as_str().unwrap_or_default());
        let body = words(record["content"].as_str().unwrap_or_default());
        self.anchors
            .iter()
            .any(|anchor| contains(&title, anchor) || contains(&body, anchor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn memory_provider_selection_topic_boundaries_and_unicode_are_consistent() {
        let plan = RecallPlan::new("Check AsterOS native startup");
        for title in ["ASTEROS startup", "AsterOS-kit", "AsterOS_kit", "(AsterOS)"] {
            assert!(
                plan.matches_current(&json!({"title":title,"content":""})),
                "{title}"
            );
        }
        for title in ["Aster startup", "AsterOS2 startup", "SuperAsterOS startup"] {
            assert!(
                !plan.matches_current(&json!({"title":title,"content":""})),
                "{title}"
            );
        }
        let unicode = RecallPlan::new("Check ÄsterOS native startup");
        assert!(unicode.matches_current(&json!({"title":"ÄSTEROS","content":""})));
        assert!(!unicode.matches_current(&json!({"title":"AsterOS","content":""})));
    }

    #[test]
    fn memory_provider_selection_accepts_either_explicit_comparison_topic() {
        let plan = RecallPlan::new("Compare AsterOS and BerylOS native startup");
        assert_eq!(plan.literal_queries, ["AsterOS", "BerylOS"]);
        for title in ["AsterOS native startup", "BerylOS native startup"] {
            assert!(plan.matches_current(&json!({"title":title,"content":""})));
        }
        assert!(!plan.matches_current(&json!({"title":"OtherProject","content":"native startup"})));
    }

    #[test]
    fn memory_provider_selection_wire_queries_and_total_work_stay_bounded() {
        let plan = RecallPlan::new(&format!(
            "{} AsterOS BerylOS ThirdProject",
            "🪴 ".repeat(110)
        ));
        assert_eq!(plan.hybrid_query.encode_utf16().count(), 200);
        assert_eq!(plan.literal_queries, ["AsterOS", "BerylOS"]);
        assert!(plan
            .literal_queries
            .iter()
            .all(|query| query.encode_utf16().count() <= 200));
        let plan = RecallPlan::new("Read Mother shared memories about native login startup");
        assert!(
            plan.anchors.is_empty(),
            "ordinary sentence words are not named-project constraints"
        );
    }
}
