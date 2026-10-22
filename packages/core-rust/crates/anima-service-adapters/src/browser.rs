//! Portable, bounded interactive-browser data and control contracts.
//! A scope is admission data, not an owner grant. Runtime authority and physical
//! cleanup belong to the host implementation; this module launches no browser.

use std::{collections::HashSet, time::Instant};

use async_trait::async_trait;
use serde::{
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use anima_core::CancelSignal;

pub const MAX_BROWSER_ROUTES: usize = 16;
pub const MAX_BROWSER_INPUT_IDS: usize = 16;
pub const MAX_BROWSER_ID_BYTES: usize = 64;
pub const MAX_BROWSER_ROUTE_BYTES: usize = 256;
pub const MAX_BROWSER_ACTIONS: u16 = 40;
pub const MAX_BROWSER_REQUESTS: u16 = 80;
pub const MAX_BROWSER_SCOPE_MS: u32 = 120_000;
pub const MAX_BROWSER_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_BROWSER_WIRE_BYTES: u32 = 1024 * 1024;
pub const MAX_BROWSER_OBSERVATION_NODES: usize = 128;
pub const MAX_BROWSER_OBSERVATION_TEXT_BYTES: u32 = 8 * 1024;
pub const MAX_BROWSER_TYPE_BYTES: usize = 4096;
pub const MAX_BROWSER_SCROLL_DELTA: i32 = 2048;
pub const MAX_BROWSER_ASSET_BYTES: u32 = 16 * 1024;
pub const MAX_BROWSER_SCREENSHOT_BYTES: u32 = 256 * 1024;
pub const MAX_BROWSER_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Cancellation and a host-selected monotonic deadline; never serialized to IPC.
#[derive(Clone, Debug)]
pub struct BrowserControl {
    pub cancel: CancelSignal,
    pub deadline: Instant,
}

/// Host-issued scope data. Owner/run/grant and installation witnesses stay in
/// host state and must be revalidated independently of these public fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserScope {
    pub id: String,
    pub revision: u64,
    pub origin: String,
    /// Optional operator-issued public address. Absence preserves the M46
    /// numeric-loopback wire shape; an explicit null is not absence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    pub public_ipv4_pin: Option<String>,
    /// Separate owner-authenticated lane; hosts must require a profile witness.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub authenticated: bool,
    pub routes: Vec<BrowserRoute>,
    pub input_ids: Vec<String>,
    pub limits: BrowserLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserRoute {
    pub id: String,
    pub method: BrowserMethod,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BrowserMethod {
    Get,
    Post,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserLimits {
    pub actions: u16,
    pub requests: u16,
    pub scope_ms: u32,
    pub frame_bytes: u32,
    pub wire_bytes: u32,
    pub observation_nodes: u16,
    pub observation_text_bytes: u32,
    pub input_bytes: u32,
    pub output_bytes: u32,
    pub screenshot_bytes: u32,
}

/// An opaque host-issued token. Format validity alone never establishes that
/// it belongs to the active session/document/observation or resolves uniquely.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserNodeRef {
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BrowserOperation {
    #[serde(rename_all = "camelCase")]
    Navigate {
        route_id: String,
    },
    Observe {
        screenshot: bool,
    },
    Click {
        node: BrowserNodeRef,
    },
    Type {
        node: BrowserNodeRef,
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    Scroll {
        delta_y: i32,
    },
    #[serde(rename_all = "camelCase")]
    Upload {
        node: BrowserNodeRef,
        input_id: String,
    },
    Download {
        node: BrowserNodeRef,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserNode {
    pub node: BrowserNodeRef,
    pub role: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserArtifactKind {
    Screenshot,
    Csv,
}

/// Owned-artifact metadata only: no bytes, base64, host path or browser filename.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserArtifact {
    pub id: String,
    pub kind: BrowserArtifactKind,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserObservation {
    pub document_generation: u64,
    pub route_id: String,
    pub nodes: Vec<BrowserNode>,
    pub shortened: bool,
    pub omitted_nodes: u32,
    pub artifacts: Vec<BrowserArtifact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserEffect {
    NotDispatched,
    Completed,
    MayHaveOccurred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserErrorCode {
    MissingFeature,
    Unavailable,
    Invalid,
    LimitsExceeded,
    Denied,
    Stale,
    Cancelled,
    Deadline,
    Protocol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserError {
    pub code: BrowserErrorCode,
    pub effect: BrowserEffect,
}

impl std::fmt::Display for BrowserError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.effect == BrowserEffect::MayHaveOccurred {
            return formatter.write_str("Browser action failed; effects may have occurred");
        }
        formatter.write_str(match self.code {
            BrowserErrorCode::MissingFeature => "Browser validation support is not implemented",
            BrowserErrorCode::Unavailable => "Browser is unavailable",
            BrowserErrorCode::Invalid => "Browser input is invalid",
            BrowserErrorCode::LimitsExceeded => "Browser limits exceeded",
            BrowserErrorCode::Denied => "Browser action is denied",
            BrowserErrorCode::Stale => "Browser action witness is stale",
            BrowserErrorCode::Cancelled => "Browser action was cancelled",
            BrowserErrorCode::Deadline => "Browser deadline reached",
            BrowserErrorCode::Protocol => "Browser protocol is invalid",
        })
    }
}
impl std::error::Error for BrowserError {}

/// Closed is a host physical-cleanup witness, not a cancelled caller future.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserClose {
    Closed,
    Quarantined,
}

#[async_trait]
pub trait BrowserPort: Send + Sync {
    async fn open(
        &self,
        scope: BrowserScope,
        control: BrowserControl,
    ) -> Result<Box<dyn BrowserSession>, BrowserError>;
}

#[async_trait]
pub trait BrowserSession: Send + Sync {
    async fn execute(
        &self,
        operation: BrowserOperation,
        control: BrowserControl,
    ) -> Result<BrowserObservation, BrowserError>;

    /// Idempotent. The host owns uncancelled retirement even if this future is
    /// dropped, and retains its lease/permits when cleanup is uncertain.
    async fn close(&self) -> BrowserClose;
}

fn invalid() -> BrowserError {
    failure(BrowserErrorCode::Invalid)
}
fn failure(code: BrowserErrorCode) -> BrowserError {
    BrowserError {
        code,
        effect: BrowserEffect::NotDispatched,
    }
}
fn cap(value: usize, maximum: usize) -> Result<(), BrowserError> {
    if value > maximum {
        Err(failure(BrowserErrorCode::LimitsExceeded))
    } else {
        Ok(())
    }
}
fn limit(value: u32, maximum: u32) -> Result<(), BrowserError> {
    if value == 0 {
        return Err(invalid());
    }
    if value > maximum {
        return Err(failure(BrowserErrorCode::LimitsExceeded));
    }
    Ok(())
}
fn identifier(value: &str) -> Result<(), BrowserError> {
    if value.is_empty()
        || value.len() > MAX_BROWSER_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid());
    }
    Ok(())
}
fn safe_positive_integer(value: u64) -> Result<(), BrowserError> {
    if value == 0 || value > MAX_BROWSER_SAFE_INTEGER {
        return Err(invalid());
    }
    Ok(())
}
fn present_string<'de, D: Deserializer<'de>>(decoder: D) -> Result<Option<String>, D::Error> {
    String::deserialize(decoder).map(Some)
}
fn loopback_origin(value: &str) -> Result<(), BrowserError> {
    let port = value
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(invalid)?;
    if port.is_empty()
        || port.len() > 5
        || port.starts_with('0')
        || !port.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid());
    }
    if port.parse::<u16>().map_err(|_| invalid())? == 0 {
        return Err(invalid());
    }
    Ok(())
}
fn public_hostname(value: &str) -> Result<(), BrowserError> {
    let name = value.strip_prefix("https://").ok_or_else(invalid)?;
    if name.len() > 253 || name.is_empty() {
        return Err(invalid());
    }
    let labels = name.split('.').collect::<Vec<_>>();
    if labels.len() < 2
        || labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with("xn--")
                || !label
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                || !label
                    .as_bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        || !labels
            .last()
            .unwrap()
            .bytes()
            .any(|byte| byte.is_ascii_lowercase())
        || (labels.last().unwrap().starts_with("0x")
            && labels.last().unwrap()[2..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()))
        || matches!(
            *labels.last().unwrap(),
            "localhost"
                | "local"
                | "internal"
                | "lan"
                | "home"
                | "onion"
                | "test"
                | "invalid"
                | "example"
                | "arpa"
        )
    {
        return Err(invalid());
    }
    Ok(())
}
fn public_ipv4_pin(value: &str) -> Result<(), BrowserError> {
    let address = value.parse::<std::net::Ipv4Addr>().map_err(|_| invalid())?;
    if address.to_string() != value {
        return Err(invalid());
    }
    let number = u32::from_be_bytes(address.octets());
    // Conservative collapsed IANA IPv4 special-purpose prefixes (2025-10-09),
    // multicast/reserved space, and Azure's documented platform address.
    const DENIED: [([u8; 4], u32); 19] = [
        ([0, 0, 0, 0], 8),
        ([10, 0, 0, 0], 8),
        ([100, 64, 0, 0], 10),
        ([127, 0, 0, 0], 8),
        ([168, 63, 129, 16], 32),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 0, 0, 0], 24),
        ([192, 0, 2, 0], 24),
        ([192, 31, 196, 0], 24),
        ([192, 52, 193, 0], 24),
        ([192, 88, 99, 0], 24),
        ([192, 168, 0, 0], 16),
        ([192, 175, 48, 0], 24),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([224, 0, 0, 0], 4),
        ([240, 0, 0, 0], 4),
    ];
    if DENIED.iter().any(|(prefix, bits)| {
        let mask = u32::MAX << (32 - *bits);
        (number & mask) == (u32::from_be_bytes(*prefix) & mask)
    }) {
        return Err(invalid());
    }
    Ok(())
}
fn route_path(value: &str) -> Result<(), BrowserError> {
    if !value.starts_with('/')
        || value.len() > MAX_BROWSER_ROUTE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.'))
    {
        return Err(invalid());
    }
    let mut segments = value.split('/').skip(1).peekable();
    while let Some(segment) = segments.next() {
        if matches!(segment, "." | "..") || (segment.is_empty() && segments.peek().is_some()) {
            return Err(invalid());
        }
    }
    Ok(())
}
fn fits_frame<T: Serialize>(value: &T, maximum: u32) -> Result<(), BrowserError> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    frame_size(bytes.len(), maximum)
}
fn frame_size(length: usize, maximum: u32) -> Result<(), BrowserError> {
    if u32::try_from(length).map_err(|_| failure(BrowserErrorCode::LimitsExceeded))? > maximum {
        return Err(failure(BrowserErrorCode::LimitsExceeded));
    }
    Ok(())
}

