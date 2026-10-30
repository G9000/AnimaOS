use super::AgentRunCoordinator;
use crate::{
    attachments::{
        files::{read_attachment_bytes, save_upload},
        AttachmentKind, AttachmentRecord,
    },
    images::{
        approval::{ImageOwnerApproval, ImageWitness},
        ImageOperation, ImageProvider, ImageStatus, ImageVersion, MAX_IMAGE_OPERATIONS,
        MAX_SAVED_IMAGE_BYTES,
    },
    runs::{ActionStatus, RunLink},
};
use anima_core::{CancelSignal, DataValue, ToolCall};
use anima_service_adapters::{
    ImageError, ImageInput, ImagePrompt, ImageRequest, ImageUsage, IMAGE_MODEL,
};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

const UNCERTAIN:&str="Image outcome is unknown. OpenAI may have processed and charged for it. This operation will not be resent automatically.";

fn operation_id(w: &ImageWitness) -> String {
    let identity = serde_json::to_vec(&(
        &w.run.agent_id,
        &w.run.session_id,
        w.generation,
        &w.run.run_id,
        &w.call.id,
    ))
    .expect("image identity serializes");
    format!("image_{}", hex::encode(Sha256::digest(identity)))
}
fn scope(state: &crate::state::DaemonState, w: &ImageWitness) -> Result<(), String> {
    state.ensure_profile_owned().map_err(|e| e.to_string())?;
    let session = super::chat_controls::validate_owner_chat(state, &w.run)?;
    crate::projects::validate_active_lease(
        state,
        state
            .runs
            .get(&w.run.run_id)
            .ok_or("Image run is unavailable")?,
    )
    .map_err(|e| e.to_string())?;
    let agent = state
        .get_agent(&w.run.agent_id)
        .ok_or("Image owner is unavailable")?;
    if session.clear_generation != w.generation
        || session.project_id != w.project_id
        || session.project_binding_revision != w.project_binding_revision
        || state.workspace.as_ref().map(|x| &x.root_path) != Some(&w.workspace)
        || !agent.state.config.allows_tool(&w.call.name)
        || state.approval_verdict(&agent.state, &w.run.session_id, &w.call)
            == crate::approvals::Verdict::Deny
    {
        return Err("Image scope or permission changed".into());
    }
    Ok(())
}
fn authority(
    state: &crate::state::DaemonState,
    approval: &ImageOwnerApproval,
) -> Result<(), String> {
    scope(state, &approval.witness)?;
    let record = state
        .approvals
        .get(&approval.record().id)
        .ok_or("Image owner approval is unavailable")?;
    if record != approval.record() {
        return Err("Image owner approval changed".into());
    }
    approval.witness.resolve(record)?;
    if !state
        .runs
        .get(&approval.witness.run.run_id)
        .is_some_and(|run| {
            run.action_receipts.iter().any(|r| {
                r.tool_call_id == approval.witness.call.id
                    && r.name == approval.witness.call.name
                    && r.status == ActionStatus::Started
            })
        })
    {
        return Err("Image native action intent is unavailable".into());
    }
    Ok(())
}
fn capacity(session: &crate::sessions::SessionRecord) -> Result<(), String> {
    let bytes = session
        .image_operations
        .iter()
        .filter_map(|o| o.output.as_ref())
        .map(|o| o.size)
        .sum::<u64>();
    if session.image_operations.len() >= MAX_IMAGE_OPERATIONS
        || bytes.saturating_add(crate::attachments::MAX_IMAGE_BYTES as u64) > MAX_SAVED_IMAGE_BYTES
    {
        return Err("This conversation has reached its saved image capacity. Start a new chat; existing versions were preserved".into());
    }
    Ok(())
}

