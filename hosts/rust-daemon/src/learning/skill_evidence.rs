//! Selected procedural evidence. Arbitrary file and tool-result bodies stay out.

use crate::app::SharedDaemonState;
use crate::runs::{ActionReceipt, ActionStatus, RunRecord, RunSource};
use anima_core::skill_learning::safe_learning_excerpt;
use serde_json::{json, Value};

pub(super) fn correction_signal(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "correction:",
        "next time",
        "instead of",
        "should have",
        "remember this",
        "don't do",
        "do not do",
        "stop doing",
    ]
    .iter()
    .any(|cue| text.contains(cue))
}

pub(super) fn owner_source(state: &crate::state::DaemonState, run: &RunRecord) -> bool {
    run.system_followup != Some(true)
        && (matches!(run.source, RunSource::Web | RunSource::Api)
            || super::owner_only_connector_chat(state, run))
}

pub(super) fn procedure_evidence(receipt: &ActionReceipt) -> Option<Value> {
    if receipt.arguments_truncated {
        return None;
    }
    let args: Value = serde_json::from_str(&receipt.arguments_preview).ok()?;
    fn safe_path(value: &Value) -> Option<&str> {
        value.as_str().filter(|p| {
            !p.is_empty()
                && p.len() < 300
                && !p.to_lowercase().contains(".env")
                && !p.to_lowercase().contains("auth.json")
                && safe_learning_excerpt(p, 300) == *p
        })
    }
    let data = match receipt.name.as_str() {
        "read_file" | "write_file" | "load_skill" => {
            let key = if receipt.name == "load_skill" {
                "name"
            } else {
                "path"
            };
            let value = safe_path(&args[key])?;
            json!({key:value})
        }
        "calculate" => {
            let expression = args["expression"].as_str()?;
            if expression.len() > 256
                || !expression
                    .chars()
                    .all(|c| c.is_ascii_digit() || " +-*/().%^\t".contains(c))
            {
                return None;
            }
            json!({"expression":expression})
        }
        "bash" => {
            let command = args["command"].as_str()?;
            let verification = [
                "cargo test",
                "cargo check",
                "cargo fmt",
                "cargo clippy",
                "bun x nx",
                "bun test",
                "npm test",
                "git diff",
                "git status",
            ]
            .iter()
            .any(|prefix| command == *prefix || command.starts_with(&format!("{prefix} ")));
            if !verification
                || command.len() > 400
                || !command
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || " -_./:\\".contains(c))
                || safe_learning_excerpt(command, 400) != command
                || command.to_lowercase().contains(".env")
                || command.to_lowercase().contains("auth.json")
            {
                return None;
            }
            json!({"command":command})
        }
        _ => return None,
    };
    Some(
        json!({"anchor":format!("receipt:{}",receipt.tool_call_id),"tool":receipt.name,"arguments":data,"status":receipt.status,"meaning":"Selected arguments and tool status only; output/file bodies omitted and whole-task success unverified."}),
    )
}

