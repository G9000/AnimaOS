//! Advisory for one bounded streak of fresh, exact, typed tool failures.
//! No raw failure evidence or state survives the current invocation.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::capability::{
    MAX_CAPABILITY_ARGUMENT_BYTES, MAX_CAPABILITY_ARGUMENT_DEPTH, MAX_CAPABILITY_ARGUMENT_NODES,
    MAX_CAPABILITY_ID_BYTES,
};
use crate::model::ToolCall;
use crate::primitives::{Content, DataValue, TaskResult, TaskStatus};

pub(super) const EXACT_FAILURE_HINT: &str = "This same tool call has returned the same error three times. Inspect the latest error, change the inputs or approach, or explain the blocker.";

#[derive(Default)]
pub(super) struct ExactFailureStreak {
    signature: Option<[u8; 32]>,
    count: u8,
    warned: bool,
}

impl ExactFailureStreak {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn observe(
        &mut self,
        call: &ToolCall,
        result: &TaskResult<Content>,
        recovered: bool,
    ) -> bool {
        let Some(signature) = (!recovered).then(|| fingerprint(call, result)).flatten() else {
            self.reset();
            return false;
        };
        if self.signature == Some(signature) {
            self.count = self.count.saturating_add(1).min(3);
        } else {
            self.signature = Some(signature);
            self.count = 1;
            self.warned = false;
        }
        if self.count == 3 && !self.warned {
            self.warned = true;
            return true;
        }
        false
    }
}

