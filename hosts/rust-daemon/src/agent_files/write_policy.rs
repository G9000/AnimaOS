//! Exact owner decisions for model writes to instruction files.
//! This checks revisions before dispatch, not an atomic filesystem transaction:
//! trusted OS processes can still race the existing filesystem writers.
use anima_core::{DataValue, ToolCall};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const EXACT_MARKER: &str = "_anima_exact_instruction_write";
pub(crate) const REFUSED: &str = "Instruction file write requires an exact owner approval";
pub(crate) const CORE_COPY_REFUSED: &str =
    "USER.md and MEMORY.md are generated from core memory; use the core_memory tool.";
const MAX_REVISION_BYTES: u64 = 262144;

pub(crate) fn protected_name(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let name = normalized.rsplit('/').next().unwrap_or("");
    matches!(
        name,
        "soul.md"
            | "user.md"
            | "memory.md"
            | "agents.md"
            | "agents.override.md"
            | ".anima.md"
            | "anima.md"
            | "claude.md"
            | ".cursorrules"
    ) || (name.ends_with(".mdc")
        && normalized
            .split('/')
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == [".cursor", "rules"]))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProtectedWrite {
    target: PathBuf,
    relative: String,
    revision: Option<String>,
    args_digest: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn resolved_target(path: &Path) -> Result<PathBuf, &'static str> {
    // Resolve existing aliases and the nearest existing ancestor for new files.
    let mut cursor = path;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(cursor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.file_name().ok_or(REFUSED)?.to_owned());
                cursor = cursor.parent().ok_or(REFUSED)?;
            }
            Err(_) => return Err(REFUSED),
        }
    }
    let mut result = cursor.canonicalize().map_err(|_| REFUSED)?;
    for component in missing.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}

fn revision(root: &Path, relative: &str) -> Result<Option<String>, &'static str> {
    use crate::tools::workspace_bytes::{InstructionReadError, WorkspaceBytes};
    let workspace = WorkspaceBytes::open(root).map_err(|_| REFUSED)?;
    match workspace.read_instruction_file(relative, MAX_REVISION_BYTES as usize) {
        Ok(bytes) => Ok(Some(digest(&bytes))),
        Err(InstructionReadError::Missing) => Ok(None),
        Err(_) => Err(REFUSED),
    }
}

pub(crate) fn capture(
    root: Option<&Path>,
    call: &ToolCall,
) -> Result<Option<ProtectedWrite>, &'static str> {
    if !matches!(
        call.name.as_str(),
        "write_file" | "edit_file" | "multi_edit"
    ) {
        return Ok(None);
    }
    let Some(DataValue::String(input)) = call.args.get("file_path") else {
        return Ok(None);
    };
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    let root = crate::tools::workspace_root_path(&call.name, root).map_err(|_| REFUSED)?;
    let canonical_root = root.canonicalize().map_err(|_| REFUSED)?;
    let lexical = crate::tools::resolve_workspace_write_path(&root, input, &call.name)
        .map_err(|_| REFUSED)?;
    let target = resolved_target(&lexical)?;
    let relative = target
        .strip_prefix(&canonical_root)
        .map_err(|_| REFUSED)?
        .to_string_lossy()
        .replace('\\', "/");
    let workspace = crate::tools::workspace_bytes::WorkspaceBytes::open(&canonical_root)
        .map_err(|_| REFUSED)?;
    if workspace
        .instruction_link_count(&relative)
        .map_err(|_| REFUSED)?
        .is_some_and(|links| links != 1)
    {
        return Err(REFUSED);
    }
    if !protected_name(input) && !protected_name(&relative) {
        return Ok(None);
    }
    let args = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone())).to_string();
    Ok(Some(ProtectedWrite {
        revision: revision(&canonical_root, &relative)?,
        target,
        relative,
        args_digest: digest(args.as_bytes()),
    }))
}

impl ProtectedWrite {
    /// Whether this targets the agent's own core memory file copy
    /// (`core_memory::files`), which no approval can unlock.
    pub(crate) fn is_core_copy(&self, agent_id: &str) -> bool {
        let directory = super::selection::profile_relative(agent_id);
        [
            crate::core_memory::CoreBlock::User,
            crate::core_memory::CoreBlock::Notes,
        ]
        .into_iter()
        .any(|block| {
            // Case-insensitive: on the usual macOS file system `user.md` is the
            // same file.
            self.relative.eq_ignore_ascii_case(&format!(
                "{directory}/{}",
                crate::core_memory::files::file_name(block)
            ))
        })
    }

