//! Discovery describes registered handlers, never grants or connection health.
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::DaemonState;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct CapabilityInventory {
    schema_version: u32,
    tools: Vec<RegisteredTool>,
    persistence: PersistenceInventory,
    extensions: Vec<PlannedExtension>,
    limitations: Vec<&'static str>,
}

#[derive(Serialize, ToSchema)]
struct RegisteredTool {
    name: String,
    description: String,
    category: &'static str,
    requirements: Vec<&'static str>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct PersistenceInventory {
    control_plane: String,
    memory: &'static str,
    execution_journal: bool,
}

#[derive(Serialize, ToSchema)]
struct PlannedExtension {
    id: &'static str,
    label: &'static str,
    status: &'static str,
    description: &'static str,
}

pub(super) fn inventory(state: &DaemonState) -> CapabilityInventory {
    let tools = state
        .tool_registry
        .tool_names()
        .into_iter()
        .filter_map(|name| {
            let descriptor = state.tool_registry.descriptor(&name)?;
            let (category, requirements) = metadata(&name);
            Some(RegisteredTool {
                name,
                description: descriptor.description,
                category,
                requirements,
            })
        })
        .collect();
    CapabilityInventory {
        schema_version: 1,
        tools,
        persistence: PersistenceInventory {
            control_plane: state.control_plane_durability(),
            memory: state.memory_store.as_ref().map_or("ephemeral", |config| config.storage_label()),
            execution_journal: state.database_configured(),
        },
        extensions: [
            ("camera", "Camera observation", "planned", "Observe an authorized camera through a bounded perception module."),
            ("voice", "Voice", "partial", "Selected WAV transcription, local audio capture, and selected-reply speech output require explicit opt-in and configuration. Device coverage and provider quality remain unverified."),
            ("smart-home", "Smart home", "planned", "Connect devices, inspect their state, and verify authorized actions."),
            ("document-engine", "Document processing", "partial", "Bounded read_document extraction supports PDF, DOCX, and XLSX with workspace access, explicit document opt-in, and a tool grant. Document creation, OCR, and other processing remain unavailable."),
            ("coding-agents", "External coding agents", "planned", "Connect an external coding worker through a bounded execution adapter."),
            ("community-bundles", "Community bundles", "partial", "Reviewed MCP and skill installation is implemented with explicit owner review and configuration. This does not provide an arbitrary execution bridge."),
        ].into_iter().map(|(id, label, status, description)| PlannedExtension { id, label, status, description }).collect(),
        limitations: vec![
            "Registered tools still require agent access and their connection or workspace prerequisites.",
            "Interactive browser tasks require a verified Windows installation, an exact operator policy and ordinary live-run scope, separate grants and Exec approval. Request interception is not an operating-system egress sandbox.",
            "Storage labels describe configuration, not a successful storage health check.",
            "Saved tasks and memories do not imply automatic resumption of interrupted runs. Uncertain scheduled actions require review.",
            "Background shell processes are managed only while this daemon is running. Shell access is not an operating-system sandbox.",
            "Native file tools handle text. read_document provides bounded PDF, DOCX, and XLSX extraction with explicit document opt-in, workspace access, and a tool grant; document creation, OCR, and other processing remain unavailable.",
            "Community bundles support reviewed MCP and skill installation. A portable arbitrary execution bridge is not available.",
        ],
    }
}

fn metadata(name: &str) -> (&'static str, Vec<&'static str>) {
    match name {
        "browser_navigate"|"browser_observe"|"browser_act"|"browser_transfer"=>("browser",vec!["An owner-selected installed policy and live ordinary chat scope, explicit native tool grants and Exec approval. No helper access or restored browser session."]),
        "read_file" | "write_file" | "edit_file" | "multi_edit" | "list_dir" | "glob" | "grep" =>
            ("workspace", vec!["Workspace access and an explicit tool grant."]),
        "read_document" =>
            ("documents", vec!["A configured workspace, explicit document opt-in, and an explicit agent tool grant."]),
        "todo_read" | "todo_write" =>
            ("workspace", vec!["An agent-owned task list in the workspace. Creating a task does not enable a schedule."]),
        "bash" | "bg_start" | "bg_output" | "bg_wait" | "bg_stop" | "bg_list" =>
            ("terminal", vec!["Shell access. Programs run with the daemon user's operating-system permissions."]),
        name if name.starts_with("app_") =>
            ("terminal", vec!["Interactive apps on a real terminal, with the daemon user's operating-system permissions. At most four apps and coding agents open per agent."]),
        name if name.starts_with("acp_") =>
            ("terminal", vec!["Coding agents over the Agent Client Protocol. Their edit, command and fetch requests go through the owner's approvals."]),
        "memory_search" | "memory_add" | "memory_update" | "memory_remove" | "memory_consolidate" | "recent_memories" =>
            ("memory", vec!["Agent memory access. Persistence depends on the configured memory storage."]),
        "spawn_helper" =>
            ("team", vec!["A direct companion run. Reuses idle helpers, up to four busy helpers, with current companion tool permissions except shell/background-process tools and no recursive communication. Execution deadlines do not roll back completed effects."]),
        "list_workspace_agents" | "delegate_to_agent" | "send_message" | "broadcast_message" =>
            ("team", vec!["Existing workspace agents and permitted communication. Delegation is bounded by the manager's current authority."]),
        "web_fetch" =>
            ("research", vec!["Network access to a public HTTP or HTTPS URL."]),
        "exa_search" =>
            ("research", vec!["A daemon-configured Exa API key and authorized provider usage; provider charges may apply."]),
        "web_search" =>
            ("research", vec!["An explicit agent tool grant and ordinary Network approval. Sends only the query and bounded search options to Exa's anonymous public service; it may be rate-limited or unavailable. No API key or paid fallback."]),
        name if name.starts_with("calendar_") =>
            ("productivity", vec!["A connected Google Calendar. Changes remain drafts until owner approval."]),
        name if name.starts_with("drive_") => ("productivity",vec!["Connected Google Drive, immutable selected file IDs, and a current agent tool grant."]),
        name if name.starts_with("mail_") =>
            ("productivity", vec!["A connected mailbox. Sending follows the connector's owner approval flow."]),
        _ => ("utility", vec!["An explicit agent tool grant."]),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_remove_inventory_is_memory_discovery() {
        assert_eq!(
            super::metadata("memory_remove"),
            super::metadata("memory_update")
        );
        assert_eq!(
            super::metadata("memory_consolidate"),
            super::metadata("memory_update")
        );
    }
}
