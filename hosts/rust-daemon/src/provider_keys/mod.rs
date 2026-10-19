//! A file-backed pool of API keys per provider, with Hermes-style rotation:
//! a key that fails for a key-specific reason rests for a while and the call
//! moves on to the next ready key.
//!
//! The pool only decides which key to use and remembers how keys behaved. The
//! env key (from `provider_config_from_env`) is always key `env`, listed first,
//! and never written to the file. Keys are never logged; views carry a masked
//! form only.
mod boot;
mod store;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use anima_core::{ModelFailure, ModelFailureKind};
use anima_model_adapters::provider_definitions;
use serde::{Deserialize, Serialize};

pub(crate) use boot::open_from_env;
use store::{StoredFile, StoredKey, StoredProvider, FILE_VERSION};

pub(crate) const ENV_KEY_ID: &str = "env";
const MAX_KEY_CHARS: usize = 2_000;
const MAX_LABEL_CHARS: usize = 80;

const AUTH_REST_S: u64 = 300;
const KEY_FAILURE_REST_S: u64 = 3_600;
const SOLE_KEY_REST_S: u64 = 60;
const MAX_REST_S: u64 = 24 * 3_600;

/// How a provider's ready keys are chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Strategy {
    #[default]
    FillFirst,
    RoundRobin,
    Random,
    LeastUsed,
}

