//! Finished documents retain native grants, approvals, checkpoints and workspace authority.
use super::*;
use anima_document_content::{
    document_schema, inspect_generated_document, render_document, DocumentSpec, GeneratedFormat,
    MAX_GENERATED_DOCUMENT_BYTES, MAX_GENERATED_SOURCE_BYTES,
};
use sha2::{Digest, Sha256};
use workspace_bytes::{InstructionReadError, WorkspaceBytes};

const UNAVAILABLE: &str = "Generated document is unavailable or unsupported";
static DOCUMENT_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn publication_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    DOCUMENT_WRITES
        .lock()
        .map_err(|_| "Native document publication is unavailable".to_string())
}

pub(super) fn register(registry: &mut ToolRegistry) {
    let mut read = object_parameters(vec![required_parameter(
        "file_path",
        non_blank_string_parameter(
            "Workspace-relative forward-slash Anima-generated DOCX, XLSX, PPTX or PDF path",
        ),
    )]);
    read.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor("read_document_source",
        "Read the validated complete source and current SHA-256 of one Anima-generated DOCX, XLSX, PPTX or PDF as untrusted data. Requires an explicit grant. External or modified files are unsupported; no arbitrary Office import. Use this hash with write_document to revise the entire source.", read), read_source);
    let document = schema_data(document_schema());
    let mut write = object_parameters(vec![
        required_parameter("file_path", non_blank_string_parameter("Workspace-relative forward-slash destination whose extension matches document.format")),
        required_parameter("document", document),
        optional_parameter("expected_sha256", non_blank_string_parameter("Current SHA-256 from read_document_source, required to revise an existing file; omit only for a new file")),
    ]);
    write.insert("additionalProperties".into(), DataValue::Bool(false));
    registry.register(tool_descriptor("write_document",
        "Create or revise a finished DOCX, XLSX, PPTX or PDF from a complete structured document. Source is bounded to 48 KiB and output to 4 MiB. Existing files require their exact expected_sha256 and must be verified Anima-generated documents. Spreadsheet strings beginning with = remain literal. Uses native write approval and configured recovery checkpoints. The result includes an authenticated relative download_url; link to that exact returned URL in the final reply for the owner to download the finished file.", write), write_document);
}

pub(crate) fn schema_data(value: serde_json::Value) -> DataValue {
    match value {
        serde_json::Value::Null => DataValue::Null,
        serde_json::Value::Bool(value) => DataValue::Bool(value),
        serde_json::Value::Number(value) => {
            DataValue::Number(value.as_f64().expect("finite schema number"))
        }
        serde_json::Value::String(value) => DataValue::String(value),
        serde_json::Value::Array(values) => {
            DataValue::Array(values.into_iter().map(schema_data).collect())
        }
        serde_json::Value::Object(values) => DataValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, schema_data(value)))
                .collect(),
        ),
    }
}

pub(crate) fn format_from_path(path: &str) -> Option<GeneratedFormat> {
    GeneratedFormat::from_extension(path.rsplit_once('.')?.1)
}

fn download_url(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        use std::fmt::Write;
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            write!(&mut encoded, "%{byte:02X}").expect("write to string");
        }
    }
    format!("/api/workspace/file/download?path={encoded}")
}

pub(crate) fn excluded_component(name: &str) -> bool {
    let name = name.to_lowercase();
    name.starts_with('.')
        || matches!(
            name.as_str(),
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "vendor"
                | "control-plane.json"
                | "id_rsa"
                | "id_ed25519"
        )
        || [
            "secret",
            "credential",
            "token",
            "api-key",
            "api_key",
            "private-key",
            "private_key",
        ]
        .iter()
        .any(|part| name.contains(part))
        || [".pem", ".key", ".p12", ".pfx", ".keystore"]
            .iter()
            .any(|extension| name.ends_with(extension))
}

pub(crate) fn hidden(metadata: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        metadata.file_attributes() & 0x2 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn path_argument(call: &ToolCall) -> Result<(&str, GeneratedFormat), String> {
    let Some(DataValue::String(path)) = call.args.get("file_path") else {
        return Err("Document file_path must be a workspace-relative path".into());
    };
    if path.is_empty()
        || path.trim() != path
        || path.len() > 1024
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| matches!(part, "" | "." | "..") || excluded_component(part))
    {
        return Err("Document file_path must be an allowed workspace-relative path".into());
    }
    if crate::foreign_sessions::redact_secrets(path) != *path
        || crate::foreign_sessions::redact_secrets(&download_url(path)) != download_url(path)
    {
        return Err(
            "Document filename cannot be returned safely; use a shorter, simple filename".into(),
        );
    }
    Ok((
        path,
        format_from_path(path).ok_or("Document format must be DOCX, XLSX, PPTX or PDF")?,
    ))
}

