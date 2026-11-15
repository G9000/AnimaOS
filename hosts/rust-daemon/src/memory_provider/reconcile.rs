//! Bounded v1 receipt interpretation. Evidence never grants dispatch authority.
use super::{
    binding::MemoryProviderBinding,
    outbox::{Publication, Transition},
};
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EvidenceError {
    Invalid,
    Remote(String),
}

pub(crate) fn envelope(text: &str) -> Result<Value, EvidenceError> {
    if text.len() > 16 * 1024 {
        return Err(EvidenceError::Invalid);
    }
    let wire: Value = serde_json::from_str(text).map_err(|_| EvidenceError::Invalid)?;
    if wire["shortened"] != false || integer(&wire["omittedBlocks"])? != 0 {
        return Err(EvidenceError::Invalid);
    }
    let failed = wire["isError"].as_bool().ok_or(EvidenceError::Invalid)?;
    let value = if wire["structuredData"].is_object() {
        wire["structuredData"].clone()
    } else {
        let texts = wire["textBlocks"]
            .as_array()
            .filter(|values| values.len() == 1)
            .ok_or(EvidenceError::Invalid)?;
        serde_json::from_str(texts[0].as_str().ok_or(EvidenceError::Invalid)?)
            .map_err(|_| EvidenceError::Invalid)?
    };
    if !value.is_object() {
        return Err(EvidenceError::Invalid);
    }
    if failed {
        let code = value["error"]["code"]
            .as_str()
            .filter(|code| {
                !code.is_empty()
                    && code.len() <= 64
                    && code
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
            })
            .ok_or(EvidenceError::Invalid)?;
        // Never propagate arbitrary remote error messages into local status/logs.
        return Err(EvidenceError::Remote(code.into()));
    }
    if value.get("error").is_some() {
        return Err(EvidenceError::Invalid);
    }
    Ok(value)
}

pub(crate) fn authority(
    binding: &MemoryProviderBinding,
    value: &Value,
) -> Result<(), EvidenceError> {
    let mode = match binding.access_mode {
        crate::workspace_context::ContextMode::Companion => "companion",
        crate::workspace_context::ContextMode::Shared => "shared",
    };
    if integer(&value["access"]["version"])? != 1
        || value["access"]["subject"] != binding.subject
        || value["access"]["mode"] != mode
        || value["workspace"]["id"] != binding.workspace_id
        || value["workspace"]["generation"] != binding.generation
    {
        return Err(EvidenceError::Invalid);
    }
    Ok(())
}

pub(crate) fn submission(
    publication: &Publication,
    value: Value,
    now_ms: i64,
) -> Result<Transition, EvidenceError> {
    let input = &publication.admission;
    if value["operationId"] != input.operation_id || value["generation"] != input.binding.generation
    {
        return Err(EvidenceError::Invalid);
    }
    match input.remote_tool.as_str() {
        "mother_propose_memory" => {
            let evidence = value["proposal"].clone();
            let proposal_id = uuid_text(&evidence["id"])?.to_owned();
            let mut observed = publication.clone();
            if observed.proposal_id.is_none() {
                observed.proposal_id = Some(proposal_id.clone());
            }
            let outcome = proposal(&observed, evidence.clone(), now_ms)?;
            // Commit the proposal identity before applying any later owner outcome.
            // The submission receipt is immutable; current review state is fetched separately.
            if publication.proposal_id.is_none() {
                Ok(Transition::Submitted {
                    proposal_id,
                    receipt: evidence,
                    next_attempt_at_ms: now_ms,
                })
            } else {
                Ok(outcome)
            }
        }
        "mother_write_record" => {
            let (canonical_id, revision) =
                canonical(publication, &value, &input.operation_id, false)?;
            Ok(Transition::Saved {
                canonical_id,
                revision,
                receipt: value,
            })
        }
        _ => Err(EvidenceError::Invalid),
    }
}

