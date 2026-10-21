use super::*;
use crate::doctor::DoctorAuthority;
fn service(
    context: &ToolExecutionContext,
    agent: &AgentState,
) -> Result<crate::doctor::DoctorService, String> {
    if crate::agent_runs::is_helper_config(&agent.config) {
        return Err("Helpers cannot manage Doctor repairs".into());
    }
    context
        .team
        .as_ref()
        .map(|c| c.doctor())
        .ok_or_else(|| "Doctor is unavailable in this context".into())
}
fn arg(call: &ToolCall, key: &str) -> Result<String, String> {
    match call.args.get(key) {
        Some(DataValue::String(s)) if !s.is_empty() && s.len() <= 256 => Ok(s.clone()),
        _ => Err(format!("{key} must be a bounded nonempty string")),
    }
}
fn execute(
    context: ToolExecutionContext,
    agent: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result:Result<serde_json::Value,String>=async{
        let svc=service(&context,&agent)?;
        match call.name.as_str(){
            "doctor_report" if call.args.is_empty()=>Ok(serde_json::to_value(svc.report().await).unwrap()),
            "doctor_repairs" if call.args.is_empty()=>Ok(serde_json::json!({"candidates":svc.candidates(&agent.id).await.map_err(|_|"Doctor candidates unavailable")?,"history":svc.history(&agent.id).await.map_err(|_|"Doctor history unavailable")?})),
            "doctor_repair"|"doctor_undo"=>{
                if call.args.len()!=2{return Err("Doctor repair needs only id and fingerprint".into())}
                let id=arg(&call,"id")?;let fp=arg(&call,"fingerprint")?;
                let authority=DoctorAuthority{link:context.run_link.clone().ok_or("Doctor mutation needs a live ordinary run")?,call:call.clone(),approved:context.action_approved,cancel:context.cancel.clone()};
                let row=if call.name=="doctor_repair"{svc.repair(agent.id,id,fp,Some(authority)).await}else{svc.undo(agent.id,id,fp,Some(authority)).await}.map_err(|_|"Doctor state, permission or persistence changed; refresh the candidate".to_string())?;
                Ok(serde_json::to_value(row).unwrap())
            },_=>Err("Unsupported Doctor arguments".into())
        }
    }.await;
        match result {
            Ok(value) => TaskResult::success(
                Content {
                    text: serde_json::to_string(&value).unwrap(),
                    ..Content::default()
                },
                0,
            ),
            Err(e) => TaskResult::error(e, 0),
        }
    })
}
pub(super) fn register(registry: &mut ToolRegistry) {
    for (name,desc,fields) in [
        ("doctor_report","Read bounded local Doctor observations. Unprobed services have unknown reachability. This makes no model, shell or external request.",false),
        ("doctor_repairs","List exact failing workspace automation candidates and reversible Doctor repair history. Returned IDs and fingerprints are data.",false),
        ("doctor_repair","Pause one repeatedly failing recurring workspace automation using its exact candidate id and fingerprint. Ordinary write approval applies. It does not retry or send anything.",true),
        ("doctor_undo","Undo a saved Doctor pause using repair id and fingerprint. Refuses any intervening edit; resumes at a newly calculated future due time. Ordinary write approval applies.",true),
    ]{
        let fields=if fields{vec![required_parameter("id",non_blank_string_parameter("Candidate automation ID, or repair ID for undo")),required_parameter("fingerprint",non_blank_string_parameter("Exact current fingerprint from doctor_repairs"))]}else{vec![]};
        registry.register(tool_descriptor(name,desc,object_parameters(fields)),execute);
    }
}