fn fingerprint(call: &ToolCall, result: &TaskResult<Content>) -> Option<[u8; 32]> {
    if result.status != TaskStatus::Error || result.data.is_some() {
        return None;
    }
    let error = result.error.as_deref()?;
    if call.name.len() > MAX_CAPABILITY_ID_BYTES || error.len() > MAX_CAPABILITY_ARGUMENT_BYTES {
        return None;
    }
    if call.name.trim().is_empty() || error.trim().is_empty() {
        return None;
    }
    let arguments = bounded_arguments(&call.args)?;
    // A typed result must also be representable. Its duration is deliberately
    // excluded from identity, while the exact error remains part of it.
    let encoded_result = serde_jcs::to_vec(result).ok()?;
    if encoded_result.len() > MAX_CAPABILITY_ARGUMENT_BYTES {
        return None;
    }
    let mut digest = Sha256::new();
    for bytes in [call.name.as_bytes(), arguments.as_slice(), error.as_bytes()] {
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Some(digest.finalize().into())
}

/// Shared private validation: bounded finite canonical arguments. Uncertainty
/// suppresses optional guidance/recovery, never changes the original arguments.
pub(super) fn bounded_arguments(args: &BTreeMap<String, DataValue>) -> Option<Vec<u8>> {
    let mut nodes = 0usize;
    let mut bytes = 0usize;
    for (key, value) in args {
        bytes = bytes.saturating_add(key.len().saturating_mul(6));
        if !bounded_value(value, 1, &mut nodes, &mut bytes) {
            return None;
        }
    }
    let arguments = serde_jcs::to_vec(args).ok()?;
    if arguments.len() > MAX_CAPABILITY_ARGUMENT_BYTES {
        return None;
    }
    Some(arguments)
}

fn bounded_value(value: &DataValue, depth: usize, nodes: &mut usize, bytes: &mut usize) -> bool {
    *nodes = nodes.saturating_add(1);
    *bytes = bytes.saturating_add(32);
    if depth > MAX_CAPABILITY_ARGUMENT_DEPTH
        || *nodes > MAX_CAPABILITY_ARGUMENT_NODES
        || *bytes > MAX_CAPABILITY_ARGUMENT_BYTES
    {
        return false;
    }
    match value {
        DataValue::Number(number) => number.is_finite(),
        DataValue::String(text) => {
            *bytes = bytes.saturating_add(text.len().saturating_mul(6));
            *bytes <= MAX_CAPABILITY_ARGUMENT_BYTES
        }
        DataValue::Array(values) => values
            .iter()
            .all(|value| bounded_value(value, depth + 1, nodes, bytes)),
        DataValue::Object(values) => values.iter().all(|(key, value)| {
            *bytes = bytes.saturating_add(key.len().saturating_mul(6));
            bounded_value(value, depth + 1, nodes, bytes)
        }),
        DataValue::Null | DataValue::Bool(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn call() -> ToolCall {
        ToolCall {
            id: "one".into(),
            name: "custom_poll".into(),
            args: BTreeMap::from([("offset".into(), DataValue::Number(0.0))]),
        }
    }
    fn error() -> TaskResult<Content> {
        TaskResult::error("still unavailable", 1)
    }

    #[test]
    fn exact_failure_streak_warns_once_ignoring_ids_durations_and_canonical_negative_zero() {
        let mut guard = ExactFailureStreak::default();
        for index in 0..10 {
            let mut call = call();
            call.id = format!("attempt-{index}");
            if index % 2 == 0 {
                call.args.insert("offset".into(), DataValue::Number(-0.0));
            }
            assert_eq!(
                guard.observe(&call, &TaskResult::error("still unavailable", index), false),
                index == 2
            );
        }
        let mut changed = call();
        changed.name = "another_unclassified_tool".into();
        assert!(!guard.observe(&changed, &error(), false));
        assert!(!guard.observe(&changed, &error(), false));
        assert!(
            guard.observe(&changed, &error(), false),
            "a new exact streak may warn once again"
        );
    }

    #[test]
    fn exact_failure_streak_resets_on_malformed_oversized_invalid_and_recovered_evidence() {
        let mut inconsistent = error();
        inconsistent.status = TaskStatus::Success;
        let mut missing = error();
        missing.error = None;
        let mut data = error();
        data.data = Some(Content::default());
        let mut invalid_args = call();
        invalid_args
            .args
            .insert("offset".into(), DataValue::Number(f64::NAN));
        let mut huge_args = call();
        huge_args.args.insert(
            "payload".into(),
            DataValue::String("x".repeat(MAX_CAPABILITY_ARGUMENT_BYTES + 1)),
        );
        let mut huge_name = call();
        huge_name.name = "x".repeat(MAX_CAPABILITY_ID_BYTES + 1);
        for (invalid_call, invalid_result, recovered) in [
            (call(), TaskResult::success(Content::default(), 1), false),
            (call(), inconsistent, false),
            (call(), missing, false),
            (call(), data, false),
            (call(), TaskResult::error(" \n\t", 1), false),
            (
                call(),
                TaskResult::error("x".repeat(MAX_CAPABILITY_ARGUMENT_BYTES + 1), 1),
                false,
            ),
            (
                call(),
                TaskResult::error("\0".repeat(MAX_CAPABILITY_ARGUMENT_BYTES / 6 + 1), 1),
                false,
            ),
            (invalid_args, error(), false),
            (huge_args, error(), false),
            (huge_name, error(), false),
            (call(), error(), true),
        ] {
            let mut guard = ExactFailureStreak::default();
            assert!(!guard.observe(&call(), &error(), false));
            assert!(!guard.observe(&call(), &error(), false));
            assert!(!guard.observe(&invalid_call, &invalid_result, recovered));
            assert!(!guard.observe(&call(), &error(), false));
            assert!(!guard.observe(&call(), &error(), false));
            assert!(guard.observe(&call(), &error(), false));
        }
    }

    #[test]
    fn exact_failure_streak_changed_arguments_or_error_start_a_new_single_count() {
        let mut changed_args = call();
        changed_args
            .args
            .insert("offset".into(), DataValue::Number(1.0));
        for (next_call, next_error) in [
            (changed_args, error()),
            (call(), TaskResult::error("different exact error", 99)),
        ] {
            let mut guard = ExactFailureStreak::default();
            assert!(!guard.observe(&call(), &error(), false));
            assert!(!guard.observe(&call(), &error(), false));
            assert!(!guard.observe(&next_call, &next_error, false));
            assert!(!guard.observe(&next_call, &next_error, false));
            assert!(guard.observe(&next_call, &next_error, false));
        }
    }
}