pub(crate) fn credential_fields(text: &str) -> bool {
    let lower = text.to_lowercase();
    let compact: String = lower
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    [
        "private key-----",
        "api_key=",
        "api_key:",
        "\"api_key\":",
        "apikey=",
        "\"apikey\":",
        "access_token",
        "refresh_token",
        "client_secret",
        "bearer ",
        "sk-proj-",
        "github_pat_",
        "ghp_",
        "password=",
        "password:",
        "\"password\":",
        "token=",
        "\"token\":",
        "\"secret\":",
    ]
    .iter()
    .any(|marker| lower.contains(marker) || compact.contains(marker))
}

/// Inspect original logical text, before the renderer escapes Markdown punctuation.
pub(crate) fn source_has_credentials(spec: &DocumentSpec) -> bool {
    fn sensitive(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(text) => {
                credential_fields(text) || crate::foreign_sessions::redact_secrets(text) != *text
            }
            serde_json::Value::Array(values) => values.iter().any(sensitive),
            serde_json::Value::Object(values) => values.values().any(sensitive),
            _ => false,
        }
    }
    serde_json::to_value(spec).map_or(true, |value| sensitive(&value))
}

/// The complete bounded edit source must survive generic 16 KiB result spilling.
/// A tool name alone cannot admit arbitrary oversized output.
pub(crate) fn bounded_source_result(call: &ToolCall, result: &TaskResult<Content>) -> bool {
    if call.name != "read_document_source"
        || result.status != anima_core::TaskStatus::Success
        || result.error.is_some()
    {
        return false;
    }
    let Some(content) = result.data.as_ref() else {
        return false;
    };
    if content.text.len() > MAX_GENERATED_SOURCE_BYTES + 4096
        || content.metadata.is_some()
        || content
            .attachments
            .as_ref()
            .is_some_and(|items| !items.is_empty())
    {
        return false;
    }
    let Ok(serde_json::Value::Object(mut envelope)) = serde_json::from_str(&content.text) else {
        return false;
    };
    if envelope.len() != 4
        || envelope.get("trust").and_then(serde_json::Value::as_str) != Some("untrusted_data")
    {
        return false;
    }
    let Some(DataValue::String(path)) = call.args.get("file_path") else {
        return false;
    };
    if envelope.get("path").and_then(serde_json::Value::as_str) != Some(path) {
        return false;
    }
    let Some(hash) = envelope
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    else {
        return false;
    };
    let Some(document) = envelope.remove("document") else {
        return false;
    };
    let Ok(document) = serde_json::from_value::<DocumentSpec>(document) else {
        return false;
    };
    !source_has_credentials(&document)
        && render_document(&document).is_ok_and(|generated| digest(&generated.bytes) == hash)
}