pub(crate) fn proposal(
    publication: &Publication,
    value: Value,
    now_ms: i64,
) -> Result<Transition, EvidenceError> {
    let input = &publication.admission;
    if input.remote_tool != "mother_propose_memory" {
        return Err(EvidenceError::Invalid);
    }
    let id = uuid_text(&value["id"])?;
    if publication
        .proposal_id
        .as_ref()
        .is_some_and(|expected| expected != id)
        || value["workspaceId"] != input.binding.workspace_id
        || value["generation"] != input.binding.generation
        || value["record"] != input.request["record"]
        || value["source"] != input.request["source"]
        || value["basis"] != input.request["basis"]
        || value["targetId"] != input.request["id"]
        || integer(&value["expectedRevision"])? != integer(&input.request["expectedRevision"])?
        || value["actor"].as_str().is_none_or(str::is_empty)
    {
        return Err(EvidenceError::Invalid);
    }
    let proposal_id = id.to_owned();
    match value["status"].as_str() {
        Some("pending") => {
            if integer(&value["revision"])? != 1 || !value["receipt"].is_null() {
                return Err(EvidenceError::Invalid);
            }
            match value["conflict"].as_bool() {
                Some(true) => Ok(Transition::Conflict {
                    error: "proposal_conflict".into(),
                    receipt: value,
                }),
                Some(false) => Ok(Transition::Submitted {
                    proposal_id,
                    receipt: value,
                    next_attempt_at_ms: now_ms.saturating_add(30_000),
                }),
                None => Err(EvidenceError::Invalid),
            }
        }
        Some(status @ ("saved" | "dismissed")) => {
            let review = &value["receipt"];
            if integer(&value["revision"])? != 2
                || review["proposalId"] != proposal_id
                || review["status"] != status
                || review["generation"] != input.binding.generation
            {
                return Err(EvidenceError::Invalid);
            }
            let operation = uuid_text(&review["operationId"])?;
            if status == "dismissed" {
                if !review["receipt"].is_null() {
                    return Err(EvidenceError::Invalid);
                }
                Ok(Transition::Dismissed { receipt: value })
            } else {
                // The owner's immutable approval receipt, not today's record or the original draft,
                // determines the canonical ID, revision and possibly edited content.
                let (canonical_id, revision) =
                    canonical(publication, &review["receipt"], operation, true)?;
                Ok(Transition::Approved {
                    proposal_id,
                    canonical_id,
                    revision,
                    receipt: value,
                })
            }
        }
        _ => Err(EvidenceError::Invalid),
    }
}

fn integer(value: &Value) -> Result<u64, EvidenceError> {
    let number = value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 9_007_199_254_740_991.0 && n.fract() == 0.0)
        .ok_or(EvidenceError::Invalid)?;
    Ok(number as u64)
}

fn uuid_text(value: &Value) -> Result<&str, EvidenceError> {
    let text = value.as_str().ok_or(EvidenceError::Invalid)?;
    uuid::Uuid::parse_str(text).map_err(|_| EvidenceError::Invalid)?;
    Ok(text)
}

fn canonical(
    publication: &Publication,
    value: &Value,
    operation: &str,
    owner_review: bool,
) -> Result<(String, u64), EvidenceError> {
    let input = &publication.admission;
    let record = &value["record"];
    let id = uuid_text(&record["id"])?;
    let revision = integer(&record["revision"])?;
    let expected = integer(&input.request["expectedRevision"])?;
    if value["operationId"] != operation
        || value["generation"] != input.binding.generation
        || revision != expected.checked_add(1).ok_or(EvidenceError::Invalid)?
        || (input.request.get("id").is_some_and(|target| target != id))
        || (input.request.get("id").is_none() && expected != 0)
        || record["kind"] != input.request["record"]["kind"]
        || record["actor"].as_str().is_none_or(str::is_empty)
    {
        return Err(EvidenceError::Invalid);
    }
    for stamp in [
        &value["committedAt"],
        &record["createdAt"],
        &record["updatedAt"],
    ] {
        chrono::DateTime::parse_from_rfc3339(stamp.as_str().ok_or(EvidenceError::Invalid)?)
            .map_err(|_| EvidenceError::Invalid)?;
    }
    for (field, max) in [("title", 240), ("content", 12_000)] {
        let text = record[field].as_str().ok_or(EvidenceError::Invalid)?;
        if text.trim().is_empty() || text.encode_utf16().count() > max {
            return Err(EvidenceError::Invalid);
        }
    }
    if record["kind"] != "memory"
        || !matches!(record["status"].as_str(), Some("active" | "retracted"))
        || !matches!(
            record["basis"].as_str(),
            Some("owner_statement" | "source" | "model_inference")
        )
        || record["references"].as_array().is_none()
        || record["sources"].as_array().is_none()
    {
        return Err(EvidenceError::Invalid);
    }
    if !owner_review {
        let draft = input.request["record"]
            .as_object()
            .ok_or(EvidenceError::Invalid)?;
        if draft.iter().any(|(key, value)| record[key] != *value) {
            return Err(EvidenceError::Invalid);
        }
    }
    Ok((id.to_owned(), revision))
}

#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod tests;
