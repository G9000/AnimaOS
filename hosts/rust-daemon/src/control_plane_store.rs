use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anima_core::AgentRuntimeSnapshot;
use anima_swarm::{SwarmConfig, SwarmState};
use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::connectors::gcalendar::{CalendarPendingWriteRecord, GoogleCalendarConnectorRecord};
use crate::connectors::{
    TelegramConnectorRecord, TelegramCredentialCleanupIntent, TelegramInboundRecord,
    TelegramOutboundRecord,
};
use crate::schedules::ScheduledPromptRecord;

/// Snapshot format version. Version 5 adds sessions (companion console M2);
/// version 6 adds the live-run fields (M3: accepted `queued` runs and each
/// run's `replyMessageId`); version 7 adds approvals, approval policies and
/// rules, and session allowances (M4); version 8 adds the skills registry and
/// skill drafts (M5); version 9 adds the automation fields, the `cron` and
/// `once` triggers, and the fire log (M6); version 10 adds the owner's pricing
/// overrides (M8); version 11 adds attachment inputs on steers (M9), version
/// 14 adds bounded learning advice and separate opt-in policies (M38), and
/// 15 adds durable reviewed skill handoff lineage (M39); 16 adds selected
/// canonical memory references (M40); 17 adds reviewed HTTP MCP connection
/// metadata (M42); 18 adds optional owned stdio MCP launch specifications
/// (M44); 19 adds reviewed declarative bundle ownership and managed draft
/// lineage (M45); 20 adds typed async helper/completion lineage and saved
/// execution bounds (FP05); 21 adds bounded pending-origin/final-output evidence
/// on exact source attempts; 22 adds Discord conversation destinations, response
/// policy and durable thread-creation claims; 23 adds durable clarification,
/// recovery lineage and event occurrences; 24 adds workflows, 25 adds reusable
/// workflow templates, and 26 adds opt-in memory maintenance with per-cohort
/// recovery journals, Doctor repair images, standing intentions and paired-node
/// identity records. Runtime transport sessions/processes are never persisted. Older
/// daemons refuse a newer version, so the first start of
/// a new version writes a backup (spec §13.3).
pub(crate) const CONTROL_PLANE_STORE_VERSION: u32 = 26;
pub(crate) const PRE_MEMORY_MAINTENANCE_BACKUP_SUFFIX: &str = ".pre-memory-maintenance.bak";
pub(crate) const PRE_WORKFLOW_TEMPLATES_BACKUP_SUFFIX: &str = ".pre-workflow-templates.bak";
pub(crate) const PRE_WORKFLOWS_BACKUP_SUFFIX: &str = ".pre-workflows.bak";
pub(crate) const PRE_COMPANION_INTERACTION_BACKUP_SUFFIX: &str = ".pre-companion-interaction.bak";
pub(crate) const ASYNC_ORIGIN_BATCHES_STORE_VERSION: u32 = 21;
pub(crate) const PRE_DISCORD_CONVERSATIONS_BACKUP_SUFFIX: &str = ".pre-discord-conversations.bak";
pub(crate) const ASYNC_HELPERS_STORE_VERSION: u32 = 20;
pub(crate) const PRE_ASYNC_ORIGIN_BATCHES_BACKUP_SUFFIX: &str = ".pre-async-origin-batches.bak";
pub(crate) const COMMUNITY_BUNDLES_STORE_VERSION: u32 = 19;
pub(crate) const PRE_ASYNC_HELPERS_BACKUP_SUFFIX: &str = ".pre-async-helpers.bak";
pub(crate) const MCP_STDIO_STORE_VERSION: u32 = 18;
pub(crate) const PRE_COMMUNITY_BUNDLES_BACKUP_SUFFIX: &str = ".pre-community-bundles.bak";
pub(crate) const MEMORY_VIEWS_STORE_VERSION: u32 = 16;
pub(crate) const PRE_MCP_BACKUP_SUFFIX: &str = ".pre-mcp.bak";
pub(crate) const MCP_STORE_VERSION: u32 = 17;
pub(crate) const PRE_MCP_STDIO_BACKUP_SUFFIX: &str = ".pre-mcp-stdio.bak";
pub(crate) const LEARNING_ADOPTION_STORE_VERSION: u32 = 15;
pub(crate) const PRE_MEMORY_VIEWS_BACKUP_SUFFIX: &str = ".pre-memory-views.bak";
pub(crate) const LEARNING_STORE_VERSION: u32 = 14;
pub(crate) const PRE_LEARNING_ADOPTION_BACKUP_SUFFIX: &str = ".pre-learning-adoption.bak";
pub(crate) const DRIVE_STORE_VERSION: u32 = 13;
pub(crate) const PRE_LEARNING_BACKUP_SUFFIX: &str = ".pre-learning.bak";
/// Version 10 pricing snapshots are backed up as `.pre-attachments.bak`.
pub(crate) const DISCORD_STORE_VERSION: u32 = 12;
pub(crate) const PRE_DRIVE_BACKUP_SUFFIX: &str = ".pre-drive.bak";
pub(crate) const ATTACHMENTS_STORE_VERSION: u32 = 11;
pub(crate) const PRE_DISCORD_BACKUP_SUFFIX: &str = ".pre-discord.bak";
pub(crate) const USAGE_STORE_VERSION: u32 = 10;
/// Version 9 automation snapshots are backed up as `.pre-usage.bak`.
pub(crate) const AUTOMATIONS_STORE_VERSION: u32 = 9;
/// The version that added skills: older snapshots are backed up as
/// `.pre-skills.bak` at the latest, this one as `.pre-automations.bak`.
pub(crate) const SKILLS_STORE_VERSION: u32 = 8;
/// The version that added approvals: older snapshots are backed up as
/// `.pre-approvals.bak` at the latest, this one as `.pre-skills.bak`.
pub(crate) const APPROVALS_STORE_VERSION: u32 = 7;
/// The version that added the live-run fields: older snapshots are backed
/// up as `.pre-live-runs.bak`, this one as `.pre-approvals.bak`.
pub(crate) const LIVE_RUNS_STORE_VERSION: u32 = 6;
/// The version that added sessions: older snapshots still need the M2
/// migration (spec §13.3 step 2).
pub(crate) const SESSIONS_STORE_VERSION: u32 = 5;
/// JSON snapshots written before the format was versioned.
const UNVERSIONED_SNAPSHOT_VERSION: u32 = 1;
/// Suffix of the JSON backup taken before the sessions upgrade (spec §13.3).
pub(crate) const PRE_SESSIONS_BACKUP_SUFFIX: &str = ".pre-sessions.bak";
/// Suffix of the JSON backup taken before the live-runs upgrade.
pub(crate) const PRE_LIVE_RUNS_BACKUP_SUFFIX: &str = ".pre-live-runs.bak";
/// Suffix of the JSON backup taken before the approvals upgrade.
pub(crate) const PRE_APPROVALS_BACKUP_SUFFIX: &str = ".pre-approvals.bak";
/// Suffix of the JSON backup taken before the skills upgrade.
pub(crate) const PRE_SKILLS_BACKUP_SUFFIX: &str = ".pre-skills.bak";
/// Suffix of the JSON backup taken before the automations upgrade.
pub(crate) const PRE_AUTOMATIONS_BACKUP_SUFFIX: &str = ".pre-automations.bak";
/// Suffix of the JSON backup taken before the usage (pricing overrides)
/// upgrade.
pub(crate) const PRE_USAGE_BACKUP_SUFFIX: &str = ".pre-usage.bak";
pub(crate) const PRE_ATTACHMENTS_BACKUP_SUFFIX: &str = ".pre-attachments.bak";
const CONTROL_PLANE_SNAPSHOT_KEY: &str = "control_plane";

#[derive(Clone, Debug)]
pub(crate) enum ControlPlaneStoreConfig {
    Json(PathBuf),
    Postgres(PgPool),
}

