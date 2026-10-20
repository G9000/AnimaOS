//! Scoped lifecycle operations reuse the existing automation transaction.
use super::*;
use crate::agent_runs::AgentRunCoordinator;
use crate::schedules::automations::{AutomationRunAuthority, AutomationRunTool};
use crate::schedules::AutomationPatch;

fn coordinator(
    context: &ToolExecutionContext,
    agent: &AgentState,
) -> Result<AgentRunCoordinator, String> {
    if is_helper_config(&agent.config) {
        return Err(HELPERS_CANNOT_MANAGE_AUTOMATIONS.into());
    }
    context
        .team
        .clone()
        .ok_or_else(|| AUTOMATIONS_UNAVAILABLE.into())
}

pub(super) fn authority(
    context: &ToolExecutionContext,
    tool: AutomationRunTool,
    call: &ToolCall,
) -> Result<AutomationRunAuthority, String> {
    context
        .run_link
        .clone()
        .map(|link| {
            AutomationRunAuthority::new(
                link,
                tool,
                call.clone(),
                context.action_approved,
                context.cancel.clone(),
            )
        })
        .ok_or_else(|| AUTOMATIONS_UNAVAILABLE.into())
}

fn id<'a>(call: &'a ToolCall, fields: &[&str]) -> Result<&'a str, String> {
    if call.args.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(format!("{} contains an unsupported field", call.name));
    }
    text_arg(call, "id")
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("{} needs an id string", call.name))
}

fn optional_text<'a>(call: &'a ToolCall, key: &str) -> Result<Option<&'a str>, String> {
    match call.args.get(key) {
        None => Ok(None),
        Some(DataValue::String(value)) if !value.trim().is_empty() => Ok(Some(value.trim())),
        _ => Err(format!("{key} must be a non-empty string")),
    }
}

fn optional_bool(call: &ToolCall, key: &str) -> Result<Option<bool>, String> {
    match call.args.get(key) {
        None => Ok(None),
        Some(DataValue::Bool(value)) => Ok(Some(*value)),
        _ => Err(format!("{key} must be a boolean")),
    }
}

fn patch(
    call: &ToolCall,
    record: &ScheduledPromptRecord,
    owner_zone: chrono_tz::Tz,
) -> Result<AutomationPatch, String> {
    let zone = optional_text(call, "timeZone")?;
    if zone.is_some()
        && !call.args.contains_key("schedule")
        && !matches!(call.args.get("activeHours"), Some(DataValue::Object(_)))
    {
        return Err("timeZone requires schedule or activeHours".into());
    }
    let existing_zone = match &record.trigger {
        ScheduleTrigger::Daily { time_zone, .. } | ScheduleTrigger::Cron { time_zone, .. } => {
            time_zone.as_str()
        }
        _ => record
            .active_hours
            .as_ref()
            .map_or(owner_zone.name(), |hours| hours.time_zone.as_str()),
    };
    let zone = zone.unwrap_or(existing_zone);
    let clear_hours = optional_bool(call, "clearActiveHours")?.unwrap_or(false);
    if clear_hours && call.args.contains_key("activeHours") {
        return Err("Use activeHours or clearActiveHours, not both".into());
    }
    Ok(AutomationPatch {
        name: optional_text(call, "name")?.map(str::to_owned),
        prompt: optional_text(call, "prompt")?.map(str::to_owned),
        trigger: optional_text(call, "schedule")?
            .map(|text| parse_schedule(text, zone))
            .transpose()
            .map_err(str::to_owned)?,
        active_hours: if clear_hours {
            Some(None)
        } else if call.args.contains_key("activeHours") {
            Some(active_hours_arg(call, zone).map_err(str::to_owned)?)
        } else {
            None
        },
        enabled: optional_bool(call, "enabled")?,
        target: None,
    })
}

pub(crate) fn update_automation(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<String, String> = async {
            let coordinator = coordinator(&context, &agent)?;
            let authority = authority(&context, AutomationRunTool::Update, &call)?;
            let id = id(
                &call,
                &[
                    "id",
                    "name",
                    "prompt",
                    "schedule",
                    "timeZone",
                    "activeHours",
                    "clearActiveHours",
                    "target",
                    "enabled",
                ],
            )?;
            let service = coordinator.automations();
            let record = service.get(&agent.id, id).await.map_err(refusal)?;
            let mut patch = patch(&call, &record, context.owner_time_zone.effective())?;
            if let Some(target) = optional_text(&call, "target")? {
                patch.target = Some(match target {
                    "thread" => ScheduleTarget::Workspace,
                    "telegram" => service
                        .telegram_target(&agent.id)
                        .await
                        .ok_or_else(|| TELEGRAM_NOT_READY.to_string())?,
                    "discord" => service
                        .discord_target(&agent.id)
                        .await
                        .ok_or_else(|| DISCORD_NOT_READY.to_string())?,
                    _ => return Err(TARGET_ARG_INVALID.into()),
                });
            }
            let saved = service
                .update_from_run(&agent.id, id, patch, now_millis(), authority)
                .await
                .map_err(refusal)?;
            Ok(format!(
                "Updated the automation \"{}\" ({}), {}. {}",
                display_name(&saved),
                saved.id,
                describe(
                    &saved,
                    display_zone(&saved, context.owner_time_zone.effective())
                ),
                if saved.enabled { "On." } else { "Paused." }
            ))
        }
        .await;
        match result {
            Ok(reply) => text(reply),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(crate) fn resume_automation(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<String, String> = async {
            let coordinator = coordinator(&context, &agent)?;
            let authority = authority(&context, AutomationRunTool::Resume, &call)?;
            let id = id(&call, &["id"])?;
            let saved = coordinator
                .automations()
                .update_from_run(
                    &agent.id,
                    id,
                    AutomationPatch {
                        enabled: Some(true),
                        ..Default::default()
                    },
                    now_millis(),
                    authority,
                )
                .await
                .map_err(refusal)?;
            Ok(format!(
                "Resumed the automation \"{}\" ({}), {}.",
                display_name(&saved),
                saved.id,
                describe(
                    &saved,
                    display_zone(&saved, context.owner_time_zone.effective())
                )
            ))
        }
        .await;
        match result {
            Ok(reply) => text(reply),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(crate) fn delete_automation(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<String, String> = async {
            let coordinator = coordinator(&context, &agent)?;
            let authority = authority(&context, AutomationRunTool::Delete, &call)?;
            let id = id(&call, &["id"])?;
            coordinator.automations().delete_from_run(&agent.id, id, authority).await.map_err(refusal)?;
            Ok(format!("Deleted the automation ({id}). Already claimed work retains its existing cancellation and outcome handling."))
        }.await;
        match result {
            Ok(reply) => text(reply),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}

pub(crate) fn trigger_automation(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result: Result<String, String> = async {
            let coordinator = coordinator(&context, &agent)?;
            let id = id(&call, &["id"])?;
            let authority = authority(&context, AutomationRunTool::Trigger, &call)?;
            let claimed = coordinator.trigger_automation_from_run(&agent.id, authority, id).await.map_err(refusal)?;
            Ok(format!("Admitted one manual occurrence of \"{}\" ({}). Its final outcome is not yet known; use list_automations to inspect it. The schedule and enabled state are unchanged.", display_name(&claimed), claimed.id))
        }.await;
        match result {
            Ok(reply) => text(reply),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}
