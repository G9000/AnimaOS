//! Existing Office editing uses private preparation and one exact native owner decision.
use super::*;
use crate::document_edits::{
    self,
    approval::APPLY_TOOL,
    service::{ProposeRequest, SourceReference},
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectArguments {
    source: SourceReference,
    cursor: Option<String>,
}

pub(super) fn register(registry: &mut ToolRegistry) {
    let source = serde_json::json!({"type":"object","oneOf":[
        {"type":"object","properties":{"kind":{"const":"attachment"},"attachment_id":{"type":"string","minLength":1,"maxLength":128}},"required":["kind","attachment_id"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"workspace"},"file_path":{"type":"string","minLength":1,"maxLength":1024}},"required":["kind","file_path"],"additionalProperties":false}]});
    let expected = serde_json::json!({"type":"object","oneOf":[
        {"type":"object","properties":{"state":{"const":"absent"}},"required":["state"],"additionalProperties":false},
        {"type":"object","properties":{"state":{"const":"present"},"sha256":{"type":"string","pattern":"^[a-f0-9]{64}$"}},"required":["state","sha256"],"additionalProperties":false}]});
    let inspect = generated_documents::schema_data(
        serde_json::json!({"type":"object","properties":{"source":source,"cursor":{"type":"string","maxLength":96}},"required":["source"],"additionalProperties":false}),
    );
    let DataValue::Object(inspect) = inspect else {
        unreachable!("schema is an object")
    };
    registry.register(tool_descriptor("inspect_office_document","Inspect a selected uploaded or workspace DOCX, XLSX, PPTX or UTF-8 HTML as untrusted plain data. Requires an explicit read grant and an ordinary owner chat. Returns bounded hash-bound inventory pages; no file effect, executable HTML or Office rendering.",inspect),inspect_document);
    let propose = generated_documents::schema_data(
        serde_json::json!({"type":"object","properties":{"source":source,"expected_sha256":{"type":"string","pattern":"^[a-f0-9]{64}$"},"file_path":{"type":"string","minLength":1,"maxLength":1024},"expected_destination":expected,"operations":{"type":"array","minItems":1,"maxItems":128,"items":operation_schema()}},"required":["source","expected_sha256","file_path","expected_destination","operations"],"additionalProperties":false}),
    );
    let DataValue::Object(propose) = propose else {
        unreachable!("schema is an object")
    };
    registry.register(tool_descriptor("propose_document_edit","Prepare supported edits to one exact existing Office or UTF-8 HTML source. Stores a private candidate and complete structural review for at most 30 minutes. Use typed operations from the current hash-bound inventory. html_replace_text uses absolute UTF-8 byte offsets and exact raw expected_text including entities; new_text is literal text escaped for HTML, never markup. HTML source/candidate limit 1 MiB; selected/replacement text 8 KiB. Requires explicit grant.",propose),propose_document);
    let apply = generated_documents::schema_data(
        serde_json::json!({"type":"object","properties":{"proposal_id":{"type":"string","minLength":1,"maxLength":128},"review_sha256":{"type":"string","pattern":"^[a-f0-9]{64}$"},"file_path":{"type":"string","minLength":1,"maxLength":1024},"expected_destination":expected},"required":["proposal_id","review_sha256","file_path","expected_destination"],"additionalProperties":false}),
    );
    let DataValue::Object(apply) = apply else {
        unreachable!("schema is an object")
    };
    registry.register(tool_descriptor(APPLY_TOOL,"Publish the exact private document candidate after the owner has reviewed all changes and chosen Apply these changes. Every new publication requires native AllowOnce; remembered permission cannot approve it. Uses current source/destination hashes, native action evidence, configured checkpoints and atomic publication. Return the exact authenticated download_url to the owner.",apply),apply_document);
}