pub(super) async fn context(
    state: &SharedDaemonState,
    run: &RunRecord,
) -> (bool, bool, Vec<Value>) {
    let (allowed, root, mut ranked) = {
        let guard = state.read().await;
        let allowed = owner_source(&guard, run);
        let correction = correction_signal(&run.input.text);
        let words: Vec<_> = run
            .input
            .text
            .split(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
            .filter(|s| s.len() >= 4)
            .collect();
        let mut ranked: Vec<_> = guard
            .skills
            .records()
            .into_iter()
            .filter_map(|record| {
                if !record.enabled
                    || record.status != crate::skills::SkillStatus::Active
                    || !crate::community_bundles::runnable_skill(&guard, &record.slug)
                {
                    return None;
                }
                let used = record.experiences.iter().any(|e| {
                    e.version == record.approved_hash
                        && e.agent_id == run.agent_id
                        && (e.run_id == run.id || (correction && e.session_id == run.session_id))
                });
                let description = format!("{} {} {}", record.slug, record.name, record.description)
                    .to_lowercase();
                let was_used = record.experiences.iter().any(|e| {
                    e.agent_id == run.agent_id
                        && (e.run_id == run.id || (correction && e.session_id == run.session_id))
                });
                let score = words
                    .iter()
                    .filter(|word| description.contains(word.as_str()))
                    .count()
                    + if was_used { 1000 } else { 0 };
                (score > 0).then(|| (score, record.clone(), used))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.slug.cmp(&b.1.slug)));
        (
            allowed,
            guard.workspace.as_ref().map(|w| w.root_path.clone()),
            ranked,
        )
    };
    let skill_signal = allowed
        && (correction_signal(&run.input.text)
            || ranked.iter().any(|(_, _, used)| *used)
            || run
                .action_receipts
                .iter()
                .any(|r| r.name == "load_skill" && r.status == ActionStatus::Succeeded)
            || (run
                .action_receipts
                .iter()
                .any(|r| r.status == ActionStatus::Failed)
                && run
                    .action_receipts
                    .iter()
                    .any(|r| r.status == ActionStatus::Succeeded)));
    let Some(root) = root else {
        return (allowed, skill_signal, Vec::new());
    };
    if !allowed {
        return (false, false, Vec::new());
    }
    ranked.truncate(2);
    let mut selected = Vec::new();
    for (_, record, used) in ranked {
        let (root, slug) = (root.clone(), record.slug.clone());
        let package = crate::skills::service::blocking(move || {
            crate::skills::package::read_package(&root, &slug)
        })
        .await;
        let Ok(Ok(Some(package))) = package else {
            continue;
        };
        if package.hash() != record.approved_hash {
            continue;
        }
        let Ok(file) = crate::skills::parse_skill_file(&package.bytes) else {
            continue;
        };
        let body = safe_learning_excerpt(&file.body, 1800);
        let requested_file = run
            .action_receipts
            .iter()
            .filter(|r| {
                r.name == "load_skill"
                    && r.status == ActionStatus::Succeeded
                    && !r.arguments_truncated
            })
            .filter_map(|r| serde_json::from_str::<Value>(&r.arguments_preview).ok())
            .find_map(|args| {
                let name = args["name"].as_str()?.trim_start_matches('/');
                ((name == record.slug || name.eq_ignore_ascii_case(&record.name))
                    && args["filePath"].is_string())
                .then(|| args["filePath"].as_str().unwrap().to_owned())
            });
        let files: Vec<_> = package
            .files
            .iter()
            .take(8)
            .map(|f| {
                let text = (requested_file.as_deref() == Some(f.path.as_str()))
                    .then(|| safe_learning_excerpt(&f.content, 900));
                json!({"path":f.path,"bytes":f.content.len(),"text":text})
            })
            .collect();
        let observed_versions: Vec<_> = record
            .experiences
            .iter()
            .filter(|e| {
                e.agent_id == run.agent_id
                    && (e.run_id == run.id
                        || (correction_signal(&run.input.text) && e.session_id == run.session_id))
            })
            .map(|e| e.version.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        selected.push(json!({"observedVersions":observed_versions,"staleUse":!used && !observed_versions.is_empty(),"slug":record.slug,"name":record.name,"description":record.description,"hash":record.approved_hash,"used":used,"body":body,"bodyShortened":body!=file.body,"files":files}));
    }
    (allowed, skill_signal, selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skill_learning_only_recognizes_correction_signals_as_review_eligibility() {
        assert!(correction_signal(
            "Correction: refund rows must subtract sales."
        ));
        assert!(!correction_signal("Please calculate 2+2."));
    }
    fn receipt(tool: &str, args: Value) -> ActionReceipt {
        ActionReceipt {
            tool_call_id: "call-1".into(),
            name: tool.into(),
            status: ActionStatus::Succeeded,
            started_at_ms: 1,
            finished_at_ms: Some(2),
            duration_ms: Some(1),
            arguments_preview: args.to_string(),
            arguments_truncated: false,
            result_preview: Some("PRIVATE FILE BODY".into()),
            result_truncated: false,
            durability: crate::runs::ActionDurability::Persistent,
        }
    }

    #[test]
    fn skill_learning_selected_evidence_omits_payloads_secrets_and_shell_programs() {
        let file = receipt(
            "write_file",
            json!({"path":"reports/result.json", "content":"PRIVATE INPUT PAYLOAD"}),
        );
        let evidence = procedure_evidence(&file).unwrap().to_string();
        assert!(evidence.contains("reports/result.json"));
        assert!(!evidence.contains("PRIVATE"));
        assert!(procedure_evidence(&receipt("read_file", json!({"path":".env.local"}))).is_none());
        for command in [
            "cargo test && echo secret",
            "bun x nx test --token=private",
            "python -c private",
            "git diff\ncat auth.json",
            "git diff --name-only .env",
        ] {
            assert!(
                procedure_evidence(&receipt("bash", json!({"command":command}))).is_none(),
                "{command}"
            );
        }
        assert!(procedure_evidence(&receipt(
            "bash",
            json!({"command":"cargo test -p anima-core"})
        ))
        .is_some());
        let mut truncated = file;
        truncated.arguments_truncated = true;
        assert!(procedure_evidence(&truncated).is_none());
    }
}