    /// Compare the exact captured raw preimage, rather than reopening a path.
    /// The checkpoint adapter retains this state through its final write lease.
    pub(crate) fn matches_preimage(&self, source: Option<&[u8]>) -> bool {
        source.map(digest) == self.revision
    }

    pub(crate) fn approval_call(&self, call: &ToolCall) -> ToolCall {
        let mut args = BTreeMap::new();
        args.insert(EXACT_MARKER.into(), DataValue::Bool(true));
        args.insert("file_path".into(), DataValue::String(self.relative.clone()));
        args.insert(
            "target_revision".into(),
            DataValue::String(self.revision.clone().unwrap_or_else(|| "absent".into())),
        );
        args.insert(
            "arguments_digest".into(),
            DataValue::String(self.args_digest.clone()),
        );
        ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            args,
        }
    }
    pub(crate) fn revalidate(&self, root: Option<&Path>, call: &ToolCall) -> bool {
        capture(root, call).ok().flatten().as_ref() == Some(self)
    }
}

pub(crate) fn exact_record(arguments: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| value.get(EXACT_MARKER).and_then(|value| value.as_bool()))
        == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(path: &str) -> ToolCall {
        ToolCall {
            id: "test".into(),
            name: "write_file".into(),
            args: BTreeMap::from([
                ("file_path".into(), DataValue::String(path.into())),
                (
                    "content".into(),
                    DataValue::String("private-new-body".into()),
                ),
            ]),
        }
    }
    #[test]
    fn reserved_names_and_cursor_rules_match_case_insensitively() {
        for path in [
            "SOUL.md",
            "USER.md",
            "MEMORY.md",
            "nested/AGENTS.override.md",
            "profile/anima.MD",
            "CLAUDE.md",
            ".cursorrules",
            ".cursor/rules/nested/x.MDC",
        ] {
            assert!(protected_name(path), "{path}");
        }
        for path in ["notes.md", "SOUL.md.txt", "cursor/rules/x.mdc"] {
            assert!(!protected_name(path));
        }
    }
    #[test]
    fn only_the_agents_own_profile_user_and_memory_files_are_its_core_copy() {
        let root = std::env::temp_dir().join(format!("anima-core-copy-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let own = super::super::selection::profile_relative("agent-1");
        let captured = |path: &str| capture(Some(&root), &call(path)).unwrap().unwrap();
        for path in [
            format!("{own}/USER.md"),
            format!("{own}/MEMORY.md"),
            format!("{own}/user.MD"),
            format!("./{own}/MEMORY.md"),
        ] {
            assert!(captured(&path).is_core_copy("agent-1"), "{path}");
        }
        for path in [
            format!("{own}/SOUL.md"),
            format!("{own}/nested/USER.md"),
            "USER.md".to_string(),
            "MEMORY.md".to_string(),
        ] {
            assert!(!captured(&path).is_core_copy("agent-1"), "{path}");
        }
        assert!(!captured(&format!("{own}/USER.md")).is_core_copy("agent-2"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn approval_is_body_free_and_revision_bound() {
        let root = std::env::temp_dir().join(format!("anima-m11-write-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let request = call("SOUL.md");
        let captured = capture(Some(&root), &request).unwrap().unwrap();
        let approval = captured.approval_call(&request);
        let json = crate::routes::data_value_to_json(&DataValue::Object(approval.args)).to_string();
        assert!(exact_record(&json));
        assert!(!json.contains("private-new-body"));
        assert!(!json.contains(root.to_string_lossy().as_ref()));
        assert!(captured.revalidate(Some(&root), &request));
        fs::write(root.join("SOUL.md"), "externally-created").unwrap();
        assert!(!captured.revalidate(Some(&root), &request));
        let captured = capture(Some(&root), &request).unwrap().unwrap();
        fs::write(root.join("SOUL.md"), "different-revision").unwrap();
        assert!(!captured.revalidate(Some(&root), &request));
        assert!(capture(Some(&root), &call("notes.md")).unwrap().is_none());
        assert!(capture(Some(&root), &call("../SOUL.md")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn exact_owner_wait_overrides_allows_and_detects_target_drift() {
        use crate::agent_runs::test_support::{
            approving_coordinator, decision, gated_context, ledger_run, patient, pending_approvals,
            tool_input, ScriptedModel,
        };
        use crate::approvals::{ApprovalDecisionKind, ApprovalPolicy};
        use anima_core::{AgentConfigUpdate, CancelSignal, TaskStatus};
        for drift in [false, true] {
            let root =
                std::env::temp_dir().join(format!("anima-m11-exact-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("SOUL.md"), "original").unwrap();
            let (coordinator, agent_id) = approving_coordinator(
                ScriptedModel::new(vec![]),
                ApprovalPolicy::default(),
                patient(),
            )
            .await;
            {
                let shared = crate::agent_runs::test_support::coordinator_state(&coordinator);
                let mut state = shared.write().await;
                state
                    .agents
                    .get_mut(&agent_id)
                    .unwrap()
                    .update_config(AgentConfigUpdate {
                        tools: Some(
                            crate::tools::ToolRegistry::new()
                                .resolve_descriptors(["write_file"])
                                .unwrap(),
                        ),
                        ..Default::default()
                    });
            }
            let shared = crate::agent_runs::test_support::coordinator_state(&coordinator);
            let agent = shared.read().await.get_agent(&agent_id).unwrap().state;
            let run = ledger_run(&coordinator, &agent_id, "chat:exact").await;
            let mut context = gated_context(&coordinator, &run, CancelSignal::new()).await;
            context.workspace_root = Some(root.clone());
            let calling = tokio::spawn(async move {
                context
                    .execute_tool(
                        agent.clone(),
                        tool_input(&agent.id, "chat:exact"),
                        call("SOUL.md"),
                    )
                    .await
            });
            let pending = pending_approvals(&coordinator, 1).await.remove(0);
            assert!(!pending.arguments.contains("private-new-body"));
            assert!(!calling.is_finished());
            // Acquiring the state lock while awaiting proves no state lock spans owner wait.
            assert!(
                crate::agent_runs::test_support::coordinator_state(&coordinator)
                    .try_write()
                    .is_ok()
            );
            if drift {
                fs::write(root.join("SOUL.md"), "changed-during-wait").unwrap();
            }
            coordinator
                .decide_approval(
                    &pending.id,
                    decision(ApprovalDecisionKind::AllowAlways, pending.revision),
                )
                .await
                .unwrap();
            let result = calling.await.unwrap();
            if drift {
                assert_eq!(result.status, TaskStatus::Error);
                assert_eq!(
                    fs::read_to_string(root.join("SOUL.md")).unwrap(),
                    "changed-during-wait"
                );
            } else {
                assert_eq!(result.status, TaskStatus::Success);
                assert_eq!(
                    fs::read_to_string(root.join("SOUL.md")).unwrap(),
                    "private-new-body"
                );
            }
            assert!(
                crate::agent_runs::test_support::coordinator_state(&coordinator)
                    .read()
                    .await
                    .approvals
                    .rules_for(&agent_id)
                    .is_empty()
            );
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn oversized_revisions_fail_closed_and_arguments_cannot_reuse_binding() {
        let root = std::env::temp_dir().join(format!("anima-m11-binding-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("SOUL.md"), "original").unwrap();
        let original = call("SOUL.md");
        let accepted = capture(Some(&root), &original).unwrap().unwrap();
        let mut changed = original.clone();
        changed
            .args
            .insert("content".into(), DataValue::String("different".into()));
        assert!(!accepted.revalidate(Some(&root), &changed));
        fs::write(
            root.join("SOUL.md"),
            vec![b'x'; MAX_REVISION_BYTES as usize + 1],
        )
        .unwrap();
        assert!(capture(Some(&root), &original).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn ordinary_alias_to_instruction_file_requires_exact_owner_decision() {
        let root = std::env::temp_dir().join(format!("anima-m11-alias-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("SOUL.md"), "identity").unwrap();
        std::os::unix::fs::symlink(root.join("SOUL.md"), root.join("notes.md")).unwrap();
        let protected = capture(Some(&root), &call("notes.md")).unwrap().unwrap();
        assert_eq!(protected.relative, "SOUL.md");
        assert!(protected.revalidate(Some(&root), &call("notes.md")));
        fs::remove_file(root.join("notes.md")).unwrap();
        fs::write(root.join("notes.md"), "ordinary").unwrap();
        assert!(!protected.revalidate(Some(&root), &call("notes.md")));
        fs::remove_dir_all(root).unwrap();
    }
}