impl ControlPlaneStoreConfig {
    pub(crate) fn file_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Json(path) => Some(path),
            Self::Postgres(_) => None,
        }
    }

    pub(crate) const fn storage_label(&self) -> &'static str {
        match self {
            Self::Json(_) => "json",
            Self::Postgres(_) => "postgres",
        }
    }

    pub(crate) fn location_label(&self) -> String {
        match self {
            Self::Json(path) => path.display().to_string(),
            Self::Postgres(_) => "postgres:host_snapshots/control_plane".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceConfig {
    pub(crate) root_path: PathBuf,
    pub(crate) company_name: String,
    pub(crate) mission: String,
    #[serde(default)]
    pub(crate) values: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ControlPlaneSnapshot {
    #[serde(default)]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) agents: Vec<AgentRuntimeSnapshot>,
    #[serde(default)]
    pub(crate) swarms: Vec<StoredSwarmSnapshot>,
    #[serde(default)]
    pub(crate) connectors: Vec<TelegramConnectorRecord>,
    #[serde(default)]
    pub(crate) credential_cleanup: Vec<TelegramCredentialCleanupIntent>,
    #[serde(default)]
    pub(crate) inbound: Vec<TelegramInboundRecord>,
    #[serde(default)]
    pub(crate) outbound: Vec<TelegramOutboundRecord>,
    #[serde(default)]
    pub(crate) drive_records: Vec<crate::connectors::drive::DriveRecord>,
    #[serde(default)]
    pub(crate) discord_connectors: Vec<crate::connectors::discord::DiscordConnectorRecord>,
    #[serde(default)]
    pub(crate) discord_credential_intents: Vec<crate::connectors::discord::DiscordCredentialIntent>,
    #[serde(default)]
    pub(crate) discord_inbound: Vec<crate::connectors::discord::DiscordInboundRecord>,
    #[serde(default)]
    pub(crate) discord_outbound: Vec<crate::connectors::discord::DiscordOutboundRecord>,
    #[serde(default)]
    pub(crate) schedules: Vec<ScheduledPromptRecord>,
    #[serde(default)]
    pub(crate) jobs: Vec<crate::jobs::AgentJobRecord>,
    #[serde(default)]
    pub(crate) goals: Vec<crate::jobs::GoalRecord>,
    #[serde(default)]
    pub(crate) workflows: Vec<anima_core::workflows::WorkflowRun>,
    #[serde(default)]
    pub(crate) workflow_templates: Vec<anima_core::workflow_templates::WorkflowTemplate>,
    #[serde(default)]
    pub(crate) calendar_connectors: Vec<GoogleCalendarConnectorRecord>,
    #[serde(default)]
    pub(crate) calendar_writes: Vec<CalendarPendingWriteRecord>,
    #[serde(default)]
    pub(crate) mail_records: Vec<crate::connectors::mail::MailRecord>,
    #[serde(default)]
    pub(crate) mail_drafts: Vec<crate::connectors::mail::MailDraft>,
    #[serde(default)]
    pub(crate) workspace: Option<WorkspaceConfig>,
    #[serde(default)]
    pub(crate) runs: Vec<crate::runs::RunRecord>,
    #[serde(default)]
    pub(crate) learning_reviews: Vec<crate::learning::LearningReviewRecord>,
    #[serde(default)]
    pub(crate) learning_policies: Vec<crate::learning::LearningPolicy>,
    #[serde(default)]
    pub(crate) memory_maintenance: crate::memory_maintenance::MaintenanceRegistry,
    #[serde(default)]
    pub(crate) doctor: crate::doctor::DoctorRegistry,
    #[serde(default)]
    pub(crate) standing_intents: crate::standing_intents::StandingIntentRegistry,
    #[serde(default)]
    pub(crate) paired_devices: crate::paired_devices::PairedDevicesSnapshot,
    #[serde(default)]
    pub(crate) clarifications: Vec<crate::clarifications::ClarificationRequest>,
    #[serde(default)]
    pub(crate) automation_events: Vec<crate::schedules::event::EventOccurrence>,
    /// Owner messages counted toward each agent's next conversation review.
    #[serde(default)]
    pub(crate) conversation_counts: std::collections::BTreeMap<String, u32>,
    /// Each agent's last counted owner turns (run ids, oldest first).
    #[serde(default)]
    pub(crate) conversation_turns: std::collections::BTreeMap<String, Vec<String>>,
    /// Memory pins from before core memory. Read only so the boot migration
    /// can fold them into core memory; never written again.
    #[serde(default, rename = "memoryViews", skip_serializing)]
    pub(crate) legacy_pins: Vec<crate::core_memory::migration::LegacyPins>,
    #[serde(default)]
    pub(crate) mcp_connections: Vec<crate::mcp::McpConnectionRecord>,
    #[serde(default)]
    pub(crate) community_bundles: Vec<crate::community_bundles::BundleRecord>,
    #[serde(default)]
    pub(crate) community_generation: u64,
    #[serde(default)]
    pub(crate) sessions: Vec<crate::sessions::SessionRecord>,
    /// Tool grant sets already applied (spec §13.3 step 5).
    #[serde(default)]
    pub(crate) tool_grants_applied: Vec<String>,
    #[serde(default)]
    pub(crate) pending_history_deletions: Vec<crate::history::HistoryDeletion>,
    /// Pending approvals and decided ones the history store does not hold
    /// yet (spec §7.3, §13.1).
    #[serde(default)]
    pub(crate) approvals: Vec<crate::approvals::ApprovalRequest>,
    #[serde(default)]
    pub(crate) approval_policies: Vec<crate::approvals::AgentApprovalPolicy>,
    #[serde(default)]
    pub(crate) approval_rules: Vec<crate::approvals::ApprovalRule>,
    /// Keys the owner hands to each agent's coding apps (names only).
    #[serde(default)]
    pub(crate) coding_keys: Vec<crate::coding_keys::AgentCodingKeys>,
    /// How much each agent asks (Free / Strict / Custom) and its floor
    /// extras; agents without a record are Custom.
    #[serde(default)]
    pub(crate) agent_autonomy: Vec<crate::approvals::autonomy::AgentAutonomy>,
    /// Skills agents in Free saved themselves, newest last (at most 200),
    /// for the owner's undo.
    #[serde(default)]
    pub(crate) skill_ledger: Vec<crate::skills::ledger::SkillLedgerEntry>,
    #[serde(default)]
    pub(crate) skill_evaluations: crate::skills::evaluation::EvaluationRegistry,
    /// Repositories where helper worktrees were created, for the startup sweep.
    #[serde(default)]
    pub(crate) helper_worktree_repos: Vec<String>,
    /// The skills registry (spec §8.1); workspace-wide.
    #[serde(default)]
    pub(crate) skills: Vec<crate::skills::SkillRecord>,
    /// Skill drafts waiting for the owner and decided ones (spec §8.2).
    #[serde(default)]
    pub(crate) skill_drafts: Vec<crate::skills::SkillDraft>,
    /// Automation fires the history store does not hold yet (spec §9.1,
    /// §13.1).
    #[serde(default)]
    pub(crate) schedule_fires: Vec<crate::schedules::ScheduleFireRecord>,
    /// The owner's price overrides (spec §11.1).
    #[serde(default)]
    pub(crate) pricing_overrides: Vec<crate::usage::PricingOverride>,
    /// The owner's IANA time zone, as their web console last reported it.
    #[serde(default)]
    pub(crate) owner_time_zone: Option<String>,
    /// The workspace's model for titles, compaction, the smart check and
    /// learning reviews; `None` uses each agent's own.
    #[serde(default)]
    pub(crate) side_model: Option<anima_core::ModelChoice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredSwarmSnapshot {
    pub(crate) config: SwarmConfig,
    pub(crate) state: SwarmState,
}

pub(crate) async fn save_control_plane_snapshot(
    config: Option<&ControlPlaneStoreConfig>,
    snapshot: &ControlPlaneSnapshot,
) -> io::Result<()> {
    let Some(config) = config else {
        return Ok(());
    };

    match config {
        ControlPlaneStoreConfig::Json(path) => save_json_snapshot(path, snapshot),
        ControlPlaneStoreConfig::Postgres(pool) => save_postgres_snapshot(pool, snapshot).await,
    }
}

#[derive(Debug)]
struct DurabilityUncertain;
impl std::fmt::Display for DurabilityUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("control-plane durability is uncertain")
    }
}
impl std::error::Error for DurabilityUncertain {}
pub(crate) fn durability_uncertain_error() -> io::Error {
    io::Error::other(DurabilityUncertain)
}
pub(crate) fn is_durability_uncertain(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<DurabilityUncertain>())
}

pub(crate) async fn load_control_plane_snapshot(
    config: &ControlPlaneStoreConfig,
) -> io::Result<Option<ControlPlaneSnapshot>> {
    match config {
        ControlPlaneStoreConfig::Json(path) => load_json_snapshot(path),
        ControlPlaneStoreConfig::Postgres(pool) => load_postgres_snapshot(pool).await,
    }
}

fn save_json_snapshot(path: &Path, snapshot: &ControlPlaneSnapshot) -> io::Result<()> {
    save_json_snapshot_with_writer(path, snapshot, |file, payload| {
        file.write_all(payload)?;
        file.sync_all()
    })
}

fn save_json_snapshot_with_writer(
    path: &Path,
    snapshot: &ControlPlaneSnapshot,
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    ensure_parent_dir(path)?;
    let payload = serde_json::to_string_pretty(snapshot).map_err(serde_error)?;
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| write(file, payload.as_bytes()))
        .map_err(atomic_write_error)?;
    // Atomic replacement already succeeded. A directory-sync failure must
    // not be reported to callers as proof that the old bytes remain canonical.
    sync_snapshot_parent(path).map_err(|_| durability_uncertain_error())
}