impl AgentRunCoordinator {
    pub(crate) async fn capture_image(
        &self,
        run: &RunLink,
        call: &ToolCall,
    ) -> Result<ImageWitness, String> {
        if !crate::images::is_tool(&call.name) || call.args.len() != 1 {
            return Err("Images accept only a prompt".into());
        }
        let Some(DataValue::String(prompt)) = call.args.get("prompt") else {
            return Err("Image prompt is required".into());
        };
        if !crate::images::valid_prompt(prompt) {
            return Err("Image prompt must be nonblank and at most 2048 bytes".into());
        }
        let (mut witness, store, ids) = {
            let state = self.state.read().await;
            state.ensure_profile_owned().map_err(|e| e.to_string())?;
            let session = super::chat_controls::validate_owner_chat(&state, run)?;
            if state.control_plane_store.is_none() {
                return Err("Configure durable storage before creating images".into());
            }
            if !state.provider_keys.holds_keys("openai") {
                return Err("Add an OpenAI API key before creating images".into());
            }
            capacity(session)?;
            let record = state
                .runs
                .get(&run.run_id)
                .ok_or("Image run is unavailable")?;
            (
                ImageWitness {
                    run: run.clone(),
                    call: call.clone(),
                    prompt: prompt.clone(),
                    source: None,
                    generation: session.clear_generation,
                    project_id: session.project_id.clone(),
                    project_binding_revision: session.project_binding_revision,
                    workspace: state
                        .workspace
                        .as_ref()
                        .ok_or("Configure a workspace before creating images")?
                        .root_path
                        .clone(),
                },
                state.history.store(),
                record
                    .edited_input
                    .as_ref()
                    .unwrap_or(&record.input)
                    .attachment_ids
                    .clone(),
            )
        };
        if call.name == "edit_image" {
            let rows = super::attachments::resolve_run_attachments(
                store.as_ref(),
                &run.agent_id,
                &run.session_id,
                &ids,
            )
            .await
            .map_err(|_| "Selected image is unavailable")?;
            crate::images::validate_generated_attachments(&*self.state.read().await, &rows)?;
            let images = rows
                .into_iter()
                .filter(|r| r.kind == AttachmentKind::Image)
                .collect::<Vec<_>>();
            if images.len() != 1
                || !matches!(
                    images[0].mime.as_str(),
                    "image/png" | "image/jpeg" | "image/webp"
                )
            {
                return Err(
                    "Attach exactly one PNG, JPEG or WebP image to this message before editing"
                        .into(),
                );
            }
            witness.source = images.into_iter().next();
            let source = witness.source.clone().unwrap();
            let root = witness.workspace.clone();
            tokio::task::spawn_blocking(move || read_attachment_bytes(&root, &source))
                .await
                .map_err(|_| "Selected image read stopped")?
                .map_err(|_| "Selected image changed or is unavailable")?;
        }
        let state = self.state.read().await;
        scope(&state, &witness)?;
        if state
            .sessions
            .get(&run.agent_id, &run.session_id)
            .unwrap()
            .image_operations
            .iter()
            .any(|o| o.id == operation_id(&witness))
        {
            return Err("This image operation already exists. Read its saved status instead of resending it".into());
        }
        Ok(witness)
    }

    pub(crate) async fn image_provider(
        &self,
        approval: &ImageOwnerApproval,
    ) -> Result<ImageProvider, String> {
        let state = self.state.read().await;
        authority(&state, approval)?;
        let definition = anima_model_adapters::provider_definitions()
            .iter()
            .find(|d| d.id == "openai")
            .ok_or("OpenAI image configuration is unavailable")?;
        let base = definition
            .base_url_envs
            .iter()
            .filter_map(|n| std::env::var(n).ok())
            .find(|v| !v.trim().is_empty())
            .unwrap_or_else(|| definition.default_base_url.into());
        if !matches!(
            base.as_str(),
            "https://api.openai.com/v1"
                | "https://api.openai.com/v1/"
                | "https://api.openai.com:443/v1"
                | "https://api.openai.com:443/v1/"
        ) {
            return Err(
                "Image generation requires the canonical OpenAI Platform configuration".into(),
            );
        }
        let picked = state
            .provider_keys
            .pick("openai", anima_core::primitives::now_millis())
            .ok_or("No OpenAI API key is ready")?;
        let port = anima_model_adapters::OpenAiImageAdapter::new(picked.key)
            .map_err(|_| "OpenAI image credentials are unavailable")?;
        Ok(ImageProvider {
            port: Arc::new(port),
            key_id: picked.id,
        })
    }

