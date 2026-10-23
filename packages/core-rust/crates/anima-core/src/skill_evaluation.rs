//! Deterministic response-only skill evaluation. An eligible report never
//! supplies approval, filesystem evidence, or tool authority.
use serde::{Deserialize, Serialize};

pub const GRADER_VERSION: &str = "response-oracle-v1";
pub const MAX_FIXTURES: usize = 16;
pub const MAX_FIXTURE_BYTES: usize = 8192;
pub const MAX_RESPONSE_BYTES: usize = 16384;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluationSuite {
    pub enforced: bool,
    pub agent_id: String,
    pub fixtures: Vec<EvaluationFixture>,
    pub min_improvement: u32,
    pub trials_per_fixture: u32,
    pub max_output_tokens: u32,
    pub trial_timeout_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluationFixture {
    pub id: String,
    pub heldout: bool,
    pub required: bool,
    pub prompt: String,
    pub oracle: EvaluationOracle,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "expected",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum EvaluationOracle {
    ExactText(String),
    CanonicalJson(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrialVariant {
    Baseline,
    Candidate,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationTrial {
    pub fixture_id: String,
    pub repetition: u32,
    pub variant: TrialVariant,
    pub response: Option<String>,
    pub error: Option<String>,
    pub usage: crate::TokenUsage,
    #[serde(default)]
    pub usage_reported: bool,
    #[serde(default)]
    pub usage_complete: bool,
    pub duration_ms: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvaluationVerdict {
    Passed,
    Failed,
    Inconclusive,
}

impl EvaluationSuite {
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut ids = std::collections::BTreeSet::new();
        if self.agent_id.is_empty()
            || self.agent_id.len() > 128
            || !(2..=MAX_FIXTURES).contains(&self.fixtures.len())
            || !(1..=3).contains(&self.trials_per_fixture)
            || !(1..=4096).contains(&self.max_output_tokens)
            || !(1000..=30_000).contains(&self.trial_timeout_ms)
            || self.min_improvement > self.fixtures.len() as u32 * self.trials_per_fixture
            || !self.fixtures.iter().any(|f| f.heldout)
            || !self.fixtures.iter().any(|f| !f.heldout)
        {
            return Err("Evaluation needs bounded train and heldout fixtures, trials, tokens, and deadlines");
        }
        for fixture in &self.fixtures {
            if fixture.id.is_empty()
                || fixture.id.len() > 64
                || !ids.insert(&fixture.id)
                || fixture.prompt.trim().is_empty()
                || fixture.prompt.len() > MAX_FIXTURE_BYTES
            {
                return Err(
                    "Evaluation fixture ids must be unique and prompts must be 1..8192 bytes",
                );
            }
            let expected = match &fixture.oracle {
                EvaluationOracle::ExactText(s) | EvaluationOracle::CanonicalJson(s) => s,
            };
            if expected.len() > MAX_RESPONSE_BYTES {
                return Err("Evaluation oracle exceeds 16384 bytes");
            }
            if matches!(fixture.oracle, EvaluationOracle::CanonicalJson(_))
                && serde_json::from_str::<serde_json::Value>(expected).is_err()
            {
                return Err("Canonical JSON oracle must contain one valid JSON value");
            }
        }
        Ok(())
    }
    pub fn digest(&self) -> String {
        digest(self)
    }
}
impl EvaluationOracle {
    pub fn matches(&self, response: &str) -> bool {
        match self {
            Self::ExactText(expected) => expected == response,
            Self::CanonicalJson(expected) => match (
                serde_json::from_str::<serde_json::Value>(expected),
                serde_json::from_str::<serde_json::Value>(response),
            ) {
                (Ok(expected), Ok(actual)) => {
                    match (serde_jcs::to_vec(&expected), serde_jcs::to_vec(&actual)) {
                        (Ok(expected), Ok(actual)) => expected == actual,
                        _ => false,
                    }
                }
                _ => false,
            },
        }
    }
}
/// Canonical JSON digest used for durable suite/configuration identities.
pub fn digest(value: &impl Serialize) -> String {
    use sha2::{Digest, Sha256};
    // All caller types are data-only serializable records. Failure cannot
    // produce a reusable identity: a distinct invalid marker is returned.
    serde_jcs::to_vec(value)
        .map(|bytes| hex::encode(Sha256::digest(bytes)))
        .unwrap_or_else(|_| "invalid-canonical-json".into())
}
pub fn grade(suite: &EvaluationSuite, trials: &[EvaluationTrial]) -> EvaluationVerdict {
    if suite.validate().is_err()
        || trials.len() != suite.fixtures.len() * suite.trials_per_fixture as usize * 2
    {
        return EvaluationVerdict::Inconclusive;
    }
    let mut baseline_passes = 0u32;
    let mut candidate_passes = 0u32;
    let mut regressed = false;
    for fixture in &suite.fixtures {
        for repetition in 0..suite.trials_per_fixture {
            let mut pass = [false; 2];
            for (index, variant) in [TrialVariant::Baseline, TrialVariant::Candidate]
                .into_iter()
                .enumerate()
            {
                let matching: Vec<_> = trials
                    .iter()
                    .filter(|t| {
                        t.fixture_id == fixture.id
                            && t.repetition == repetition
                            && t.variant == variant
                    })
                    .collect();
                if matching.len() != 1 {
                    return EvaluationVerdict::Inconclusive;
                }
                let trial = matching[0];
                let Some(response) = trial
                    .response
                    .as_deref()
                    .filter(|text| text.len() <= MAX_RESPONSE_BYTES)
                else {
                    return EvaluationVerdict::Inconclusive;
                };
                if trial.error.is_some() || trial.duration_ms > suite.trial_timeout_ms {
                    return EvaluationVerdict::Inconclusive;
                }
                pass[index] = fixture.oracle.matches(response);
            }
            baseline_passes += u32::from(pass[0]);
            candidate_passes += u32::from(pass[1]);
            regressed |= (fixture.required && pass[0] && !pass[1]) || (fixture.heldout && !pass[1]);
        }
    }
    if regressed || candidate_passes < baseline_passes.saturating_add(suite.min_improvement) {
        EvaluationVerdict::Failed
    } else {
        EvaluationVerdict::Passed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn suite() -> EvaluationSuite {
        EvaluationSuite {
            enforced: true,
            agent_id: "agent".into(),
            fixtures: vec![
                EvaluationFixture {
                    id: "train".into(),
                    heldout: false,
                    required: true,
                    prompt: "training question".into(),
                    oracle: EvaluationOracle::ExactText("yes".into()),
                },
                EvaluationFixture {
                    id: "heldout".into(),
                    heldout: true,
                    required: true,
                    prompt: "unseen question".into(),
                    oracle: EvaluationOracle::CanonicalJson("{\"a\":1,\"b\":2}".into()),
                },
            ],
            min_improvement: 1,
            trials_per_fixture: 1,
            max_output_tokens: 1024,
            trial_timeout_ms: 1000,
        }
    }
    fn trials() -> Vec<EvaluationTrial> {
        vec![
            ("train", TrialVariant::Baseline, "no"),
            ("train", TrialVariant::Candidate, "yes"),
            ("heldout", TrialVariant::Baseline, "null"),
            ("heldout", TrialVariant::Candidate, "{\"b\":2,\"a\":1}"),
        ]
        .into_iter()
        .map(|(id, variant, text)| EvaluationTrial {
            fixture_id: id.into(),
            repetition: 0,
            variant,
            response: Some(text.into()),
            error: None,
            usage: crate::TokenUsage::default(),
            usage_reported: false,
            usage_complete: false,
            duration_ms: 1,
        })
        .collect()
    }
    #[test]
    fn skill_evaluation_requires_train_and_heldout_and_valid_bounds() {
        let mut s = suite();
        assert_eq!(s.validate(), Ok(()));
        s.fixtures[1].heldout = false;
        assert!(s.validate().is_err());
        let mut s = suite();
        s.trials_per_fixture = 0;
        assert!(s.validate().is_err());
        let mut s = suite();
        s.fixtures[1].id = "train".into();
        assert!(s.validate().is_err());
    }
    #[test]
    fn skill_evaluation_oracles_use_exact_text_and_canonical_json() {
        assert!(EvaluationOracle::ExactText("yes".into()).matches("yes"));
        assert!(!EvaluationOracle::ExactText("yes".into()).matches("yes\n"));
        assert!(EvaluationOracle::CanonicalJson("{\"a\":1,\"b\":2}".into())
            .matches("{\"b\":2, \"a\":1}"));
        assert!(!EvaluationOracle::CanonicalJson("{}".into()).matches("```json\n{}\n```"));
    }
    #[test]
    fn skill_evaluation_requires_complete_error_free_trials() {
        assert_eq!(grade(&suite(), &trials()), EvaluationVerdict::Passed);
        assert_eq!(
            grade(&suite(), &trials()[1..]),
            EvaluationVerdict::Inconclusive
        );
        let mut t = trials();
        t[0].error = Some("provider unavailable".into());
        assert_eq!(grade(&suite(), &t), EvaluationVerdict::Inconclusive);
        let mut t = trials();
        t.push(t[0].clone());
        assert_eq!(grade(&suite(), &t), EvaluationVerdict::Inconclusive);
    }
    #[test]
    fn skill_evaluation_rejects_regression_heldout_failure_and_no_improvement() {
        let mut t = trials();
        t[3].response = Some("null".into());
        assert_eq!(grade(&suite(), &t), EvaluationVerdict::Failed);
        let mut t = trials();
        t[0].response = Some("yes".into());
        t[1].response = Some("no".into());
        assert_eq!(grade(&suite(), &t), EvaluationVerdict::Failed);
        let mut t = trials();
        t[0].response = Some("yes".into());
        t[2].response = t[3].response.clone();
        assert_eq!(grade(&suite(), &t), EvaluationVerdict::Failed);
    }
    #[test]
    fn skill_evaluation_digest_binds_every_owner_setting_and_oracle() {
        let s = suite();
        let mut edited = s.clone();
        edited.fixtures[1].oracle = EvaluationOracle::ExactText("secret new answer".into());
        assert_ne!(s.digest(), edited.digest());
    }
}