/// Validates admission data only; does not establish a live owner/grant/lease.
pub fn validate_scope(scope: &BrowserScope) -> Result<(), BrowserError> {
    identifier(&scope.id)?;
    safe_positive_integer(scope.revision)?;
    match scope.public_ipv4_pin.as_deref() {
        Some(pin) => {
            public_hostname(&scope.origin)?;
            public_ipv4_pin(pin)?;
            if !scope.input_ids.is_empty()
                || (!scope.authenticated
                    && scope
                        .routes
                        .iter()
                        .any(|route| route.method != BrowserMethod::Get))
            {
                return Err(invalid());
            }
        }
        None => loopback_origin(&scope.origin)?,
    }
    if scope.routes.is_empty() {
        return Err(invalid());
    }
    cap(scope.routes.len(), MAX_BROWSER_ROUTES)?;
    cap(scope.input_ids.len(), MAX_BROWSER_INPUT_IDS)?;
    let mut route_ids = HashSet::new();
    let mut method_paths = HashSet::new();
    for route in &scope.routes {
        identifier(&route.id)?;
        route_path(&route.path)?;
        if !route_ids.insert(&route.id)
            || !method_paths.insert((route.method == BrowserMethod::Get, &route.path))
        {
            return Err(invalid());
        }
    }
    let mut input_ids = HashSet::new();
    for input in &scope.input_ids {
        identifier(input)?;
        if !input_ids.insert(input) {
            return Err(invalid());
        }
    }
    let limits = &scope.limits;
    limit(u32::from(limits.actions), u32::from(MAX_BROWSER_ACTIONS))?;
    limit(u32::from(limits.requests), u32::from(MAX_BROWSER_REQUESTS))?;
    limit(limits.scope_ms, MAX_BROWSER_SCOPE_MS)?;
    limit(limits.frame_bytes, MAX_BROWSER_FRAME_BYTES as u32)?;
    limit(limits.wire_bytes, MAX_BROWSER_WIRE_BYTES)?;
    limit(
        u32::from(limits.observation_nodes),
        MAX_BROWSER_OBSERVATION_NODES as u32,
    )?;
    limit(
        limits.observation_text_bytes,
        MAX_BROWSER_OBSERVATION_TEXT_BYTES,
    )?;
    limit(limits.input_bytes, MAX_BROWSER_ASSET_BYTES)?;
    limit(limits.output_bytes, MAX_BROWSER_ASSET_BYTES)?;
    limit(limits.screenshot_bytes, MAX_BROWSER_SCREENSHOT_BYTES)?;
    if limits.frame_bytes > limits.wire_bytes {
        return Err(invalid());
    }
    fits_frame(scope, limits.frame_bytes)
}

