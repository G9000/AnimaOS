//! Portable validation for reviewed external-tool candidates. No transport or authority.
use std::collections::HashSet;
use std::fmt;

use jsonschema::{Draft, Validator};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::DataValue;

pub const MAX_EXTERNAL_BYTES: usize = 16 * 1024;
const MAX_WIRE_BYTES: usize = 256 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 1024;
const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalToolDescriptor {
    pub remote_name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub descriptor_digest: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalToolResult {
    pub text_blocks: Vec<String>,
    pub structured_data: Option<DataValue>,
    pub is_error: bool,
    pub omitted_blocks: u32,
    pub shortened: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalToolError {
    DescriptorBounds,
    SchemaRejected,
    ArgumentBounds,
    ArgumentSchema,
    OutputSchema,
    ResultBounds,
    UnsupportedResult,
}
impl fmt::Display for ExternalToolError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::DescriptorBounds => "External tool descriptor exceeds its bounds",
            Self::SchemaRejected => "External tool schema is unsupported or invalid",
            Self::ArgumentBounds => "External tool arguments exceed their bounds",
            Self::ArgumentSchema => "External tool arguments do not match the reviewed schema",
            Self::OutputSchema => "External tool result does not match the reviewed schema",
            Self::ResultBounds => "External tool result exceeds its bounds",
            Self::UnsupportedResult => "External tool result is unsupported",
        })
    }
}
impl std::error::Error for ExternalToolError {}

fn bounded(value: &Value, max_bytes: usize) -> bool {
    fn visit(v: &Value, depth: usize, nodes: &mut usize) -> bool {
        *nodes += 1;
        if depth > MAX_DEPTH || *nodes > MAX_NODES {
            return false;
        }
        match v {
            Value::Number(n) => n.as_f64().is_some_and(|n| {
                n.is_finite() && (n.fract() != 0.0 || n.abs() <= MAX_SAFE_INTEGER)
            }),
            Value::Array(items) => items.iter().all(|v| visit(v, depth + 1, nodes)),
            Value::Object(items) => items.values().all(|v| visit(v, depth + 1, nodes)),
            _ => true,
        }
    }
    visit(value, 1, &mut 0) && serde_json::to_vec(value).is_ok_and(|v| v.len() <= max_bytes)
}

fn safe_schema(schema: &Value) -> Result<(), ExternalToolError> {
    fn walk<'a>(
        v: &'a Value,
        root: &'a Value,
        depth: usize,
        nodes: &mut usize,
        active: &mut HashSet<usize>,
        schema_position: bool,
    ) -> bool {
        *nodes += 1;
        if depth > MAX_DEPTH || *nodes > MAX_NODES {
            return false;
        }
        let address = v as *const Value as usize;
        if !active.insert(address) {
            return false;
        }
        let okay = match v {
            Value::Object(object) if schema_position => {
                let forbidden = [
                    "$id",
                    "$dynamicRef",
                    "$dynamicAnchor",
                    "$recursiveRef",
                    "$recursiveAnchor",
                ];
                let vocabulary_ok = object.get("$vocabulary").is_none_or(|v| match v {
                    Value::Object(v) => v.iter().all(|(name, value)| {
                        value.is_boolean()
                            && matches!(
                                name.as_str(),
                                "https://json-schema.org/draft/2020-12/vocab/core"
                                    | "https://json-schema.org/draft/2020-12/vocab/applicator"
                                    | "https://json-schema.org/draft/2020-12/vocab/validation"
                                    | "https://json-schema.org/draft/2020-12/vocab/meta-data"
                            )
                    }),
                    _ => false,
                });
                let reference_ok = object.get("$ref").is_none_or(|reference| {
                    reference
                        .as_str()
                        .filter(|reference| reference.starts_with("#/$defs/"))
                        .and_then(|reference| root.pointer(&reference[1..]))
                        .is_some_and(|target| walk(target, root, depth + 1, nodes, active, true))
                });
                !forbidden.iter().any(|key| object.contains_key(*key))
                    && vocabulary_ok
                    && object
                        .get("$schema")
                        .is_none_or(|v| v.as_str() == Some(DIALECT))
                    && reference_ok
                    && object.iter().all(|(key, value)| {
                        // Names in property/definition maps and JSON annotation or
                        // enum values are data, even when named "$id" or "$ref".
                        let single = matches!(
                            key.as_str(),
                            "additionalProperties"
                                | "unevaluatedProperties"
                                | "propertyNames"
                                | "items"
                                | "contains"
                                | "unevaluatedItems"
                                | "additionalItems"
                                | "not"
                                | "if"
                                | "then"
                                | "else"
                                | "contentSchema"
                        );
                        let schemas =
                            matches!(key.as_str(), "allOf" | "anyOf" | "oneOf" | "prefixItems");
                        let map = matches!(
                            key.as_str(),
                            "properties"
                                | "patternProperties"
                                | "$defs"
                                | "definitions"
                                | "dependentSchemas"
                                | "dependencies"
                        );
                        if schemas && value.is_array() {
                            *nodes += 1;
                            depth < MAX_DEPTH
                                && *nodes <= MAX_NODES
                                && value
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .all(|v| walk(v, root, depth + 2, nodes, active, true))
                        } else if map && value.is_object() {
                            *nodes += 1;
                            depth < MAX_DEPTH
                                && *nodes <= MAX_NODES
                                && value.as_object().unwrap().values().all(|v| {
                                    walk(
                                        v,
                                        root,
                                        depth + 2,
                                        nodes,
                                        active,
                                        v.is_object() || v.is_boolean(),
                                    )
                                })
                        } else {
                            walk(value, root, depth + 1, nodes, active, single)
                        }
                    })
            }
            Value::Object(object) => object
                .values()
                .all(|v| walk(v, root, depth + 1, nodes, active, false)),
            Value::Array(items) => items
                .iter()
                .all(|v| walk(v, root, depth + 1, nodes, active, false)),
            _ => true,
        };
        active.remove(&address);
        okay
    }
    if !bounded(schema, MAX_EXTERNAL_BYTES)
        || !walk(schema, schema, 1, &mut 0, &mut HashSet::new(), true)
    {
        return Err(ExternalToolError::SchemaRejected);
    }
    Ok(())
}
fn compile(schema: &Value) -> Result<Validator, ExternalToolError> {
    safe_schema(schema)?;
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(schema)
        .map_err(|_| ExternalToolError::SchemaRejected)
}
/// Compile a bounded JSON output contract without remote references or retrieval.
pub fn validate_output_contract(schema: &Value) -> Result<(), ExternalToolError> {
    compile(schema).map(|_| ())
}

