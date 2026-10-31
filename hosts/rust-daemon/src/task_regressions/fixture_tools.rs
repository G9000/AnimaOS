//! Fixture-owned files and simulated effects, with no canonical host tool access.
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

use anima_core::execution_limits::{ExecutionContext, SettlementOutcome};
use anima_core::task_regressions::{ActorFixture, MockEffect};
use anima_core::{CancelSignal, Content, DataValue, TaskResult, ToolCall, ToolDescriptor};
use sha2::{Digest, Sha256};

use crate::tools::workspace_bytes::WorkspaceBytes;

const FILE_BYTES: usize = 64 * 1024;
const MAX_EFFECTS: usize = 64;

pub(super) struct FixtureWorkspace {
    _temporary: tempfile::TempDir,
    held: WorkspaceBytes,
    actor: ActorFixture,
    allow_effects: bool,
    effects: Mutex<Vec<MockEffect>>,
    sealed: AtomicBool,
    identity: String,
    next_operation: AtomicU64,
}

/// Validated actor operations retain their exact content and filesystem preimage.
#[derive(Debug)]
pub(super) enum PreparedFixtureOperation {
    Read(String),
    Write(PreparedFixtureWrite),
    RecordEffect(PreparedFixtureEffect),
}

#[derive(Debug)]
pub(super) struct PreparedFixtureWrite {
    path: String,
    previous: Option<String>,
    content: String,
    receipt: &'static str,
}

#[derive(Debug)]
pub(super) struct PreparedFixtureEffect {
    key: String,
    payload: String,
}

impl PreparedFixtureOperation {
    fn artifact_bytes(&self) -> u64 {
        match self {
            Self::Write(write) => write.content.len() as u64,
            Self::Read(_) | Self::RecordEffect(_) => 0,
        }
    }
}

pub(super) fn fingerprint() -> String {
    hex::encode(Sha256::digest(include_bytes!("fixture_tools.rs")))
}

impl FixtureWorkspace {
    pub(super) fn new(actor: &ActorFixture, allow_effects: bool) -> Result<Self, String> {
        actor.validate()?;
        let temporary = tempfile::Builder::new()
            .prefix("anima-task-regression-")
            .tempdir()
            .map_err(|_| "fixture workspace unavailable")?;
        let held =
            WorkspaceBytes::open(temporary.path()).map_err(|_| "fixture capability unavailable")?;
        for (name, content) in &actor.initial_files {
            held.create_new_bytes(name, content.as_bytes())
                .map_err(|_| "fixture setup failed")?;
        }
        Ok(Self {
            _temporary: temporary,
            held,
            actor: actor.clone(),
            allow_effects: allow_effects && actor.mock_effects,
            effects: Mutex::new(Vec::new()),
            sealed: AtomicBool::new(false),
            identity: uuid::Uuid::new_v4().to_string(),
            next_operation: AtomicU64::new(0),
        })
    }

    #[cfg(test)]
    pub(super) fn path(&self) -> &Path {
        self._temporary.path()
    }

    #[cfg(test)]
    pub(super) fn descriptors(&self) -> Vec<ToolDescriptor> {
        public_descriptors(&self.actor, self.allow_effects)
    }