fn validate_operation_shape(operation: &BrowserOperation) -> Result<(), BrowserError> {
    match operation {
        BrowserOperation::Navigate { route_id } => identifier(route_id)?,
        BrowserOperation::Observe { .. } => {}
        BrowserOperation::Click { node } | BrowserOperation::Download { node } => {
            identifier(&node.token)?
        }
        BrowserOperation::Type { node, text } => {
            identifier(&node.token)?;
            cap(text.len(), MAX_BROWSER_TYPE_BYTES)?;
        }
        BrowserOperation::Scroll { delta_y } => {
            if *delta_y == 0
                || !(-MAX_BROWSER_SCROLL_DELTA..=MAX_BROWSER_SCROLL_DELTA).contains(delta_y)
            {
                return Err(invalid());
            }
        }
        BrowserOperation::Upload { node, input_id } => {
            identifier(&node.token)?;
            identifier(input_id)?;
        }
    }
    Ok(())
}

/// Runtime node/session/document witnesses must additionally be checked by the host.
pub fn validate_operation(
    scope: &BrowserScope,
    operation: &BrowserOperation,
) -> Result<(), BrowserError> {
    validate_scope(scope)?;
    validate_operation_shape(operation)?;
    match operation {
        BrowserOperation::Navigate { route_id } => {
            if !scope
                .routes
                .iter()
                .any(|route| &route.id == route_id && route.method == BrowserMethod::Get)
            {
                return Err(failure(BrowserErrorCode::Denied));
            }
        }
        BrowserOperation::Upload { input_id, .. } => {
            if !scope.input_ids.contains(input_id) {
                return Err(failure(BrowserErrorCode::Denied));
            }
        }
        _ => {}
    }
    fits_frame(operation, scope.limits.frame_bytes)
}

pub fn decode_scope(bytes: &[u8]) -> Result<BrowserScope, BrowserError> {
    let scope = decode_strict(bytes)?;
    validate_scope(&scope)?;
    frame_size(bytes.len(), scope.limits.frame_bytes)?;
    Ok(scope)
}

pub fn decode_operation(
    scope: &BrowserScope,
    bytes: &[u8],
) -> Result<BrowserOperation, BrowserError> {
    let operation = decode_strict(bytes)?;
    validate_operation(scope, &operation)?;
    frame_size(bytes.len(), scope.limits.frame_bytes)?;
    Ok(operation)
}

fn digest<T: Serialize>(value: &T) -> Result<String, BrowserError> {
    let bytes = serde_jcs::to_vec(value).map_err(|_| invalid())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}
pub fn scope_digest(scope: &BrowserScope) -> Result<String, BrowserError> {
    validate_scope(scope)?;
    digest(scope)
}

/// Shape identity only: this digest neither validates scope membership nor grants authority.
pub fn operation_digest(operation: &BrowserOperation) -> Result<String, BrowserError> {
    validate_operation_shape(operation)?;
    digest(operation)
}

pub fn validate_observation(
    scope: &BrowserScope,
    observation: &BrowserObservation,
) -> Result<(), BrowserError> {
    validate_scope(scope)?;
    safe_positive_integer(observation.document_generation)?;
    identifier(&observation.route_id)?;
    if !scope
        .routes
        .iter()
        .any(|route| route.id == observation.route_id)
    {
        return Err(failure(BrowserErrorCode::Denied));
    }
    cap(
        observation.nodes.len(),
        usize::from(scope.limits.observation_nodes),
    )?;
    if observation.shortened != (observation.omitted_nodes > 0) {
        return Err(invalid());
    }
    let mut tokens = HashSet::new();
    let mut text_bytes = 0usize;
    for node in &observation.nodes {
        identifier(&node.node.token)?;
        if node.role.is_empty() || node.role.len() > 64 || !tokens.insert(&node.node.token) {
            return Err(invalid());
        }
        text_bytes = text_bytes
            .checked_add(node.role.len())
            .and_then(|length| length.checked_add(node.label.len()))
            .ok_or_else(|| failure(BrowserErrorCode::LimitsExceeded))?;
        frame_size(text_bytes, scope.limits.observation_text_bytes)?;
    }
    cap(observation.artifacts.len(), 2)?;
    let mut artifact_ids = HashSet::new();
    let mut screenshot = false;
    let mut csv = false;
    for artifact in &observation.artifacts {
        identifier(&artifact.id)?;
        if !artifact_ids.insert(&artifact.id)
            || artifact.bytes == 0
            || artifact.sha256.len() != 64
            || !artifact
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(invalid());
        }
        let maximum = match artifact.kind {
            BrowserArtifactKind::Screenshot => {
                if screenshot {
                    return Err(invalid());
                }
                screenshot = true;
                scope.limits.screenshot_bytes
            }
            BrowserArtifactKind::Csv => {
                if csv {
                    return Err(invalid());
                }
                csv = true;
                scope.limits.output_bytes
            }
        };
        if artifact.bytes > u64::from(maximum) {
            return Err(failure(BrowserErrorCode::LimitsExceeded));
        }
    }
    fits_frame(observation, scope.limits.frame_bytes)
}

pub fn decode_observation(
    scope: &BrowserScope,
    bytes: &[u8],
) -> Result<BrowserObservation, BrowserError> {
    let observation = decode_strict(bytes)?;
    validate_observation(scope, &observation)?;
    frame_size(bytes.len(), scope.limits.frame_bytes)?;
    Ok(observation)
}

// Private, browser-specific bounded JSON visitor. Duplicate keys must be detected
// before serde_json::Value collapses maps; decoded strings also catch escaped keys.
const MAX_JSON_DEPTH: usize = 8;
const MAX_JSON_NODES: usize = 2048;
fn decode_strict<T: de::DeserializeOwned>(bytes: &[u8]) -> Result<T, BrowserError> {
    cap(bytes.len(), MAX_BROWSER_FRAME_BYTES)?;
    std::str::from_utf8(bytes).map_err(|_| invalid())?;
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
    .map_err(|_| failure(bounds.error.unwrap_or(BrowserErrorCode::Invalid)))?;
    deserializer.end().map_err(|_| invalid())?;
    if !value.is_object() {
        return Err(invalid());
    }
    serde_json::from_value(value).map_err(|_| invalid())
}
struct ParseBounds {
    nodes: usize,
    error: Option<BrowserErrorCode>,
}
impl ParseBounds {
    fn fail<E: de::Error>(&mut self, code: BrowserErrorCode) -> E {
        self.error = Some(code);
        E::custom("Invalid browser JSON")
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
            return Err(self.bounds.fail(BrowserErrorCode::LimitsExceeded));
        }
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for StrictValue<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("bounded non-null browser JSON")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Err(self.bounds.fail(BrowserErrorCode::Invalid))
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
            .ok_or_else(|| self.bounds.fail(BrowserErrorCode::Invalid))
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
                return Err(self.bounds.fail(BrowserErrorCode::Invalid));
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