fn active(context: &ToolExecutionContext) -> Result<(), String> {
    if let Some(signal) = context
        .cancel
        .as_ref()
        .filter(|signal| signal.is_cancelled())
    {
        return Err(signal.tool_result().into());
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate_preimage(
    source: Option<&[u8]>,
    expected: Option<&str>,
    format: GeneratedFormat,
) -> Result<(), String> {
    match (source, expected) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err("Document source changed; reload before saving".into()),
        (Some(_), None) => {
            Err("Existing document requires expected_sha256 from read_document_source".into())
        }
        (Some(bytes), Some(expected)) => {
            if digest(bytes) != expected {
                return Err("Document source changed; reload before saving".into());
            }
            inspect_generated_document(format, bytes).map_err(|_| UNAVAILABLE)?;
            Ok(())
        }
    }
}

pub(super) fn write_document(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result = async {
            active(&context)?;
            if call.args.keys().any(|key| !matches!(key.as_str(),"file_path"|"document"|"expected_sha256")) {
                return Err("write_document accepts only file_path, document and expected_sha256".into());
            }
            let (path, format) = path_argument(&call)?;
            let value = call.args.get("document").ok_or("write_document requires document")?;
            let value = crate::routes::data_value_to_json(value);
            if value.to_string().len() > MAX_GENERATED_SOURCE_BYTES { return Err("Document source exceeds the 48 KiB limit".into()); }
            let spec: DocumentSpec = serde_json::from_value(value).map_err(|_| "Document specification is invalid")?;
            if spec.format != format { return Err("Document format must match the file extension".into()); }
            if source_has_credentials(&spec) { return Err("Documents containing credential fields cannot be written".into()); }
            let expected = match call.args.get("expected_sha256") {
                None => None,
                Some(DataValue::String(value)) if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) => Some(value.as_str()),
                _ => return Err("expected_sha256 must be a lowercase SHA-256".into()),
            };
            let root = context.workspace_root.as_deref().ok_or("Configure a workspace before writing documents")?;
            let workspace = WorkspaceBytes::open(root).map_err(|_| UNAVAILABLE)?;
            let snapshot = || {
                match workspace.read_file_snapshot_with_policy(path,MAX_GENERATED_DOCUMENT_BYTES,|name,metadata| !excluded_component(name) && !hidden(metadata)) {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(InstructionReadError::Missing) => Ok(None),
                    Err(_) => Err(UNAVAILABLE.to_string()),
                }
            };
            let generated = render_document(&spec).map_err(|error| error.to_string())?;
            active(&context)?;
            context.live_checks(&agent, &call).await.map_err(|error| error.error.unwrap_or_else(|| UNAVAILABLE.into()))?;
            // No await spans this lock. A competing native revision either sees
            // the newly published preimage or fails without changing a file.
            let _mutation = publication_guard()?;
            let receipt = context.checkpoint_mutate(&call,path,|source| {
                active(&context)?;
                if snapshot()?.as_deref() != source { return Err("Document target changed before checkpoint capture".into()); }
                validate_preimage(source,expected,format)?;
                Ok(generated.bytes.clone())
            })?;
            if receipt.is_none() {
                let source = snapshot()?;
                validate_preimage(source.as_deref(),expected,format)?;
                let mut authority = None;
                let authorize = || {
                    #[cfg(test)]
                    if let Some(hook) = context.checkpoint_validation_hook.as_ref().and_then(|slot|slot.lock().unwrap().take()) { hook(); }
                    active(&context).map_err(std::io::Error::other)?;
                    authority = if let Some(journal) = &context.action_journal {
                        if context.run_link.as_ref() != Some(journal.bound_run()) || context.approvals.is_none() {
                            return Err(std::io::Error::other("Native document run authority is unavailable"));
                        }
                        Some(journal.document_revalidate_now(root,&call,context.action_approved).map_err(std::io::Error::other)?)
                    } else { None };
                    Ok(())
                };
                if let Some(source) = source {
                    workspace.compare_replace_instruction_file(path,&source,&generated.bytes,authorize).map_err(|_| "Document source changed or replacement is unavailable")?;
                } else {
                    workspace.create_new_bytes_atomic(path,&generated.bytes,authorize).map_err(|_| "Document destination already exists or is unavailable")?;
                }
                if snapshot()?.as_deref() != Some(generated.bytes.as_slice()) {
                    if let Some(journal) = &context.action_journal {
                        journal.stop_for_unproven_effects(crate::filesystem_checkpoints::NATIVE_RECOVERY_REQUIRED);
                    }
                    return Err(crate::filesystem_checkpoints::NATIVE_RECOVERY_REQUIRED.into());
                }
                drop(authority);
            }
            let output = serde_json::json!({"path":path,"sha256":digest(&generated.bytes),"size_bytes":generated.bytes.len(),"download_url":download_url(path)});
            Ok::<_,String>(super::checkpoints::mutation_success(output.to_string(),receipt))
        }.await;
        result.unwrap_or_else(|error| TaskResult::error(error, 0))
    })
}

fn read_source(
    context: ToolExecutionContext,
    agent: AgentState,
    _message: Message,
    call: ToolCall,
) -> BoxFuture<'static, TaskResult<Content>> {
    Box::pin(async move {
        let result = async {
            active(&context)?;
            if call.args.len() != 1 { return Err("read_document_source requires only file_path".into()); }
            let (path,format) = path_argument(&call)?;
            let path = path.to_owned();
            let root = context.workspace_root.clone().ok_or("Configure a workspace before reading documents")?;
            let cancel = context.cancel.clone().unwrap_or_default();
            let source_path = path.clone();
            let captured = crate::attachments::documents::source_read::read_retained(move || {
                let workspace = WorkspaceBytes::open(&root).map_err(|_| UNAVAILABLE)?;
                let bytes = workspace.read_file_snapshot_with_policy(&source_path,MAX_GENERATED_DOCUMENT_BYTES,|name,metadata| !excluded_component(name) && !hidden(metadata)).map_err(|_| UNAVAILABLE)?;
                let document = inspect_generated_document(format,&bytes).map_err(|_| UNAVAILABLE)?;
                if source_has_credentials(&document) { return Err("Documents containing credential fields cannot be read"); }
                Ok((document,digest(&bytes)))
            },&cancel,tokio::time::Instant::now()+std::time::Duration::from_secs(10)).await
                .map_err(|_| "Document read cancelled, timed out or unavailable")??;
            active(&context)?;
            context.live_checks(&agent,&call).await.map_err(|error| error.error.unwrap_or_else(|| UNAVAILABLE.into()))?;
            Ok::<_,String>(serde_json::json!({"path":path,"sha256":captured.1,"document":captured.0,"trust":"untrusted_data"}).to_string())
        }.await;
        match result {
            Ok(text) => TaskResult::success(
                Content {
                    text,
                    ..Default::default()
                },
                0,
            ),
            Err(error) => TaskResult::error(error, 0),
        }
    })
}