fn object(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn operation_schema() -> serde_json::Value {
    let text = serde_json::json!({"type":"string","maxLength":65536});
    let id = serde_json::json!({"type":"string","minLength":1,"maxLength":1024});
    let coordinate =
        serde_json::json!({"type":"string","pattern":"^[A-Za-z]{1,3}[1-9][0-9]{0,6}$"});
    let range = object(
        serde_json::json!({"sheet_id":id,"start":coordinate,"end":coordinate}),
        &["sheet_id", "start", "end"],
    );
    let value = serde_json::json!({"type":"object","oneOf":[
        object(serde_json::json!({"type":{"const":"string"},"value":text}),&["type","value"]),
        object(serde_json::json!({"type":{"const":"number"},"value":{"type":"number"}}),&["type","value"]),
        object(serde_json::json!({"type":{"const":"boolean"},"value":{"type":"boolean"}}),&["type","value"]),
        object(serde_json::json!({"type":{"const":"blank"}}),&["type"])]});
    let mut variants = Vec::new();
    variants.push(object(serde_json::json!({"type":{"const":"html_replace_text"},"start_utf8":{"type":"integer","minimum":0,"maximum":1048576},"end_utf8":{"type":"integer","minimum":0,"maximum":1048576},"expected_text":{"type":"string","minLength":1,"maxLength":8192},"new_text":{"type":"string","maxLength":8192}}), &["type","start_utf8","end_utf8","expected_text","new_text"]));
    for kind in ["word_replace_text", "slide_replace_text"] {
        variants.push(object(serde_json::json!({"type":{"const":kind},"target_id":id,"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"expected_text":text,"new_text":text}),&["type","target_id","start","end","expected_text","new_text"]));
    }
    variants.push(object(serde_json::json!({"type":{"const":"set_cell_value"},"sheet_id":id,"cell":coordinate,"value":value}),&["type","sheet_id","cell","value"]));
    variants.push(object(serde_json::json!({"type":{"const":"set_cell_formula"},"sheet_id":id,"cell":coordinate,"formula":{"type":"string","minLength":2,"maxLength":4096}}),&["type","sheet_id","cell","formula"]));
    variants.push(object(serde_json::json!({"type":{"const":"set_chart_title"},"chart_id":id,"expected_title":text,"title":text}),&["type","chart_id","expected_title","title"]));
    variants.push(object(serde_json::json!({"type":{"const":"set_chart_series_ranges"},"series_id":id,"categories":range,"values":range}),&["type","series_id","categories","values"]));
    variants.push(object(serde_json::json!({"type":{"const":"reorder_slides"},"slide_ids":{"type":"array","minItems":1,"maxItems":128,"items":id}}),&["type","slide_ids"]));
    serde_json::json!({"type":"object","oneOf":variants})
}
fn service(context: &ToolExecutionContext) -> Result<document_edits::DocumentEditService, String> {
    context
        .document_edits
        .clone()
        .ok_or("Document editing is unavailable in this context".into())
}
fn run(context: &ToolExecutionContext) -> Result<crate::runs::RunLink, String> {
    let run = context
        .run_link
        .clone()
        .ok_or("Document editing requires an ordinary owner run")?;
    let journal = context
        .action_journal
        .as_ref()
        .ok_or("Document editing requires a native action journal")?;
    if context.approvals.is_none() || journal.bound_run() != &run || context.rpc_fence.is_some() {
        return Err("Document editing requires direct owner run authority".into());
    }
    Ok(run)
}
fn arguments<T: serde::de::DeserializeOwned>(call: &ToolCall) -> Result<T, String> {
    serde_json::from_value(crate::routes::data_value_to_json(&DataValue::Object(
        call.args.clone(),
    )))
    .map_err(|_| "Document editing arguments are invalid".into())
}

fn proposal_arguments(call: &ToolCall) -> Result<ProposeRequest, String> {
    let mut value = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()));
    if let Some(operations) = value
        .get_mut("operations")
        .and_then(serde_json::Value::as_array_mut)
    {
        for operation in operations {
            let Some(fields) = operation.as_object_mut() else {
                continue;
            };
            let keys = match fields.get("type").and_then(serde_json::Value::as_str) {
                Some("word_replace_text" | "slide_replace_text") => ["start", "end"],
                Some("html_replace_text") => ["start_utf8", "end_utf8"],
                _ => continue,
            };
            for key in keys {
                if let Some(value) = fields.get_mut(key) {
                    let number = value
                        .as_f64()
                        .filter(|n| {
                            n.is_finite() && *n >= 0.0 && *n <= u32::MAX as f64 && n.fract() == 0.0
                        })
                        .ok_or("Document editing arguments are invalid")?;
                    *value = serde_json::Value::from(number as u32);
                }
            }
        }
    }
    serde_json::from_value(value).map_err(|_| "Document editing arguments are invalid".into())
}
fn result(value: Result<serde_json::Value, String>) -> TaskResult<Content> {
    match value {
        Ok(value) => TaskResult::success(
            Content {
                text: value.to_string(),
                ..Default::default()
            },
            0,
        ),
        Err(error) => TaskResult::error(error, 0),
    }
}
fn inspect_document(
    context: ToolExecutionContext,
    _: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        result(
            async {
                let input: InspectArguments = arguments(&call)?;
                let page = service(&context)?
                    .inspect(
                        &run(&context)?,
                        input.source,
                        input.cursor.as_deref(),
                        context.cancel.clone().unwrap_or_default(),
                    )
                    .await?;
                serde_json::to_value(page)
                    .map_err(|_| "Document inventory cannot be encoded".into())
            }
            .await,
        )
    })
}
fn propose_document(
    context: ToolExecutionContext,
    _: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        result(
            async {
                service(&context)?
                    .propose(
                        run(&context)?,
                        proposal_arguments(&call)?,
                        context.cancel.clone().unwrap_or_default(),
                    )
                    .await
            }
            .await,
        )
    })
}
fn apply_document(
    context: ToolExecutionContext,
    agent: AgentState,
    _: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let outcome=async {
        let service=service(&context)?;let run=run(&context)?;
        if let Some(receipt)=service.replay(&run,&call).await? {
            return Ok(checkpoints::mutation_success(serde_json::json!({"version":1,"path":receipt.file_path,"sha256":receipt.sha256,"size_bytes":receipt.size_bytes,"download_url":format!("/api/workspace/file/download?path={}",document_edits::service::encode(&receipt.file_path)),"provenance":"imported_edit","preview_quality":"structural_content","checkpoint_id":receipt.checkpoint_id,"replayed":true}).to_string(),None));
        }
        let witness=context.document_edit_witness.clone().ok_or("Document exact review witness is unavailable")?;
        let approval=context.document_edit_approval.clone().ok_or("Document exact owner AllowOnce decision is unavailable")?;
        if !context.action_approved { return Err("Document owner has not approved this exact review".into()); }
        context.live_checks(&agent,&call).await.map_err(|result|result.error.unwrap_or_else(||"Document authority changed".into()))?;
        let applying=service.reserve(run.clone(),call.clone(),witness,approval,context.cancel.clone().unwrap_or_default()).await?;
        if let Err(result)=context.live_checks(&agent,&call).await {
            service.abandon_before_publication(applying.draft.id.clone(),call.id.clone()).await?;
            return Err(result.error.unwrap_or_else(||"Document authority changed".into()));
        }
        service.begin_publication(run.clone(),call.clone(),context.cancel.clone().unwrap_or_default()).await?;
        context.live_checks(&agent,&call).await.map_err(|result|result.error.unwrap_or_else(||"Document publication authority changed".into()))?;
        let root=context.workspace_root.as_deref().ok_or("Document workspace is unavailable")?;
        let publication=(|| {
            let _publication=generated_documents::publication_guard()?;
            let path=&applying.draft.file_path;
            let receipt=context.checkpoint_mutate(&call,path,|preimage| {
                document_edits::service::destination_matches(preimage,&applying.draft.expected_destination)?;
                Ok(applying.bytes.clone())
            })?;
            if receipt.is_none() {
                let workspace=workspace_bytes::WorkspaceBytes::open(root).map_err(|_|"Document workspace is unavailable")?;
                let preimage=document_edits::service::workspace_snapshot(root,path)?;
                document_edits::service::destination_matches(preimage.as_deref(),&applying.draft.expected_destination)?;
                let mut authority=None;
                let authorize=|| {
                    #[cfg(test)]
                    if let Some(hook)=context.checkpoint_validation_hook.as_ref().and_then(|slot|slot.lock().unwrap().take()) {hook();}
                    authority=Some(context.action_journal.as_ref().ok_or_else(||std::io::Error::other("Document journal is unavailable"))?.document_revalidate_now(root,&call,context.action_approved).map_err(std::io::Error::other)?);
                    Ok(())
                };
                match preimage { Some(preimage)=>workspace.compare_replace_instruction_file(path,&preimage,&applying.bytes,authorize),None=>workspace.create_new_bytes_atomic(path,&applying.bytes,authorize) }.map_err(|_|"Document destination changed or publication was refused")?;
                let observed=document_edits::service::workspace_snapshot(root,path)?.ok_or(document_edits::service::UNCERTAIN)?;
                if observed!=applying.bytes { return Err(document_edits::service::UNCERTAIN.into()); }
                drop(authority);
                return Ok((document_edits::service::digest(&observed),receipt));
            }
            let observed=receipt.as_ref().ok_or(document_edits::service::UNCERTAIN)?.observed_sha256.clone();
            if observed!=document_edits::service::digest(&applying.bytes) { return Err(document_edits::service::UNCERTAIN.into()); }
            Ok::<_,String>((observed,receipt))
        })();
        let (observed,checkpoint)=match publication {Ok(value)=>value,Err(error)=>{if let Some(journal)=&context.action_journal {journal.stop_for_unproven_effects(document_edits::service::UNCERTAIN);}return Err(error)}};
        service.record_readback(applying.draft.id.clone(),call.id.clone(),observed.clone(),applying.bytes.len() as u64,checkpoint.as_ref().map(|receipt|receipt.checkpoint_id.clone())).await.map_err(|_|{if let Some(journal)=&context.action_journal {journal.stop_for_unproven_effects(document_edits::service::UNCERTAIN);}document_edits::service::UNCERTAIN.to_string()})?;
        let receipt=service.confirm(applying.draft.id,call.id.clone(),observed,checkpoint.as_ref().map(|receipt|receipt.checkpoint_id.clone())).await.map_err(|error|{if let Some(journal)=&context.action_journal {journal.stop_for_unproven_effects(document_edits::service::UNCERTAIN);}error})?;
        let output=serde_json::json!({"version":1,"path":receipt.file_path,"sha256":receipt.sha256,"size_bytes":receipt.size_bytes,"download_url":format!("/api/workspace/file/download?path={}",document_edits::service::encode(&receipt.file_path)),"provenance":"imported_edit","preview_quality":"structural_content","checkpoint_id":receipt.checkpoint_id});
        Ok::<_,String>(checkpoints::mutation_success(output.to_string(),checkpoint))
    }.await;
        outcome.unwrap_or_else(|error| TaskResult::error(error, 0))
    })
}