/// Validate one result against the same safe contract used for external tools.
pub fn matches_output_contract(schema: &Value, value: &Value) -> Result<bool, ExternalToolError> {
    Ok(compile(schema)?.is_valid(value))
}

fn normalize_schema(mut schema: Value) -> Result<Value, ExternalToolError> {
    let object = schema
        .as_object_mut()
        .ok_or(ExternalToolError::SchemaRejected)?;
    if object.get("type").and_then(Value::as_str) != Some("object") {
        return Err(ExternalToolError::SchemaRejected);
    }
    if object
        .get("$schema")
        .is_some_and(|v| v.as_str() != Some(DIALECT))
    {
        return Err(ExternalToolError::SchemaRejected);
    }
    object.insert("$schema".into(), json!(DIALECT));
    compile(&schema)?;
    Ok(schema)
}
pub fn prepare_external_tool(
    remote_name: &str,
    description: &str,
    input: Value,
    output: Option<Value>,
) -> Result<ExternalToolDescriptor, ExternalToolError> {
    if remote_name.trim().is_empty()
        || remote_name.len() > 128
        || remote_name.chars().any(char::is_control)
        || description.len() > 2048
    {
        return Err(ExternalToolError::DescriptorBounds);
    }
    let input_schema = normalize_schema(input)?;
    let output_schema = output.map(normalize_schema).transpose()?;
    if serde_json::to_vec(&json!([input_schema, output_schema]))
        .map_err(|_| ExternalToolError::SchemaRejected)?
        .len()
        > MAX_EXTERNAL_BYTES
    {
        return Err(ExternalToolError::DescriptorBounds);
    }
    let descriptor_digest = external_descriptor_digest(
        remote_name,
        description,
        &input_schema,
        output_schema.as_ref(),
    )?;
    let descriptor = ExternalToolDescriptor {
        remote_name: remote_name.into(),
        description: description.into(),
        input_schema,
        output_schema,
        descriptor_digest,
    };
    if serde_json::to_vec(&descriptor)
        .map_err(|_| ExternalToolError::DescriptorBounds)?
        .len()
        > MAX_EXTERNAL_BYTES
    {
        return Err(ExternalToolError::DescriptorBounds);
    }
    Ok(descriptor)
}
pub fn external_descriptor_digest(
    name: &str,
    description: &str,
    input: &Value,
    output: Option<&Value>,
) -> Result<String, ExternalToolError> {
    safe_schema(input)?;
    if let Some(output) = output {
        safe_schema(output)?;
    }
    let bytes = serde_jcs::to_vec(&json!({"remoteName":name,"description":description,
        "inputSchema":input,"outputSchema":output,"classification":"exec"}))
    .map_err(|_| ExternalToolError::SchemaRejected)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}
