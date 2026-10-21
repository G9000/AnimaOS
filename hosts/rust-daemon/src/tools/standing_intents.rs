use super::*;
use crate::standing_intents::StandingAuthority;
fn text(call: &ToolCall, key: &str) -> Result<String, String> {
    match call.args.get(key) {
        Some(DataValue::String(s)) if !s.trim().is_empty() && s.len() <= 256 => Ok(s.clone()),
        _ => Err(format!("{key} must be a bounded string")),
    }
}
fn revision(call: &ToolCall) -> Result<u64, String> {
    match call.args.get("revision") {
        Some(DataValue::Number(n))
            if n.is_finite() && *n >= 1.0 && *n <= 9_007_199_254_740_991.0 && n.fract() == 0.0 =>
        {
            Ok(*n as u64)
        }
        _ => Err("revision must be an exact positive integer".into()),
    }
}
fn spec(call: &ToolCall) -> Result<anima_core::standing_intents::IntentSpec, String> {
    let value = call.args.get("spec").ok_or("spec is required")?;
    let DataValue::Object(fields) = value else {
        return Err("spec must be an object".into());
    };
    let mut json = crate::routes::data_value_to_json(value);
    for key in ["expiresAtMs", "cooldownMs", "maxFires"] {
        let Some(DataValue::Number(n)) = fields.get(key) else {
            return Err(format!("{key} must be an integer"));
        };
        if !n.is_finite() || *n < 0.0 || *n > 9_007_199_254_740_991.0 || n.fract() != 0.0 {
            return Err(format!(
                "{key} must be an exact bounded nonnegative integer"
            ));
        }
        json[key] = serde_json::json!(*n as u64);
    }
    if json.to_string().len() > 8192 {
        return Err("Intention specification is too large".into());
    }
    serde_json::from_value(json).map_err(|_| "Invalid standing intention specification".into())
}
fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result:Result<serde_json::Value,String>=async {
  if !context.standing_owner_turn { return Err("Standing intention tools require an authenticated local-owner turn".into()); }
  if crate::agent_runs::is_helper_config(&agent.config){return Err("Helpers cannot manage standing intentions".into())}let coordinator=context.team.as_ref().ok_or("Standing intentions require an ordinary daemon run")?;let svc=coordinator.standing_intents();
  if call.name=="list_standing_intents" {if !call.args.is_empty(){return Err("list_standing_intents takes no arguments".into())}return svc.list(&agent.id).await.map_err(|_|"Standing intention state is unavailable".into())}
  let authority=Some(StandingAuthority{link:context.run_link.clone().ok_or("A live owner run is required")?,call:call.clone(),approved:context.action_approved,owner_context:context.standing_owner_turn,cancel:context.cancel.clone()});
  let row=match call.name.as_str(){
   "create_standing_intent" if call.args.len()==1=>svc.create(agent.id.clone(),spec(&call)?,authority).await,
   "update_standing_intent" if call.args.len()==3=>svc.update(agent.id.clone(),text(&call,"id")?,revision(&call)?,spec(&call)?,authority).await,
   "cancel_standing_intent" if call.args.len()==2=>svc.cancel(agent.id.clone(),text(&call,"id")?,revision(&call)?,authority).await,
   "undo_standing_intent" if call.args.len()==1=>svc.undo(agent.id.clone(),text(&call,"change_id")?,authority).await,
   _=>return Err("Unsupported standing intention arguments".into()),
  }.map_err(|_|"Standing intention state, owner authority, approval or persistence changed; inspect current state".to_string())?;
  serde_json::to_value(row).map_err(|_|"Standing intention output is unavailable".into())
 }.await;
        match result {
            Ok(v) => TaskResult::success(
                Content {
                    text: v.to_string(),
                    ..Default::default()
                },
                0,
            ),
            Err(e) => TaskResult::error(e, 0),
        }
    })
}
fn exact_object(fields: Vec<SchemaParameter>) -> BTreeMap<String, DataValue> {
    let mut object = object_parameters(fields);
    object.insert("additionalProperties".into(), DataValue::Bool(false));
    object
}
fn spec_parameter() -> DataValue {
    let mut session = typed_parameter_schema(
        "string",
        "Exact session ID, or null/omit for all owner conversations",
    );
    session.insert(
        "type".into(),
        DataValue::Array(vec![
            DataValue::String("string".into()),
            DataValue::String("null".into()),
        ]),
    );
    let scope = exact_object(vec![
        optional_parameter("sessionId", DataValue::Object(session)),
        required_parameter(
            "sender",
            string_enum_parameter(
                "Proven owner source scope; arbitrary participants are unsupported",
                &["owner", "localOwner", "telegramOwner", "discordOwner"],
            ),
        ),
    ]);
    let condition = exact_object(vec![
        required_parameter(
            "kind",
            string_enum_parameter("Deterministic condition type", &["keyword", "event"]),
        ),
        optional_parameter(
            "phrase",
            non_blank_string_parameter("Case-insensitive whole phrase; at most 128 UTF-8 bytes"),
        ),
        optional_parameter(
            "event",
            string_enum_parameter(
                "Host-derived conversational event; no clock task is created",
                &["conversation.started", "conversation.message"],
            ),
        ),
    ]);
    DataValue::Object(exact_object(vec![required_parameter("scope",DataValue::Object(scope)),required_parameter("condition",DataValue::Object(condition)),required_parameter("instruction",non_blank_string_parameter("Owner-directed instruction appended to the same matching run; at most 2048 UTF-8 bytes")),required_parameter("expiresAtMs",integer_parameter("Future Unix epoch milliseconds, at most 366 days away",1)),required_parameter("cooldownMs",integer_parameter("Minimum milliseconds between matching admissions; 0 allows consecutive turns",0)),required_parameter("maxFires",integer_parameter("Total durable fire budget, 1 to 32; failure does not rearm it",1))]))
}
pub(super) fn register(registry: &mut ToolRegistry) {
    registry.register(tool_descriptor("list_standing_intents","Inspect your saved owner conversational intentions, revisions, consumed fire budgets, cancellation state and exact Undo handles. This does not trigger a match.",exact_object(vec![])),execute);
    registry.register(tool_descriptor("create_standing_intent","Save an explicit instruction from the authenticated local owner for a future matching owner turn. Keyword and host conversation events only. Requires ordinary write approval; connector, peer, helper and background sources cannot create intentions.",exact_object(vec![required_parameter("spec",spec_parameter())])),execute);
    registry.register(tool_descriptor("update_standing_intent","Replace a saved conversational intention using the exact current revision from list_standing_intents. Only an authenticated local-owner run may change or widen it; ordinary write approval applies.",exact_object(vec![required_parameter("id",non_blank_string_parameter("Exact intention ID")),required_parameter("revision",integer_parameter("Exact current revision",1)),required_parameter("spec",spec_parameter())])),execute);
    registry.register(tool_descriptor("cancel_standing_intent","Cancel an exact current intention before future admissions. A run already admitted retains normal Stop control. Requires current revision and ordinary write approval.",exact_object(vec![required_parameter("id",non_blank_string_parameter("Exact intention ID")),required_parameter("revision",integer_parameter("Exact current revision",1))])),execute);
    registry.register(tool_descriptor("undo_standing_intent","Undo one saved standing intention change using its change_id from list_standing_intents. Refuses any intervening edit or consumed fire; ordinary write approval and authenticated local-owner source are required.",exact_object(vec![required_parameter("change_id",non_blank_string_parameter("Exact change ID; this differs from the intention ID"))])),execute);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standing_intents_tools_have_write_risk_and_ordinary_read_list() {
        let r = ToolRegistry::new();
        for name in [
            "create_standing_intent",
            "update_standing_intent",
            "cancel_standing_intent",
            "undo_standing_intent",
        ] {
            assert!(r.descriptor(name).is_some());
            assert_eq!(
                crate::approvals::risk_class(name),
                crate::approvals::RiskClass::Write
            )
        }
        assert_eq!(
            crate::approvals::risk_class("list_standing_intents"),
            crate::approvals::RiskClass::Read
        );
    }
    #[test]
    fn standing_intents_model_spec_decodes_exact_integral_runtime_numbers_only() {
        let mut fields = BTreeMap::from([
            (
                "scope".into(),
                DataValue::Object(BTreeMap::from([
                    ("sessionId".into(), DataValue::Null),
                    ("sender".into(), DataValue::String("owner".into())),
                ])),
            ),
            (
                "condition".into(),
                DataValue::Object(BTreeMap::from([
                    ("kind".into(), DataValue::String("event".into())),
                    (
                        "event".into(),
                        DataValue::String("conversation.message".into()),
                    ),
                ])),
            ),
            (
                "instruction".into(),
                DataValue::String("Ask about the project".into()),
            ),
            ("expiresAtMs".into(), DataValue::Number(2_000_000_000_000.0)),
            ("cooldownMs".into(), DataValue::Number(0.0)),
            ("maxFires".into(), DataValue::Number(2.0)),
        ]);
        let call = |fields: BTreeMap<String, DataValue>| ToolCall {
            id: "call".into(),
            name: "create_standing_intent".into(),
            args: BTreeMap::from([("spec".into(), DataValue::Object(fields))]),
        };
        assert_eq!(spec(&call(fields.clone())).unwrap().max_fires, 2);
        for invalid in [
            DataValue::Number(1.5),
            DataValue::Number(f64::NAN),
            DataValue::Number(9_007_199_254_740_992.0),
            DataValue::String("2".into()),
        ] {
            fields.insert("maxFires".into(), invalid);
            assert!(spec(&call(fields.clone())).is_err());
        }
    }
}