#[cfg(test)]
mod argument_tests {
    use super::*;
    use anima_document_content::{OfficeCellValue, OfficeEditOperation};

    fn proposal(operation: serde_json::Value) -> ToolCall {
        let DataValue::Object(args) = generated_documents::schema_data(serde_json::json!({
            "source": {"kind":"attachment", "attachment_id":"selected"},
            "expected_sha256": "a".repeat(64),
            "file_path": "edited.docx",
            "expected_destination": {"state":"absent"},
            "operations": [operation]
        })) else {
            unreachable!();
        };
        ToolCall {
            id: "test".into(),
            name: "propose_document_edit".into(),
            args,
        }
    }

    fn text_operation(
        kind: &str,
        start: serde_json::Value,
        end: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({"type":kind,"target_id":"target","start":start,"end":end,"expected_text":"old","new_text":"new"})
    }

    #[test]
    fn model_number_offsets_decode_for_both_text_operations() {
        for kind in ["word_replace_text", "slide_replace_text"] {
            let call = proposal(text_operation(
                kind,
                serde_json::json!(0),
                serde_json::json!(8),
            ));
            let before = crate::routes::data_value_to_json(&DataValue::Object(call.args.clone()));
            let offset = &before["operations"][0]["end"];
            assert_eq!(offset.as_f64(), Some(8.0));
            assert_eq!(offset.as_u64(), None);
            let parsed = proposal_arguments(&call).unwrap();
            match &parsed.operations[0] {
                OfficeEditOperation::WordReplaceText { start, end, .. }
                | OfficeEditOperation::SlideReplaceText { start, end, .. } => {
                    assert_eq!((*start, *end), (0, 8))
                }
                other => panic!("unexpected operation: {other:?}"),
            }
            let max = proposal(text_operation(
                kind,
                serde_json::json!(u32::MAX),
                serde_json::json!(u32::MAX),
            ));
            assert!(proposal_arguments(&max).is_ok());
        }
    }