    #[test]
    fn m46_valid_scope_and_finite_operation_preserve_literal_values() {
        let scope = BrowserScope {
            id: "quote-task_1".into(),
            revision: 1,
            origin: "http://127.0.0.1:41873".into(),
            public_ipv4_pin: None,
            authenticated: false,
            routes: vec![
                BrowserRoute {
                    id: "offers".into(),
                    method: BrowserMethod::Get,
                    path: "/offers".into(),
                },
                BrowserRoute {
                    id: "quote".into(),
                    method: BrowserMethod::Post,
                    path: "/quote".into(),
                },
            ],
            input_ids: vec!["request-csv_1".into()],
            limits: BrowserLimits {
                actions: 40,
                requests: 80,
                scope_ms: 120_000,
                frame_bytes: 64 * 1024,
                wire_bytes: 1024 * 1024,
                observation_nodes: 128,
                observation_text_bytes: 8 * 1024,
                input_bytes: 16 * 1024,
                output_bytes: 16 * 1024,
                screenshot_bytes: 256 * 1024,
            },
        };
        let operation = BrowserOperation::Type {
            node: BrowserNodeRef {
                token: "observed-node_1".into(),
            },
            text: "  quoted \"violet\" \\ --literal π  ".into(),
        };

        validate_scope(&scope).expect("a finite exact loopback scope must validate");
        validate_operation(&scope, &operation)
            .expect("a finite typed operation must preserve its literal text");
        let decoded_scope = decode_scope(&serde_json::to_vec(&scope).unwrap())
            .expect("strict scope decoding must retain the declared values");
        let decoded_operation = decode_operation(&scope, &serde_json::to_vec(&operation).unwrap())
            .expect("strict operation decoding must retain the literal values");
        assert_eq!(decoded_scope, scope);
        assert_eq!(decoded_operation, operation);
        let BrowserOperation::Type { text, .. } = decoded_operation else {
            panic!("literal typing must remain the finite Type variant");
        };
        assert_eq!(text, "  quoted \"violet\" \\ --literal π  ");
    }
    fn sample() -> BrowserScope {
        BrowserScope {
            id: "quote-task_1".into(),
            revision: 1,
            origin: "http://127.0.0.1:41873".into(),
            public_ipv4_pin: None,
            authenticated: false,
            routes: vec![
                BrowserRoute {
                    id: "offers".into(),
                    method: BrowserMethod::Get,
                    path: "/offers".into(),
                },
                BrowserRoute {
                    id: "quote".into(),
                    method: BrowserMethod::Post,
                    path: "/quote".into(),
                },
            ],
            input_ids: vec!["request-csv_1".into()],
            limits: BrowserLimits {
                actions: 40,
                requests: 80,
                scope_ms: 120_000,
                frame_bytes: 64 * 1024,
                wire_bytes: 1024 * 1024,
                observation_nodes: 128,
                observation_text_bytes: 8 * 1024,
                input_bytes: 16 * 1024,
                output_bytes: 16 * 1024,
                screenshot_bytes: 256 * 1024,
            },
        }
    }
    fn public_scope_json(origin: &str, pin: &str) -> serde_json::Value {
        let mut value = serde_json::to_value(sample()).unwrap();
        value["origin"] = serde_json::json!(origin);
        value["routes"] = serde_json::json!([
            {"id": "home", "method": "GET", "path": "/"},
            {"id": "article", "method": "GET", "path": "/article"}
        ]);
        value["inputIds"] = serde_json::json!([]);
        value["publicIpv4Pin"] = serde_json::json!(pin);
        value
    }
    fn node() -> BrowserNodeRef {
        BrowserNodeRef {
            token: "observed-node_1".into(),
        }
    }
    fn observation() -> BrowserObservation {
        BrowserObservation {
            document_generation: 1,
            route_id: "offers".into(),
            nodes: vec![BrowserNode {
                node: node(),
                role: "button".into(),
                label: "Quote A".into(),
            }],
            shortened: false,
            omitted_nodes: 0,
            artifacts: vec![
                BrowserArtifact {
                    id: "shot_1".into(),
                    kind: BrowserArtifactKind::Screenshot,
                    bytes: 256 * 1024,
                    sha256: "a".repeat(64),
                },
                BrowserArtifact {
                    id: "csv_1".into(),
                    kind: BrowserArtifactKind::Csv,
                    bytes: 16 * 1024,
                    sha256: "b".repeat(64),
                },
            ],
        }
    }
    fn rejects<T: std::fmt::Debug>(result: Result<T, BrowserError>, code: BrowserErrorCode) {
        assert_eq!(result.unwrap_err(), failure(code));
    }

    #[test]
    fn m46_all_finite_operations_preserve_literals_and_bound_inputs() {
        let scope = sample();
        let mut operations = vec![
            BrowserOperation::Navigate {
                route_id: "offers".into(),
            },
            BrowserOperation::Observe { screenshot: false },
            BrowserOperation::Observe { screenshot: true },
            BrowserOperation::Click { node: node() },
            BrowserOperation::Scroll { delta_y: -2048 },
            BrowserOperation::Scroll { delta_y: 2048 },
            BrowserOperation::Upload {
                node: node(),
                input_id: "request-csv_1".into(),
            },
            BrowserOperation::Download { node: node() },
        ];
        for text in [
            "".to_owned(),
            "  \"quoted\" \\ π\n\t ".to_owned(),
            "π".repeat(2048),
        ] {
            operations.push(BrowserOperation::Type { node: node(), text });
        }
        for operation in operations {
            validate_operation(&scope, &operation).unwrap();
            assert_eq!(
                decode_operation(&scope, &serde_json::to_vec(&operation).unwrap()).unwrap(),
                operation
            );
        }
        rejects(
            validate_operation(
                &scope,
                &BrowserOperation::Type {
                    node: node(),
                    text: "π".repeat(2048) + "a",
                },
            ),
            BrowserErrorCode::LimitsExceeded,
        );
        for delta_y in [0, -2049, 2049, i32::MIN, i32::MAX] {
            rejects(
                validate_operation(&scope, &BrowserOperation::Scroll { delta_y }),
                BrowserErrorCode::Invalid,
            );
        }
        for operation in [
            BrowserOperation::Navigate {
                route_id: "quote".into(),
            },
            BrowserOperation::Navigate {
                route_id: "foreign".into(),
            },
            BrowserOperation::Upload {
                node: node(),
                input_id: "foreign-input".into(),
            },
        ] {
            rejects(
                validate_operation(&scope, &operation),
                BrowserErrorCode::Denied,
            );
            rejects(
                decode_operation(&scope, &serde_json::to_vec(&operation).unwrap()),
                BrowserErrorCode::Denied,
            );
        }
        rejects(
            validate_operation(
                &scope,
                &BrowserOperation::Click {
                    node: BrowserNodeRef {
                        token: "../node".into(),
                    },
                },
            ),
            BrowserErrorCode::Invalid,
        );
    }