fn load_json_snapshot(path: &Path) -> io::Result<Option<ControlPlaneSnapshot>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(None);
    }

    let mut snapshot =
        serde_json::from_str::<ControlPlaneSnapshot>(&contents).map_err(serde_error)?;
    if snapshot.version == 0 {
        snapshot.version = UNVERSIONED_SNAPSHOT_VERSION;
    }
    if snapshot.version > CONTROL_PLANE_STORE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported control plane store version: {}",
                snapshot.version
            ),
        ));
    }

    Ok(Some(snapshot))
}

/// Where the JSON snapshot is backed up before the sessions upgrade (from a
/// version below `SESSIONS_STORE_VERSION`).
pub(crate) fn pre_sessions_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_SESSIONS_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// Where the JSON snapshot is backed up before the live-runs upgrade (from
/// version `SESSIONS_STORE_VERSION`).
pub(crate) fn pre_live_runs_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_LIVE_RUNS_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// Where the JSON snapshot is backed up before the approvals upgrade (from
/// version `LIVE_RUNS_STORE_VERSION`).
pub(crate) fn pre_approvals_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_APPROVALS_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// Where the JSON snapshot is backed up before the skills upgrade (from
/// version `APPROVALS_STORE_VERSION`).
pub(crate) fn pre_skills_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_SKILLS_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// Where the JSON snapshot is backed up before the automations upgrade (from
/// version `SKILLS_STORE_VERSION`).
pub(crate) fn pre_automations_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_AUTOMATIONS_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// Where the JSON snapshot is backed up before the usage upgrade (from
/// version `AUTOMATIONS_STORE_VERSION`).
pub(crate) fn pre_usage_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_USAGE_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// Where a version 10 snapshot is backed up before attachment steer inputs.
pub(crate) fn pre_attachments_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_ATTACHMENTS_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// Where the JSON snapshot of `loaded_version` is backed up before it is
/// upgraded: each upgrade keeps its own file, so a later upgrade never
/// overwrites an earlier one's backup.
pub(crate) fn pre_upgrade_backup_path(path: &Path, loaded_version: u32) -> PathBuf {
    if loaded_version < SESSIONS_STORE_VERSION {
        pre_sessions_backup_path(path)
    } else if loaded_version < LIVE_RUNS_STORE_VERSION {
        pre_live_runs_backup_path(path)
    } else if loaded_version < APPROVALS_STORE_VERSION {
        pre_approvals_backup_path(path)
    } else if loaded_version < SKILLS_STORE_VERSION {
        pre_skills_backup_path(path)
    } else if loaded_version < AUTOMATIONS_STORE_VERSION {
        pre_automations_backup_path(path)
    } else if loaded_version < USAGE_STORE_VERSION {
        pre_usage_backup_path(path)
    } else if loaded_version < ATTACHMENTS_STORE_VERSION {
        pre_attachments_backup_path(path)
    } else if loaded_version < DISCORD_STORE_VERSION {
        pre_discord_backup_path(path)
    } else if loaded_version < DRIVE_STORE_VERSION {
        pre_drive_backup_path(path)
    } else {
        let mut name = path
            .file_name()
            .map(|name| name.to_os_string())
            .unwrap_or_default();
        name.push(if loaded_version < LEARNING_STORE_VERSION {
            PRE_LEARNING_BACKUP_SUFFIX
        } else if loaded_version < LEARNING_ADOPTION_STORE_VERSION {
            PRE_LEARNING_ADOPTION_BACKUP_SUFFIX
        } else if loaded_version < MEMORY_VIEWS_STORE_VERSION {
            PRE_MEMORY_VIEWS_BACKUP_SUFFIX
        } else if loaded_version < MCP_STORE_VERSION {
            PRE_MCP_BACKUP_SUFFIX
        } else if loaded_version < MCP_STDIO_STORE_VERSION {
            PRE_MCP_STDIO_BACKUP_SUFFIX
        } else if loaded_version < COMMUNITY_BUNDLES_STORE_VERSION {
            PRE_COMMUNITY_BUNDLES_BACKUP_SUFFIX
        } else if loaded_version < ASYNC_HELPERS_STORE_VERSION {
            PRE_ASYNC_HELPERS_BACKUP_SUFFIX
        } else if loaded_version < ASYNC_ORIGIN_BATCHES_STORE_VERSION {
            PRE_ASYNC_ORIGIN_BATCHES_BACKUP_SUFFIX
        } else {
            if loaded_version < 22 {
                PRE_DISCORD_CONVERSATIONS_BACKUP_SUFFIX
            } else if loaded_version < 23 {
                PRE_COMPANION_INTERACTION_BACKUP_SUFFIX
            } else if loaded_version < 24 {
                PRE_WORKFLOWS_BACKUP_SUFFIX
            } else if loaded_version < 25 {
                PRE_WORKFLOW_TEMPLATES_BACKUP_SUFFIX
            } else {
                PRE_MEMORY_MAINTENANCE_BACKUP_SUFFIX
            }
        });
        path.with_file_name(name)
    }
}

pub(crate) fn pre_drive_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_DRIVE_BACKUP_SUFFIX);
    path.with_file_name(name)
}