    #[test]
    fn invalid_text_offsets_and_unknown_fields_are_rejected() {
        for kind in ["word_replace_text", "slide_replace_text"] {
            for invalid in [
                serde_json::json!(-1),
                serde_json::json!(1.5),
                serde_json::json!(4294967296_u64),
                serde_json::json!("1"),
                serde_json::json!(true),
            ] {
                for key in ["start", "end"] {
                    let mut op = text_operation(kind, serde_json::json!(0), serde_json::json!(8));
                    op[key] = invalid.clone();
                    assert!(
                        proposal_arguments(&proposal(op)).is_err(),
                        "{kind} {key} {invalid}"
                    );
                }
            }
            for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut call = proposal(text_operation(
                    kind,
                    serde_json::json!(0),
                    serde_json::json!(8),
                ));
                let Some(DataValue::Array(operations)) = call.args.get_mut("operations") else {
                    unreachable!()
                };
                let DataValue::Object(fields) = &mut operations[0] else {
                    unreachable!()
                };
                fields.insert("start".into(), DataValue::Number(number));
                assert!(proposal_arguments(&call).is_err());
            }
            let mut unknown = text_operation(kind, serde_json::json!(0), serde_json::json!(8));
            unknown["unexpected"] = serde_json::json!(true);
            assert!(proposal_arguments(&proposal(unknown)).is_err());
            let mut missing = text_operation(kind, serde_json::json!(0), serde_json::json!(8));
            missing.as_object_mut().unwrap().remove("end");
            assert!(proposal_arguments(&proposal(missing)).is_err());
        }
    }

    #[test]
    fn spreadsheet_numbers_and_range_coordinates_are_unchanged() {
        for number in [7.25, -2.5, 8.0] {
            let call = proposal(
                serde_json::json!({"type":"set_cell_value","sheet_id":"sheet","cell":"A1","value":{"type":"number","value":number}}),
            );
            let parsed = proposal_arguments(&call).unwrap();
            assert!(
                matches!(&parsed.operations[0], OfficeEditOperation::SetCellValue { value: OfficeCellValue::Number(actual), .. } if *actual == number)
            );
        }
        let call = proposal(
            serde_json::json!({"type":"set_chart_series_ranges","series_id":"series","categories":{"sheet_id":"sheet","start":"A1","end":"A2"},"values":{"sheet_id":"sheet","start":"B1","end":"B2"}}),
        );
        let parsed = proposal_arguments(&call).unwrap();
        assert!(
            matches!(&parsed.operations[0], OfficeEditOperation::SetChartSeriesRanges { categories, values, .. } if categories.start == "A1" && categories.end == "A2" && values.start == "B1" && values.end == "B2")
        );
    }

    #[test]
    fn html_model_offsets_decode_as_utf8_byte_offsets_and_reject_unknown_markup_fields() {
        let operation = serde_json::json!({"type":"html_replace_text","start_utf8":3,"end_utf8":6,"expected_text":"old","new_text":"<literal>"});
        let parsed = proposal_arguments(&proposal(operation.clone())).unwrap();
        assert!(matches!(
            &parsed.operations[0],
            OfficeEditOperation::HtmlReplaceText {
                start_utf8: 3,
                end_utf8: 6,
                ..
            }
        ));
        for invalid in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("3"),
        ] {
            let mut operation = operation.clone();
            operation["start_utf8"] = invalid;
            assert!(proposal_arguments(&proposal(operation)).is_err());
        }
        let mut unknown = operation;
        unknown["new_html"] = serde_json::json!("<script>injected</script>");
        assert!(proposal_arguments(&proposal(unknown)).is_err());
    }
}

#[cfg(test)]
impl ToolExecutionContext {
    pub(crate) fn with_document_publication_hook(
        mut self,
        hook: impl FnOnce() + Send + 'static,
    ) -> Self {
        self.checkpoint_validation_hook =
            Some(Arc::new(std::sync::Mutex::new(Some(Box::new(hook)))));
        self
    }
}