    fn read(&self, path: &str) -> Result<String, &'static str> {
        if self
            .held
            .instruction_link_count(path)
            .map_err(|_| "fixture_file_unavailable")?
            != Some(1)
        {
            return Err("fixture_file_unavailable");
        }
        String::from_utf8(
            self.held
                .read_file_snapshot(path, FILE_BYTES)
                .map_err(|_| "fixture_file_unavailable")?,
        )
        .map_err(|_| "fixture_file_unavailable")
    }

    fn check_controlled(
        &self,
        context: &ExecutionContext,
        logical_cancel: &CancelSignal,
    ) -> Result<(), &'static str> {
        if self.sealed.load(Ordering::SeqCst)
            || logical_cancel.is_cancelled()
            || context.check_live().is_err()
        {
            Err("fixture_authority_closed")
        } else {
            Ok(())
        }
    }

    fn logical_operation_id(&self, context: &ExecutionContext) -> Result<String, &'static str> {
        let sequence = self
            .next_operation
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "fixture_operation_limit")?;
        let mut identity = Sha256::new();
        identity.update(context.run_id.as_bytes());
        identity.update([0]);
        identity.update(self.identity.as_bytes());
        Ok(format!(
            "fixture:{}:{sequence}",
            hex::encode(identity.finalize())
        ))
    }

    /// Captures bounded actor-authorized inputs without changing fixture files.
    pub(super) fn prepare(&self, call: ToolCall) -> Result<PreparedFixtureOperation, &'static str> {
        if self.sealed.load(Ordering::SeqCst) {
            return Err("fixture_closed");
        }
        let get = |key: &str| match call.args.get(key) {
            Some(DataValue::String(value)) => Ok(value.as_str()),
            _ => Err("fixture_invalid_arguments"),
        };
        if call.name == "record_effect" {
            if !self.allow_effects || call.args.len() != 2 {
                return Err("fixture_tool_denied");
            }
            let (key, payload) = (get("key")?, get("payload")?);
            if key.is_empty()
                || key.len() > 128
                || !key.bytes().all(|byte| byte.is_ascii_graphic())
                || payload.len() > 4096
            {
                return Err("fixture_content_limit");
            }
            return Ok(PreparedFixtureOperation::RecordEffect(
                PreparedFixtureEffect {
                    key: key.into(),
                    payload: payload.into(),
                },
            ));
        }
        let path = get("path")?;
        match call.name.as_str() {
            "read_file"
                if call.args.len() == 1 && self.actor.readable_files.iter().any(|p| p == path) =>
            {
                Ok(PreparedFixtureOperation::Read(self.read(path)?))
            }
            "write_file"
                if call.args.len() == 2 && self.actor.writable_files.iter().any(|p| p == path) =>
            {
                let content = get("content")?;
                if content.len() > FILE_BYTES {
                    return Err("fixture_content_limit");
                }
                let previous = match self
                    .held
                    .instruction_link_count(path)
                    .map_err(|_| "fixture_file_unavailable")?
                {
                    None => None,
                    Some(1) => Some(self.read(path)?),
                    _ => return Err("fixture_file_unavailable"),
                };
                Ok(PreparedFixtureOperation::Write(PreparedFixtureWrite {
                    path: path.into(),
                    previous,
                    content: content.into(),
                    receipt: "fixture_written",
                }))
            }
            "replace_file"
                if call.args.len() == 3 && self.actor.writable_files.iter().any(|p| p == path) =>
            {
                let (old, new) = (get("old")?, get("new")?);
                if old.is_empty() || old.len() > FILE_BYTES || new.len() > FILE_BYTES {
                    return Err("fixture_invalid_arguments");
                }
                let previous = self.read(path)?;
                if previous.matches(old).count() != 1 {
                    return Err("fixture_changed_or_ambiguous_reload");
                }
                let content = previous.replacen(old, new, 1);
                if content.len() > FILE_BYTES {
                    return Err("fixture_content_limit");
                }
                Ok(PreparedFixtureOperation::Write(PreparedFixtureWrite {
                    path: path.into(),
                    previous: Some(previous),
                    content,
                    receipt: "fixture_changed",
                }))
            }
            _ => Err("fixture_tool_or_path_denied"),
        }
    }

    fn apply(
        &self,
        prepared: PreparedFixtureOperation,
        mut authorize: impl FnMut() -> Result<(), &'static str>,
    ) -> Result<String, &'static str> {
        authorize()?;
        if self.sealed.load(Ordering::SeqCst) {
            return Err("fixture_closed");
        }
        match prepared {
            PreparedFixtureOperation::Read(content) => Ok(content),
            PreparedFixtureOperation::Write(write) => {
                match write.previous {
                    None => {
                        if self
                            .held
                            .instruction_link_count(&write.path)
                            .map_err(|_| "fixture_file_unavailable")?
                            .is_some()
                        {
                            return Err("fixture_changed_reload");
                        }
                        authorize()?;
                        self.held
                            .create_new_bytes(&write.path, write.content.as_bytes())
                            .map_err(|_| "fixture_write_failed")?;
                    }
                    Some(previous) => {
                        let mut denied = None;
                        let applied = self.held.compare_replace_instruction_file(
                            &write.path,
                            previous.as_bytes(),
                            write.content.as_bytes(),
                            || {
                                authorize().map_err(|code| {
                                    denied = Some(code);
                                    std::io::Error::new(
                                        std::io::ErrorKind::PermissionDenied,
                                        "fixture authority closed",
                                    )
                                })
                            },
                        );
                        applied.map_err(|_| denied.unwrap_or("fixture_changed_reload"))?;
                    }
                }
                Ok(write.receipt.into())
            }
            PreparedFixtureOperation::RecordEffect(PreparedFixtureEffect { key, payload }) => {
                let mut effects = self
                    .effects
                    .lock()
                    .map_err(|_| "fixture_state_unavailable")?;
                authorize()?;
                if let Some(previous) = effects.iter().find(|effect| effect.key == key) {
                    return if previous.payload == payload {
                        Ok("mock_receipt_reused".into())
                    } else {
                        Err("fixture_effect_key_conflict")
                    };
                }
                if effects.len() >= MAX_EFFECTS {
                    return Err("fixture_effect_limit");
                }
                effects.push(MockEffect { key, payload });
                Ok("mock_receipt_recorded".into())
            }
        }
    }

    /// Reserves produced bytes against the retained run before any filesystem mutation.
    pub(super) async fn execute_controlled(
        &self,
        call: ToolCall,
        context: &ExecutionContext,
        logical_cancel: &CancelSignal,
    ) -> TaskResult<Content> {
        let operation = async {
            self.check_controlled(context, logical_cancel)?;
            let prepared = self.prepare(call)?;
            let artifact_bytes = prepared.artifact_bytes();
            let mut permit = if artifact_bytes == 0 {
                context.check_authority().await.map_err(|_| {
                    self.check_controlled(context, logical_cancel)
                        .err()
                        .unwrap_or("fixture_budget_denied")
                })?;
                None
            } else {
                let id = self.logical_operation_id(context)?;
                Some(
                    context
                        .reserve_native_artifact(&id, artifact_bytes)
                        .await
                        .map_err(|_| {
                            self.check_controlled(context, logical_cancel)
                                .err()
                                .unwrap_or("fixture_budget_denied")
                        })?,
                )
            };
            let authorization = async {
                self.check_controlled(context, logical_cancel)?;
                if let Some(permit) = permit.as_ref() {
                    permit.revalidate().await.map_err(|_| {
                        self.check_controlled(context, logical_cancel)
                            .err()
                            .unwrap_or("fixture_budget_denied")
                    })?;
                    self.check_controlled(context, logical_cancel)?;
                }
                Ok(())
            }
            .await;
            if let Err(code) = authorization {
                if let Some(permit) = permit.as_mut() {
                    permit
                        .finish_owned(SettlementOutcome::ProvenNeverSent, true)
                        .await
                        .map_err(|_| "fixture_accounting_failed")?;
                }
                return Err(code);
            }
            let result = self.apply(prepared, || self.check_controlled(context, logical_cancel));
            if let Some(permit) = permit.as_mut() {
                // A filesystem error may follow a physical write. Keep its entire
                // reservation charged rather than infer a refund or replay right.
                let outcome = match &result {
                    Ok(_) => SettlementOutcome::Artifact { artifact_bytes },
                    Err(_) => SettlementOutcome::Unknown { observed: None },
                };
                permit
                    .finish_owned(outcome, true)
                    .await
                    .map_err(|_| "fixture_accounting_failed")?;
                self.check_controlled(context, logical_cancel)?;
            }
            result
        }
        .await;
        Self::tool_result(operation)
    }

    #[cfg(test)]
    pub(super) fn execute(&self, call: ToolCall) -> TaskResult<Content> {
        Self::tool_result(
            self.prepare(call)
                .and_then(|prepared| self.apply(prepared, || Ok(()))),
        )
    }

    fn tool_result(operation: Result<String, &'static str>) -> TaskResult<Content> {
        match operation {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    ..Content::default()
                },
                0,
            ),
            Err(code) => TaskResult::error(code, 0),
        }
    }

    pub(super) fn effects(&self) -> Vec<MockEffect> {
        self.effects
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(super) fn seal(&self) -> Result<(BTreeMap<String, String>, BTreeSet<String>), String> {
        self.sealed.store(true, Ordering::SeqCst);
        let directory = self
            .held
            .held_directory()
            .map_err(|_| "fixture snapshot unavailable")?;
        let mut files = BTreeMap::new();
        for entry in directory
            .entries()
            .map_err(|_| "fixture snapshot unavailable")?
        {
            let entry = entry.map_err(|_| "fixture snapshot unavailable")?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "fixture snapshot path invalid")?;
            if files.len() >= 64 {
                return Err("fixture snapshot limit".into());
            }
            files.insert(name.clone(), self.read(&name).map_err(str::to_string)?);
        }
        Ok((files, BTreeSet::new()))
    }
}

