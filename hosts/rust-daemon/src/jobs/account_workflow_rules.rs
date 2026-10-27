use super::{now_ms, JobError, JobService};
use crate::{
    account_workflows::{
        build_rule, creation_digest, AccountRuleOutcome, AccountRuleOutcomeStatus,
        AccountWorkflowEvent, AccountWorkflowRule, AccountWorkflowRuleInput,
        AccountWorkflowRulePatch, MAX_ACCOUNT_RULES,
    },
    connected_search::source_authority,
};

impl JobService {
    pub(crate) async fn list_account_workflow_rules(
        &self,
        agent: &str,
    ) -> Result<Vec<AccountWorkflowRule>, JobError> {
        let _transaction = self.runs.control_plane_transaction().await;
        let state = self.state.read().await;
        if state.get_agent(agent).is_none() {
            return Err(JobError::NotFound);
        }
        let mut rules = state
            .account_workflows
            .rules
            .values()
            .filter(|rule| rule.agent_id == agent && !rule.deleted)
            .cloned()
            .collect::<Vec<_>>();
        rules.sort_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms).then(a.id.cmp(&b.id)));
        Ok(rules)
    }

    pub(crate) async fn create_account_workflow_rule(
        &self,
        agent: &str,
        input: AccountWorkflowRuleInput,
    ) -> Result<AccountWorkflowRule, JobError> {
        self.refuse_while_draining()?;
        let agent = agent.to_owned();
        self.mutate(move |state| {
            let config = state
                .get_agent(&agent)
                .ok_or(JobError::NotFound)?
                .state
                .config;
            if crate::agent_runs::is_helper_config(&config) {
                return Err(JobError::Conflict(
                    "Helpers cannot own account workflow rules".into(),
                ));
            }
            if let Some(key) = &input.request_key {
                if let Some(original) = state.account_workflows.rules.values().find(|rule| {
                    rule.agent_id == agent && rule.creation_request_key.as_ref() == Some(key)
                }) {
                    return if original.creation_payload_sha256.as_deref()
                        == Some(creation_digest(&input).as_str())
                    {
                        Ok(original.clone())
                    } else {
                        Err(JobError::Conflict(
                            "Account rule request key was already used for different input".into(),
                        ))
                    };
                }
            }
            if state.account_workflows.rules.len() >= MAX_ACCOUNT_RULES {
                return Err(JobError::Conflict(
                    "Account workflow rule history capacity is exhausted".into(),
                ));
            }
            let source =
                source_authority(state, &agent, &input.connector_id).ok_or(JobError::NotFound)?;
            let rule = build_rule(
                state,
                &source,
                input,
                uuid::Uuid::new_v4().to_string(),
                now_ms(),
            )?;
            state
                .account_workflows
                .rules
                .insert(rule.id.clone(), rule.clone());
            Ok(rule)
        })
        .await
    }

    pub(crate) async fn update_account_workflow_rule(
        &self,
        agent: &str,
        id: &str,
        patch: AccountWorkflowRulePatch,
    ) -> Result<AccountWorkflowRule, JobError> {
        self.refuse_while_draining()?;
        let (agent, id) = (agent.to_owned(), id.to_owned());
        self.mutate(move |state| {
            if state.get_agent(&agent).is_none() {
                return Err(JobError::NotFound);
            }
            let old = state
                .account_workflows
                .rules
                .get(&id)
                .filter(|rule| rule.agent_id == agent && !rule.deleted)
                .cloned()
                .ok_or(JobError::NotFound)?;
            if old.revision != patch.revision {
                return Err(JobError::Conflict(
                    "Account rule revision changed; refresh before saving".into(),
                ));
            }
            // Disable remains available when the connection or template was revoked.
            let disable_only = patch.enabled == Some(false)
                && patch.name.is_none()
                && patch.connector_id.is_none()
                && patch.event_kind.is_none()
                && patch.template_slug.is_none()
                && patch.template_revision.is_none()
                && patch.filters.is_none()
                && patch.inputs.is_none();
            let now = now_ms().max(old.updated_at_ms.saturating_add(1));
            let mut next = if disable_only {
                let mut next = old.clone();
                next.enabled = false;
                next.baseline_at_ms = None;
                next.updated_at_ms = now;
                next.last_outcome = Some(AccountRuleOutcome {
                    status: AccountRuleOutcomeStatus::Paused,
                    message: "Rule disabled by its owner".into(),
                    at_ms: now,
                });
                next
            } else {
                let input = old.apply_patch(patch);
                let source = source_authority(state, &agent, &input.connector_id)
                    .ok_or(JobError::NotFound)?;
                let mut next = build_rule(state, &source, input, id.clone(), now)?;
                next.created_at_ms = old.created_at_ms;
                next.creation_request_key = old.creation_request_key.clone();
                next.creation_payload_sha256 = old.creation_payload_sha256.clone();
                next
            };
            next.revision = old
                .revision
                .checked_add(1)
                .ok_or_else(|| JobError::Conflict("Account rule revision limit reached".into()))?;
            state.account_workflows.rules.insert(id, next.clone());
            Ok(next)
        })
        .await
    }

    pub(crate) async fn delete_account_workflow_rule(
        &self,
        agent: &str,
        id: &str,
        revision: u64,
    ) -> Result<AccountWorkflowRule, JobError> {
        let (agent, id) = (agent.to_owned(), id.to_owned());
        self.mutate(move |state| {
            if state.get_agent(&agent).is_none() {
                return Err(JobError::NotFound);
            }
            let rule = state
                .account_workflows
                .rules
                .get_mut(&id)
                .filter(|rule| rule.agent_id == agent)
                .ok_or(JobError::NotFound)?;
            if rule.deleted && revision.checked_add(1) == Some(rule.revision) {
                return Ok(rule.clone());
            }
            if rule.deleted || rule.revision != revision {
                return Err(JobError::Conflict(
                    "Account rule revision changed; refresh before deleting".into(),
                ));
            }
            rule.revision = rule
                .revision
                .checked_add(1)
                .ok_or_else(|| JobError::Conflict("Account rule revision limit reached".into()))?;
            rule.enabled = false;
            rule.deleted = true;
            rule.baseline_at_ms = None;
            rule.updated_at_ms = now_ms().max(rule.updated_at_ms.saturating_add(1));
            rule.last_outcome = Some(AccountRuleOutcome {
                status: AccountRuleOutcomeStatus::Paused,
                message: "Rule deleted by its owner; admitted workflow history is retained".into(),
                at_ms: rule.updated_at_ms,
            });
            Ok(rule.clone())
        })
        .await
    }

    pub(crate) async fn account_workflow_events(
        &self,
        agent: &str,
        id: &str,
    ) -> Result<Vec<AccountWorkflowEvent>, JobError> {
        let _transaction = self.runs.control_plane_transaction().await;
        let state = self.state.read().await;
        if state.get_agent(agent).is_none()
            || !state
                .account_workflows
                .rules
                .get(id)
                .is_some_and(|rule| rule.agent_id == agent)
        {
            return Err(JobError::NotFound);
        }
        let mut events = state
            .account_workflows
            .events
            .iter()
            .filter(|event| event.agent_id == agent && event.rule_id == id)
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms).then(a.id.cmp(&b.id)));
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account_workflows::{AccountEventFilters, AccountEventKind},
        connected_search::SourceIndex,
        connectors::mail::{MailConnector, MailRecord, Provider},
        control_plane_store::{load_control_plane_snapshot, ControlPlaneStoreConfig},
    };
    use anima_core::{
        workflow_templates::WorkflowTemplateInput,
        workflows::{WorkflowStepKind, WorkflowStepSpec},
    };
    use std::collections::BTreeMap;

    async fn setup() -> (JobService, String, std::path::PathBuf, String) {
        let (service, agent, path) = super::super::tests::setup();
        service
            .save_workflow_template(
                &agent,
                WorkflowTemplateInput {
                    slug: "mail-review".into(),
                    revision: 0,
                    title: "Review".into(),
                    parameters: vec![],
                    steps: vec![WorkflowStepSpec {
                        id: "review".into(),
                        title: "Review".into(),
                        kind: WorkflowStepKind::Approval,
                        prompt: "Review the account activity".into(),
                    }],
                },
            )
            .await
            .unwrap();
        let id = "mail-rule-source".to_string();
        let mut state = service.state.write().await;
        let descriptor = state
            .tool_registry
            .descriptor("mail_list_messages")
            .unwrap();
        state
            .update_agent(
                &agent,
                anima_core::AgentConfigUpdate {
                    tools: Some(vec![descriptor]),
                    ..Default::default()
                },
            )
            .unwrap();
        state.mail_records.insert(
            id.clone(),
            MailRecord {
                connector: MailConnector {
                    id: id.clone(),
                    agent_id: agent.clone(),
                    provider: Provider::Gmail,
                    status: "active".into(),
                    account_label: Some("owner@example.test".into()),
                    error: None,
                    last_synced_at_ms: None,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    email: None,
                },
                messages: vec![],
                deleted: false,
                auth_generation: "auth-1".into(),
                email: None,
            },
        );
        let source = source_authority(&state, &agent, &id).unwrap();
        state.connected_search.sources.insert(
            id.clone(),
            SourceIndex {
                agent_id: agent.clone(),
                generation: source.generation,
                enabled: true,
                grant_epoch: "test-grant-epoch".into(),
                stream_epoch: None,
                documents: vec![],
                seen: BTreeMap::new(),
                baseline_at_ms: Some(1),
                last_attempt_at_ms: Some(1),
                last_success_at_ms: Some(1),
                error: None,
            },
        );
        drop(state);
        (service, agent, path, id)
    }

    fn input(connector_id: String) -> AccountWorkflowRuleInput {
        AccountWorkflowRuleInput {
            request_key: None,
            name: "Review mail".into(),
            connector_id,
            event_kind: AccountEventKind::MailObserved,
            template_slug: "mail-review".into(),
            template_revision: 1,
            filters: AccountEventFilters::default(),
            inputs: BTreeMap::new(),
            enabled: true,
        }
    }

    #[tokio::test]
    async fn account_rule_mutations_persist_revisions_disable_and_tombstone() {
        let (service, agent, path, connector) = setup().await;
        let rule = service
            .create_account_workflow_rule(&agent, input(connector))
            .await
            .unwrap();
        let disabled = service
            .update_account_workflow_rule(
                &agent,
                &rule.id,
                AccountWorkflowRulePatch {
                    revision: rule.revision,
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.revision, 2);
        assert!(disabled.baseline_at_ms.is_none());
        assert!(service
            .update_account_workflow_rule(
                &agent,
                &rule.id,
                AccountWorkflowRulePatch {
                    revision: rule.revision,
                    name: Some("Stale".into()),
                    ..Default::default()
                }
            )
            .await
            .is_err());
        let deleted = service
            .delete_account_workflow_rule(&agent, &rule.id, disabled.revision)
            .await
            .unwrap();
        assert!(deleted.deleted);
        assert!(service
            .list_account_workflow_rules(&agent)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            service
                .delete_account_workflow_rule(&agent, &rule.id, disabled.revision)
                .await
                .unwrap(),
            deleted
        );
        let snapshot = load_control_plane_snapshot(&ControlPlaneStoreConfig::Json(path.clone()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.account_workflows.rules[&rule.id], deleted);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn account_rule_failed_persistence_restores_all_mutations() {
        let (service, agent, path, connector) = setup().await;
        let rule = service
            .create_account_workflow_rule(&agent, input(connector))
            .await
            .unwrap();
        let blocked = tempfile::tempdir().unwrap();
        service
            .state
            .write()
            .await
            .set_control_plane_store(Some(ControlPlaneStoreConfig::Json(
                blocked.path().to_path_buf(),
            )));
        assert!(service
            .update_account_workflow_rule(
                &agent,
                &rule.id,
                AccountWorkflowRulePatch {
                    revision: rule.revision,
                    enabled: Some(false),
                    ..Default::default()
                }
            )
            .await
            .is_err());
        assert_eq!(
            service.list_account_workflow_rules(&agent).await.unwrap(),
            vec![rule]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn account_rule_create_request_survives_lost_ack_edit_delete_and_replay() {
        let (service, agent, path, connector) = setup().await;
        let mut input = input(connector);
        input.request_key = Some("owner-create-1".into());
        let rule = service
            .create_account_workflow_rule(&agent, input.clone())
            .await
            .unwrap();
        assert_eq!(
            service
                .create_account_workflow_rule(&agent, input.clone())
                .await
                .unwrap(),
            rule
        );
        assert_eq!(
            service
                .list_account_workflow_rules(&agent)
                .await
                .unwrap()
                .len(),
            1
        );
        let edited = service
            .update_account_workflow_rule(
                &agent,
                &rule.id,
                AccountWorkflowRulePatch {
                    revision: rule.revision,
                    name: Some("Edited".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            service
                .create_account_workflow_rule(&agent, input.clone())
                .await
                .unwrap(),
            edited
        );
        let deleted = service
            .delete_account_workflow_rule(&agent, &rule.id, edited.revision)
            .await
            .unwrap();
        assert_eq!(
            service
                .create_account_workflow_rule(&agent, input.clone())
                .await
                .unwrap(),
            deleted
        );
        input.name = "Changed request bytes".into();
        assert!(service
            .create_account_workflow_rule(&agent, input)
            .await
            .is_err());
        assert_eq!(service.state.read().await.account_workflows.rules.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn account_rule_revocation_blocks_queued_task_and_preserves_owner_review() {
        use crate::account_workflows::admit_changes;
        use crate::connected_search::{
            DocumentKind, IndexedDocument, SourceChange, SourceChangeKind,
        };
        use anima_core::workflows::WorkflowStatus;
        let (service, agent, path, connector) = setup().await;
        service
            .save_workflow_template(
                &agent,
                WorkflowTemplateInput {
                    slug: "mail-review".into(),
                    revision: 1,
                    title: "Review".into(),
                    parameters: vec![],
                    steps: vec![WorkflowStepSpec {
                        id: "prepare".into(),
                        title: "Prepare".into(),
                        kind: WorkflowStepKind::Task,
                        prompt: "Prepare a summary; do not send messages".into(),
                    }],
                },
            )
            .await
            .unwrap();
        let mut definition = input(connector.clone());
        definition.template_revision = 2;
        let rule = service
            .create_account_workflow_rule(&agent, definition)
            .await
            .unwrap();
        let (a, c) = (agent.clone(), connector.clone());
        let rule_time = rule.updated_at_ms;
        service
            .mutate(move |state| {
                let mut source = source_authority(state, &a, &c).unwrap();
                source.sync_started_at_ms = rule_time + 1;
                admit_changes(state, &source, &[], false)?;
                source.sync_started_at_ms = rule_time + 2;
                let event = SourceChange {
                    id: "observed-once".into(),
                    connector_id: c,
                    generation: source.generation.clone(),
                    provider: "gmail".into(),
                    kind: SourceChangeKind::Observed,
                    observed_at_ms: rule_time + 2,
                    document: IndexedDocument {
                        citation_id: "citation".into(),
                        external_id: "message".into(),
                        collection_id: "INBOX".into(),
                        kind: DocumentKind::Mail,
                        title: "Summary".into(),
                        text: "Account event".into(),
                        sender: None,
                        source_url: None,
                        source_modified_at: None,
                        event_start: None,
                        event_end: None,
                        revision: "rev".into(),
                        truncated: false,
                        indexed_at_ms: rule_time + 2,
                    },
                };
                admit_changes(state, &source, &[event], false)
            })
            .await
            .unwrap();
        service.reconcile_workflows().await.unwrap();
        let workflow = service.list_workflows(&agent).await.unwrap().remove(0);
        let job_id = workflow.steps[0].job_id.clone().unwrap();
        {
            let state = service.state.read().await;
            assert!(super::super::workflows::dispatch_allowed(
                &state,
                &state.jobs[&job_id]
            ));
        }
        service
            .update_account_workflow_rule(
                &agent,
                &rule.id,
                AccountWorkflowRulePatch {
                    revision: rule.revision,
                    enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        {
            let state = service.state.read().await;
            assert!(!super::super::workflows::dispatch_allowed(
                &state,
                &state.jobs[&job_id]
            ));
        }
        service.reconcile_workflows().await.unwrap();
        let state = service.state.read().await;
        assert_eq!(
            state.workflows[&workflow.id].status,
            WorkflowStatus::NeedsReview
        );
        assert_eq!(
            state.jobs[&job_id].status,
            super::super::AgentJobStatus::Cancelled
        );
        super::super::workflows::validate_snapshot(
            &state.workflows.values().cloned().collect::<Vec<_>>(),
            &state.jobs.values().cloned().collect::<Vec<_>>(),
        )
        .unwrap();
        drop(state);
        std::fs::remove_file(path).unwrap();
    }
}