pub(crate) fn pre_discord_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(PRE_DISCORD_BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// The `host_snapshots` key of the Postgres backup of a `version` snapshot.
pub(crate) fn postgres_backup_key(version: u32) -> String {
    format!("{CONTROL_PLANE_SNAPSHOT_KEY}.backup.{version}")
}

/// Saves the loaded snapshot, unchanged, before the upgrade rewrites it
/// (spec §13.3 step 1), and returns where the backup is.
pub(crate) async fn write_pre_upgrade_backup(
    config: &ControlPlaneStoreConfig,
    loaded_version: u32,
) -> io::Result<String> {
    match config {
        ControlPlaneStoreConfig::Json(path) => {
            backup_json_snapshot(path, loaded_version).map(|backup| backup.display().to_string())
        }
        ControlPlaneStoreConfig::Postgres(pool) => {
            backup_postgres_snapshot(pool, loaded_version).await
        }
    }
}

fn backup_json_snapshot(path: &Path, loaded_version: u32) -> io::Result<PathBuf> {
    let bytes = fs::read(path)?;
    let backup = pre_upgrade_backup_path(path, loaded_version);
    AtomicFile::new(&backup, AllowOverwrite)
        .write(|file| {
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .map_err(atomic_write_error)?;
    sync_snapshot_parent(&backup)?;
    Ok(backup)
}

async fn backup_postgres_snapshot(pool: &PgPool, version: u32) -> io::Result<String> {
    let key = postgres_backup_key(version);
    sqlx::query(
        r#"
        INSERT INTO host_snapshots (key, version, payload, updated_at)
        SELECT $1::text, version, payload, now() FROM host_snapshots WHERE key = $2
        ON CONFLICT (key)
        DO UPDATE SET
            version = EXCLUDED.version,
            payload = EXCLUDED.payload,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(&key)
    .bind(CONTROL_PLANE_SNAPSHOT_KEY)
    .execute(pool)
    .await
    .map_err(postgres_error)?;
    Ok(format!("postgres:host_snapshots/{key}"))
}

async fn save_postgres_snapshot(pool: &PgPool, snapshot: &ControlPlaneSnapshot) -> io::Result<()> {
    let payload = serde_json::to_value(snapshot).map_err(serde_error)?;
    sqlx::query(
        r#"
        INSERT INTO host_snapshots (key, version, payload, updated_at)
        VALUES ($1, $2, $3, now())
        ON CONFLICT (key)
        DO UPDATE SET
            version = EXCLUDED.version,
            payload = EXCLUDED.payload,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(CONTROL_PLANE_SNAPSHOT_KEY)
    .bind(CONTROL_PLANE_STORE_VERSION as i32)
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|_| durability_uncertain_error())?;
    Ok(())
}

async fn load_postgres_snapshot(pool: &PgPool) -> io::Result<Option<ControlPlaneSnapshot>> {
    let Some(row) = sqlx::query("SELECT version, payload FROM host_snapshots WHERE key = $1")
        .bind(CONTROL_PLANE_SNAPSHOT_KEY)
        .fetch_optional(pool)
        .await
        .map_err(postgres_error)?
    else {
        return Ok(None);
    };
    let version: i32 = row.get("version");
    if version > CONTROL_PLANE_STORE_VERSION as i32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported control plane store version: {version}"),
        ));
    }
    let payload: serde_json::Value = row.get("payload");
    let mut snapshot =
        serde_json::from_value::<ControlPlaneSnapshot>(payload).map_err(serde_error)?;
    if snapshot.version == 0 {
        snapshot.version = version.max(1) as u32;
    }
    if snapshot.version > CONTROL_PLANE_STORE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported control plane store version: {}",
                snapshot.version
            ),
        ));
    }
    Ok(Some(snapshot))
}

fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(unix)]
fn snapshot_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(unix)]
fn sync_snapshot_parent(path: &Path) -> io::Result<()> {
    File::open(snapshot_parent(path))?.sync_all()
}

#[cfg(not(unix))]
fn sync_snapshot_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn atomic_write_error(error: atomicwrites::Error<io::Error>) -> io::Error {
    match error {
        atomicwrites::Error::Internal(error) | atomicwrites::Error::User(error) => error,
    }
}

fn serde_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn postgres_error(error: sqlx::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error)
}

impl ControlPlaneSnapshot {
    #[cfg(test)]
    pub(crate) fn new(agents: Vec<AgentRuntimeSnapshot>, swarms: Vec<StoredSwarmSnapshot>) -> Self {
        Self::with_connector_state(agents, swarms, vec![], vec![], vec![], vec![])
    }

    pub(crate) fn with_connector_state(
        agents: Vec<AgentRuntimeSnapshot>,
        swarms: Vec<StoredSwarmSnapshot>,
        connectors: Vec<TelegramConnectorRecord>,
        inbound: Vec<TelegramInboundRecord>,
        outbound: Vec<TelegramOutboundRecord>,
        schedules: Vec<ScheduledPromptRecord>,
    ) -> Self {
        Self::with_connector_state_and_cleanup(
            agents,
            swarms,
            connectors,
            vec![],
            inbound,
            outbound,
            schedules,
        )
    }

    pub(crate) fn with_connector_state_and_cleanup(
        agents: Vec<AgentRuntimeSnapshot>,
        swarms: Vec<StoredSwarmSnapshot>,
        connectors: Vec<TelegramConnectorRecord>,
        credential_cleanup: Vec<TelegramCredentialCleanupIntent>,
        inbound: Vec<TelegramInboundRecord>,
        outbound: Vec<TelegramOutboundRecord>,
        schedules: Vec<ScheduledPromptRecord>,
    ) -> Self {
        Self {
            version: CONTROL_PLANE_STORE_VERSION,
            agents,
            swarms,
            connectors,
            credential_cleanup,
            inbound,
            outbound,
            drive_records: vec![],
            discord_connectors: vec![],
            discord_credential_intents: vec![],
            discord_inbound: vec![],
            discord_outbound: vec![],
            schedules,
            mail_records: vec![],
            jobs: vec![],
            goals: vec![],
            workflows: vec![],
            workflow_templates: vec![],
            mail_drafts: vec![],
            calendar_connectors: vec![],
            calendar_writes: vec![],
            workspace: None,
            runs: vec![],
            sessions: vec![],
            tool_grants_applied: vec![],
            pending_history_deletions: vec![],
            approvals: vec![],
            approval_policies: vec![],
            approval_rules: vec![],
            coding_keys: vec![],
            agent_autonomy: vec![],
            skill_ledger: vec![],
            skill_evaluations: crate::skills::evaluation::EvaluationRegistry::default(),
            helper_worktree_repos: vec![],
            skills: vec![],
            skill_drafts: vec![],
            schedule_fires: vec![],
            pricing_overrides: vec![],
            owner_time_zone: None,
            side_model: None,
            learning_reviews: vec![],
            learning_policies: vec![],
            memory_maintenance: crate::memory_maintenance::MaintenanceRegistry::default(),
            doctor: crate::doctor::DoctorRegistry::default(),
            standing_intents: crate::standing_intents::StandingIntentRegistry::default(),
            paired_devices: crate::paired_devices::PairedDevicesSnapshot::default(),
            clarifications: vec![],
            automation_events: vec![],
            conversation_counts: Default::default(),
            conversation_turns: Default::default(),
            legacy_pins: vec![],
            mcp_connections: vec![],
            community_bundles: vec![],
            community_generation: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlPlaneSnapshot, WorkspaceConfig};
    use std::path::PathBuf;

    #[test]
    fn legacy_memory_pins_are_read_but_never_written() {
        let snapshot: ControlPlaneSnapshot = serde_json::from_value(serde_json::json!({
            "version": 21,
            "memoryViews": [{
                "agentId": "agent-1",
                "version": 2,
                "userProfileIds": ["mem-1"],
                "agentNoteIds": ["mem-2"]
            }]
        }))
        .unwrap();
        assert_eq!(snapshot.legacy_pins[0].agent_id, "agent-1");
        assert_eq!(snapshot.legacy_pins[0].user_profile_ids, ["mem-1"]);
        assert_eq!(snapshot.legacy_pins[0].agent_note_ids, ["mem-2"]);
        let written = serde_json::to_value(&snapshot).unwrap();
        assert!(written.get("memoryViews").is_none());
    }

    #[test]
    fn v3_snapshot_without_workspace_loads_as_unconfigured() {
        let json = r#"{
            "version": 3,
            "agents": [],
            "swarms": [],
            "connectors": [],
            "credentialCleanup": [],
            "inbound": [],
            "outbound": [],
            "schedules": []
        }"#;
        let snapshot: ControlPlaneSnapshot =
            serde_json::from_str(json).expect("v3 snapshot parses");
        assert!(snapshot.workspace.is_none());
    }

