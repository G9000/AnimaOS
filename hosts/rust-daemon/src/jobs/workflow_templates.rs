use std::collections::HashSet;

use anima_core::{
    workflow_templates::{
        validate_run_input, validate_slug, WorkflowTemplate, WorkflowTemplateInput,
        WorkflowTemplateRunInput,
    },
    workflows::WorkflowRun,
};

use super::{now_ms, JobError, JobService};
use crate::{
    runs::{RunLink, RunSource, RunStatus},
    state::DaemonState,
};

const MAX_TEMPLATES: usize = 25;
const MAX_WORKFLOWS: usize = 50;

pub(crate) fn template_key(agent_id: &str, slug: &str) -> String {
    format!("{agent_id}:{slug}")
}

impl JobService {
    pub(crate) async fn list_workflow_templates(
        &self,
        agent_id: &str,
    ) -> Result<Vec<WorkflowTemplate>, JobError> {
        self.list_workflow_templates_inner(agent_id, None).await
    }

    pub(crate) async fn list_workflow_templates_from_run(
        &self,
        link: &RunLink,
    ) -> Result<Vec<WorkflowTemplate>, JobError> {
        self.list_workflow_templates_inner(&link.agent_id, Some(link))
            .await
    }

    async fn list_workflow_templates_inner(
        &self,
        agent_id: &str,
        caller: Option<&RunLink>,
    ) -> Result<Vec<WorkflowTemplate>, JobError> {
        let _transaction = self.runs.control_plane_transaction().await;
        let state = self.state.read().await;
        if state.get_agent(agent_id).is_none() {
            return Err(JobError::NotFound);
        }
        if let Some(link) = caller {
            check_caller(&state, link, "list_workflow_templates")?;
        }
        let mut records: Vec<_> = state
            .workflow_templates
            .values()
            .filter(|template| template.agent_id == agent_id && !template.deleted)
            .cloned()
            .collect();
        records.sort_by(|left, right| left.slug.cmp(&right.slug));
        Ok(records)
    }

    pub(crate) async fn get_workflow_template(
        &self,
        agent_id: &str,
        slug: &str,
    ) -> Result<WorkflowTemplate, JobError> {
        validate_slug(slug).map_err(JobError::Validation)?;
        let _transaction = self.runs.control_plane_transaction().await;
        let state = self.state.read().await;
        if state.get_agent(agent_id).is_none() {
            return Err(JobError::NotFound);
        }
        state
            .workflow_templates
            .get(&template_key(agent_id, slug))
            .filter(|template| !template.deleted)
            .cloned()
            .ok_or(JobError::NotFound)
    }

    pub(crate) async fn save_workflow_template(
        &self,
        agent_id: &str,
        input: WorkflowTemplateInput,
    ) -> Result<WorkflowTemplate, JobError> {
        self.save_workflow_template_inner(agent_id, input, None)
            .await
    }

    pub(crate) async fn save_workflow_template_from_run(
        &self,
        link: RunLink,
        input: WorkflowTemplateInput,
    ) -> Result<WorkflowTemplate, JobError> {
        let agent = link.agent_id.clone();
        self.save_workflow_template_inner(&agent, input, Some(link))
            .await
    }