impl Strategy {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "fill_first" => Some(Self::FillFirst),
            "round_robin" => Some(Self::RoundRobin),
            "random" => Some(Self::Random),
            "least_used" => Some(Self::LeastUsed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeySource {
    Env,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum KeyError {
    /// The provider or key id is not known.
    Unknown,
    Empty,
    TooLong,
    Duplicate,
    /// The env key can only be changed in the environment.
    EnvKey,
    #[allow(dead_code)] // Never constructed: without a key file keys stay in memory.
    NotPersisted,
    Io(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyOutcome {
    Ok,
    Failed(ModelFailure),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PickedKey {
    pub id: String,
    pub key: String,
}

impl std::fmt::Debug for PickedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickedKey")
            .field("id", &self.id)
            .field("key", &mask_key(&self.key))
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyView {
    pub id: String,
    pub label: Option<String>,
    pub masked: String,
    pub source: KeySource,
    /// `"ready"` or `"resting"`.
    pub status: &'static str,
    pub resting_until_ms: Option<u64>,
    pub last_error: Option<String>,
    pub uses: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderKeysView {
    pub id: String,
    pub strategy: Strategy,
    /// False when there is no key file, so added keys last only until restart.
    pub persisted: bool,
    pub keys: Vec<KeyView>,
}

#[derive(Clone)]
struct KeyEntry {
    id: String,
    label: Option<String>,
    key: String,
    source: KeySource,
    added_at_ms: u64,
    uses: u64,
    resting_until_ms: Option<u64>,
    last_error: Option<String>,
}

impl std::fmt::Debug for KeyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyEntry")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("key", &mask_key(&self.key))
            .field("source", &self.source)
            .field("uses", &self.uses)
            .field("resting_until_ms", &self.resting_until_ms)
            .finish()
    }
}

impl KeyEntry {
    fn is_ready(&self, now_ms: u64) -> bool {
        self.resting_until_ms.is_none_or(|until| until <= now_ms)
    }
}

#[derive(Debug, Default)]
struct ProviderState {
    strategy: Strategy,
    keys: Vec<KeyEntry>,
    /// The key `round_robin` handed out last.
    last_picked: Option<String>,
}

#[derive(Debug)]
struct Inner {
    path: Option<PathBuf>,
    providers: BTreeMap<String, ProviderState>,
    /// xorshift state for `random`; seeded from the first pick's clock.
    rng: u64,
}

/// The shared key pool. Cloning shares the same state.
#[derive(Clone)]
pub(crate) struct KeyPool {
    inner: Arc<Mutex<Inner>>,
}

/// Providers and key counts only; keys never appear in debug output.
impl std::fmt::Debug for KeyPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("KeyPool");
        match self.inner.try_lock() {
            Ok(inner) => {
                let counts: BTreeMap<&String, usize> = inner
                    .providers
                    .iter()
                    .map(|(id, state)| (id, state.keys.len()))
                    .collect();
                out.field("persisted", &inner.path.is_some())
                    .field("keys_per_provider", &counts)
            }
            Err(_) => out.field("state", &"locked"),
        }
        .finish()
    }
}

impl KeyPool {
    /// Opens the pool. `env` maps provider id to the key read from the environment.
    /// A malformed key file is an error rather than being overwritten.
    pub(crate) fn open(
        path: Option<PathBuf>,
        env: &BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let mut providers: BTreeMap<String, ProviderState> = BTreeMap::new();
        if let Some(path) = &path {
            for (provider, stored) in store::load(path)?.providers {
                let keys = stored
                    .keys
                    .into_iter()
                    .map(|key| KeyEntry {
                        id: key.id,
                        label: key.label,
                        key: key.key,
                        source: KeySource::File,
                        added_at_ms: key.added_at_ms,
                        uses: key.uses,
                        resting_until_ms: key.resting_until_ms,
                        last_error: key.last_error,
                    })
                    .collect();
                providers.insert(
                    provider,
                    ProviderState {
                        strategy: stored.strategy,
                        keys,
                        last_picked: None,
                    },
                );
            }
        }
        for (provider, key) in env {
            if key.trim().is_empty() {
                continue;
            }
            providers.entry(provider.clone()).or_default().keys.insert(
                0,
                KeyEntry {
                    id: ENV_KEY_ID.to_owned(),
                    label: None,
                    key: key.clone(),
                    source: KeySource::Env,
                    added_at_ms: 0,
                    uses: 0,
                    resting_until_ms: None,
                    last_error: None,
                },
            );
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                path,
                providers,
                rng: 0,
            })),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Chooses a ready key for `provider` by its strategy and counts the use.
    /// `None` when the provider has no key or every key is resting.
    pub(crate) fn pick(&self, provider: &str, now_ms: u64) -> Option<PickedKey> {
        self.pick_untried(provider, &[], now_ms)
    }

    /// Like `pick`, never choosing a key in `tried`: a call that rotated past a
    /// key does not get it back after its short rest ends mid-call.
    pub(crate) fn pick_untried(
        &self,
        provider: &str,
        tried: &[String],
        now_ms: u64,
    ) -> Option<PickedKey> {
        let mut inner = self.lock();
        if inner.rng == 0 {
            inner.rng = now_ms | 1;
        }
        let random = next_random(&mut inner.rng);
        let state = inner.providers.get_mut(provider)?;
        let ready: Vec<usize> = (0..state.keys.len())
            .filter(|&index| {
                state.keys[index].is_ready(now_ms) && !tried.contains(&state.keys[index].id)
            })
            .collect();
        let index = match state.strategy {
            _ if ready.is_empty() => return None,
            Strategy::FillFirst => ready[0],
            Strategy::RoundRobin => {
                let after = state
                    .last_picked
                    .as_ref()
                    .and_then(|id| state.keys.iter().position(|key| &key.id == id));
                after
                    .and_then(|after| ready.iter().copied().find(|&index| index > after))
                    .unwrap_or(ready[0])
            }
            Strategy::Random => ready[(random % ready.len() as u64) as usize],
            Strategy::LeastUsed => *ready
                .iter()
                .min_by_key(|&&index| state.keys[index].uses)
                .expect("ready is not empty"),
        };
        let entry = &mut state.keys[index];
        entry.uses = entry.uses.saturating_add(1);
        let picked = PickedKey {
            id: entry.id.clone(),
            key: entry.key.clone(),
        };
        state.last_picked = Some(picked.id.clone());
        Some(picked)
    }

    /// Whether the pool holds any key (env or file, ready or resting) for
    /// `provider`.
    pub(crate) fn holds_keys(&self, provider: &str) -> bool {
        self.lock()
            .providers
            .get(provider)
            .is_some_and(|state| !state.keys.is_empty())
    }

    /// When every key held for `provider` is resting: the cause most of them
    /// rest for (ties go to the key that wakes first) and when the first one is
    /// ready again. `None` when some key is ready or there is no key.
    pub(crate) fn resting_state(
        &self,
        provider: &str,
        now_ms: u64,
    ) -> Option<(ModelFailureKind, u64)> {
        let inner = self.lock();
        let keys = &inner.providers.get(provider)?.keys;
        if keys.iter().any(|key| key.is_ready(now_ms)) {
            return None;
        }
        let first = keys
            .iter()
            .min_by_key(|key| key.resting_until_ms.unwrap_or(u64::MAX))?;
        let ready_at_ms = first.resting_until_ms?;
        let first_kind = rest_cause(first);
        let count =
            |kind: ModelFailureKind| keys.iter().filter(|key| rest_cause(key) == kind).count();
        let kind = [
            ModelFailureKind::RateLimited,
            ModelFailureKind::Billing,
            ModelFailureKind::AuthFailed,
        ]
        .into_iter()
        .max_by_key(|&kind| (count(kind), kind == first_kind))?;
        Some((kind, ready_at_ms))
    }

    /// Records how a call made with `key_id` went. Key failures rest the key
    /// (never for less than it already rests); other failures leave it ready.
    /// Resting keys are never woken here.
    pub(crate) fn report(&self, provider: &str, key_id: &str, outcome: KeyOutcome, now_ms: u64) {
        let mut inner = self.lock();
        let Some(state) = inner.providers.get_mut(provider) else {
            return;
        };
        let Some(position) = state.keys.iter().position(|key| key.id == key_id) else {
            return;
        };
        match outcome {
            KeyOutcome::Ok => {
                let entry = &mut state.keys[position];
                if entry.is_ready(now_ms) {
                    entry.resting_until_ms = None;
                    entry.last_error = None;
                }
            }
            KeyOutcome::Failed(failure) => {
                if !failure.kind.rotates_key() {
                    return;
                }
                let sole = !state
                    .keys
                    .iter()
                    .enumerate()
                    .any(|(index, key)| index != position && key.is_ready(now_ms));
                let rest_s = rest_seconds(failure, sole);
                let until = now_ms.saturating_add(rest_s.saturating_mul(1_000));
                let entry = &mut state.keys[position];
                // A late failure from a call that picked the key before it
                // rested only ever extends the rest, and keeps its cause.
                if entry
                    .resting_until_ms
                    .is_some_and(|current| current > now_ms && current >= until)
                {
                    return;
                }
                entry.resting_until_ms = Some(until);
                entry.last_error = Some(describe_failure(failure.kind).to_owned());
                if entry.source == KeySource::File {
                    inner.save_logged();
                }
            }
        }
    }

    /// Ends a key's rest now ("Try now"), the env key included, for when the
    /// owner fixed what it rested for. A failed save is logged; the key stays
    /// awake until restart.
    pub(crate) fn wake(
        &self,
        provider: &str,
        key_id: &str,
        now_ms: u64,
    ) -> Result<KeyView, KeyError> {
        let mut inner = self.lock();
        let entry = inner
            .providers
            .get_mut(provider)
            .and_then(|state| state.keys.iter_mut().find(|key| key.id == key_id))
            .ok_or(KeyError::Unknown)?;
        entry.resting_until_ms = None;
        entry.last_error = None;
        let (view, saved) = (entry.view(now_ms), entry.source == KeySource::File);
        if saved {
            inner.save_logged();
        }
        Ok(view)
    }

    /// Adds a key to `provider`. Without a key file it lasts until restart.
    pub(crate) fn add(
        &self,
        provider: &str,
        label: Option<String>,
        key: String,
        now_ms: u64,
    ) -> Result<KeyView, KeyError> {
        let key = key.trim().to_owned();
        if key.is_empty() {
            return Err(KeyError::Empty);
        }
        if key.chars().count() > MAX_KEY_CHARS {
            return Err(KeyError::TooLong);
        }
        let label = label
            .map(|label| {
                label
                    .trim()
                    .chars()
                    .take(MAX_LABEL_CHARS)
                    .collect::<String>()
            })
            .filter(|label| !label.is_empty());
        let mut inner = self.lock();
        if !inner.providers.contains_key(provider) && !is_known_provider(provider) {
            return Err(KeyError::Unknown);
        }
        let state = inner.providers.entry(provider.to_owned()).or_default();
        if state.keys.iter().any(|existing| existing.key == key) {
            return Err(KeyError::Duplicate);
        }
        let entry = KeyEntry {
            id: new_key_id(),
            label,
            key,
            source: KeySource::File,
            added_at_ms: now_ms,
            uses: 0,
            resting_until_ms: None,
            last_error: None,
        };
        let view = entry.view(now_ms);
        state.keys.push(entry);
        if let Err(error) = inner.save() {
            if let Some(state) = inner.providers.get_mut(provider) {
                state.keys.pop();
            }
            return Err(KeyError::Io(error));
        }
        Ok(view)
    }

    /// Removes a file key. The env key cannot be removed.
    pub(crate) fn remove(&self, provider: &str, key_id: &str) -> Result<(), KeyError> {
        if key_id == ENV_KEY_ID {
            return Err(KeyError::EnvKey);
        }
        let mut inner = self.lock();
        let state = inner.providers.get_mut(provider).ok_or(KeyError::Unknown)?;
        let position = state
            .keys
            .iter()
            .position(|key| key.id == key_id)
            .ok_or(KeyError::Unknown)?;
        let removed = state.keys.remove(position);
        if let Err(error) = inner.save() {
            if let Some(state) = inner.providers.get_mut(provider) {
                state.keys.insert(position.min(state.keys.len()), removed);
            }
            return Err(KeyError::Io(error));
        }
        Ok(())
    }

    /// Sets the provider's strategy; `Unknown` for a provider that takes no
    /// key. A failed save is logged; the change still holds until restart.
    pub(crate) fn set_strategy(&self, provider: &str, strategy: Strategy) -> Result<(), KeyError> {
        let mut inner = self.lock();
        if !inner.providers.contains_key(provider) && !is_known_provider(provider) {
            return Err(KeyError::Unknown);
        }
        inner
            .providers
            .entry(provider.to_owned())
            .or_default()
            .strategy = strategy;
        inner.save_logged();
        Ok(())
    }

    /// Every provider that takes a key, plus any other with keys, by id.
    pub(crate) fn list(&self, now_ms: u64) -> Vec<ProviderKeysView> {
        let inner = self.lock();
        let persisted = inner.path.is_some();
        let mut ids: Vec<String> = provider_definitions()
            .iter()
            .filter(|definition| definition.requires_key)
            .map(|definition| definition.id.to_owned())
            .collect();
        ids.extend(inner.providers.keys().cloned());
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .map(|id| {
                let state = inner.providers.get(&id);
                ProviderKeysView {
                    strategy: state.map(|state| state.strategy).unwrap_or_default(),
                    persisted,
                    keys: state
                        .map(|state| state.keys.iter().map(|key| key.view(now_ms)).collect())
                        .unwrap_or_default(),
                    id,
                }
            })
            .collect()
    }
}

impl Inner {
    /// Writes the file keys; a no-op without a key file.
    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let providers = self
            .providers
            .iter()
            .filter_map(|(id, state)| {
                let keys: Vec<StoredKey> = state
                    .keys
                    .iter()
                    .filter(|key| key.source == KeySource::File)
                    .map(|key| StoredKey {
                        id: key.id.clone(),
                        label: key.label.clone(),
                        key: key.key.clone(),
                        added_at_ms: key.added_at_ms,
                        uses: key.uses,
                        resting_until_ms: key.resting_until_ms,
                        last_error: key.last_error.clone(),
                    })
                    .collect();
                (!keys.is_empty() || state.strategy != Strategy::default()).then(|| {
                    (
                        id.clone(),
                        StoredProvider {
                            strategy: state.strategy,
                            keys,
                        },
                    )
                })
            })
            .collect();
        store::save(
            path,
            &StoredFile {
                version: FILE_VERSION,
                providers,
            },
        )
    }

    fn save_logged(&self) {
        if let Err(error) = self.save() {
            tracing::warn!(%error, "could not save the provider key file");
        }
    }
}

impl KeyEntry {
    fn view(&self, now_ms: u64) -> KeyView {
        let ready = self.is_ready(now_ms);
        KeyView {
            id: self.id.clone(),
            label: self.label.clone(),
            masked: mask_key(&self.key),
            source: self.source,
            status: if ready { "ready" } else { "resting" },
            resting_until_ms: if ready { None } else { self.resting_until_ms },
            last_error: self.last_error.clone(),
            uses: self.uses,
        }
    }
}

fn is_known_provider(id: &str) -> bool {
    provider_definitions()
        .iter()
        .any(|definition| definition.id == id)
}

fn new_key_id() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("key_{}", &id[..12])
}

