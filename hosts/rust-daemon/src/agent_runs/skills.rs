//! Skills from the coordinator (spec §8): the service its tools and the
//! routes use, and what each run sees.

use std::sync::Arc;

use anima_core::{AgentRuntime, Content};
use tracing::warn;

use super::AgentRunCoordinator;
use crate::skills::runtime::{index_text, requested_skill, requested_text, SkillContextProvider};
use crate::skills::SkillService;

impl AgentRunCoordinator {
    /// The workspace's skills, changed under this coordinator's
    /// control-plane transaction.
    pub(crate) fn skills(&self) -> SkillService {
        SkillService::new(Arc::clone(&self.state), self.control_plane_transactions())
            .with_evaluation_admission(self.work_admission_probe())
    }

    /// Adds the skills index and a `/skill` message's instructions to this
    /// run's isolated runtime (spec §8.3). Neither ever fails the run: a
    /// skill that cannot be loaded is logged and noted for the model.
    pub(super) async fn apply_skills(
        &self,
        runtime: &mut AgentRuntime,
        content: &Content,
        run_id: &str,
        session_id: &str,
    ) {
        if let Some(guidance) = anima_core::learn::learn_guidance(&content.text) {
            runtime.register_provider(Arc::new(SkillContextProvider::learn(guidance.into())));
        }
        let skills = self.skills();
        // An agent without `load_skill` could not load a listed skill.
        if runtime.config().allows_tool("load_skill") {
            if let Some(text) = index_text(&skills.index().await) {
                runtime.register_provider(Arc::new(SkillContextProvider::index(text)));
            }
        }
        if runtime.config().allows_tool("propose_skill")
            && anima_core::learn::learn_guidance(&content.text).is_none()
        {
            runtime.register_provider(Arc::new(SkillContextProvider::learn(
                anima_core::learn::skill_authoring_guidance().into(),
            )));
        }
        // Plan mode is the owner's per-session choice; without the plan tool
        // there is nothing to plan with.
        if runtime.config().allows_tool(crate::tools::PLAN_TOOL)
            && self
                .state
                .read()
                .await
                .plan_first_for(runtime.id(), session_id)
        {
            runtime.register_provider(Arc::new(SkillContextProvider::plan_first()));
        }
        if let Some(slug) = requested_skill(content) {
            // By exact slug, reread and rehashed now: the route's check at
            // acceptance is advisory.
            let loaded = skills.load_slug(&slug).await;
            if let Ok(skill) = &loaded {
                let by = crate::skills::ProposedBy {
                    agent_id: runtime.id().to_owned(),
                    session_id: session_id.into(),
                    run_id: run_id.into(),
                };
                if let Err(error) = skills
                    .record_use(&skill.slug, &skill.approved_hash, by)
                    .await
                {
                    warn!(reason=%error.message(), "explicit skill use observation could not be saved");
                }
            }
            if let Err(problem) = &loaded {
                warn!(skill = %slug, problem = %problem, "a skill message's skill could not be loaded");
            }
            runtime.register_provider(Arc::new(SkillContextProvider::requested(requested_text(
                &slug,
                loaded.as_ref().map_err(String::as_str),
            ))));
        }
    }
}