    #[test]
    fn workspace_config_round_trips() {
        let config = WorkspaceConfig {
            root_path: PathBuf::from("C:\\workspaces\\northwind"),
            company_name: "Northwind Research".into(),
            mission: "Continuous equity research".into(),
            values: vec!["cite sources".into()],
        };
        let snapshot = ControlPlaneSnapshot {
            workspace: Some(config.clone()),
            ..Default::default()
        };
        let payload = serde_json::to_string(&snapshot).expect("serialize");
        let restored: ControlPlaneSnapshot = serde_json::from_str(&payload).expect("deserialize");
        assert_eq!(restored.workspace, Some(config));
    }

    #[test]
    fn json_snapshot_replaces_an_existing_snapshot_only_after_a_synced_temp_write() {
        let path = test_snapshot_path("atomic-replace");
        let mut previous = ControlPlaneSnapshot::new(vec![], vec![]);
        previous.version = 1;
        let replacement = ControlPlaneSnapshot::new(vec![], vec![]);
        let previous_payload = serde_json::to_string_pretty(&previous).expect("serializes");
        std::fs::write(&path, &previous_payload).expect("previous snapshot should be written");

        super::save_json_snapshot_with_writer(&path, &replacement, |file, payload| {
            use std::io::Write;

            file.write_all(payload)?;
            file.sync_all()?;
            assert_eq!(
                std::fs::read_to_string(&path)?,
                previous_payload,
                "the prior snapshot remains intact until the complete temp write finishes"
            );
            Ok(())
        })
        .expect("atomic replacement should succeed");

        let loaded = super::load_json_snapshot(&path)
            .expect("replacement should load")
            .expect("replacement should exist");
        assert_eq!(loaded.version, super::CONTROL_PLANE_STORE_VERSION);
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().expect("snapshot path has a parent"));
    }

    #[test]
    fn json_snapshot_replacement_failure_preserves_prior_snapshot_and_cleans_temp_file() {
        let path = test_snapshot_path("atomic-failure");
        let mut previous = ControlPlaneSnapshot::new(vec![], vec![]);
        previous.version = 1;
        let replacement = ControlPlaneSnapshot::new(vec![], vec![]);
        let previous_payload = serde_json::to_string_pretty(&previous).expect("serializes");
        std::fs::write(&path, &previous_payload).expect("previous snapshot should be written");

        let error = super::save_json_snapshot_with_writer(&path, &replacement, |file, _| {
            use std::io::Write;

            file.write_all(b"partial")?;
            Err(std::io::Error::other("simulated replacement failure"))
        })
        .expect_err("replacement failure should be returned");
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        assert_eq!(
            std::fs::read_to_string(&path).expect("prior snapshot remains readable"),
            previous_payload
        );
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().expect("snapshot path has a parent"));
    }

    fn test_snapshot_path(label: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "anima-control-plane-{label}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).expect("snapshot test directory should be created");
        directory.join("control-plane.json")
    }

    fn assert_no_temp_residue(path: &std::path::Path) {
        let parent = path.parent().expect("snapshot path should have a parent");
        let temp_prefix = ".atomicwrite";
        let residue = std::fs::read_dir(parent)
            .expect("snapshot parent should be readable")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(temp_prefix));
        assert!(!residue, "temporary snapshot files must be cleaned up");
    }

    #[test]
    fn snapshot_serializes_current_version_with_empty_connector_collections() {
        let snapshot = ControlPlaneSnapshot::new(vec![], vec![]);
        let payload = serde_json::to_value(snapshot).expect("snapshot should serialize");

        assert_eq!(
            payload["version"],
            crate::control_plane_store::CONTROL_PLANE_STORE_VERSION
        );
        assert_eq!(payload["scheduleFires"], serde_json::json!([]));
        assert_eq!(payload["connectors"], serde_json::json!([]));
        assert_eq!(payload["credentialCleanup"], serde_json::json!([]));
        assert_eq!(payload["inbound"], serde_json::json!([]));
        assert_eq!(payload["outbound"], serde_json::json!([]));
        assert_eq!(payload["schedules"], serde_json::json!([]));
        assert_eq!(payload["runs"], serde_json::json!([]));
        assert_eq!(payload["sessions"], serde_json::json!([]));
        assert_eq!(payload["pendingHistoryDeletions"], serde_json::json!([]));
        assert_eq!(payload["skills"], serde_json::json!([]));
        assert_eq!(payload["skillDrafts"], serde_json::json!([]));
        assert_eq!(payload["approvals"], serde_json::json!([]));
        assert_eq!(payload["approvalPolicies"], serde_json::json!([]));
        assert_eq!(payload["approvalRules"], serde_json::json!([]));
    }

    #[test]
    fn version_one_payload_round_trips_new_collections_as_empty() {
        let snapshot: ControlPlaneSnapshot = serde_json::from_value(serde_json::json!({
            "version": 1,
            "agents": [],
            "swarms": []
        }))
        .expect("version-one snapshot should deserialize");

        let payload = serde_json::to_value(snapshot).expect("snapshot should serialize");
        assert_eq!(payload["connectors"], serde_json::json!([]));
        assert_eq!(payload["credentialCleanup"], serde_json::json!([]));
        assert_eq!(payload["inbound"], serde_json::json!([]));
        assert_eq!(payload["outbound"], serde_json::json!([]));
        assert_eq!(payload["schedules"], serde_json::json!([]));
    }

    #[test]
    fn version_two_payload_defaults_cleanup_intents_to_empty() {
        let snapshot: ControlPlaneSnapshot = serde_json::from_value(serde_json::json!({
            "version": 2,
            "agents": [],
            "swarms": [],
            "connectors": [],
            "inbound": [],
            "outbound": [],
            "schedules": []
        }))
        .expect("version-two snapshot should deserialize");

        assert!(snapshot.credential_cleanup.is_empty());
    }

    #[test]
    fn version_one_json_file_loads_new_collections_as_empty() {
        let path = std::env::temp_dir().join(format!(
            "anima-control-plane-v1-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        std::fs::write(&path, r#"{"version":1,"agents":[],"swarms":[]}"#)
            .expect("version-one snapshot should be written");

        let snapshot = super::load_json_snapshot(&path)
            .expect("version-one snapshot should load")
            .expect("version-one snapshot should exist");
        assert!(snapshot.connectors.is_empty());
        assert!(snapshot.credential_cleanup.is_empty());
        assert!(snapshot.inbound.is_empty());
        assert!(snapshot.outbound.is_empty());
        assert!(snapshot.schedules.is_empty());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn version_one_postgres_payload_deserializes_new_collections_as_empty() {
        let snapshot: ControlPlaneSnapshot = serde_json::from_value(serde_json::json!({
            "agents": [],
            "swarms": []
        }))
        .expect("version-one postgres payload should deserialize");

        assert_eq!(
            snapshot.version, 0,
            "the row version supplies v1 when absent"
        );
        assert!(snapshot.connectors.is_empty());
        assert!(snapshot.credential_cleanup.is_empty());
        assert!(snapshot.inbound.is_empty());
        assert!(snapshot.outbound.is_empty());
        assert!(snapshot.schedules.is_empty());
    }

    #[test]
    fn version_four_snapshot_loads_with_an_empty_run_ledger() {
        let snapshot: ControlPlaneSnapshot = serde_json::from_value(serde_json::json!({
            "version": 4,
            "agents": [],
            "swarms": []
        }))
        .expect("version-four snapshot should deserialize");

        assert!(snapshot.runs.is_empty());
        assert!(snapshot.pending_history_deletions.is_empty());
    }

    #[test]
    fn unversioned_json_snapshots_load_as_legacy_version_one() {
        let path = test_snapshot_path("unversioned");
        std::fs::write(&path, r#"{"agents":[],"swarms":[]}"#).unwrap();

        let loaded = super::load_json_snapshot(&path).unwrap().unwrap();

        assert_eq!(loaded.version, 1, "an unversioned file predates sessions");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_backup_path_appends_the_suffix_to_the_file_name() {
        assert_eq!(
            super::pre_sessions_backup_path(std::path::Path::new("/data/control-plane.json")),
            std::path::PathBuf::from("/data/control-plane.json.pre-sessions.bak")
        );
        assert_eq!(super::postgres_backup_key(4), "control_plane.backup.4");
        assert_eq!(super::postgres_backup_key(5), "control_plane.backup.5");
        assert_eq!(super::postgres_backup_key(7), "control_plane.backup.7");
        assert_eq!(super::postgres_backup_key(8), "control_plane.backup.8");
    }

    #[test]
    fn the_postgres_backup_key_for_version_nine() {
        assert_eq!(super::postgres_backup_key(9), "control_plane.backup.9");
    }

    #[test]
    fn the_version_constants() {
        assert_eq!(
            ControlPlaneSnapshot::new(vec![], vec![]).version,
            super::CONTROL_PLANE_STORE_VERSION
        );
        assert_eq!(super::USAGE_STORE_VERSION, 10);
        assert_eq!(super::PRE_ATTACHMENTS_BACKUP_SUFFIX, ".pre-attachments.bak");
        assert_eq!(super::postgres_backup_key(10), "control_plane.backup.10");
        assert_eq!(super::AUTOMATIONS_STORE_VERSION, 9);
        assert_eq!(super::PRE_USAGE_BACKUP_SUFFIX, ".pre-usage.bak");
    }

    /// Controller ruling (M3 pre-flight audit I2): each upgrade keeps its own
    /// backup, so the M3 upgrade never overwrites the one M2 wrote.
    #[test]
    fn the_backup_is_named_by_the_version_it_upgrades_from() {
        let path = std::path::Path::new("/data/control-plane.json");
        for version in [1, 2, 3, 4] {
            assert_eq!(
                super::pre_upgrade_backup_path(path, version),
                std::path::PathBuf::from("/data/control-plane.json.pre-sessions.bak"),
                "version {version}"
            );
        }
        assert_eq!(
            super::pre_upgrade_backup_path(path, 5),
            std::path::PathBuf::from("/data/control-plane.json.pre-live-runs.bak")
        );
        assert_eq!(
            super::pre_live_runs_backup_path(path),
            super::pre_upgrade_backup_path(path, 5)
        );
        assert_eq!(
            super::pre_upgrade_backup_path(path, 6),
            std::path::PathBuf::from("/data/control-plane.json.pre-approvals.bak")
        );
        assert_eq!(
            super::pre_approvals_backup_path(path),
            super::pre_upgrade_backup_path(path, 6)
        );
        assert_eq!(
            super::pre_upgrade_backup_path(path, 7),
            std::path::PathBuf::from("/data/control-plane.json.pre-skills.bak")
        );
        assert_eq!(
            super::pre_skills_backup_path(path),
            super::pre_upgrade_backup_path(path, 7)
        );
        assert_eq!(
            super::pre_upgrade_backup_path(path, 8),
            std::path::PathBuf::from("/data/control-plane.json.pre-automations.bak")
        );
        assert_eq!(
            super::pre_automations_backup_path(path),
            super::pre_upgrade_backup_path(path, 8)
        );
        assert_eq!(
            super::pre_upgrade_backup_path(path, 9),
            std::path::PathBuf::from("/data/control-plane.json.pre-usage.bak")
        );
        assert_eq!(
            super::pre_usage_backup_path(path),
            super::pre_upgrade_backup_path(path, 9)
        );
    }

    #[test]
    fn backup_paths_follow_the_loaded_version() {
        let path = std::path::Path::new("/data/control-plane.json");
        for (loaded, suffix) in [
            (4, ".pre-sessions.bak"),
            (5, ".pre-live-runs.bak"),
            (6, ".pre-approvals.bak"),
            (7, ".pre-skills.bak"),
            (8, ".pre-automations.bak"),
            (9, ".pre-usage.bak"),
            (10, ".pre-attachments.bak"),
            (20, ".pre-async-origin-batches.bak"),
            (21, ".pre-discord-conversations.bak"),
            (22, ".pre-companion-interaction.bak"),
            (23, ".pre-workflows.bak"),
        ] {
            assert_eq!(
                super::pre_upgrade_backup_path(path, loaded),
                std::path::PathBuf::from(format!("/data/control-plane.json{suffix}")),
                "loaded version {loaded}"
            );
        }
    }

    #[tokio::test]
    async fn workflow_upgrade_keeps_an_independent_exact_version_twenty_three_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let original =
            b"{\r\n  \"version\" : 23,\r\n  \"agents\": [],\r\n  \"swarms\": []\r\n}\r\n";
        std::fs::write(&path, original).unwrap();
        let previous_backup = super::pre_upgrade_backup_path(&path, 22);
        let previous_bytes = b"earlier companion-interaction backup\r\n";
        std::fs::write(&previous_backup, previous_bytes).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());
        let mut snapshot = super::load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.version, 23);
        assert!(snapshot.workflows.is_empty());

        let backup = super::pre_upgrade_backup_path(&path, snapshot.version);
        assert_eq!(
            backup,
            path.with_file_name("control-plane.json.pre-workflows.bak")
        );
        assert_ne!(backup, previous_backup);
        let location = super::write_pre_upgrade_backup(&config, snapshot.version)
            .await
            .unwrap();
        assert_eq!(location, backup.display().to_string());
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(&previous_backup).unwrap(), previous_bytes);

        snapshot.version = super::CONTROL_PLANE_STORE_VERSION;
        super::save_control_plane_snapshot(Some(&config), &snapshot)
            .await
            .unwrap();
        let upgraded = super::load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(upgraded.version, super::CONTROL_PLANE_STORE_VERSION);
        assert!(upgraded.workflows.is_empty());
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        assert_eq!(std::fs::read(previous_backup).unwrap(), previous_bytes);
        assert_no_temp_residue(&path);
    }

    #[tokio::test]
    async fn discord_conversation_upgrade_keeps_an_independent_exact_version_twenty_one_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let original = "{\n  \"version\": 21, \"agents\": [], \"swarms\": []\n}\n";
        std::fs::write(&path, original).unwrap();
        let previous_backup = super::pre_upgrade_backup_path(&path, 20);
        std::fs::write(&previous_backup, "previous backup").unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());
        let mut snapshot = super::load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.version, 21);
        let backup = super::pre_upgrade_backup_path(&path, snapshot.version);
        assert_ne!(backup, previous_backup);
        super::write_pre_upgrade_backup(&config, snapshot.version)
            .await
            .unwrap();
        snapshot.version = super::CONTROL_PLANE_STORE_VERSION;
        super::save_control_plane_snapshot(Some(&config), &snapshot)
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(previous_backup).unwrap(),
            "previous backup"
        );
        assert_eq!(
            super::load_control_plane_snapshot(&config)
                .await
                .unwrap()
                .unwrap()
                .version,
            super::CONTROL_PLANE_STORE_VERSION
        );
    }

    #[tokio::test]
    async fn companion_upgrade_keeps_exact_version_twenty_two_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control-plane.json");
        let original = "{\n  \"version\": 22, \"agents\": [], \"swarms\": []\n}\n";
        std::fs::write(&path, original).unwrap();
        let previous_backup = super::pre_upgrade_backup_path(&path, 21);
        std::fs::write(&previous_backup, "previous backup").unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());
        let mut snapshot = super::load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap();
        let backup = super::pre_upgrade_backup_path(&path, 22);
        assert_ne!(backup, previous_backup);
        assert!(backup
            .to_string_lossy()
            .ends_with(super::PRE_COMPANION_INTERACTION_BACKUP_SUFFIX));
        super::write_pre_upgrade_backup(&config, 22).await.unwrap();
        snapshot.version = super::CONTROL_PLANE_STORE_VERSION;
        super::save_control_plane_snapshot(Some(&config), &snapshot)
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(previous_backup).unwrap(),
            "previous backup"
        );
        assert_eq!(
            super::load_control_plane_snapshot(&config)
                .await
                .unwrap()
                .unwrap()
                .version,
            super::CONTROL_PLANE_STORE_VERSION
        );
    }

    #[tokio::test]
    async fn a_version_five_backup_leaves_the_pre_sessions_backup_alone() {
        let path = test_snapshot_path("live-runs-backup");
        let m2_backup = "{\"version\":4,\"agents\":[],\"swarms\":[]}";
        std::fs::write(super::pre_sessions_backup_path(&path), m2_backup).unwrap();
        let original = "{\n  \"version\": 5,\n  \"agents\": [],\n  \"swarms\": []\n}\n";
        std::fs::write(&path, original).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());

        let location = super::write_pre_upgrade_backup(&config, 5).await.unwrap();

        let backup = super::pre_live_runs_backup_path(&path);
        assert_eq!(location, backup.display().to_string());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(super::pre_sessions_backup_path(&path)).unwrap(),
            m2_backup,
            "the M2 upgrade's backup survives the M3 upgrade"
        );
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_version_six_backup_leaves_the_earlier_backups_alone() {
        let path = test_snapshot_path("approvals-backup");
        let m3_backup = "{\"version\":5,\"agents\":[],\"swarms\":[]}";
        std::fs::write(super::pre_live_runs_backup_path(&path), m3_backup).unwrap();
        let original = "{\n  \"version\": 6,\n  \"agents\": [],\n  \"swarms\": []\n}\n";
        std::fs::write(&path, original).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());

        let location = super::write_pre_upgrade_backup(&config, 6).await.unwrap();

        let backup = super::pre_approvals_backup_path(&path);
        assert_eq!(location, backup.display().to_string());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(super::pre_live_runs_backup_path(&path)).unwrap(),
            m3_backup,
            "the M3 upgrade's backup survives the M4 upgrade"
        );
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_version_seven_backup_leaves_the_earlier_backups_alone() {
        let path = test_snapshot_path("skills-backup");
        let m4_backup = "{\"version\":6,\"agents\":[],\"swarms\":[]}";
        std::fs::write(super::pre_approvals_backup_path(&path), m4_backup).unwrap();
        let original = "{
  \"version\": 7,
  \"agents\": [],
  \"swarms\": []
}
";
        std::fs::write(&path, original).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());

        let location = super::write_pre_upgrade_backup(&config, 7).await.unwrap();

        let backup = super::pre_skills_backup_path(&path);
        assert_eq!(location, backup.display().to_string());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(super::pre_approvals_backup_path(&path)).unwrap(),
            m4_backup,
            "the M4 upgrade's backup survives the M5 upgrade"
        );
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_version_nine_backup_leaves_the_earlier_backups_alone() {
        let path = test_snapshot_path("usage-backup");
        let m6_backup = "{\"version\":8,\"agents\":[],\"swarms\":[]}";
        std::fs::write(super::pre_automations_backup_path(&path), m6_backup).unwrap();
        let original = "{\n  \"version\": 9,\n  \"agents\": [],\n  \"swarms\": []\n}\n";
        std::fs::write(&path, original).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());

        let location = super::write_pre_upgrade_backup(&config, 9).await.unwrap();

        let backup = super::pre_usage_backup_path(&path);
        assert_eq!(location, backup.display().to_string());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(super::pre_automations_backup_path(&path)).unwrap(),
            m6_backup,
            "the M6 upgrade's backup survives the M8 upgrade"
        );
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_version_eight_backup_leaves_the_earlier_backups_alone() {
        let path = test_snapshot_path("automations-backup");
        let m5_backup = "{\"version\":7,\"agents\":[],\"swarms\":[]}";
        std::fs::write(super::pre_skills_backup_path(&path), m5_backup).unwrap();
        let original = "{
  \"version\": 8,
  \"agents\": [],
  \"swarms\": []
}
";
        std::fs::write(&path, original).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());

        let location = super::write_pre_upgrade_backup(&config, 8).await.unwrap();

        let backup = super::pre_automations_backup_path(&path);
        assert_eq!(location, backup.display().to_string());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(super::pre_skills_backup_path(&path)).unwrap(),
            m5_backup,
            "the M5 upgrade's backup survives the M6 upgrade"
        );
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn the_pre_upgrade_backup_copies_the_exact_file_bytes_and_a_later_upgrade_replaces_it() {
        let path = test_snapshot_path("backup");
        let original = "{\n  \"version\": 4,\n  \"agents\": [],\n  \"swarms\": []\n}\n";
        std::fs::write(&path, original).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());

        let location = super::write_pre_upgrade_backup(&config, 4).await.unwrap();

        let backup = super::pre_sessions_backup_path(&path);
        assert_eq!(location, backup.display().to_string());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        std::fs::write(&path, "{\"version\":3}").unwrap();
        super::write_pre_upgrade_backup(&config, 3).await.unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{\"version\":3}");
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[ignore = "requires DATABASE_URL-backed Postgres"]
    #[sqlx::test(migrations = "./migrations")]
    async fn the_postgres_pre_upgrade_backup_copies_the_snapshot_row(pool: sqlx::PgPool) {
        use sqlx::Row;

        sqlx::query(
            "INSERT INTO host_snapshots (key, version, payload) VALUES ('control_plane', 4, '{\"version\":4,\"agents\":[]}')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let config = super::ControlPlaneStoreConfig::Postgres(pool.clone());

        let location = super::write_pre_upgrade_backup(&config, 4).await.unwrap();

        assert_eq!(location, "postgres:host_snapshots/control_plane.backup.4");
        let row = sqlx::query(
            "SELECT version, payload FROM host_snapshots WHERE key = 'control_plane.backup.4'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<i32, _>("version"), 4);
        assert_eq!(
            row.get::<serde_json::Value, _>("payload"),
            serde_json::json!({"version": 4, "agents": []})
        );
    }
    #[tokio::test]
    async fn version_ten_attachment_backup_preserves_usage_backup_and_exact_bytes() {
        let path = test_snapshot_path("attachments-backup");
        let prior = "usage backup";
        std::fs::write(super::pre_usage_backup_path(&path), prior).unwrap();
        let original = "{\n  \"version\": 10, \"agents\": [], \"swarms\": []\n}\n";
        std::fs::write(&path, original).unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());
        let backup = super::write_pre_upgrade_backup(&config, 10).await.unwrap();
        assert_eq!(
            backup,
            super::pre_attachments_backup_path(&path)
                .display()
                .to_string()
        );
        assert_eq!(
            std::fs::read_to_string(super::pre_attachments_backup_path(&path)).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(super::pre_usage_backup_path(&path)).unwrap(),
            prior
        );
        assert_eq!(super::postgres_backup_key(10), "control_plane.backup.10");
        assert_no_temp_residue(&path);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn future_attachment_snapshot_is_refused() {
        let path = test_snapshot_path("future-attachments");
        std::fs::write(&path,serde_json::json!({"version":super::CONTROL_PLANE_STORE_VERSION+1,"agents":[],"swarms":[]}).to_string()).unwrap();
        assert!(super::load_json_snapshot(&path)
            .unwrap_err()
            .to_string()
            .contains("unsupported control plane store version"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

#[cfg(test)]
mod discord_upgrade_tests {
    use super::*;
    #[tokio::test]
    async fn version_eleven_gets_independent_discord_backup_and_empty_native_journals() {
        let path = std::env::temp_dir().join(format!(
            "anima-discord-upgrade-{}.json",
            uuid::Uuid::new_v4()
        ));
        let original = "{\"version\":11,\"agents\":[],\"swarms\":[]}";
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let loaded = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert!(loaded.discord_connectors.is_empty());
        assert!(loaded.discord_inbound.is_empty());
        assert!(loaded.discord_outbound.is_empty());
        assert_eq!(loaded.version, 11);
        write_pre_upgrade_backup(&config, 11).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(pre_discord_backup_path(&path)).unwrap(),
            original
        );
        assert_ne!(
            pre_discord_backup_path(&path),
            pre_attachments_backup_path(&path)
        );
        std::fs::remove_file(pre_discord_backup_path(&path)).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod drive_snapshot_tests {
    use super::*;
    #[tokio::test]
    async fn version_twelve_gets_independent_drive_backup_and_empty_selected_records() {
        let path =
            std::env::temp_dir().join(format!("anima-drive-upgrade-{}.json", uuid::Uuid::new_v4()));
        let original = "{\"version\":12,\"agents\":[],\"swarms\":[]}";
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let loaded = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert!(loaded.drive_records.is_empty());
        write_pre_upgrade_backup(&config, 12).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(pre_drive_backup_path(&path)).unwrap(),
            original
        );
        assert_ne!(pre_drive_backup_path(&path), pre_discord_backup_path(&path));
        std::fs::remove_file(pre_drive_backup_path(&path)).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod learning_snapshot_tests {
    use super::*;

    #[tokio::test]
    async fn m42_version_sixteen_has_independent_exact_backup_and_empty_catalog() {
        let path =
            std::env::temp_dir().join(format!("anima-mcp-upgrade-{}.json", uuid::Uuid::new_v4()));
        let original = "{\"version\":16,\"agents\":[],\"swarms\":[]}";
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let loaded = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert!(loaded.mcp_connections.is_empty());
        let prior = pre_upgrade_backup_path(&path, 15);
        std::fs::write(&prior, "memory reference backup").unwrap();
        write_pre_upgrade_backup(&config, 16).await.unwrap();
        let backup = pre_upgrade_backup_path(&path, 16);
        assert_ne!(backup, prior);
        assert!(backup.to_string_lossy().ends_with(PRE_MCP_BACKUP_SUFFIX));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&prior).unwrap(),
            "memory reference backup"
        );
        for file in [backup, prior, path] {
            std::fs::remove_file(file).unwrap();
        }
    }

    #[tokio::test]
    async fn m44_version_seventeen_gets_an_independent_exact_stdio_backup() {
        let path = std::env::temp_dir().join(format!(
            "anima-mcp-stdio-upgrade-{}.json",
            uuid::Uuid::new_v4()
        ));
        let original = r#"{"version":17,"agents":[],"swarms":[],"mcpConnections":[{"id":"00000000-0000-4000-8000-000000000017","agentId":"owner","label":"Existing HTTP","endpoint":"https://example.com/mcp","enabled":false,"version":1,"reviewedTools":[]}]}"#;
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let loaded = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert_eq!(loaded.mcp_connections.len(), 1);
        assert!(loaded.mcp_connections[0].stdio.is_none());
        assert_eq!(
            loaded.mcp_connections[0].endpoint,
            "https://example.com/mcp"
        );
        let prior = pre_upgrade_backup_path(&path, 16);
        std::fs::write(&prior, "M42 backup remains untouched").unwrap();

        write_pre_upgrade_backup(&config, 17).await.unwrap();

        let backup = pre_upgrade_backup_path(&path, 17);
        assert_ne!(backup, prior);
        assert!(backup
            .to_string_lossy()
            .ends_with(PRE_MCP_STDIO_BACKUP_SUFFIX));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&prior).unwrap(),
            "M42 backup remains untouched"
        );
        for file in [backup, prior, path] {
            std::fs::remove_file(file).unwrap();
        }
    }

    #[tokio::test]
    async fn m40_version_fifteen_defaults_empty_and_has_an_independent_exact_backup() {
        let path = std::env::temp_dir().join(format!(
            "anima-memory-views-upgrade-{}.json",
            uuid::Uuid::new_v4()
        ));
        let original = "{\"version\":15,\"agents\":[],\"swarms\":[]}";
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let loaded = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert!(loaded.legacy_pins.is_empty());
        let prior_backup = pre_upgrade_backup_path(&path, 14);
        std::fs::write(&prior_backup, "prior adoption backup").unwrap();
        write_pre_upgrade_backup(&config, 15).await.unwrap();
        let backup = pre_upgrade_backup_path(&path, 15);
        assert_ne!(backup, prior_backup);
        assert!(backup
            .to_string_lossy()
            .ends_with(PRE_MEMORY_VIEWS_BACKUP_SUFFIX));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&prior_backup).unwrap(),
            "prior adoption backup"
        );
        write_pre_upgrade_backup(&config, 15).await.unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_file(prior_backup).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn m39_version_fourteen_has_an_independent_exact_adoption_backup() {
        let path = std::env::temp_dir().join(format!(
            "anima-learning-adoption-{}.json",
            uuid::Uuid::new_v4()
        ));
        let original = "{\"version\":14,\"agents\":[],\"swarms\":[]}";
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let loaded = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert!(loaded.skill_drafts.is_empty());
        assert!(loaded.learning_reviews.is_empty());
        let old_backup = pre_upgrade_backup_path(&path, 13);
        std::fs::write(&old_backup, "older backup").unwrap();
        write_pre_upgrade_backup(&config, 14).await.unwrap();
        let backup = pre_upgrade_backup_path(&path, 14);
        assert_ne!(backup, old_backup);
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&old_backup).unwrap(),
            "older backup"
        );
        assert_eq!(loaded.version, 14);
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_file(old_backup).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn m38_version_thirteen_defaults_off_and_has_an_independent_exact_backup() {
        let path = std::env::temp_dir().join(format!(
            "anima-learning-upgrade-{}.json",
            uuid::Uuid::new_v4()
        ));
        let original = "{\"version\":13,\"agents\":[],\"swarms\":[]}";
        std::fs::write(&path, original).unwrap();
        let config = ControlPlaneStoreConfig::Json(path.clone());
        let loaded = load_control_plane_snapshot(&config).await.unwrap().unwrap();
        assert!(loaded.learning_reviews.is_empty());
        assert!(loaded.learning_policies.is_empty());
        write_pre_upgrade_backup(&config, 13).await.unwrap();
        let backup = pre_upgrade_backup_path(&path, 13);
        assert_ne!(backup, pre_drive_backup_path(&path));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        write_pre_upgrade_backup(&config, 13).await.unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn version_twenty_four_templates_default_empty_and_keep_workflow_backup() {
        let directory =
            std::env::temp_dir().join(format!("anima-template-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("control-plane.json");
        let original = "{\"version\":24,\"agents\":[],\"workflows\":[]}";
        std::fs::write(&path, original).unwrap();
        let prior = super::pre_upgrade_backup_path(&path, 23);
        std::fs::write(&prior, "prior workflow backup").unwrap();
        let config = super::ControlPlaneStoreConfig::Json(path.clone());
        let loaded = super::load_control_plane_snapshot(&config)
            .await
            .unwrap()
            .unwrap();
        assert!(loaded.workflow_templates.is_empty());
        let backup = super::pre_upgrade_backup_path(&path, 24);
        assert_ne!(backup, prior);
        assert!(backup
            .to_string_lossy()
            .ends_with(".pre-workflow-templates.bak"));
        super::write_pre_upgrade_backup(&config, 24).await.unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&prior).unwrap(),
            "prior workflow backup"
        );
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 3);
        for file in [path, prior, backup] {
            std::fs::remove_file(file).unwrap();
        }
        std::fs::remove_dir(directory).unwrap();
    }
}