pub fn validate_external_arguments(
    tool: &ExternalToolDescriptor,
    args: &Value,
) -> Result<(), ExternalToolError> {
    if !args.is_object() || !bounded(args, MAX_EXTERNAL_BYTES) {
        return Err(ExternalToolError::ArgumentBounds);
    }
    if !compile(&tool.input_schema)?.is_valid(args) {
        return Err(ExternalToolError::ArgumentSchema);
    }
    Ok(())
}
pub fn validate_external_output(
    tool: &ExternalToolDescriptor,
    output: &Value,
) -> Result<(), ExternalToolError> {
    if !bounded(output, MAX_WIRE_BYTES) {
        return Err(ExternalToolError::ResultBounds);
    }
    if let Some(schema) = &tool.output_schema {
        if !compile(schema)?.is_valid(output) {
            return Err(ExternalToolError::OutputSchema);
        }
    }
    Ok(())
}
fn clip(text: &str, bytes: usize) -> String {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].into()
}
pub fn bound_external_result(
    mut result: ExternalToolResult,
    reserved_envelope_bytes: usize,
) -> Result<ExternalToolResult, ExternalToolError> {
    fn valid_data(v: &DataValue, depth: usize, nodes: &mut usize) -> bool {
        *nodes += 1;
        if depth > MAX_DEPTH || *nodes > MAX_NODES {
            return false;
        }
        match v {
            DataValue::Number(v) => {
                v.is_finite() && (v.fract() != 0.0 || v.abs() <= MAX_SAFE_INTEGER)
            }
            DataValue::Array(v) => v.iter().all(|v| valid_data(v, depth + 1, nodes)),
            DataValue::Object(v) => v.values().all(|v| valid_data(v, depth + 1, nodes)),
            _ => true,
        }
    }
    if result
        .structured_data
        .as_ref()
        .is_some_and(|v| !valid_data(v, 1, &mut 0))
    {
        return Err(ExternalToolError::ResultBounds);
    }
    let budget = MAX_EXTERNAL_BYTES
        .checked_sub(reserved_envelope_bytes)
        .ok_or(ExternalToolError::ResultBounds)?;
    if result.text_blocks.len() > 16 {
        result.omitted_blocks = result.omitted_blocks.saturating_add(
            (result.text_blocks.len() - 16)
                .try_into()
                .unwrap_or(u32::MAX),
        );
        result.text_blocks.truncate(16);
        result.shortened = true;
    }
    loop {
        let bytes = serde_json::to_vec(&result).map_err(|_| ExternalToolError::ResultBounds)?;
        if bytes.len() <= budget {
            return Ok(result);
        }
        result.shortened = true;
        if let Some(text) = result
            .text_blocks
            .iter_mut()
            .rev()
            .find(|text| !text.is_empty())
        {
            *text = clip(text, text.len().saturating_sub(256));
        } else if result.structured_data.take().is_some() {
            // Omit the whole already-validated object, never manufacture a
            // partial object that would appear to satisfy its output schema.
            result.omitted_blocks = result.omitted_blocks.saturating_add(1);
        } else {
            return Err(ExternalToolError::ResultBounds);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor() -> ExternalToolDescriptor {
        prepare_external_tool("echo","Synthetic echo",json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}),Some(json!({"type":"object","properties":{"ok":{"const":true}},"required":["ok"]}))).unwrap()
    }
    #[test]
    fn reviewed_digest_dialect_and_actual_schema_constraints() {
        let tool = descriptor();
        assert_eq!(tool.input_schema["$schema"], DIALECT);
        validate_external_arguments(&tool, &json!({"value":"safe"})).unwrap();
        assert_eq!(
            validate_external_arguments(&tool, &json!({"value":3})),
            Err(ExternalToolError::ArgumentSchema)
        );
        assert_eq!(
            validate_external_output(&tool, &json!({"ok":false})),
            Err(ExternalToolError::OutputSchema)
        );
        let reordered: Value = serde_json::from_str(&tool.input_schema.to_string()).unwrap();
        assert_eq!(
            external_descriptor_digest(
                &tool.remote_name,
                &tool.description,
                &reordered,
                tool.output_schema.as_ref()
            )
            .unwrap(),
            tool.descriptor_digest
        );
        assert_ne!(
            prepare_external_tool(
                "echo",
                "Changed description",
                tool.input_schema.clone(),
                tool.output_schema.clone()
            )
            .unwrap()
            .descriptor_digest,
            tool.descriptor_digest
        );
        let mut forged = tool.clone();
        forged.input_schema = json!({"type":"object","$ref":"file:///private"});
        assert_eq!(
            validate_external_arguments(&forged, &json!({})),
            Err(ExternalToolError::SchemaRejected)
        );
        let mut forged = tool;
        forged.output_schema =
            Some(json!({"type":"object","$ref":"https://invalid.example/schema"}));
        assert_eq!(
            validate_external_output(&forged, &json!({})),
            Err(ExternalToolError::SchemaRejected)
        );
    }
    #[test]
    fn rejects_network_file_cycles_wrong_dialect_and_unsafe_numbers_before_compile() {
        for schema in [
            json!({"type":"object","$ref":"https://invalid.example/schema"}),
            json!({"type":"object","$ref":"file:///private"}),
            json!({"type":"object","$defs":{"loop":{"$ref":"#/$defs/loop"}},"$ref":"#/$defs/loop"}),
            json!({"type":"object","$schema":"http://json-schema.org/draft-07/schema#"}),
            json!({"type":"object","properties":{"n":{"minimum":9007199254740992u64}}}),
            json!({"type":"object","$ref":"#/$defs/missing"}),
        ] {
            assert_eq!(
                prepare_external_tool("echo", "", schema, None).unwrap_err(),
                ExternalToolError::SchemaRejected
            );
        }
        prepare_external_tool("echo","",json!({"type":"object","$defs":{"value":{"type":"string"}},"properties":{"x":{"$ref":"#/$defs/value"}}}),None).unwrap();
        let mut deep = json!(null);
        for _ in 0..16 {
            deep = json!([deep]);
        }
        assert!(!bounded(&deep, MAX_EXTERNAL_BYTES));
        assert!(!bounded(&json!(vec![0; 1024]), MAX_EXTERNAL_BYTES));
        assert!(!bounded(
            &json!({"x":"x".repeat(MAX_EXTERNAL_BYTES)}),
            MAX_EXTERNAL_BYTES
        ));
    }
    #[test]
    fn schema_positions_allow_literal_keyword_names_without_allowing_external_refs() {
        let tool=prepare_external_tool("metadata","",json!({"type":"object","properties":{
            "$id":{"type":"string"},
            "$ref":{"enum":[{"$ref":"https://literal.invalid/value","$schema":"literal","$id":"literal"}]},
            "record":{"$ref":"#/$defs/$id"},
            "annotation":{"type":"object","default":{"$ref":"file:///literal","$id":"literal"}}
        },"$defs":{"$id":{"type":"object","properties":{"$schema":{"const":"literal"}},"required":["$schema"]}},
        "required":["$id","$ref","record"]}),None).unwrap();
        validate_external_arguments(&tool,&json!({"$id":"field","$ref":{"$ref":"https://literal.invalid/value","$schema":"literal","$id":"literal"},
            "record":{"$schema":"literal"}})).unwrap();
        assert_eq!(
            validate_external_arguments(
                &tool,
                &json!({"$id":"field","$ref":{"$ref":"different"},"record":{"$schema":"literal"}})
            ),
            Err(ExternalToolError::ArgumentSchema)
        );
        for key in [
            "properties",
            "patternProperties",
            "$defs",
            "dependentSchemas",
        ] {
            let mut schema = json!({"type":"object"});
            schema[key] = json!({"safe":{"$ref":"https://invalid.example/schema"}});
            assert_eq!(
                prepare_external_tool("metadata", "", schema, None).unwrap_err(),
                ExternalToolError::SchemaRejected
            );
        }
        assert_eq!(prepare_external_tool("metadata","",json!({"type":"object","properties":{"literal":{"const":{"n":9007199254740992u64}}}}),None)
            .unwrap_err(),ExternalToolError::SchemaRejected);
    }
    #[test]
    fn final_serialized_unicode_result_counts_envelope_and_whole_structured_omission() {
        let result = ExternalToolResult {
            text_blocks: vec!["🫖\"\0".repeat(1000); 17],
            structured_data: Some(DataValue::String("x".repeat(18000))),
            is_error: true,
            omitted_blocks: 2,
            shortened: false,
        };
        let bounded = bound_external_result(result, 1024).unwrap();
        assert!(serde_json::to_vec(&bounded).unwrap().len() + 1024 <= MAX_EXTERNAL_BYTES);
        assert_eq!(bounded.text_blocks.len(), 16);
        assert!(bounded.shortened);
        assert!(bounded.is_error);
        assert!(bounded.structured_data.is_none());
        assert_eq!(bounded.omitted_blocks, 4);
        assert_eq!(
            bound_external_result(bounded, MAX_EXTERNAL_BYTES).unwrap_err(),
            ExternalToolError::ResultBounds
        );
    }
}