    pub(crate) async fn execute_image(
        &self,
        approval: ImageOwnerApproval,
        cancel: CancelSignal,
        provider: ImageProvider,
    ) -> Result<serde_json::Value, String> {
        let this = self.clone();
        tokio::spawn(async move { this.execute_image_owned(approval, cancel, provider).await })
            .await
            .map_err(|_| UNCERTAIN.to_string())?
    }

    async fn execute_image_owned(
        &self,
        approval: ImageOwnerApproval,
        cancel: CancelSignal,
        provider: ImageProvider,
    ) -> Result<serde_json::Value, String> {
        static ACTIVE: OnceLock<Arc<Semaphore>> = OnceLock::new();
        let _permit = Arc::clone(ACTIVE.get_or_init(|| Arc::new(Semaphore::new(2))))
            .try_acquire_owned()
            .map_err(|_| "Image generation is busy")?;
        if cancel.is_cancelled() {
            return Err("Image request stopped before sending".into());
        }
        let w = &approval.witness;
        let id = operation_id(w);
        self.start_image(&approval, &provider.key_id).await?;
        let response=async {
            let prompt=ImagePrompt::new(w.prompt.clone()).map_err(|_|ImageError::NotSent)?;
            let request=if let Some(source)=w.source.clone() {
                let root=w.workspace.clone();
                let bytes=tokio::task::spawn_blocking(move||read_attachment_bytes(&root,&source)).await.map_err(|_|ImageError::NotSent)?.map_err(|_|ImageError::NotSent)?;
                ImageRequest::Edit {prompt,image:ImageInput::new(bytes)?}
            } else {ImageRequest::Generate {prompt}};
            super::ownership::verify(&self.state).await.map_err(|_|ImageError::NotSent)?;
            authority(&*self.state.read().await,&approval).map_err(|_|ImageError::NotSent)?;
            if cancel.is_cancelled() {return Err(ImageError::NotSent);}
            let deadline=Instant::now()+Duration::from_secs(180);
            tokio::select! {biased;
                ()=cancel.cancelled()=>Err(ImageError::OutcomeUnknown),
                ()=tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))=>Err(ImageError::OutcomeUnknown),
                result=provider.port.generate(request,cancel.clone(),deadline)=>result,
            }
        }.await;
        let output = match response {
            Ok(output) => output,
            Err(error) => {
                let status = if error == ImageError::OutcomeUnknown {
                    ImageStatus::OutcomeUnknown
                } else {
                    ImageStatus::Failed
                };
                self.finish_image(&approval, status, None, None, None)
                    .await?;
                return Err(if status == ImageStatus::OutcomeUnknown {
                    UNCERTAIN.into()
                } else {
                    "Image request was not completed. Check the source, permissions and OpenAI model access before making a new request".into()
                });
            }
        };
        let usage = output.usage().cloned();
        let publication = self.control_plane_transaction().await;
        let owned = super::ownership::verify(&self.state).await.is_ok();
        if !owned
            || cancel.is_cancelled()
            || authority(&*self.state.read().await, &approval).is_err()
        {
            self.finish_image_locked(
                &approval,
                ImageStatus::OutcomeUnknown,
                None,
                None,
                usage,
                publication,
            )
            .await?;
            return Err(UNCERTAIN.into());
        }
        let ordinal = self
            .state
            .read()
            .await
            .sessions
            .get(&w.run.agent_id, &w.run.session_id)
            .and_then(|s| s.image_operations.iter().position(|o| o.id == id))
            .map(|n| n + 1)
            .unwrap_or(1);
        let now = anima_core::primitives::now_millis();
        let root = w.workspace.clone();
        let name = format!("image-{ordinal}.png");
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let bytes = output.into_bytes();
        let size = bytes.len() as u64;
        let file_name = format!("{id}.png");
        let write_guard = self.state.clone().read_owned().await;
        let write_approval = approval.clone();
        let write_cancel = cancel.clone();
        let saved = match tokio::task::spawn_blocking(move || {
            authority(&write_guard, &write_approval)
                .map_err(|_| crate::attachments::AttachmentError::Unavailable)?;
            if write_cancel.is_cancelled() {
                return Err(crate::attachments::AttachmentError::Unavailable);
            }
            save_upload(&root, &date, &file_name, &bytes)
        })
        .await
        {
            Ok(Ok(saved)) => saved,
            _ => {
                self.finish_image_locked(
                    &approval,
                    ImageStatus::OutcomeUnknown,
                    None,
                    None,
                    usage,
                    publication,
                )
                .await?;
                return Err(UNCERTAIN.into());
            }
        };
        let row = AttachmentRecord {
            id: id.clone(),
            agent_id: w.run.agent_id.clone(),
            session_id: w.run.session_id.clone(),
            name,
            mime: "image/png".into(),
            size,
            kind: AttachmentKind::Image,
            path: saved.path,
            created_at_ms: now,
            workspace_root: saved.workspace_root,
            sha256: saved.sha256,
        };
        if cancel.is_cancelled() || authority(&*self.state.read().await, &approval).is_err() {
            self.finish_image_locked(
                &approval,
                ImageStatus::OutcomeUnknown,
                None,
                None,
                usage,
                publication,
            )
            .await?;
            return Err(UNCERTAIN.into());
        }
        let store = self.state.read().await.history.store();
        if store
            .upsert_attachments(std::slice::from_ref(&row))
            .await
            .is_err()
            || cancel.is_cancelled()
        {
            self.finish_image_locked(
                &approval,
                ImageStatus::OutcomeUnknown,
                None,
                None,
                usage,
                publication,
            )
            .await?;
            return Err(UNCERTAIN.into());
        }
        let version = ImageVersion {
            id: format!("version_{}", &id[6..]),
            attachment_id: row.id.clone(),
            parent_attachment_id: w.source.as_ref().map(|s| s.id.clone()),
            width: 1024,
            height: 1024,
            model: IMAGE_MODEL.into(),
            created_at_ms: now,
        };
        let mut result = self
            .finish_image_locked(
                &approval,
                ImageStatus::Succeeded,
                Some(row),
                Some(version),
                usage,
                publication,
            )
            .await?;
        let operation = {
            let state = self.state.read().await;
            authority(&state, &approval).map_err(|_| UNCERTAIN)?;
            state
                .sessions
                .get(&w.run.agent_id, &w.run.session_id)
                .and_then(|session| {
                    session
                        .image_operations
                        .iter()
                        .find(|operation| operation.id == id)
                })
                .cloned()
                .ok_or(UNCERTAIN)?
        };
        let observation = crate::images::observation::observe_saved_images(
            Some(w.workspace.clone()),
            std::slice::from_ref(&operation),
        )
        .await
        .pop()
        .flatten();
        let state = self.state.read().await;
        authority(&state, &approval).map_err(|_| UNCERTAIN)?;
        if cancel.is_cancelled()
            || !state
                .sessions
                .get(&w.run.agent_id, &w.run.session_id)
                .is_some_and(|session| {
                    session
                        .image_operations
                        .iter()
                        .any(|current| current == &operation)
                })
        {
            return Err(UNCERTAIN.into());
        }
        if let Some(observation) = observation {
            result["observation"] =
                serde_json::to_value(observation).expect("host image observation serializes");
        }
        Ok(result)
    }

    async fn start_image(&self, approval: &ImageOwnerApproval, key_id: &str) -> Result<(), String> {
        super::ownership::verify(&self.state).await?;
        let _transaction = self.control_plane_transaction().await;
        let w = &approval.witness;
        let (previous, persist) = {
            let mut state = self.state.write().await;
            authority(&state, approval)?;
            if state.control_plane_store.is_none() {
                return Err("Durable image storage is unavailable".into());
            }
            let session = state
                .sessions
                .get_mut(&w.run.agent_id, &w.run.session_id)
                .ok_or("Image conversation is unavailable")?;
            capacity(session)?;
            let id = operation_id(w);
            if session.image_operations.iter().any(|o| o.id == id) {
                return Err("Image operation already exists; it was not resent".into());
            }
            let previous = session.image_operations.clone();
            session.image_operations.push(ImageOperation {
                id,
                run_id: w.run.run_id.clone(),
                tool_call_id: w.call.id.clone(),
                clear_generation: w.generation,
                prompt: w.prompt.clone(),
                source: w.source.clone(),
                status: ImageStatus::Dispatching,
                created_at_ms: anima_core::primitives::now_millis(),
                output: None,
                version: None,
                usage: None,
                credential_id: key_id.into(),
                approval_id: approval.record().id.clone(),
            });
            (previous, state.control_plane_persist_request())
        };
        if persist.save().await.is_err() {
            if let Some(session) = self
                .state
                .write()
                .await
                .sessions
                .get_mut(&w.run.agent_id, &w.run.session_id)
                .filter(|s| s.clear_generation == w.generation)
            {
                session.image_operations = previous;
            }
            return Err("Image intent could not be saved; no provider request was sent".into());
        }
        self.state.read().await.publish_session_event(
            &w.run.agent_id,
            &w.run.session_id,
            crate::live::LiveEventBody::SessionUpdated,
        );
        Ok(())
    }

    async fn finish_image(
        &self,
        approval: &ImageOwnerApproval,
        status: ImageStatus,
        output: Option<AttachmentRecord>,
        version: Option<ImageVersion>,
        usage: Option<ImageUsage>,
    ) -> Result<serde_json::Value, String> {
        let transaction = self.control_plane_transaction().await;
        self.finish_image_locked(approval, status, output, version, usage, transaction)
            .await
    }

    async fn finish_image_locked(
        &self,
        approval: &ImageOwnerApproval,
        status: ImageStatus,
        output: Option<AttachmentRecord>,
        version: Option<ImageVersion>,
        usage: Option<ImageUsage>,
        _transaction: tokio::sync::OwnedMutexGuard<()>,
    ) -> Result<serde_json::Value, String> {
        let w = &approval.witness;
        let (previous, result, persist) = {
            let mut state = self.state.write().await;
            state.ensure_profile_owned().map_err(|_| UNCERTAIN)?;
            if status == ImageStatus::Succeeded {
                authority(&state, approval).map_err(|_| UNCERTAIN)?;
            }
            let session = state
                .sessions
                .get_mut(&w.run.agent_id, &w.run.session_id)
                .filter(|s| s.clear_generation == w.generation)
                .ok_or(UNCERTAIN)?;
            let previous = session.image_operations.clone();
            let operation = session
                .image_operations
                .iter_mut()
                .find(|o| o.id == operation_id(w))
                .ok_or(UNCERTAIN)?;
            if operation.status != ImageStatus::Dispatching {
                return Err("Image operation already settled".into());
            }
            operation.status = status;
            operation.output = output;
            operation.version = version;
            operation.usage = usage;
            let result = operation.public_value();
            crate::images::validate_session(session)?;
            (previous, result, state.control_plane_persist_request())
        };
        if persist.save().await.is_err() {
            if let Some(session) = self
                .state
                .write()
                .await
                .sessions
                .get_mut(&w.run.agent_id, &w.run.session_id)
                .filter(|s| s.clear_generation == w.generation)
            {
                session.image_operations = previous;
            }
            return Err(UNCERTAIN.into());
        }
        self.state.read().await.publish_session_event(
            &w.run.agent_id,
            &w.run.session_id,
            crate::live::LiveEventBody::SessionUpdated,
        );
        Ok(result)
    }
}

#[cfg(test)]
#[path = "images_tests.rs"]
mod tests;
