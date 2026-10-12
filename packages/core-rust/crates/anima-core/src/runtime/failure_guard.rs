//! Advisories for bounded streaks of fresh, exact, typed tool results.
//! No raw evidence or streak state survives the current invocation.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::capability::{
    MAX_CAPABILITY_ARGUMENT_BYTES, MAX_CAPABILITY_ARGUMENT_DEPTH, MAX_CAPABILITY_ARGUMENT_NODES,
    MAX_CAPABILITY_ID_BYTES,
};
use crate::model::ToolCall;
use crate::primitives::{Content, DataValue, TaskResult, TaskStatus};

pub(super) const EXACT_FAILURE_HINT: &str = "This same tool call has returned the same error three times. Inspect the latest error, change the inputs or approach, or explain the blocker.";
pub(super) const REPEATED_SUCCESS_HINT: &str = "This same tool call has returned the same result three times. If that result is sufficient, use it; otherwise change the inputs or approach, or continue necessary polling.";
pub(super) const REPEATED_TOOL_CYCLE_HINT: &str = "The same sequence of tool calls has returned the same results three times. If those results are sufficient, use them; otherwise change the inputs or approach, or continue necessary polling.";

#[derive(Default)]
pub(super) struct SuccessfulToolCycle {
    history: Vec<[u8; 32]>,
    warned: bool,
}

impl SuccessfulToolCycle {
    pub(super) fn reset_history(&mut self) {
        self.history.clear();
    }

    pub(super) fn observe(
        &mut self,
        call: &ToolCall,
        result: &TaskResult<Content>,
        recovered: bool,
    ) -> bool {
        let Some(signature) = (!recovered)
            .then(|| success_fingerprint(call, result))
            .flatten()
        else {
            self.reset_history();
            return false;
        };
        if self.history.len() == 12 {
            self.history.remove(0);
        }
        self.history.push(signature);
        if !self.warned && repeated_period(&self.history).is_some() {
            self.warned = true;
            return true;
        }
        false
    }
}

fn repeated_period(history: &[[u8; 32]]) -> Option<usize> {
    for period in 2..=4 {
        let Some(start) = history.len().checked_sub(period * 3) else {
            continue;
        };
        let candidate = &history[start..];
        // A constant sequence belongs to M27, regardless of its length.
        if candidate[..period].iter().all(|item| item == &candidate[0]) {
            continue;
        }
        if candidate[period..]
            .iter()
            .enumerate()
            .all(|(index, item)| item == &candidate[index % period])
        {
            return Some(period);
        }
    }
    None
}

#[derive(Default)]
pub(super) struct SuccessfulResultStreak {
    signature: Option<[u8; 32]>,
    count: u8,
    warned: bool,
}

