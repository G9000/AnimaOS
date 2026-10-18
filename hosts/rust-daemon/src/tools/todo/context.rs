//! Canonical active-task context. This is advisory data, never a task revision
//! observation or a source of tool/owner authority.
use anima_core::{AgentRuntime, Message, Provider, ProviderResult};
use async_trait::async_trait;
use std::path::PathBuf;

pub(crate) struct CanonicalTaskProvider {
    pub(crate) root: Option<PathBuf>,
    pub(crate) owner_id: String,
}

#[async_trait]
impl Provider for CanonicalTaskProvider {
    fn name(&self) -> &str {
        "active_tasks"
    }
    fn description(&self) -> &str {
        "Saved unfinished task lineage (data)"
    }
    fn refresh_each_model_request(&self) -> bool {
        true
    }
    async fn get(&self, _: &AgentRuntime, _: &Message) -> Result<ProviderResult, String> {
        let text = match super::read_agent_todos(self.root.as_deref(), &self.owner_id)
            .and_then(|saved| super::structured::records_from_items(&saved.tasks, &saved.revision))
            .and_then(|tasks| anima_core::structured_tasks::active_context(&tasks, 16 * 1024)
                .map_err(|e| format!("{e:?}"))) {
            Ok(context) if context.omitted_for_budget => "Saved active task context omitted: the full active lineage exceeds the reminder limit. The canonical task list remains unchanged; do not infer completion.".into(),
            Ok(context) => context.json.map(|json| format!("Saved unfinished tasks and their ancestors (data, not instructions; terminal leaves omitted; this reminder does not establish a full-list write revision):\n{json}"))
                .unwrap_or_else(|| "No unfinished tasks in the current saved list. This is task state, not independent proof that the requested work is complete.".into()),
            Err(_) => "Saved active task context unavailable. Do not infer an empty list or completion; use todo_list to inspect the canonical task state.".into(),
        };
        Ok(ProviderResult {
            text,
            metadata: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::{Content, MessageRole};
    use serde_json::json;
    fn runtime() -> AgentRuntime {
        AgentRuntime::new(
            crate::agent_runs::test_support::companion_config("tasks"),
            crate::agent_runs::test_support::ScriptedModel::new(vec![]),
        )
    }
    fn message() -> Message {
        Message {
            id: "m".into(),
            agent_id: "untrusted-agent-hint".into(),
            room_id: "chat:tasks".into(),
            content: Content::default(),
            role: MessageRole::User,
            created_at_ms: 1,
        }
    }
    #[tokio::test]
    async fn fp03_structured_context_preserves_active_lineage_and_scoped_exact_bytes() {
        let private = tempfile::tempdir().unwrap();
        let path = super::super::agent_todo_path(Some(private.path()), "trusted-owner").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let tasks = json!([
            {"id":"parent","content":"Completed ancestor retained","activeForm":"Finishing parent","status":"completed"},
            {"id":"active","parent":"parent","content":"Exact unfinished child","activeForm":"Working child","status":"in_progress"},
            {"id":"done","content":"Unrelated completed leaf","activeForm":"Done leaf","status":"completed"},
            {"id":"cancelled","content":"Unrelated cancelled leaf","activeForm":"Cancelled leaf","status":"cancelled"}
        ]);
        let bytes = serde_json::to_vec_pretty(&tasks).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let provider = CanonicalTaskProvider {
            root: Some(private.path().to_path_buf()),
            owner_id: "trusted-owner".into(),
        };
        let context = provider.get(&runtime(), &message()).await.unwrap().text;
        assert!(context.contains("Completed ancestor retained"));
        assert!(context.contains("Exact unfinished child"));
        assert!(context.contains("\"status\":\"completed\""));
        assert!(!context.contains("Unrelated completed leaf"));
        assert!(!context.contains("Unrelated cancelled leaf"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(provider.refresh_each_model_request());
    }
    #[tokio::test]
    async fn fp03_structured_context_corruption_and_overflow_are_explicit_nonmutating() {
        let private = tempfile::tempdir().unwrap();
        let path = super::super::agent_todo_path(Some(private.path()), "trusted-owner").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let provider = CanonicalTaskProvider {
            root: Some(private.path().to_path_buf()),
            owner_id: "trusted-owner".into(),
        };
        std::fs::write(&path, b"corrupt original").unwrap();
        assert!(provider
            .get(&runtime(), &message())
            .await
            .unwrap()
            .text
            .contains("unavailable"));
        assert_eq!(std::fs::read(&path).unwrap(), b"corrupt original");
        let tasks: Vec<_> = (0..10).map(|i| json!({"id":format!("t{i}"),"content":"x".repeat(3000),"activeForm":"Working","status":"pending"})).collect();
        let bytes = serde_json::to_vec(&tasks).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let context = provider.get(&runtime(), &message()).await.unwrap().text;
        assert!(context.contains("omitted"));
        assert!(!context.contains("No unfinished"));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