    async fn save_workflow_template_inner(
        &self,
        agent_id: &str,
        input: WorkflowTemplateInput,
        caller: Option<RunLink>,
    ) -> Result<WorkflowTemplate, JobError> {
        self.refuse_while_draining()?;
        // Validate the full bounded definition before entering the state mutation.
        let mut validation = input.clone();
        validation.revision = 0;
        WorkflowTemplate::new(agent_id, validation, now_ms()).map_err(JobError::Validation)?;
        let agent = agent_id.to_owned();
        self.mutate(move |state| {
            let config = state
                .get_agent(&agent)
                .ok_or(JobError::NotFound)?
                .state
                .config;
            if crate::agent_runs::is_helper_config(&config) {
                return Err(JobError::Conflict(
                    "Helpers cannot save workflow templates".into(),
                ));
            }
            if let Some(link) = &caller {
                check_caller(state, link, "save_workflow_template")?;
            }
            let key = template_key(&agent, &input.slug);
            if let Some(template) = state.workflow_templates.get_mut(&key) {
                template
                    .save(input, now_ms().max(template.updated_at_ms))
                    .map_err(JobError::Conflict)?;
                return Ok(template.clone());
            }
            if state.workflow_templates.len() >= MAX_TEMPLATES {
                return Err(JobError::Conflict(
                    "Workflow template capacity is exhausted".into(),
                ));
            }
            let template =
                WorkflowTemplate::new(&agent, input, now_ms()).map_err(JobError::Conflict)?;
            state.workflow_templates.insert(key, template.clone());
            Ok(template)
        })
        .await
    }

    pub(crate) async fn delete_workflow_template(
        &self,
        agent_id: &str,
        slug: &str,
        revision: u64,
    ) -> Result<WorkflowTemplate, JobError> {
        self.refuse_while_draining()?;
        validate_slug(slug).map_err(JobError::Validation)?;
        let (agent, key) = (agent_id.to_owned(), template_key(agent_id, slug));
        self.mutate(move |state| {
            if state.get_agent(&agent).is_none() {
                return Err(JobError::NotFound);
            }
            let template = state
                .workflow_templates
                .get_mut(&key)
                .ok_or(JobError::NotFound)?;
            template
                .delete(revision, now_ms())
                .map_err(JobError::Conflict)?;
            Ok(template.clone())
        })
        .await
    }

    pub(crate) async fn run_workflow_template(
        &self,
        agent_id: &str,
        slug: &str,
        input: WorkflowTemplateRunInput,
    ) -> Result<WorkflowRun, JobError> {
        self.run_workflow_template_inner(agent_id, slug, input, None)
            .await
    }

    pub(crate) async fn run_workflow_template_from_run(
        &self,
        link: RunLink,
        slug: &str,
        input: WorkflowTemplateRunInput,
    ) -> Result<WorkflowRun, JobError> {
        let agent = link.agent_id.clone();
        self.run_workflow_template_inner(&agent, slug, input, Some(link))
            .await
    }

    async fn run_workflow_template_inner(
        &self,
        agent_id: &str,
        slug: &str,
        input: WorkflowTemplateRunInput,
        caller: Option<RunLink>,
    ) -> Result<WorkflowRun, JobError> {
        self.refuse_while_draining()?;
        validate_slug(slug).map_err(JobError::Validation)?;
        validate_run_input(&input).map_err(JobError::Validation)?;
        let (agent, slug) = (agent_id.to_owned(), slug.to_owned());
        self.mutate(move |state| {
            let config = state
                .get_agent(&agent)
                .ok_or(JobError::NotFound)?
                .state
                .config;
            if crate::agent_runs::is_helper_config(&config) {
                return Err(JobError::Conflict(
                    "Helpers cannot run workflow templates".into(),
                ));
            }
            if let Some(link) = &caller {
                check_caller(state, link, "run_workflow_template")?;
            }
            // Replay identity is checked against the pinned request before consulting a changed/deleted template.
            if let Some(original) = state
                .workflows
                .values()
                .find(|run| run.agent_id == agent && run.request_key == input.request_key)
            {
                return if original
                    .source_template
                    .as_ref()
                    .is_some_and(|source| source.slug == slug && source.request == input)
                {
                    Ok(original.clone())
                } else {
                    Err(JobError::Conflict(
                        "Workflow request key was already used for different input".into(),
                    ))
                };
            }
            if state.workflows.len() >= MAX_WORKFLOWS {
                return Err(JobError::Conflict(
                    "Workflow history capacity is exhausted".into(),
                ));
            }
            let template = state
                .workflow_templates
                .get(&template_key(&agent, &slug))
                .ok_or(JobError::NotFound)?;
            let workflow = template
                .materialize(
                    &uuid::Uuid::new_v4().to_string(),
                    &input,
                    now_ms().max(template.updated_at_ms),
                )
                .map_err(JobError::Conflict)?;
            state
                .workflows
                .insert(workflow.id.clone(), workflow.clone());
            Ok(workflow)
        })
        .await
    }
}