    #[test]
    fn m46_direct_scope_rejects_noncanonical_origins_and_route_escapes() {
        for origin in ["http://127.0.0.1:1", "http://127.0.0.1:65535"] {
            let mut scope = sample();
            scope.origin = origin.into();
            validate_scope(&scope).unwrap();
        }
        for origin in [
            "https://127.0.0.1:41873",
            "http://localhost:41873",
            "http://127.1:41873",
            "http://127.0.0.1",
            "http://127.0.0.1:0",
            "http://127.0.0.1:041873",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:41873/",
            "http://127.0.0.1:41873?q",
            "http://127.0.0.1:41873#x",
            "http://user@127.0.0.1:41873",
            "http://[::1]:41873",
            " http://127.0.0.1:41873",
            "http://127.0.0.1:41873\n",
        ] {
            let mut scope = sample();
            scope.origin = origin.into();
            rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
            rejects(
                decode_scope(&serde_json::to_vec(&scope).unwrap()),
                BrowserErrorCode::Invalid,
            );
        }
        for path in ["/", "/app.js", "/assets/quote-csv_1.csv", "/offers/"] {
            let mut scope = sample();
            scope.routes[0].path = path.into();
            validate_scope(&scope).unwrap();
        }
        for path in [
            "offers",
            "//sink",
            "/offers//child",
            "/.",
            "/..",
            "/offers/../quote",
            "/offers/./quote",
            "/%2e%2e",
            "/offers?q=1",
            "/offers#x",
            "/offers\\quote",
            "/offers:quote",
            "/π",
            "/\0",
        ] {
            let mut scope = sample();
            scope.routes[0].path = path.into();
            rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        }
        let mut scope = sample();
        scope.routes[0].path = "/".to_owned() + &"a".repeat(256);
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
    }

