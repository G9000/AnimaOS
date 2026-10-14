//! Host-neutral declarative community bundle manifests.
//! Bundles describe content and requested permissions; they confer no authority.
use std::collections::HashSet;

use serde::{
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize, Serialize,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const MAX_MANIFEST_BYTES: usize = 128 * 1024;
pub const MAX_COMPONENTS: usize = 4;
pub const MAX_MCP_COMPONENTS: usize = 2;
pub const MAX_SKILL_COMPONENTS: usize = 2;
const MAX_JSON_DEPTH: usize = 16;
const MAX_JSON_NODES: usize = 4096;
const MAX_SKILL_SOURCE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommunityManifest {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub compatibility: ManifestCompatibility,
    pub permissions: ManifestPermissions,
    pub attribution: Vec<ManifestAttribution>,
    pub components: Vec<ManifestComponent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestCompatibility {
    pub host: String,
    pub min_version: String,
    pub max_version_exclusive: String,
    pub required_capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestPermissions {
    pub native_tool_names: Vec<String>,
    pub external_tools: Vec<ExternalToolPermission>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalToolPermission {
    pub component: String,
    pub remote_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestAttribution {
    pub component: String,
    pub source: String,
    pub revision: String,
    pub license: String,
    pub notice: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ManifestComponent {
    #[serde(rename_all = "camelCase")]
    McpHttp {
        key: String,
        label: String,
        endpoint: String,
    },
    #[serde(rename_all = "camelCase")]
    McpStdio {
        key: String,
        label: String,
        program_ref: String,
        args: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Skill { key: String, skill_file: String },
}

/// Actual host facts supplied by its owner service, never trusted manifest input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCompatibility {
    pub host: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

/// Original inline UTF-8 bytes remain independent of host SKILL.md composition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSourceDigest {
    pub component_key: String,
    pub source_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedManifest {
    pub manifest: CommunityManifest,
    pub manifest_digest: String,
    pub source_skill_digests: Vec<SkillSourceDigest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestError {
    MissingFeature,
    InvalidInput,
    LimitsExceeded,
    InvalidManifest,
    UnsupportedSchema,
    IncompatibleRuntime,
}
impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MissingFeature => "Community manifest support is not implemented",
            Self::InvalidInput => "Community manifest input is invalid",
            Self::LimitsExceeded => "Community manifest exceeds its limits",
            Self::InvalidManifest => "Community manifest declaration is invalid",
            Self::UnsupportedSchema => "Community manifest schema is unsupported",
            Self::IncompatibleRuntime => "Community manifest runtime is incompatible",
        })
    }
}
impl std::error::Error for ManifestError {}

pub fn parse_manifest(bytes: &[u8]) -> Result<CommunityManifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::LimitsExceeded);
    }
    std::str::from_utf8(bytes).map_err(|_| ManifestError::InvalidInput)?;
    let mut bounds = ParseBounds {
        nodes: 0,
        error: None,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue {
        bounds: &mut bounds,
        depth: 1,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| bounds.error.unwrap_or(ManifestError::InvalidInput))?;
    deserializer
        .end()
        .map_err(|_| ManifestError::InvalidInput)?;
    if !value.is_object() {
        return Err(ManifestError::InvalidInput);
    }
    serde_json::from_value(value).map_err(|_| ManifestError::InvalidInput)
}

pub fn validate_manifest(
    manifest: &CommunityManifest,
    runtime: &RuntimeCompatibility,
) -> Result<ValidatedManifest, ManifestError> {
    if manifest.schema_version != 1 {
        return Err(ManifestError::UnsupportedSchema);
    }
    identifier(&manifest.id, 64, true)?;
    release(&manifest.version)?;
    text(&manifest.name, 128)?;
    text(&manifest.description, 1024)?;
    let compatibility = &manifest.compatibility;
    identifier(&compatibility.host, 64, false)?;
    let minimum = release(&compatibility.min_version)?;
    let maximum = release(&compatibility.max_version_exclusive)?;
    if maximum <= minimum {
        return Err(ManifestError::InvalidManifest);
    }
    cap(compatibility.required_capabilities.len(), 32)?;
    let mut required = HashSet::new();
    for capability in &compatibility.required_capabilities {
        identifier(capability, 64, false)?;
        if !required.insert(capability) {
            return Err(ManifestError::InvalidManifest);
        }
    }
    if runtime.host != compatibility.host || identifier(&runtime.host, 64, false).is_err() {
        return Err(ManifestError::IncompatibleRuntime);
    }
    let actual_version =
        release(&runtime.version).map_err(|_| ManifestError::IncompatibleRuntime)?;
    if actual_version < minimum || actual_version >= maximum {
        return Err(ManifestError::IncompatibleRuntime);
    }
    let mut actual = HashSet::new();
    for capability in &runtime.capabilities {
        if identifier(capability, 64, false).is_err() || !actual.insert(capability) {
            return Err(ManifestError::IncompatibleRuntime);
        }
    }
    if !required.is_subset(&actual) {
        return Err(ManifestError::IncompatibleRuntime);
    }

    if manifest.components.is_empty() {
        return Err(ManifestError::InvalidManifest);
    }
    cap(manifest.components.len(), MAX_COMPONENTS)?;
    let mut keys = HashSet::new();
    let mut mcp_keys = HashSet::new();
    let mut source_skill_digests = Vec::new();
    for component in &manifest.components {
        let key = match component {
            ManifestComponent::McpHttp {
                key,
                label,
                endpoint,
            } => {
                text(label, 128)?;
                printable_ascii(endpoint, 2048)?;
                mcp_keys.insert(key.as_str());
                key
            }
            ManifestComponent::McpStdio {
                key,
                label,
                program_ref,
                args,
            } => {
                text(label, 128)?;
                identifier(program_ref, 64, false)?;
                cap(args.len(), 32)?;
                let mut total = 0usize;
                for argument in args {
                    cap(argument.len(), 2048)?;
                    total += argument.len();
                    if argument.chars().any(|c| c.is_control() || hidden(c))
                        || selector_run(argument)
                    {
                        return Err(ManifestError::InvalidManifest);
                    }
                }
                cap(total, 16 * 1024)?;
                mcp_keys.insert(key.as_str());
                key
            }
            ManifestComponent::Skill { key, skill_file } => {
                cap(skill_file.len(), MAX_SKILL_SOURCE_BYTES)?;
                if skill_file.trim().is_empty() || skill_file.contains('\0') {
                    return Err(ManifestError::InvalidManifest);
                }
                // Preserve original decoded UTF-8 bytes. The host uses its
                // existing SKILL.md parser/composer for the installed hash.
                source_skill_digests.push(SkillSourceDigest {
                    component_key: key.clone(),
                    source_digest: digest(skill_file.as_bytes()),
                });
                key
            }
        };
        identifier(key, 24, true)?;
        if !keys.insert(key) {
            return Err(ManifestError::InvalidManifest);
        }
    }
    cap(mcp_keys.len(), MAX_MCP_COMPONENTS)?;
    cap(source_skill_digests.len(), MAX_SKILL_COMPONENTS)?;

    cap(manifest.permissions.native_tool_names.len(), 32)?;
    cap(manifest.permissions.external_tools.len(), 32)?;
    let mut native_names = HashSet::new();
    for name in &manifest.permissions.native_tool_names {
        printable_ascii(name, 128)?;
        if !native_names.insert(name) {
            return Err(ManifestError::InvalidManifest);
        }
    }
    let mut external_names = HashSet::new();
    for permission in &manifest.permissions.external_tools {
        identifier(&permission.component, 24, true)?;
        printable_ascii(&permission.remote_name, 128)?;
        if !mcp_keys.contains(permission.component.as_str())
            || !external_names.insert((&permission.component, &permission.remote_name))
        {
            return Err(ManifestError::InvalidManifest);
        }
    }
    if manifest.attribution.is_empty() {
        return Err(ManifestError::InvalidManifest);
    }
    cap(manifest.attribution.len(), 8)?;
    let mut attribution = HashSet::new();
    for entry in &manifest.attribution {
        identifier(&entry.component, 64, false)?;
        text(&entry.source, 2048)?;
        text(&entry.revision, 256)?;
        text(&entry.license, 256)?;
        text(&entry.notice, 2048)?;
        if !attribution.insert((
            &entry.component,
            &entry.source,
            &entry.revision,
            &entry.license,
            &entry.notice,
        )) {
            return Err(ManifestError::InvalidManifest);
        }
    }
    let canonical = serde_jcs::to_vec(manifest).map_err(|_| ManifestError::InvalidManifest)?;
    cap(canonical.len(), MAX_MANIFEST_BYTES)?;
    Ok(ValidatedManifest {
        manifest: manifest.clone(),
        manifest_digest: digest(&canonical),
        source_skill_digests,
    })
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn cap(actual: usize, maximum: usize) -> Result<(), ManifestError> {
    if actual > maximum {
        Err(ManifestError::LimitsExceeded)
    } else {
        Ok(())
    }
}
fn identifier(value: &str, maximum: usize, reserved: bool) -> Result<(), ManifestError> {
    cap(value.len(), maximum)?;
    let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    if value.is_empty()
        || !value
            .bytes()
            .all(|b| alphanumeric(b) || matches!(b, b'.' | b'_' | b'-'))
        || value.split(['.', '_', '-']).any(str::is_empty)
        || (reserved && matches!(value, "system" | "core" | "anima" | "admin"))
    {
        return Err(ManifestError::InvalidManifest);
    }
    Ok(())
}
fn text(value: &str, maximum: usize) -> Result<(), ManifestError> {
    cap(value.len(), maximum)?;
    if value.trim().is_empty()
        || value.chars().any(|c| c.is_control() || hidden(c))
        || selector_run(value)
    {
        Err(ManifestError::InvalidManifest)
    } else {
        Ok(())
    }
}
fn printable_ascii(value: &str, maximum: usize) -> Result<(), ManifestError> {
    cap(value.len(), maximum)?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        Err(ManifestError::InvalidManifest)
    } else {
        Ok(())
    }
}
fn release(value: &str) -> Result<[u32; 3], ManifestError> {
    let mut result = [0; 3];
    let mut pieces = value.split('.');
    for integer in &mut result {
        let piece = pieces.next().ok_or(ManifestError::InvalidManifest)?;
        if piece.is_empty()
            || piece.len() > 10
            || (piece.len() > 1 && piece.starts_with('0'))
            || !piece.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ManifestError::InvalidManifest);
        }
        *integer = piece.parse().map_err(|_| ManifestError::InvalidManifest)?;
    }
    if pieces.next().is_some() {
        return Err(ManifestError::InvalidManifest);
    }
    Ok(result)
}
fn hidden(character: char) -> bool {
    matches!(character as u32,
        0x00AD | 0x034F | 0x115F | 0x1160 | 0x3164 | 0xFFA0 |
        0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2 | 0x180E |
        0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB |
        0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A |
        0xE0000..=0xE007F | 0xE0100..=0xE01EF)
}
fn selector_run(value: &str) -> bool {
    let selector = |c: char| matches!(c as u32, 0xFE00..=0xFE0F);
    value
        .chars()
        .zip(value.chars().skip(1))
        .any(|(a, b)| selector(a) && selector(b))
}