pub(super) fn public_descriptors(actor: &ActorFixture, allow_effects: bool) -> Vec<ToolDescriptor> {
    let mut tools = Vec::new();
    if !actor.readable_files.is_empty() {
        tools.push(descriptor(
            "read_file",
            "Read a permitted isolated fixture file.",
            &["path"],
            Some(&actor.readable_files),
        ));
    }
    if !actor.writable_files.is_empty() {
        tools.push(descriptor(
            "write_file",
            "Write bounded text to a permitted isolated fixture file.",
            &["path", "content"],
            Some(&actor.writable_files),
        ));
        tools.push(descriptor("replace_file", "Replace exactly one current occurrence in a permitted fixture file. Read again after a stale or ambiguous edit.", &["path", "old", "new"], Some(&actor.writable_files)));
    }
    if allow_effects && actor.mock_effects {
        tools.push(descriptor("record_effect", "Record a simulated external effect only. Reusing the same key and payload is idempotent; no account or network is contacted.", &["key", "payload"], None));
    }
    tools
}

fn descriptor(
    name: &str,
    description: &str,
    fields: &[&str],
    paths: Option<&[String]>,
) -> ToolDescriptor {
    let properties = fields
        .iter()
        .map(|field| {
            let mut property =
                BTreeMap::from([("type".into(), DataValue::String("string".into()))]);
            if *field == "path" {
                if let Some(paths) = paths {
                    property.insert(
                        "enum".into(),
                        DataValue::Array(paths.iter().cloned().map(DataValue::String).collect()),
                    );
                }
            }
            ((*field).into(), DataValue::Object(property))
        })
        .collect();
    ToolDescriptor {
        name: name.into(),
        description: description.into(),
        parameters_schema: BTreeMap::from([
            ("type".into(), DataValue::String("object".into())),
            ("properties".into(), DataValue::Object(properties)),
            (
                "required".into(),
                DataValue::Array(
                    fields
                        .iter()
                        .map(|f| DataValue::String((*f).into()))
                        .collect(),
                ),
            ),
            ("additionalProperties".into(), DataValue::Bool(false)),
        ]),
        examples: None,
    }
}

#[cfg(test)]
#[path = "fixture_artifact_budget_tests.rs"]
mod artifact_budget_tests;