impl SuccessfulResultStreak {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn observe(
        &mut self,
        call: &ToolCall,
        result: &TaskResult<Content>,
        recovered: bool,
    ) -> bool {
        let Some(signature) = (!recovered)
            .then(|| success_fingerprint(call, result))
            .flatten()
        else {
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

fn success_fingerprint(call: &ToolCall, result: &TaskResult<Content>) -> Option<[u8; 32]> {
    if result.status != TaskStatus::Success || result.error.is_some() {
        return None;
    }
    let content = result.data.as_ref()?;
    // Check byte bounds before scanning or allocating; never serialize Content
    // or infer stable hidden evidence from its visible text.
    if call.name.len() > MAX_CAPABILITY_ID_BYTES
        || content.text.len() > MAX_CAPABILITY_ARGUMENT_BYTES
        || call.name.trim().is_empty()
        || content.text.trim().is_empty()
        || content
            .attachments
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        || content
            .metadata
            .as_ref()
            .is_some_and(|items| !items.is_empty())
    {
        return None;
    }
    let arguments = bounded_arguments(&call.args)?;
    let mut digest = Sha256::new();
    digest.update(b"anima/repeated-success/v1\0");
    for bytes in [
        call.name.as_bytes(),
        arguments.as_slice(),
        content.text.as_bytes(),
    ] {
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Some(digest.finalize().into())
}

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

    fn success(text: &str) -> TaskResult<Content> {
        TaskResult::success(
            Content {
                text: text.into(),
                ..Default::default()
            },
            1,
        )
    }

    #[test]
    fn repeated_tool_cycle_matches_three_complete_laps_in_either_phase_and_smallest_period() {
        for period in 2..=4 {
            for rotation in 0..period {
                let history: Vec<_> = (0..period * 3)
                    .map(|index| [((index + rotation) % period) as u8; 32])
                    .collect();
                for length in 0..history.len() {
                    assert_eq!(repeated_period(&history[..length]), None);
                }
                assert_eq!(repeated_period(&history), Some(period));
            }
        }
        assert_eq!(
            repeated_period(&[[7; 32]; 12]),
            None,
            "constant evidence belongs to M27"
        );
        let repeated: Vec<_> = (0..12).map(|index| [(index % 2) as u8; 32]).collect();
        assert_eq!(
            repeated_period(&repeated),
            Some(2),
            "choose the smallest matching period"
        );
        assert_eq!(
            repeated_period(&[[0; 32], [1; 32], [0; 32], [1; 32], [1; 32], [0; 32]]),
            None
        );
    }

    #[test]
    fn repeated_tool_cycle_bounds_history_and_warns_once_even_after_reset_or_changed_cycle() {
        let mut guard = SuccessfulToolCycle::default();
        for index in 0..30 {
            let mut call = call();
            call.id = format!("fresh-{index}");
            let mut result = success(&format!("result-{}", index % 2));
            result.duration_ms = index;
            assert_eq!(guard.observe(&call, &result, false), index == 5);
            assert!(guard.history.len() <= 12);
        }
        assert_eq!(guard.history.len(), 12);
        assert!(!guard.observe(&call(), &error(), false));
        assert!(guard.history.is_empty());
        assert!(guard.warned);
        for index in 0..12 {
            assert!(!guard.observe(
                &call(),
                &success(&format!("different-{}", index % 3)),
                false
            ));
        }
        let mut fresh = SuccessfulToolCycle::default();
        for index in 0..6 {
            assert_eq!(
                fresh.observe(&call(), &success(&format!("result-{}", index % 2)), false),
                index == 5
            );
        }
    }

    #[test]
    fn repeated_tool_cycle_reuses_success_identity_and_resets_ineligible_or_recovered_evidence() {
        let mut metadata = success("result-0");
        metadata.data.as_mut().unwrap().metadata =
            Some(BTreeMap::from([("hidden".into(), DataValue::Null)]));
        let mut invalid_args = call();
        invalid_args
            .args
            .insert("offset".into(), DataValue::Number(f64::NAN));
        for (boundary_call, boundary_result, recovered) in [
            (call(), error(), false),
            (call(), metadata, false),
            (invalid_args, success("result-0"), false),
            (call(), success("result-0"), true),
        ] {
            let mut guard = SuccessfulToolCycle::default();
            for index in 0..5 {
                assert!(!guard.observe(&call(), &success(&format!("result-{}", index % 2)), false));
            }
            assert!(!guard.observe(&boundary_call, &boundary_result, recovered));
            assert!(guard.history.is_empty());
            for index in 0..6 {
                assert_eq!(
                    guard.observe(&call(), &success(&format!("result-{}", index % 2)), false),
                    index == 5
                );
            }
        }
        // M27's full fingerprint defines identity here too; changed arguments,
        // names and text are not allowed to fill an earlier partial lap.
        let mut renamed = call();
        renamed.name = "other_tool".into();
        let mut changed_args = call();
        changed_args
            .args
            .insert("offset".into(), DataValue::Number(1.0));
        for (changed_call, changed_result) in [
            (renamed, success("result-1")),
            (changed_args, success("result-1")),
            (call(), success("result-X")),
        ] {
            let mut guard = SuccessfulToolCycle::default();
            for index in 0..5 {
                assert!(!guard.observe(&call(), &success(&format!("result-{}", index % 2)), false));
            }
            assert!(!guard.observe(&changed_call, &changed_result, false));
        }
    }

    #[test]
    fn repeated_success_result_streak_warns_once_with_canonical_args_and_empty_containers() {
        let mut guard = SuccessfulResultStreak::default();
        for index in 0..10 {
            let mut call = call();
            call.id = format!("fresh-{index}");
            call.args.insert(
                "offset".into(),
                DataValue::Number(if index % 2 == 0 { -0.0 } else { 0.0 }),
            );
            let mut result = success("polling unchanged");
            result.duration_ms = index;
            if index % 2 == 0 {
                result.data.as_mut().unwrap().metadata = Some(BTreeMap::new());
                result.data.as_mut().unwrap().attachments = Some(Vec::new());
            }
            assert_eq!(guard.observe(&call, &result, false), index == 2);
        }
        assert_eq!(guard.count, 3);
        guard.reset();
        assert!(!guard.observe(&call(), &success("polling unchanged"), false));
        assert!(!guard.observe(&call(), &success("polling unchanged"), false));
        assert!(guard.observe(&call(), &success("polling unchanged"), false));
    }

    #[test]
    fn repeated_success_result_streak_compares_full_text_name_and_arguments() {
        let original = format!("{}a{}", "x".repeat(1024), "y".repeat(1024));
        let changed_middle = format!("{}b{}", "x".repeat(1024), "y".repeat(1024));
        let mut changed_name = call();
        changed_name.name = "another_poll".into();
        let mut changed_args = call();
        changed_args
            .args
            .insert("offset".into(), DataValue::Number(1.0));
        for (next_call, next_result) in [
            (call(), success(&changed_middle)),
            (changed_name, success(&original)),
            (changed_args, success(&original)),
        ] {
            let mut guard = SuccessfulResultStreak::default();
            for index in 0..3 {
                assert_eq!(
                    guard.observe(&call(), &success(&original), false),
                    index == 2
                );
            }
            assert!(!guard.observe(&next_call, &next_result, false));
            assert!(!guard.observe(&next_call, &next_result, false));
            assert!(
                guard.observe(&next_call, &next_result, false),
                "changed evidence starts a new exact streak"
            );
        }
    }

    #[test]
    fn repeated_success_result_streak_resets_for_ineligible_and_recovered_evidence() {
        let good = success("unchanged");
        let mut inconsistent = good.clone();
        inconsistent.error = Some(String::new());
        let mut missing = good.clone();
        missing.data = None;
        let mut metadata = good.clone();
        metadata.data.as_mut().unwrap().metadata =
            Some(BTreeMap::from([("hidden".into(), DataValue::Null)]));
        let mut attached = good.clone();
        attached.data.as_mut().unwrap().attachments = Some(vec![crate::primitives::Attachment {
            attachment_type: crate::primitives::AttachmentType::File,
            name: "evidence".into(),
            data: "private".into(),
            mime_type: None,
        }]);
        let mut invalid_args = call();
        invalid_args
            .args
            .insert("offset".into(), DataValue::Number(f64::INFINITY));
        let mut huge_args = call();
        huge_args.args.insert(
            "payload".into(),
            DataValue::String("x".repeat(MAX_CAPABILITY_ARGUMENT_BYTES + 1)),
        );
        let mut deep_args = call();
        let mut nested = DataValue::Null;
        for _ in 0..=MAX_CAPABILITY_ARGUMENT_DEPTH {
            nested = DataValue::Array(vec![nested]);
        }
        deep_args.args.insert("deep".into(), nested);
        let mut huge_name = call();
        huge_name.name = "x".repeat(MAX_CAPABILITY_ID_BYTES + 1);
        let mut empty_name = call();
        empty_name.name = " \t".into();
        for (invalid_call, invalid_result, recovered) in [
            (call(), error(), false),
            (call(), inconsistent, false),
            (call(), missing, false),
            (call(), success(""), false),
            (call(), success(" \n\t"), false),
            (call(), metadata, false),
            (call(), attached, false),
            (
                call(),
                success(&"x".repeat(MAX_CAPABILITY_ARGUMENT_BYTES + 1)),
                false,
            ),
            (invalid_args, good.clone(), false),
            (huge_args, good.clone(), false),
            (deep_args, good.clone(), false),
            (huge_name, good.clone(), false),
            (empty_name, good.clone(), false),
            (call(), good.clone(), true),
        ] {
            let mut guard = SuccessfulResultStreak::default();
            assert!(!guard.observe(&call(), &good, false));
            assert!(!guard.observe(&call(), &good, false));
            assert!(!guard.observe(&invalid_call, &invalid_result, recovered));
            assert!(!guard.observe(&call(), &good, false));
            assert!(!guard.observe(&call(), &good, false));
            assert!(guard.observe(&call(), &good, false));
        }
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