/// How long a failed key rests, capped at a day. For a rate limit the
/// provider's own retry time wins (a billing or auth failure ignores it: the
/// hint is often a request-bucket reset, not when credit or the key returns);
/// billing rests an hour even when it is the only key; a rejected key (401)
/// rests 5 minutes even when it is the only key; any other failure on the only
/// usable key rests just a minute so the owner is not locked out; otherwise an
/// hour.
fn rest_seconds(failure: ModelFailure, sole_usable_key: bool) -> u64 {
    let retry_after = failure.retry_after_s.filter(|_| {
        matches!(
            failure.kind,
            ModelFailureKind::RateLimited | ModelFailureKind::Overloaded
        )
    });
    let seconds = if let Some(retry_after) = retry_after {
        retry_after.max(1)
    } else if failure.kind == ModelFailureKind::Billing {
        KEY_FAILURE_REST_S
    } else if failure.kind == ModelFailureKind::AuthFailed {
        AUTH_REST_S
    } else if sole_usable_key {
        SOLE_KEY_REST_S
    } else {
        KEY_FAILURE_REST_S
    };
    seconds.min(MAX_REST_S)
}

fn describe_failure(kind: ModelFailureKind) -> &'static str {
    match kind {
        ModelFailureKind::RateLimited => "rate limited",
        ModelFailureKind::AuthFailed => "not accepting the key",
        ModelFailureKind::Billing => "out of credit",
        _ => "failed",
    }
}

/// Why a resting key rests, read back from its recorded (and saved) error.
fn rest_cause(key: &KeyEntry) -> ModelFailureKind {
    match key.last_error.as_deref() {
        Some(text) if text == describe_failure(ModelFailureKind::Billing) => {
            ModelFailureKind::Billing
        }
        Some(text) if text == describe_failure(ModelFailureKind::AuthFailed) => {
            ModelFailureKind::AuthFailed
        }
        _ => ModelFailureKind::RateLimited,
    }
}

/// `prefix…last4` for keys of 20 characters or more; shorter keys show no
/// prefix and at most their last 4 (a quarter of the key, at most 4).
pub(super) fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() >= 20 {
        let head: String = chars[..6].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}…{tail}")
    } else {
        let shown = (chars.len() / 4).min(4);
        let tail: String = chars[chars.len() - shown..].iter().collect();
        format!("…{tail}")
    }
}

fn next_random(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}
