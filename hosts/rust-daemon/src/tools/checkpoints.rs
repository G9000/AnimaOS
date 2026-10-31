//! Selected native-file recovery uses the same grants, approvals and run journal
//! as other tools. No path, scope or lineage authority is accepted from the model.
use super::*;
use crate::filesystem_checkpoints::{
    NativeCheckpointService, NativeMutationReceipt, NATIVE_RECOVERY_REQUIRED,
};

impl ToolExecutionContext {
    #[cfg(test)]
    pub(crate) fn with_checkpoint_preimage_hook(
        mut self,
        hook: impl FnOnce() + Send + 'static,
    ) -> Self {
        self.checkpoint_preimage_hook = Some(Arc::new(std::sync::Mutex::new(Some(Box::new(hook)))));
        self
    }
    fn checkpoint_parts(
        &self,
    ) -> Result<
        (
            &Path,
            &Path,
            &crate::runs::RunLink,
            &crate::agent_runs::action_receipts::RunActionJournal,
        ),
        String,
    > {
        let store = self
            .checkpoint_root
            .as_deref()
            .ok_or("Native file checkpoints are disabled")?;
        let workspace = self
            .workspace_root
            .as_deref()
            .ok_or("Checkpoint workspace is unavailable")?;
        let run = self
            .run_link
            .as_ref()
            .ok_or("Checkpoint action requires an ordinary run")?;
        let journal = self
            .action_journal
            .as_ref()
            .ok_or("Checkpoint action requires an authentic native journal")?;
        if journal.bound_run() != run || self.approvals.is_none() {
            return Err("Checkpoint run authority is unavailable".into());
        }
        Ok((workspace, store, run, journal))
    }

    pub(super) fn checkpoint_mutate<F>(
        &self,
        call: &ToolCall,
        path: &str,
        transform: F,
    ) -> Result<Option<NativeMutationReceipt>, String>
    where
        F: FnOnce(Option<&[u8]>) -> Result<Vec<u8>, String>,
    {
        if self.checkpoint_root.is_none() {
            return Ok(None);
        }
        let (workspace, store, run, journal) = self.checkpoint_parts()?;
        journal.checkpoint_revalidate_now(workspace, store, call, self.action_approved, false)?;
        #[cfg(test)]
        if let Some(hook) = self
            .checkpoint_preimage_hook
            .as_ref()
            .and_then(|slot| slot.lock().unwrap().take())
        {
            hook();
        }
        let validate = || {
            #[cfg(test)]
            if let Some(hook) = self
                .checkpoint_validation_hook
                .as_ref()
                .and_then(|slot| slot.lock().unwrap().take())
            {
                hook();
            }
            let authority = journal.checkpoint_mutation_authority(
                workspace,
                store,
                call,
                self.action_approved,
            )?;
            if let Some(witness) = &self.outcome_write {
                crate::outcomes::validate_write_witness(&authority, run, call, witness)?;
            }
            Ok(authority)
        };
        let bound_transform = |source: Option<&[u8]>| {
            if self
                .outcome_write
                .as_ref()
                .is_some_and(|witness| !witness.matches(source))
            {
                return Err(
                    "Outcome correction target changed before native checkpoint capture".into(),
                );
            }
            if self
                .protected_write
                .as_ref()
                .is_some_and(|witness| !witness.matches_preimage(source))
            {
                return Err("Instruction file target changed before checkpoint capture".into());
            }
            transform(source)
        };
        let result = NativeCheckpointService::new(store.to_path_buf())
            .with_profile_owner(self.profile_owner.clone())
            .mutate_with_authority(workspace, run, call, path, bound_transform, validate);
        if result
            .as_ref()
            .err()
            .is_some_and(|error| error == NATIVE_RECOVERY_REQUIRED)
        {
            journal.stop_for_unproven_effects(NATIVE_RECOVERY_REQUIRED);
        }
        result.map(Some)
    }
}

pub(super) fn list(
    context: ToolExecutionContext,
    _agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result = (|| {
            if !call.args.is_empty() {
                return Err("checkpoint_list accepts no arguments".to_string());
            }
            let (workspace, store, run, journal) = context.checkpoint_parts()?;
            journal.checkpoint_revalidate_now(
                workspace,
                store,
                &call,
                context.action_approved,
                true,
            )?;
            let rows = NativeCheckpointService::new(store.to_path_buf())
                .with_profile_owner(context.profile_owner.clone())
                .list(workspace, run)?;
            serde_json::to_string(&rows)
                .map_err(|_| "Checkpoint listing could not be encoded".into())
        })();
        text_result(result)
    })
}

pub(super) fn restore(
    context: ToolExecutionContext,
    _agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result = (|| {
            if call.args.len() != 1 {
                return Err("checkpoint_restore requires only checkpoint_id".to_string());
            }
            let id = match call.args.get("checkpoint_id") {
                Some(DataValue::String(id)) if !id.trim().is_empty() => id.as_str(),
                _ => return Err("checkpoint_restore needs a checkpoint_id string".into()),
            };
            let (workspace, store, run, journal) = context.checkpoint_parts()?;
            let validate = || {
                journal.checkpoint_revalidate_now(
                    workspace,
                    store,
                    &call,
                    context.action_approved,
                    true,
                )
            };
            validate()?;
            let result = NativeCheckpointService::new(store.to_path_buf())
                .with_profile_owner(context.profile_owner.clone())
                .restore_checked(workspace, run, id, validate);
            if result
                .as_ref()
                .err()
                .is_some_and(|error| error == NATIVE_RECOVERY_REQUIRED)
            {
                journal.stop_for_unproven_effects(NATIVE_RECOVERY_REQUIRED);
            }
            let restored = result?;
            Ok(format!(
                "Restored checkpoint {} for {}. Safety checkpoint: {}",
                restored.checkpoint_id,
                restored.restored_paths.join(", "),
                restored.safety_checkpoint_id
            ))
        })();
        text_result(result)
    })
}

pub(super) fn mutation_success(
    text: String,
    receipt: Option<NativeMutationReceipt>,
) -> TaskResult<Content> {
    let (text, metadata) = match receipt {
        Some(receipt) => (
            format!(
                "{text}\nRecovery checkpoint: {} (selected native file)",
                receipt.checkpoint_id
            ),
            Some(std::collections::BTreeMap::from([(
                "filesystemCheckpointId".into(),
                DataValue::String(receipt.checkpoint_id),
            )])),
        ),
        None => (text, None),
    };
    TaskResult::success(
        Content {
            text,
            attachments: None,
            metadata,
        },
        0,
    )
}

fn text_result(result: Result<String, String>) -> TaskResult<Content> {
    match result {
        Ok(text) => mutation_success(text, None),
        Err(error) => TaskResult::error(error, 0),
    }
}