fn check_caller(state: &DaemonState, link: &RunLink, tool: &str) -> Result<(), JobError> {
    let agent = state.get_agent(&link.agent_id).ok_or(JobError::NotFound)?;
    if crate::agent_runs::is_helper_config(&agent.state.config)
        || !agent
            .state
            .config
            .tools
            .as_ref()
            .is_some_and(|tools| tools.iter().any(|entry| entry.name == tool))
        || !state.runs.get(&link.run_id).is_some_and(|run| {
            run.agent_id == link.agent_id
                && run.session_id == link.session_id
                && matches!(
                    run.source,
                    RunSource::Web | RunSource::Api | RunSource::Telegram | RunSource::Discord
                )
                && run.status == RunStatus::Running
        })
    {
        return Err(JobError::Conflict(
            "Workflow template tools require an explicitly granted ordinary companion run".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_snapshot(
    templates: &[WorkflowTemplate],
    workflows: &[WorkflowRun],
) -> Result<(), String> {
    if templates.len() > MAX_TEMPLATES {
        return Err("Workflow template snapshot exceeds capacity".into());
    }
    let mut keys = HashSet::new();
    for template in templates {
        template.validate()?;
        if !keys.insert((&template.agent_id, &template.slug)) {
            return Err("Duplicate workflow template identity".into());
        }
    }
    for workflow in workflows {
        let Some(source) = &workflow.source_template else {
            continue;
        };
        let template = templates
            .iter()
            .find(|template| template.agent_id == workflow.agent_id && template.slug == source.slug)
            .ok_or("Pinned workflow template is missing")?;
        let receipt = template
            .save_history
            .get(
                source
                    .revision
                    .checked_sub(1)
                    .ok_or("Invalid template revision")? as usize,
            )
            .ok_or("Pinned workflow template revision is missing")?;
        if workflow.created_at_ms < receipt.saved_at_ms {
            return Err("Workflow predates its pinned template revision".into());
        }
        // Rebuild the pinned revision to prove immutable definition and resolved defaults.
        let mut pinned = template.clone();
        pinned.revision = source.revision;
        pinned.deleted = false;
        pinned.title = receipt.request.title.clone();
        pinned.parameters = receipt.request.parameters.clone();
        pinned.steps = receipt.request.steps.clone();
        pinned.updated_at_ms = receipt.saved_at_ms;
        pinned.save_history.truncate(source.revision as usize);
        let materialized =
            pinned.materialize(&workflow.id, &source.request, workflow.created_at_ms)?;
        let definition = anima_core::workflows::WorkflowInput {
            title: materialized.title,
            request_key: materialized.request_key,
            steps: materialized
                .steps
                .into_iter()
                .map(|step| anima_core::workflows::WorkflowStepSpec {
                    id: step.id,
                    title: step.title,
                    kind: step.kind,
                    prompt: step.prompt,
                })
                .collect(),
        };
        if !workflow.definition_matches(&definition)
            || workflow.source_template != materialized.source_template
        {
            return Err("Workflow differs from its pinned template definition or inputs".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::{
        workflow_templates::WorkflowTemplateParameter,
        workflows::{WorkflowStepKind, WorkflowStepSpec},
    };
    use std::collections::BTreeMap;

    fn definition() -> WorkflowTemplateInput {
        WorkflowTemplateInput {
            slug: "brief".into(),
            revision: 0,
            title: "Brief ${topic}".into(),
            parameters: vec![WorkflowTemplateParameter {
                name: "topic".into(),
                default: None,
            }],
            steps: vec![WorkflowStepSpec {
                id: "review".into(),
                title: "Review".into(),
                kind: WorkflowStepKind::Approval,
                prompt: "Review ${topic}".into(),
            }],
        }
    }
    fn request(key: &str, topic: &str, revision: u64) -> WorkflowTemplateRunInput {
        WorkflowTemplateRunInput {
            request_key: key.into(),
            revision,
            inputs: BTreeMap::from([("topic".into(), topic.into())]),
        }
    }

    #[tokio::test]
    async fn workflow_templates_fresh_runs_exact_replay_after_edit_delete_and_restart() {
        let (service, agent, path) = crate::jobs::tests::setup();
        let saved = service
            .save_workflow_template(&agent, definition())
            .await
            .unwrap();
        let first = service
            .run_workflow_template(&agent, "brief", request("first", "Alpha", 1))
            .await
            .unwrap();
        let second = service
            .run_workflow_template(&agent, "brief", request("second", "Beta", 1))
            .await
            .unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(first.title, "Brief Alpha");
        assert_eq!(second.title, "Brief Beta");
        let mut changed = definition();
        changed.revision = saved.revision;
        changed.title = "New ${topic}".into();
        service
            .save_workflow_template(&agent, changed.clone())
            .await
            .unwrap();
        assert_eq!(
            service
                .save_workflow_template(&agent, changed)
                .await
                .unwrap()
                .revision,
            2
        );
        assert_eq!(
            service
                .run_workflow_template(&agent, "brief", request("first", "Alpha", 1))
                .await
                .unwrap(),
            first
        );
        assert!(service
            .run_workflow_template(&agent, "brief", request("first", "Changed", 1))
            .await
            .is_err());
        assert!(service
            .run_workflow_template(&agent, "brief", request("stale", "Alpha", 1))
            .await
            .is_err());
        service
            .delete_workflow_template(&agent, "brief", 2)
            .await
            .unwrap();
        assert!(service
            .list_workflow_templates(&agent)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            service
                .run_workflow_template(&agent, "brief", request("first", "Alpha", 1))
                .await
                .unwrap(),
            first
        );
        let snapshot = crate::control_plane_store::load_control_plane_snapshot(
            &crate::control_plane_store::ControlPlaneStoreConfig::Json(path.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        validate_snapshot(&snapshot.workflow_templates, &snapshot.workflows).unwrap();
        let mut restored = DaemonState::new();
        restored.restore_control_plane_snapshot(snapshot).unwrap();
        assert_eq!(restored.workflows[&first.id], first);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn workflow_templates_failed_persistence_rolls_back_and_cross_agent_reads_are_empty() {
        let (service, agent, path) = crate::jobs::tests::setup();
        std::fs::create_dir(&path).unwrap();
        assert!(service
            .save_workflow_template(&agent, definition())
            .await
            .is_err());
        assert!(service.state.read().await.workflow_templates.is_empty());
        std::fs::remove_dir(&path).unwrap();
        service
            .save_workflow_template(&agent, definition())
            .await
            .unwrap();
        let other = service
            .state
            .write()
            .await
            .create_agent(
                serde_json::from_value(serde_json::json!({"name":"other","model":"deterministic"}))
                    .unwrap(),
            )
            .unwrap()
            .state
            .id;
        assert!(service
            .list_workflow_templates(&other)
            .await
            .unwrap()
            .is_empty());
        assert!(service
            .get_workflow_template(&other, "brief")
            .await
            .is_err());
        let mut invalid = service
            .state
            .read()
            .await
            .workflow_templates
            .values()
            .cloned()
            .collect::<Vec<_>>();
        invalid[0].title = "Forged".into();
        assert!(validate_snapshot(&invalid, &[]).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn workflow_templates_recheck_explicit_grants_and_ordinary_run_source() {
        use crate::runs::{RunRecord, RunStart};
        let (service, agent, _path) = crate::jobs::tests::setup();
        let mut state = service.state.write().await;
        for tool in [
            "save_workflow_template",
            "list_workflow_templates",
            "run_workflow_template",
        ] {
            let mut grant = crate::tools::ToolRegistry::new()
                .descriptor("create_workflow")
                .unwrap();
            grant.name = tool.into();
            state
                .update_agent(
                    &agent,
                    serde_json::from_value(serde_json::json!({"tools":[grant]})).unwrap(),
                )
                .unwrap();
            for source in [
                RunSource::Web,
                RunSource::Api,
                RunSource::Telegram,
                RunSource::Discord,
                RunSource::Job,
                RunSource::Schedule,
                RunSource::Delegation,
                RunSource::Peer,
            ] {
                let record = RunRecord::running(
                    RunStart {
                        agent_id: agent.clone(),
                        session_id: "template-session".into(),
                        source,
                        source_ref: None,
                        idempotency_key: None,
                        text: "save template".into(),
                        model: "deterministic".into(),
                        provider: None,
                        parent_run_id: None,
                    },
                    now_ms(),
                );
                let link = RunLink {
                    agent_id: agent.clone(),
                    session_id: record.session_id.clone(),
                    run_id: record.id.clone(),
                };
                state.runs.insert(record);
                assert_eq!(
                    check_caller(&state, &link, tool).is_ok(),
                    matches!(
                        source,
                        RunSource::Web | RunSource::Api | RunSource::Telegram | RunSource::Discord
                    )
                );
                state.runs.get_mut(&link.run_id).unwrap().status = RunStatus::Completed;
                assert!(check_caller(&state, &link, tool).is_err());
            }
            state
                .update_agent(
                    &agent,
                    serde_json::from_value(serde_json::json!({"tools":[]})).unwrap(),
                )
                .unwrap();
            let record = RunRecord::running(
                RunStart {
                    agent_id: agent.clone(),
                    session_id: "revoked-session".into(),
                    source: RunSource::Web,
                    source_ref: None,
                    idempotency_key: None,
                    text: "save template".into(),
                    model: "deterministic".into(),
                    provider: None,
                    parent_run_id: None,
                },
                now_ms(),
            );
            let link = RunLink {
                agent_id: agent.clone(),
                session_id: record.session_id.clone(),
                run_id: record.id.clone(),
            };
            state.runs.insert(record);
            assert!(check_caller(&state, &link, tool).is_err());
            let mut grant = crate::tools::ToolRegistry::new()
                .descriptor("create_workflow")
                .unwrap();
            grant.name = tool.into();
            // Role is immutable through AgentConfigUpdate. Create an actual helper
            // with the same explicit tool grant and an otherwise eligible live run.
            let mut helper_config = state.get_agent(&agent).unwrap().state.config;
            helper_config.name = format!("helper-{tool}");
            helper_config.tools = Some(vec![grant]);
            helper_config
                .settings
                .get_or_insert_with(Default::default)
                .additional
                .insert(
                    "workspaceRole".into(),
                    anima_core::DataValue::String("helper".into()),
                );
            let helper = state.create_agent(helper_config).unwrap().state;
            assert!(crate::agent_runs::is_helper_config(&helper.config));
            assert!(helper.config.allows_tool(tool));
            let record = RunRecord::running(
                RunStart {
                    agent_id: helper.id.clone(),
                    session_id: "helper-template-session".into(),
                    source: RunSource::Web,
                    source_ref: None,
                    idempotency_key: None,
                    text: "save template".into(),
                    model: "deterministic".into(),
                    provider: None,
                    parent_run_id: None,
                },
                now_ms(),
            );
            let helper_link = RunLink {
                agent_id: helper.id,
                session_id: record.session_id.clone(),
                run_id: record.id.clone(),
            };
            state.runs.insert(record);
            assert!(check_caller(&state, &helper_link, tool).is_err());
        }
    }
}