// Parse duplicate keys and nulls before serde_json::Value can erase them. A
// seed bounds recursion/node allocation even in fields later rejected by DTOs.
struct ParseBounds {
    nodes: usize,
    error: Option<ManifestError>,
}
impl ParseBounds {
    fn fail<E: de::Error>(&mut self, error: ManifestError) -> E {
        self.error = Some(error);
        E::custom("Invalid community manifest JSON")
    }
}
struct StrictValue<'a> {
    bounds: &'a mut ParseBounds,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for StrictValue<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        self.bounds.nodes += 1;
        if self.depth > MAX_JSON_DEPTH || self.bounds.nodes > MAX_JSON_NODES {
            return Err(self.bounds.fail(ManifestError::LimitsExceeded));
        }
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for StrictValue<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("bounded non-null community manifest JSON")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Err(self.bounds.fail(ManifestError::InvalidInput))
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| self.bounds.fail(ManifestError::InvalidInput))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValue {
            bounds: &mut *self.bounds,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(self.bounds.fail(ManifestError::InvalidInput));
            }
            let value = map.next_value_seed(StrictValue {
                bounds: &mut *self.bounds,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn runtime() -> RuntimeCompatibility {
        RuntimeCompatibility {
            host: "rust-daemon".into(),
            version: "0.1.0".into(),
            capabilities: vec!["mcp.owner.v1".into(), "skills.import-draft.v1".into()],
        }
    }
    fn sample() -> CommunityManifest {
        CommunityManifest {
            schema_version: 1, id: "example.synthetic-violet".into(), version: "1.0.0".into(),
            name: "Synthetic violet".into(), description: "A local declaration".into(),
            compatibility: ManifestCompatibility {
                host: "rust-daemon".into(), min_version: "0.1.0".into(), max_version_exclusive: "0.2.0".into(),
                required_capabilities: vec!["mcp.owner.v1".into(), "skills.import-draft.v1".into()],
            },
            permissions: ManifestPermissions {
                native_tool_names: Vec::new(),
                external_tools: vec![ExternalToolPermission { component: "violet-server".into(), remote_name: "synthetic_stdio_record".into() }],
            },
            attribution: vec![ManifestAttribution {
                component: "example".into(), source: "repository-local example".into(),
                revision: "release-recorded".into(), license: "repository-license".into(), notice: "Retain this notice".into(),
            }],
            components: vec![
                ManifestComponent::McpStdio { key: "violet-server".into(), label: "Synthetic violet".into(),
                    program_ref: "synthetic-peer".into(), args: vec!["--mode".into(), "normal".into()] },
                ManifestComponent::Skill { key: "violet-procedure".into(),
                    skill_file: "---\r\nname: Violet\r\ndescription: Inspect granted tools\r\n---\r\n\r\nUse only granted tools.\r\n".into() },
            ],
        }
    }

    #[test]
    fn m45_recursive_duplicate_keys_null_unknown_and_mixed_fields_are_rejected() {
        let source = serde_json::to_string(&sample()).unwrap();
        for (from, to) in [
            ("\"schemaVersion\":1", "\"schemaVersion\":1,\"schemaVersion\":1"),
            ("\"host\":\"rust-daemon\"", "\"host\":\"rust-daemon\",\"h\\u006fst\":\"rust-daemon\""),
            ("\"kind\":\"mcpStdio\"", "\"kind\":\"mcpStdio\",\"kind\":\"mcpHttp\""),
            ("\"remoteName\":\"synthetic_stdio_record\"", "\"remoteName\":\"synthetic_stdio_record\",\"remoteName\":\"synthetic_stdio_record\""),
        ] {
            let duplicated = source.replace(from, to);
            assert_ne!(duplicated, source);
            assert_eq!(parse_manifest(duplicated.as_bytes()), Err(ManifestError::InvalidInput));
        }
        for pointer in [
            "/schemaVersion",
            "/compatibility/host",
            "/components/0/args/0",
            "/attribution/0/notice",
        ] {
            let mut value = serde_json::to_value(sample()).unwrap();
            *value.pointer_mut(pointer).unwrap() = Value::Null;
            assert_eq!(
                parse_manifest(&serde_json::to_vec(&value).unwrap()),
                Err(ManifestError::InvalidInput)
            );
        }
        let mut mixed = serde_json::to_value(sample()).unwrap();
        mixed["components"][0]["endpoint"] = Value::String("https://example.invalid/mcp".into());
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&mixed).unwrap()),
            Err(ManifestError::InvalidInput)
        );
        for unknown in [
            "enabled",
            "targetAgentId",
            "programBindings",
            "reviewId",
            "grants",
            "approvalClassification",
        ] {
            let mut value = serde_json::to_value(sample()).unwrap();
            value[unknown] = Value::Bool(true);
            assert_eq!(
                parse_manifest(&serde_json::to_vec(&value).unwrap()),
                Err(ManifestError::InvalidInput)
            );
        }
    }

    #[test]
    fn m45_raw_utf8_root_trailing_depth_node_and_byte_limits_precede_typed_decode() {
        assert_eq!(parse_manifest(&[0xff]), Err(ManifestError::InvalidInput));
        for raw in [b"[]".as_slice(), b"null", b"{} true", b"{\"unknown\":null}"] {
            assert_eq!(parse_manifest(raw), Err(ManifestError::InvalidInput));
        }
        let deep = format!(
            "{{\"unknown\":{}0{}}}",
            "[".repeat(MAX_JSON_DEPTH),
            "]".repeat(MAX_JSON_DEPTH)
        );
        assert_eq!(
            parse_manifest(deep.as_bytes()),
            Err(ManifestError::LimitsExceeded)
        );
        let nodes = serde_json::json!({"unknown":vec![false; MAX_JSON_NODES]});
        assert_eq!(
            parse_manifest(&serde_json::to_vec(&nodes).unwrap()),
            Err(ManifestError::LimitsExceeded)
        );
        let mut boundary = serde_json::to_vec(&sample()).unwrap();
        boundary.resize(MAX_MANIFEST_BYTES, b' ');
        assert!(parse_manifest(&boundary).is_ok());
        boundary.push(b' ');
        assert_eq!(
            parse_manifest(&boundary),
            Err(ManifestError::LimitsExceeded)
        );
    }

    #[test]
    fn m45_literal_empty_argv_and_unicode_skill_source_are_never_trimmed_or_tokenized() {
        for args in [
            vec![],
            vec![""],
            vec!["", " --quoted='violet $()' ", "", "", "雪"],
        ] {
            let mut manifest = sample();
            let owned: Vec<String> = args.into_iter().map(String::from).collect();
            let ManifestComponent::McpStdio { args, .. } = &mut manifest.components[0] else {
                unreachable!()
            };
            *args = owned.clone();
            let bytes = serde_json::to_vec(&manifest).unwrap();
            let parsed = parse_manifest(&bytes).unwrap();
            let validated = validate_manifest(&parsed, &runtime()).unwrap();
            let ManifestComponent::McpStdio { args, .. } = &validated.manifest.components[0] else {
                unreachable!()
            };
            assert_eq!(args, &owned);
        }
        let mut manifest = sample();
        let original = "\u{feff}---\r\nname: 雪\r\ndescription: Preserve source\r\n---\r\n\r\n Exact bytes.\r\n";
        let ManifestComponent::Skill { skill_file, .. } = &mut manifest.components[1] else {
            unreachable!()
        };
        *skill_file = original.into();
        let validated = validate_manifest(&manifest, &runtime()).unwrap();
        assert_eq!(
            validated.source_skill_digests[0].source_digest,
            digest(original.as_bytes())
        );
        assert_eq!(validated.manifest, manifest);
    }

    #[test]
    fn m45_digest_changes_with_ordered_args_skill_source_and_requested_permissions() {
        let source = sample();
        let baseline = validate_manifest(&source, &runtime()).unwrap();
        for change in 0..3 {
            let mut changed = source.clone();
            match change {
                0 => {
                    let ManifestComponent::McpStdio { args, .. } = &mut changed.components[0]
                    else {
                        unreachable!()
                    };
                    args.swap(0, 1);
                }
                1 => {
                    let ManifestComponent::Skill { skill_file, .. } = &mut changed.components[1]
                    else {
                        unreachable!()
                    };
                    *skill_file = skill_file.replace("\r\n", "\n");
                }
                _ => changed
                    .permissions
                    .native_tool_names
                    .push("bg_start".into()),
            }
            let validated = validate_manifest(&changed, &runtime()).unwrap();
            assert_ne!(baseline.manifest_digest, validated.manifest_digest);
            if change == 1 {
                assert_ne!(
                    baseline.source_skill_digests,
                    validated.source_skill_digests
                );
            } else {
                assert_eq!(
                    baseline.source_skill_digests,
                    validated.source_skill_digests
                );
            }
        }
        let mut http = source;
        http.components[0] = ManifestComponent::McpHttp {
            key: "violet-server".into(),
            label: "Synthetic violet".into(),
            endpoint: "https://example.invalid/mcp".into(),
        };
        assert_ne!(
            baseline.manifest_digest,
            validate_manifest(&http, &runtime())
                .unwrap()
                .manifest_digest
        );
    }

    #[test]
    fn m45_direct_constructed_manifests_recheck_schema_identifiers_and_hidden_text() {
        let mut manifest = sample();
        manifest.schema_version = 2;
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::UnsupportedSchema)
        );
        for id in [
            "",
            "UPPER",
            "../violet",
            "violet..server",
            "-violet",
            "violet-",
            "雪",
            "system",
            "anima",
            "violet\u{200b}",
        ] {
            let mut manifest = sample();
            manifest.id = id.into();
            assert_eq!(
                validate_manifest(&manifest, &runtime()),
                Err(ManifestError::InvalidManifest)
            );
        }
        for name in [
            " ",
            "two\nlines",
            "a\u{2066}b",
            "a\u{e0041}b",
            "a\u{fe00}\u{fe01}b",
        ] {
            let mut manifest = sample();
            manifest.name = name.into();
            assert_eq!(
                validate_manifest(&manifest, &runtime()),
                Err(ManifestError::InvalidManifest)
            );
        }
        let mut manifest = sample();
        manifest.name = "n".repeat(129);
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::LimitsExceeded)
        );
    }

    #[test]
    fn m45_strict_release_ranges_and_actual_capability_facts_are_required() {
        for version in [
            "1",
            "1.0",
            "1.0.0.0",
            "01.0.0",
            "+1.0.0",
            "1.0.0-beta",
            "^1.0.0",
            "4294967296.0.0",
        ] {
            let mut manifest = sample();
            manifest.version = version.into();
            assert_eq!(
                validate_manifest(&manifest, &runtime()),
                Err(ManifestError::InvalidManifest)
            );
        }
        for (version, compatible) in [
            ("0.0.9", false),
            ("0.1.0", true),
            ("0.1.4294967295", true),
            ("0.2.0", false),
        ] {
            let mut actual = runtime();
            actual.version = version.into();
            assert_eq!(validate_manifest(&sample(), &actual).is_ok(), compatible);
        }
        let mut manifest = sample();
        manifest.compatibility.max_version_exclusive = "0.1.0".into();
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::InvalidManifest)
        );
        let mut actual = runtime();
        actual.host = "different-host".into();
        assert_eq!(
            validate_manifest(&sample(), &actual),
            Err(ManifestError::IncompatibleRuntime)
        );
        let mut actual = runtime();
        actual.capabilities.pop();
        assert_eq!(
            validate_manifest(&sample(), &actual),
            Err(ManifestError::IncompatibleRuntime)
        );
        let mut manifest = sample();
        manifest
            .compatibility
            .required_capabilities
            .push("mcp.stdio.windows.v1".into());
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::IncompatibleRuntime)
        );
        let mut manifest = sample();
        manifest
            .compatibility
            .required_capabilities
            .push("mcp.owner.v1".into());
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::InvalidManifest)
        );
    }

    #[test]
    fn m45_requested_permissions_reference_declared_mcp_only_and_confer_no_authority() {
        for component in ["violet-procedure", "undeclared"] {
            let mut manifest = sample();
            manifest.permissions.external_tools[0].component = component.into();
            assert_eq!(
                validate_manifest(&manifest, &runtime()),
                Err(ManifestError::InvalidManifest)
            );
        }
        let mut manifest = sample();
        manifest
            .permissions
            .external_tools
            .push(manifest.permissions.external_tools[0].clone());
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::InvalidManifest)
        );
        let mut manifest = sample();
        manifest.permissions.native_tool_names = vec!["bg_start".into(), "bg_start".into()];
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::InvalidManifest)
        );
        let mut manifest = sample();
        manifest.permissions.native_tool_names = vec!["bg_start".into()];
        let validated = validate_manifest(&manifest, &runtime()).unwrap();
        assert_eq!(
            validated.manifest.permissions.native_tool_names,
            ["bg_start"]
        );
        let value = serde_json::to_value(validated.manifest).unwrap();
        for authority in ["enabled", "grants", "approved", "targetAgentId", "reviewId"] {
            assert!(value.get(authority).is_none());
        }
    }

    #[test]
    fn m45_component_quotas_duplicate_keys_and_exact_argv_byte_limits_are_joint() {
        let mut manifest = sample();
        manifest.components.push(manifest.components[0].clone());
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::InvalidManifest)
        );
        let mut manifest = sample();
        manifest.permissions.external_tools.clear();
        manifest.components = (0..3)
            .map(|index| ManifestComponent::McpHttp {
                key: format!("peer-{index}"),
                label: "Peer".into(),
                endpoint: "https://example.invalid/mcp".into(),
            })
            .collect();
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::LimitsExceeded)
        );
        for (args, expected) in [
            (vec![String::new(); 32], true),
            (vec![String::new(); 33], false),
            (vec!["x".repeat(2048); 8], true),
            (vec!["x".repeat(2048); 9], false),
            (vec!["x".repeat(2049)], false),
        ] {
            let mut manifest = sample();
            let ManifestComponent::McpStdio {
                args: arguments, ..
            } = &mut manifest.components[0]
            else {
                unreachable!()
            };
            *arguments = args;
            assert_eq!(validate_manifest(&manifest, &runtime()).is_ok(), expected);
        }
        for argument in ["nul\0", "line\n", "tab\t", "hidden\u{200b}", "bidi\u{202e}"] {
            let mut manifest = sample();
            let ManifestComponent::McpStdio { args, .. } = &mut manifest.components[0] else {
                unreachable!()
            };
            *args = vec![argument.into()];
            assert_eq!(
                validate_manifest(&manifest, &runtime()),
                Err(ManifestError::InvalidManifest)
            );
        }
    }

    #[test]
    fn m45_direct_constructed_aggregate_and_skill_source_limits_cannot_bypass_parser_caps() {
        let mut manifest = sample();
        let ManifestComponent::Skill { skill_file, .. } = &mut manifest.components[1] else {
            unreachable!()
        };
        *skill_file = "s".repeat(MAX_SKILL_SOURCE_BYTES + 1);
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::LimitsExceeded)
        );
        let mut manifest = sample();
        manifest.permissions.external_tools.clear();
        manifest.components = (0..2)
            .map(|index| ManifestComponent::Skill {
                key: format!("skill-{index}"),
                skill_file: "s".repeat(MAX_SKILL_SOURCE_BYTES),
            })
            .collect();
        assert_eq!(
            validate_manifest(&manifest, &runtime()),
            Err(ManifestError::LimitsExceeded)
        );
    }

    #[test]
    fn m45_errors_never_echo_untrusted_fields_parser_details_or_argv() {
        let mut manifest = sample();
        manifest.name = "synthetic-sensitive\ntext".into();
        let error = validate_manifest(&manifest, &runtime()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Community manifest declaration is invalid"
        );
        let error = parse_manifest(br#"{"synthetic-sensitive":broken}"#).unwrap_err();
        assert_eq!(error.to_string(), "Community manifest input is invalid");
    }

    #[test]
    fn m45_valid_manifest_preserves_literals_and_normalizes_json_identity() {
        let bytes = br#"{
          "schemaVersion": 1,
          "id": "example.synthetic-violet",
          "version": "1.0.0",
          "name": "Synthetic violet procedure",
          "description": "A local declarative example",
          "compatibility": {"host":"rust-daemon","minVersion":"0.1.0",
            "maxVersionExclusive":"0.2.0","requiredCapabilities":["mcp.owner.v1","skills.import-draft.v1"]},
          "permissions": {"nativeToolNames":[],"externalTools":[
            {"component":"violet-server","remoteName":"synthetic_stdio_record"}]},
          "attribution": [{"component":"example","source":"repository-local example",
            "revision":"release-recorded","license":"repository-license","notice":"Retain this notice"}],
          "components": [
            {"kind":"mcpStdio","key":"violet-server","label":"Synthetic violet",
             "programRef":"synthetic-peer","args":["--literal","violet '$()'",""]},
            {"kind":"skill","key":"violet-procedure",
             "skillFile":"---\r\nname: Violet\r\ndescription: Check an approved result\r\n---\r\n\r\nUse only granted tools.\r\n"}
          ]
        }"#;
        let manifest = parse_manifest(bytes)
            .expect("a valid declarative manifest must parse before host staging");
        let validated = validate_manifest(
            &manifest,
            &RuntimeCompatibility {
                host: "rust-daemon".into(),
                version: "0.1.0".into(),
                capabilities: vec!["mcp.owner.v1".into(), "skills.import-draft.v1".into()],
            },
        )
        .expect("compatible declarations must produce bounded review identity");
        let ManifestComponent::McpStdio {
            args, program_ref, ..
        } = &validated.manifest.components[0]
        else {
            panic!("the declared stdio variant must be retained");
        };
        assert_eq!(program_ref, "synthetic-peer");
        assert_eq!(args, &["--literal", "violet '$()'", ""]);
        let ManifestComponent::Skill { skill_file, .. } = &validated.manifest.components[1] else {
            panic!("the declared skill variant must be retained");
        };
        assert!(
            skill_file.contains("\r\n"),
            "host skill composition must not erase source-byte review identity"
        );
        let json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let canonical = serde_jcs::to_vec(&json).unwrap();
        assert_eq!(
            validated.manifest_digest,
            format!("{:x}", Sha256::digest(&canonical))
        );
        assert_eq!(
            validated.source_skill_digests,
            vec![SkillSourceDigest {
                component_key: "violet-procedure".into(),
                source_digest: format!("{:x}", Sha256::digest(skill_file.as_bytes())),
            }]
        );
        let reformatted = serde_json::to_vec(&json).unwrap();
        let reformatted = parse_manifest(&reformatted).unwrap();
        let again = validate_manifest(
            &reformatted,
            &RuntimeCompatibility {
                host: "rust-daemon".into(),
                version: "0.1.0".into(),
                capabilities: vec!["mcp.owner.v1".into(), "skills.import-draft.v1".into()],
            },
        )
        .unwrap();
        assert_eq!(validated.manifest_digest, again.manifest_digest);
    }
}
