use crate::tools::workspace_bytes::{InstructionReadError, WorkspaceBytes};
use anima_core::outcomes::{
    OutcomeCriterion, OutcomeDefinition, OutcomeObservation, OutcomeVerdict,
};
use std::path::Path;

const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * MAX_FILE_BYTES;

pub(super) fn observe(
    definition: &OutcomeDefinition,
    root: Option<&Path>,
    response: &str,
) -> Vec<OutcomeObservation> {
    let workspace = root.and_then(|root| WorkspaceBytes::open(root).ok());
    let mut remaining = MAX_TOTAL_BYTES;
    definition.criteria.iter().map(|criterion| {
        let mut observation = OutcomeObservation {
            criterion_id: criterion.id().into(), verdict: OutcomeVerdict::Inconclusive,
            subject_sha256: None, report_sha256: None, locator: None, detail: String::new(),
        };
        if let OutcomeCriterion::ResponseText { contains, .. } = criterion {
            observation.subject_sha256 = Some(super::digest(response.as_bytes()));
            observation.verdict = if response.contains(contains) { OutcomeVerdict::Met } else { OutcomeVerdict::NotMet };
            observation.detail = if observation.verdict == OutcomeVerdict::Met { "Public response requirement met" } else { "The response does not contain the required public text" }.into();
            return observation;
        }
        let Some(path) = criterion.path() else {
            observation.detail = match criterion {
                OutcomeCriterion::Quality { .. } => "An independent quality verifier has not been authorized; quality remains unverified",
                _ => "The bound artifact inspection observation is unavailable; rendering or calculation is unverified",
            }.into();
            return observation;
        };
        let Some(workspace) = &workspace else {
            observation.detail = "The authorized workspace is unavailable".into();
            return observation;
        };
        if crate::tools::credential_paths::check_path(&workspace.canonical_root, path).is_err() {
            observation.detail = "The file is outside the permitted artifact scope".into();
            return observation;
        }
        if remaining < MAX_FILE_BYTES {
            observation.detail = "The bounded observation byte allowance is exhausted".into();
            return observation;
        }
        let bytes = match workspace.read_file_snapshot(path, MAX_FILE_BYTES) {
            Ok(bytes) => bytes,
            Err(InstructionReadError::Missing) => {
                observation.verdict = OutcomeVerdict::NotMet;
                observation.detail = "The required file does not exist".into();
                return observation;
            }
            Err(_) => {
                observation.detail = "The file is unreadable, linked, oversized or changed during observation".into();
                return observation;
            }
        };
        remaining = remaining.saturating_sub(bytes.len());
        let sha = super::digest(&bytes);
        let met = match criterion {
            OutcomeCriterion::FileSha256 { sha256, .. } => sha256 == &sha,
            OutcomeCriterion::FileText { contains, .. } => match std::str::from_utf8(&bytes) {
                Ok(text) => text.contains(contains),
                Err(_) => {
                    observation.detail = "The file is not UTF-8 text".into();
                    return observation;
                }
            },
            OutcomeCriterion::FileJson { pointer, expected, .. } => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(value) => value.pointer(pointer).is_some_and(|actual| actual == expected),
                Err(_) => false,
            },
            _ => false,
        };
        observation.subject_sha256 = Some(sha);
        observation.verdict = if met { OutcomeVerdict::Met } else { OutcomeVerdict::NotMet };
        observation.detail = if met { "Public file requirement met on observed bytes" } else { "The observed file does not meet the public requirement" }.into();
        observation
    }).collect()
}

pub(super) fn same_subjects(left: &[OutcomeObservation], right: &[OutcomeObservation]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            a.criterion_id == b.criterion_id
                && a.subject_sha256 == b.subject_sha256
                && a.verdict == b.verdict
                && a.report_sha256 == b.report_sha256
                && a.locator == b.locator
        })
}