    #[test]
    fn m63_one_public_origin_and_pin_are_host_issued_scope_data_not_a_new_operation() {
        let loopback = sample();
        let old_json = serde_json::to_value(&loopback).unwrap();
        assert!(old_json.get("publicIpv4Pin").is_none());
        assert_eq!(
            scope_digest(&loopback).unwrap(),
            "8a921f1b63fece7799c595fa856d71c03398ce6258f1ccff41022b6680a2a442"
        );
        assert_eq!(
            scope_digest(&loopback).unwrap(),
            scope_digest(&decode_scope(&serde_json::to_vec(&old_json).unwrap()).unwrap()).unwrap()
        );
        assert_eq!(loopback.routes[1].method, BrowserMethod::Post);

        let value = public_scope_json("https://m63-browser.anima.tools", "93.184.216.34");
        let scope = decode_scope(&serde_json::to_vec(&value).unwrap())
            .expect("the exact operator public origin and pin must be accepted");
        assert_eq!(serde_json::to_value(&scope).unwrap(), value);
        assert_eq!(scope.origin, "https://m63-browser.anima.tools");
        assert!(scope.input_ids.is_empty());
        assert!(scope
            .routes
            .iter()
            .all(|route| route.method == BrowserMethod::Get));
        assert!(validate_operation(
            &scope,
            &BrowserOperation::Navigate {
                route_id: "article".into()
            }
        )
        .is_ok());
        rejects(
            validate_operation(
                &scope,
                &BrowserOperation::Navigate {
                    route_id: "foreign".into(),
                },
            ),
            BrowserErrorCode::Denied,
        );
        let first_digest = scope_digest(&scope).unwrap();
        let changed = public_scope_json("https://m63-browser.anima.tools", "9.9.9.9");
        let changed = decode_scope(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_ne!(scope_digest(&changed).unwrap(), first_digest);
        let max_host = format!(
            "https://{}.{}.{}.{}.com",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(57)
        );
        assert_eq!(max_host.len() - "https://".len(), 253);
        let max_scope = public_scope_json(&max_host, "93.184.216.34");
        decode_scope(&serde_json::to_vec(&max_scope).unwrap())
            .expect("253-byte hostname and 63-byte labels are allowed");
        let ordinary_hex_prefix = public_scope_json("https://browser.0xyz", "93.184.216.34");
        decode_scope(&serde_json::to_vec(&ordinary_hex_prefix).unwrap())
            .expect("a nonnumeric LDH suffix with 0x prefix is a DNS name");
    }

    #[test]
    fn m63_public_scope_rejects_noncanonical_hosts_missing_pin_post_and_inputs() {
        let mut no_pin = public_scope_json("https://m63-browser.anima.tools", "93.184.216.34");
        no_pin.as_object_mut().unwrap().remove("publicIpv4Pin");
        rejects(
            decode_scope(&serde_json::to_vec(&no_pin).unwrap()),
            BrowserErrorCode::Invalid,
        );
        let mut loopback_with_pin = serde_json::to_value(sample()).unwrap();
        loopback_with_pin["publicIpv4Pin"] = serde_json::json!("93.184.216.34");
        rejects(
            decode_scope(&serde_json::to_vec(&loopback_with_pin).unwrap()),
            BrowserErrorCode::Invalid,
        );
        for origin in ["http://127.0.0.1:41873", "https://m63-browser.anima.tools"] {
            let mut value = if origin.starts_with("http://") {
                serde_json::to_value(sample()).unwrap()
            } else {
                public_scope_json(origin, "93.184.216.34")
            };
            value["publicIpv4Pin"] = serde_json::Value::Null;
            rejects(
                decode_scope(&serde_json::to_vec(&value).unwrap()),
                BrowserErrorCode::Invalid,
            );
        }
        for hostname in [
            "https://M63-browser.anima.tools",
            "https://m63-browser.anima.tools:443",
            "https://m63-browser.anima.tools/",
            "https://m63-browser.anima.tools?q=1",
            "https://m63-browser.anima.tools#frag",
            "https://user@m63-browser.anima.tools",
            "https://m63-browser.anima.tools.",
            "https://m63--browser..anima.tools",
            "https://-browser.anima.tools",
            "https://browser-.anima.tools",
            "https://browser.anima.123",
            "https://browser.0x7f",
            "https://browser.0x",
            "https://xn--a.tools",
            "https://xn--bcher-kva.example.com",
            "https://browser_anima.tools",
            "https://browser.%74ools",
            "https://browser.anima.tools\n",
            "https://127.0.0.1",
            "https://[::1]",
            "https://browser.localhost",
            "https://browser.local",
            "https://browser.internal",
            "https://browser.lan",
            "https://browser.home",
            "https://browser.onion",
            "https://browser.test",
            "https://browser.invalid",
            "https://browser.example",
            "https://browser.arpa",
            "https://bröwser.anima.tools",
        ] {
            let value = public_scope_json(hostname, "93.184.216.34");
            rejects(
                decode_scope(&serde_json::to_vec(&value).unwrap()),
                BrowserErrorCode::Invalid,
            );
        }
        for hostname in [
            format!("https://{}.anima.tools", "a".repeat(64)),
            format!(
                "https://{}.{}.{}.{}.com",
                "a".repeat(63),
                "b".repeat(63),
                "c".repeat(63),
                "d".repeat(63)
            ),
        ] {
            let value = public_scope_json(&hostname, "93.184.216.34");
            rejects(
                decode_scope(&serde_json::to_vec(&value).unwrap()),
                BrowserErrorCode::Invalid,
            );
        }
        let mut post = public_scope_json("https://m63-browser.anima.tools", "93.184.216.34");
        post["routes"][1]["method"] = serde_json::json!("POST");
        rejects(
            decode_scope(&serde_json::to_vec(&post).unwrap()),
            BrowserErrorCode::Invalid,
        );
        let mut inputs = public_scope_json("https://m63-browser.anima.tools", "93.184.216.34");
        inputs["inputIds"] = serde_json::json!(["owned-input"]);
        rejects(
            decode_scope(&serde_json::to_vec(&inputs).unwrap()),
            BrowserErrorCode::Invalid,
        );
        let mut unknown = public_scope_json("https://m63-browser.anima.tools", "93.184.216.34");
        unknown["operatorMayRetry"] = serde_json::json!(true);
        rejects(
            decode_scope(&serde_json::to_vec(&unknown).unwrap()),
            BrowserErrorCode::Invalid,
        );
    }

    #[test]
    fn m63_public_pin_rejects_noncanonical_and_all_bound_special_use_classes() {
        for pin in [
            "93.184.216.34",
            "9.9.9.9",
            "1.0.0.0",
            "9.255.255.255",
            "11.0.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "126.255.255.255",
            "128.0.0.0",
            "169.253.255.255",
            "169.255.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "191.255.255.255",
            "192.0.1.0",
            "192.0.1.255",
            "192.0.3.0",
            "192.31.195.255",
            "192.31.197.0",
            "192.52.192.255",
            "192.52.194.0",
            "192.88.98.255",
            "192.88.100.0",
            "192.167.255.255",
            "192.169.0.0",
            "192.175.47.255",
            "192.175.49.0",
            "198.17.255.255",
            "198.20.0.0",
            "198.51.99.255",
            "198.51.101.0",
            "203.0.112.255",
            "203.0.114.0",
            "223.255.255.255",
            "168.63.129.15",
            "168.63.129.17",
        ] {
            let value = public_scope_json("https://m63-browser.anima.tools", pin);
            decode_scope(&serde_json::to_vec(&value).unwrap())
                .expect("canonical non-special-use IPv4 pin must be admitted");
        }
        for pin in [
            "93.184.216.034",
            "93.184.216",
            "0x5db8d822",
            "+93.184.216.34",
            " 93.184.216.34",
            "93.184.216.34 ",
            "::ffff:93.184.216.34",
            "[::ffff:93.184.216.34]",
            "93.184.216.256",
            "93.184.216.-1",
            "",
            "0.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "168.63.129.16",
            "169.254.169.254",
            "172.16.0.1",
            "192.0.0.9",
            "192.0.2.1",
            "192.31.196.1",
            "192.52.193.1",
            "192.88.99.1",
            "192.168.1.1",
            "192.175.48.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "239.255.255.255",
            "240.0.0.1",
            "255.255.255.255",
            "0.0.0.0",
            "0.255.255.255",
            "10.0.0.0",
            "10.255.255.255",
            "100.64.0.0",
            "100.127.255.255",
            "127.0.0.0",
            "127.255.255.255",
            "169.254.0.0",
            "169.254.255.255",
            "172.16.0.0",
            "172.31.255.255",
            "192.0.0.0",
            "192.0.0.255",
            "192.0.2.0",
            "192.0.2.255",
            "192.31.196.0",
            "192.31.196.255",
            "192.52.193.0",
            "192.52.193.255",
            "192.88.99.0",
            "192.88.99.255",
            "192.168.0.0",
            "192.168.255.255",
            "192.175.48.0",
            "192.175.48.255",
            "198.18.0.0",
            "198.19.255.255",
            "198.51.100.0",
            "198.51.100.255",
            "203.0.113.0",
            "203.0.113.255",
            "224.0.0.0",
            "240.0.0.0",
        ] {
            let value = public_scope_json("https://m63-browser.anima.tools", pin);
            rejects(
                decode_scope(&serde_json::to_vec(&value).unwrap()),
                BrowserErrorCode::Invalid,
            );
        }
        let mut non_string = public_scope_json("https://m63-browser.anima.tools", "93.184.216.34");
        non_string["publicIpv4Pin"] = serde_json::json!(934);
        rejects(
            decode_scope(&serde_json::to_vec(&non_string).unwrap()),
            BrowserErrorCode::Invalid,
        );
    }

    #[test]
    fn m46_scope_uniqueness_ids_and_js_safe_revisions_are_enforced() {
        let mut scope = sample();
        scope.revision = MAX_BROWSER_SAFE_INTEGER;
        validate_scope(&scope).unwrap();
        for revision in [0, MAX_BROWSER_SAFE_INTEGER + 1, u64::MAX] {
            scope.revision = revision;
            rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        }
        for id in ["", "space id", "../id", "id:selector", "id\n", "π"] {
            let mut scope = sample();
            scope.id = id.into();
            rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        }
        let mut scope = sample();
        scope.id = "a".repeat(64);
        validate_scope(&scope).unwrap();
        scope.id.push('a');
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        let mut scope = sample();
        scope.routes[1].id = scope.routes[0].id.clone();
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        let mut scope = sample();
        scope.routes[1].method = scope.routes[0].method;
        scope.routes[1].path = scope.routes[0].path.clone();
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        let mut scope = sample();
        scope.input_ids.push(scope.input_ids[0].clone());
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        let mut scope = sample();
        scope.input_ids.clear();
        validate_scope(&scope).unwrap();
        scope.routes.clear();
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        let mut scope = sample();
        scope.routes = (0..17)
            .map(|i| BrowserRoute {
                id: format!("route_{i}"),
                method: BrowserMethod::Get,
                path: format!("/route_{i}"),
            })
            .collect();
        rejects(validate_scope(&scope), BrowserErrorCode::LimitsExceeded);
        let mut scope = sample();
        scope.input_ids = (0..17).map(|i| format!("input_{i}")).collect();
        rejects(validate_scope(&scope), BrowserErrorCode::LimitsExceeded);
    }

    #[test]
    fn m46_scope_limits_and_tighter_frames_are_enforced() {
        let mutations: [fn(&mut BrowserLimits); 10] = [
            |l| l.actions = 41,
            |l| l.requests = 81,
            |l| l.scope_ms = 120_001,
            |l| l.frame_bytes = 65_537,
            |l| l.wire_bytes = 1_048_577,
            |l| l.observation_nodes = 129,
            |l| l.observation_text_bytes = 8193,
            |l| l.input_bytes = 16_385,
            |l| l.output_bytes = 16_385,
            |l| l.screenshot_bytes = 262_145,
        ];
        for mutate in mutations {
            let mut scope = sample();
            mutate(&mut scope.limits);
            rejects(validate_scope(&scope), BrowserErrorCode::LimitsExceeded);
        }
        let mut scope = sample();
        scope.limits.actions = 0;
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        let mut scope = sample();
        scope.limits.wire_bytes = scope.limits.frame_bytes - 1;
        rejects(validate_scope(&scope), BrowserErrorCode::Invalid);
        let mut scope = sample();
        scope.limits.frame_bytes = 1;
        rejects(validate_scope(&scope), BrowserErrorCode::LimitsExceeded);
        let mut scope = sample();
        scope.limits.frame_bytes = 1024;
        validate_scope(&scope).unwrap();
        rejects(
            validate_operation(
                &scope,
                &BrowserOperation::Type {
                    node: node(),
                    text: "a".repeat(1024),
                },
            ),
            BrowserErrorCode::LimitsExceeded,
        );
        let mut padded = serde_json::to_vec(&scope).unwrap();
        padded.resize(1025, b' ');
        rejects(decode_scope(&padded), BrowserErrorCode::LimitsExceeded);
    }

    #[test]
    fn m46_decoders_reject_recursive_duplicates_unknown_null_and_mixed_variants() {
        let scope = sample();
        let source = serde_json::to_string(&scope).unwrap();
        for (from, to) in [
            ("\"revision\":1", "\"revision\":1,\"revision\":1"),
            (
                "\"method\":\"GET\"",
                "\"method\":\"GET\",\"m\\u0065thod\":\"GET\"",
            ),
            ("\"actions\":40", "\"actions\":40,\"actions\":40"),
        ] {
            let duplicate = source.replace(from, to);
            assert_ne!(duplicate, source);
            rejects(
                decode_scope(duplicate.as_bytes()),
                BrowserErrorCode::Invalid,
            );
        }
        for pointer in [
            "/revision",
            "/routes/0/method",
            "/inputIds/0",
            "/limits/actions",
        ] {
            let mut value = serde_json::to_value(&scope).unwrap();
            *value.pointer_mut(pointer).unwrap() = Value::Null;
            rejects(
                decode_scope(&serde_json::to_vec(&value).unwrap()),
                BrowserErrorCode::Invalid,
            );
        }
        for pointer in ["", "/routes/0", "/limits"] {
            let mut value = serde_json::to_value(&scope).unwrap();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(
                    "hostPath".into(),
                    Value::String("synthetic-private-path".into()),
                );
            rejects(
                decode_scope(&serde_json::to_vec(&value).unwrap()),
                BrowserErrorCode::Invalid,
            );
        }
        for raw in [
            r#"{"kind":"click","node":{"token":"node_1","t\u006fken":"node_2"}}"#,
            r#"{"kind":"observe","kind":"observe","screenshot":false}"#,
            r#"{"kind":"observe","screenshot":false,"node":{"token":"node_1"}}"#,
            r#"{"kind":"type","node":{"token":"node_1"},"text":null}"#,
            r#"{"kind":"click","node":{"token":"node_1","selector":"body"}}"#,
            r#"{"kind":"evaluate","script":"synthetic"}"#,
        ] {
            rejects(
                decode_operation(&scope, raw.as_bytes()),
                BrowserErrorCode::Invalid,
            );
        }
    }

    #[test]
    fn m46_decoders_enforce_utf8_root_trailing_depth_nodes_and_raw_frame() {
        rejects(decode_scope(&[0xff]), BrowserErrorCode::Invalid);
        for raw in [b"[]".as_slice(), b"null", b"{} true", b"{\"unknown\":null}"] {
            rejects(decode_scope(raw), BrowserErrorCode::Invalid);
        }
        let deep = format!("{{\"unknown\":{}0{}}}", "[".repeat(8), "]".repeat(8));
        rejects(
            decode_scope(deep.as_bytes()),
            BrowserErrorCode::LimitsExceeded,
        );
        let many = serde_json::json!({"unknown":vec![false; 2048]});
        rejects(
            decode_scope(&serde_json::to_vec(&many).unwrap()),
            BrowserErrorCode::LimitsExceeded,
        );
        let mut boundary = serde_json::to_vec(&sample()).unwrap();
        boundary.resize(MAX_BROWSER_FRAME_BYTES, b' ');
        decode_scope(&boundary).unwrap();
        boundary.push(b' ');
        rejects(decode_scope(&boundary), BrowserErrorCode::LimitsExceeded);
        let mut unsafe_revision = serde_json::to_value(sample()).unwrap();
        unsafe_revision["revision"] = Value::from(MAX_BROWSER_SAFE_INTEGER + 1);
        rejects(
            decode_scope(&serde_json::to_vec(&unsafe_revision).unwrap()),
            BrowserErrorCode::Invalid,
        );
    }

    #[test]
    fn m46_observation_bounds_shortening_nodes_and_artifacts_are_enforced() {
        let scope = sample();
        validate_observation(&scope, &observation()).unwrap();
        let mut view = observation();
        view.document_generation = MAX_BROWSER_SAFE_INTEGER;
        validate_observation(&scope, &view).unwrap();
        for generation in [0, MAX_BROWSER_SAFE_INTEGER + 1] {
            view.document_generation = generation;
            rejects(
                validate_observation(&scope, &view),
                BrowserErrorCode::Invalid,
            );
        }
        let mut view = observation();
        view.shortened = true;
        view.omitted_nodes = 1;
        validate_observation(&scope, &view).unwrap();
        view.omitted_nodes = 0;
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::Invalid,
        );
        view.shortened = false;
        view.omitted_nodes = 1;
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::Invalid,
        );
        let mut view = observation();
        view.nodes.push(view.nodes[0].clone());
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::Invalid,
        );
        let mut view = observation();
        view.nodes[0].label = "π".repeat(4096);
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::LimitsExceeded,
        );
        view.nodes[0].label = "a".repeat(8192 - 6);
        validate_observation(&scope, &view).unwrap();
        let mut view = observation();
        view.nodes = (0..128)
            .map(|i| BrowserNode {
                node: BrowserNodeRef {
                    token: format!("node_{i}"),
                },
                role: "button".into(),
                label: "".into(),
            })
            .collect();
        validate_observation(&scope, &view).unwrap();
        view.nodes.push(BrowserNode {
            node: node(),
            role: "button".into(),
            label: "".into(),
        });
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::LimitsExceeded,
        );
        let mut view = observation();
        view.route_id = "foreign".into();
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::Denied,
        );
        for kind in [BrowserArtifactKind::Screenshot, BrowserArtifactKind::Csv] {
            let mut view = observation();
            let artifact = view.artifacts.iter_mut().find(|a| a.kind == kind).unwrap();
            artifact.bytes += 1;
            rejects(
                validate_observation(&scope, &view),
                BrowserErrorCode::LimitsExceeded,
            );
        }
        for hash in ["A".repeat(64), "g".repeat(64), "a".repeat(63)] {
            let mut view = observation();
            view.artifacts[0].sha256 = hash;
            rejects(
                validate_observation(&scope, &view),
                BrowserErrorCode::Invalid,
            );
        }
        let mut view = observation();
        view.artifacts[0].bytes = 0;
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::Invalid,
        );
        let mut view = observation();
        view.artifacts[1].id = view.artifacts[0].id.clone();
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::Invalid,
        );
        let mut view = observation();
        view.artifacts[1].kind = BrowserArtifactKind::Screenshot;
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::Invalid,
        );
        let mut view = observation();
        view.artifacts.push(view.artifacts[0].clone());
        rejects(
            validate_observation(&scope, &view),
            BrowserErrorCode::LimitsExceeded,
        );
    }

    #[test]
    fn m46_observation_decoder_rejects_nested_malformed_and_admits_valid_metadata() {
        let scope = sample();
        let view = observation();
        assert_eq!(
            decode_observation(&scope, &serde_json::to_vec(&view).unwrap()).unwrap(),
            view
        );
        let source = serde_json::to_string(&view).unwrap();
        let duplicate = source.replace(
            "\"role\":\"button\"",
            "\"role\":\"button\",\"role\":\"button\"",
        );
        assert_ne!(duplicate, source);
        rejects(
            decode_observation(&scope, duplicate.as_bytes()),
            BrowserErrorCode::Invalid,
        );
        let mut value = serde_json::to_value(&view).unwrap();
        value["nodes"][0]["node"]["token"] = Value::Null;
        rejects(
            decode_observation(&scope, &serde_json::to_vec(&value).unwrap()),
            BrowserErrorCode::Invalid,
        );
        let mut value = serde_json::to_value(&view).unwrap();
        value["artifacts"][0]["base64"] = Value::String("synthetic".into());
        rejects(
            decode_observation(&scope, &serde_json::to_vec(&value).unwrap()),
            BrowserErrorCode::Invalid,
        );
        let mut view = view;
        view.artifacts[0].bytes = u64::MAX;
        rejects(
            decode_observation(&scope, &serde_json::to_vec(&view).unwrap()),
            BrowserErrorCode::LimitsExceeded,
        );
    }

    #[test]
    fn m46_jcs_digests_are_static_stable_and_change_with_content() {
        let observe = BrowserOperation::Observe { screenshot: false };
        assert_eq!(
            operation_digest(&observe).unwrap(),
            "3d8917e55df5c45d96407b638e07a2ed462e99e23af2680f7e14ec896a9b6faf"
        );
        let scope = sample();
        let hash = scope_digest(&scope).unwrap();
        let value = serde_json::to_value(&scope).unwrap();
        let reordered = format!(
            "{{{}}}",
            value
                .as_object()
                .unwrap()
                .iter()
                .rev()
                .map(|(key, value)| format!("{}:{}", serde_json::to_string(key).unwrap(), value))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            scope_digest(&decode_scope(reordered.as_bytes()).unwrap()).unwrap(),
            hash
        );
        let mut scope = scope;
        scope.routes.reverse();
        assert_ne!(scope_digest(&scope).unwrap(), hash);
        let mut with_inputs = sample();
        with_inputs.input_ids.push("request-csv_2".into());
        let ordered = scope_digest(&with_inputs).unwrap();
        with_inputs.input_ids.reverse();
        assert_ne!(scope_digest(&with_inputs).unwrap(), ordered);
        let empty = BrowserOperation::Type {
            node: node(),
            text: "".into(),
        };
        let space = BrowserOperation::Type {
            node: node(),
            text: " ".into(),
        };
        assert_ne!(
            operation_digest(&empty).unwrap(),
            operation_digest(&space).unwrap()
        );
        let original = BrowserOperation::Type {
            node: node(),
            text: "π".into(),
        };
        let literal = decode_operation(
            &sample(),
            br#"{"text":"\u03c0","node":{"token":"observed-node_1"},"kind":"type"}"#,
        )
        .unwrap();
        assert_eq!(
            operation_digest(&original).unwrap(),
            operation_digest(&literal).unwrap()
        );
    }

    #[test]
    fn m46_static_errors_and_shape_digest_grant_no_scope_authority() {
        let foreign = BrowserOperation::Navigate {
            route_id: "foreign".into(),
        };
        operation_digest(&foreign).unwrap(); // Shape identity deliberately has no scope parameter.
        rejects(
            validate_operation(&sample(), &foreign),
            BrowserErrorCode::Denied,
        );
        let error = decode_scope(br#"{"private-sentinel":"not-a-scope"}"#).unwrap_err();
        assert_eq!(error.to_string(), "Browser input is invalid");
        assert!(!error.to_string().contains("private-sentinel"));
        let uncertain = BrowserError {
            code: BrowserErrorCode::Cancelled,
            effect: BrowserEffect::MayHaveOccurred,
        };
        assert_eq!(
            uncertain.to_string(),
            "Browser action failed; effects may have occurred"
        );
    }
}
