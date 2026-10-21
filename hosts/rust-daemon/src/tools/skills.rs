//! `load_skill` and `propose_skill` (spec §8.3). `load_skill` reads an
//! owner-approved skill's instructions, checked against the hash the owner
//! approved; `propose_skill` leaves a draft for the owner and never touches
//! `SKILL.md`, except in Free, where the draft is approved at once through
//! the owner's approval path and logged in the skill ledger for undo.

use anima_core::{AgentState, Content, DataValue, Message, TaskResult, ToolCall};
use futures::future::BoxFuture;

use super::ToolExecutionContext;
use crate::agent_runs::is_helper_config;
use crate::skills::ledger::{activated_reply, ProposeOutcome};
use crate::skills::{
    proposed_reply, Proposal, ProposedBy, SkillError, HELPERS_CANNOT_PROPOSE_SKILLS,
    PROPOSAL_NOT_SAVED, SKILLS_UNAVAILABLE, SKILL_INSTRUCTIONS_HEADER,
};

fn text_arg<'a>(call: &'a ToolCall, key: &str) -> Option<&'a str> {
    match call.args.get(key) {
        Some(DataValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn text(text: String) -> TaskResult<Content> {
    TaskResult::success(
        Content {
            text,
            ..Content::default()
        },
        0,
    )
}

pub(super) fn load_skill(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let Some(name) = text_arg(&call, "name")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
        else {
            return TaskResult::error("load_skill name must be a non-empty string", 0);
        };
        let Some(coordinator) = context.team.clone() else {
            return TaskResult::error(SKILLS_UNAVAILABLE, 0);
        };
        let file_path = match call.args.get("filePath") {
            None => None,
            Some(DataValue::String(path)) => Some(path.as_str()),
            Some(_) => return TaskResult::error("load_skill filePath must be a string", 0),
        };
        match coordinator.skills().load_file(&name, file_path).await {
            Ok(skill) => {
                if let Some(link) = context.run_link {
                    if let Err(error) = coordinator
                        .skills()
                        .record_use(
                            &skill.slug,
                            &skill.approved_hash,
                            ProposedBy {
                                agent_id: agent.id,
                                session_id: link.session_id,
                                run_id: link.run_id,
                            },
                        )
                        .await
                    {
                        tracing::warn!(reason = %error.message(), "skill use observation could not be saved");
                    }
                }
                text(format!(
                    "{SKILL_INSTRUCTIONS_HEADER}\n\nSkill version (SHA-256): {}\n\n{}",
                    skill.approved_hash, skill.body
                ))
            }
            Err(message) => TaskResult::error(message, 0),
        }
    })
}

pub(super) fn propose_skill(
    context: ToolExecutionContext,
    agent: AgentState,
    _user_message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        // Spec §8.3: helpers cannot use propose_skill, even if a config
        // somehow carries it.
        if is_helper_config(&agent.config) {
            return TaskResult::error(HELPERS_CANNOT_PROPOSE_SKILLS, 0);
        }
        let (Some(name), Some(description), Some(body)) = (
            text_arg(&call, "name"),
            text_arg(&call, "description"),
            text_arg(&call, "body"),
        ) else {
            return TaskResult::error("propose_skill needs name, description, and body strings", 0);
        };
        let (Some(coordinator), Some(link)) = (context.team.clone(), context.run_link.clone())
        else {
            return TaskResult::error(SKILLS_UNAVAILABLE, 0);
        };
        let mut files = match supporting_files(&call) {
            Ok(files) => files,
            Err(error) => return TaskResult::error(error, 0),
        };
        let mut expected_base = None;
        let mut slug = text_arg(&call, "slug").map(str::to_string);
        // Omission preserves a freshly read approved package, pinned to its base.
        // An explicit [] requests removal for the owner's ordinary review.
        if !call.args.contains_key("files") {
            match coordinator
                .skills()
                .supporting_files_for_proposal(name, slug.as_deref())
                .await
            {
                Ok((normalized_slug, base, preserved)) => {
                    slug = Some(normalized_slug);
                    expected_base = Some(base);
                    files = preserved;
                }
                Err(error) => return TaskResult::error(error.message(), 0),
            }
        }
        let proposal = Proposal {
            files,
            by: ProposedBy {
                agent_id: agent.id.clone(),
                session_id: link.session_id,
                run_id: link.run_id,
            },
            name: name.to_string(),
            description: description.to_string(),
            body: body.to_string(),
            slug,
        };
        // Autonomy spec 2026-10-19: in Free the skill is approved at once
        // (marked `autonomy:free`, logged for the owner's undo).
        match coordinator
            .skills()
            .propose_from_agent_with_base(proposal, expected_base)
            .await
        {
            Ok(ProposeOutcome::Drafted(draft)) => text(proposed_reply(&draft)),
            Ok(ProposeOutcome::Activated { approved, action }) => {
                text(activated_reply(&approved, action))
            }
            Err(SkillError::Unavailable(_)) => TaskResult::error(PROPOSAL_NOT_SAVED, 0),
            Err(error) => TaskResult::error(error.message(), 0),
        }
    })
}

fn supporting_files(
    call: &ToolCall,
) -> Result<Vec<crate::skills::SkillSupportingFile>, &'static str> {
    let Some(value) = call.args.get("files") else {
        return Ok(Vec::new());
    };
    let DataValue::Array(values) = value else {
        return Err("propose_skill files must be an array of path/content objects");
    };
    if values.len() > 32 {
        return Err("A skill package supports at most 32 supporting files");
    }
    values
        .iter()
        .map(|value| {
            let DataValue::Object(fields) = value else {
                return Err("Each supporting file needs path and content strings");
            };
            if fields.len() != 2 {
                return Err("Each supporting file needs only path and content strings");
            }
            match (fields.get("path"), fields.get("content")) {
                (Some(DataValue::String(path)), Some(DataValue::String(content))) => {
                    Ok(crate::skills::SkillSupportingFile {
                        path: path.clone(),
                        content: content.clone(),
                    })
                }
                _ => Err("Each supporting file needs path and content strings"),
            }
        })
        .collect()
}
