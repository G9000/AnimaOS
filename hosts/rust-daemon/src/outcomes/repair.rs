use anima_core::{DataValue, ToolCall};

pub(super) fn check_call(paths: &[String], call: &ToolCall) -> Result<(), String> {
    if !matches!(
        call.name.as_str(),
        "read_file" | "read_document_source" | "write_file" | "edit_file" | "multi_edit"
    ) {
        return Err("Outcome correction permits only reads and repairs of its declared files; this action needs owner review".into());
    }
    let Some(DataValue::String(path)) = call.args.get("file_path") else {
        return Err("Outcome correction requires an exact declared file path".into());
    };
    let path = path.replace('\\', "/");
    if !paths.iter().any(|allowed| allowed == &path) {
        return Err("Outcome correction cannot access an unrelated file".into());
    }
    Ok(())
}
