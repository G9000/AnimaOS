use std::collections::BTreeMap;

use anima_core::{ModelFailure, ModelFailureKind};

use super::*;

const HOUR_MS: u64 = 3_600_000;

fn failure(kind: ModelFailureKind, status: Option<u16>, retry_after_s: Option<u64>) -> KeyOutcome {
    KeyOutcome::Failed(ModelFailure {
        kind,
        status,
        retry_after_s,
    })
}

fn memory_pool() -> KeyPool {
    KeyPool::open(None, &BTreeMap::new()).unwrap()
}

fn add(pool: &KeyPool, provider: &str, key: &str) -> KeyView {
    pool.add(provider, None, key.to_owned(), 0).unwrap()
}

fn keys_of(pool: &KeyPool, provider: &str, now_ms: u64) -> Vec<KeyView> {
    pool.list(now_ms)
        .into_iter()
        .find(|view| view.id == provider)
        .map(|view| view.keys)
        .unwrap_or_default()
}

fn resting_until(pool: &KeyPool, provider: &str, key_id: &str) -> Option<u64> {
    keys_of(pool, provider, 0)
        .into_iter()
        .find(|view| view.id == key_id)
        .and_then(|view| view.resting_until_ms)
}

#[test]
fn fill_first_uses_the_first_ready_key_and_skips_resting_ones() {
    let pool = memory_pool();
    let first = add(&pool, "openai", "sk-first-0000");
    let second = add(&pool, "openai", "sk-second-111");
    assert_eq!(pool.pick("openai", 1).unwrap().id, first.id);
    assert_eq!(pool.pick("openai", 2).unwrap().id, first.id);
    pool.report(
        "openai",
        &first.id,
        failure(ModelFailureKind::RateLimited, Some(429), None),
        3,
    );
    let picked = pool.pick("openai", 4).unwrap();
    assert_eq!(picked.id, second.id);
    assert_eq!(picked.key, "sk-second-111");
    pool.report(
        "openai",
        &second.id,
        failure(ModelFailureKind::RateLimited, Some(429), None),
        5,
    );
    assert!(pool.pick("openai", 6).is_none());
    assert!(pool.pick("anthropic", 6).is_none());
}

#[test]
fn a_strategy_needs_a_known_provider() {
    let pool = memory_pool();
    assert_eq!(
        pool.set_strategy("nope", Strategy::Random),
        Err(KeyError::Unknown)
    );
    assert!(pool.list(0).iter().all(|view| view.id != "nope"));
    assert_eq!(pool.set_strategy("openai", Strategy::Random), Ok(()));
}

#[test]
fn round_robin_cycles() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    let b = add(&pool, "openai", "sk-bbbb-1111");
    let c = add(&pool, "openai", "sk-cccc-2222");
    pool.set_strategy("openai", Strategy::RoundRobin).unwrap();
    let picked: Vec<String> = (0..6).map(|n| pool.pick("openai", n).unwrap().id).collect();
    assert_eq!(
        picked,
        [&a.id, &b.id, &c.id, &a.id, &b.id, &c.id].map(String::clone)
    );
    pool.report(
        "openai",
        &c.id,
        failure(ModelFailureKind::AuthFailed, Some(401), None),
        10,
    );
    let picked: Vec<String> = (0..4)
        .map(|n| pool.pick("openai", 11 + n).unwrap().id)
        .collect();
    assert_eq!(picked, [&a.id, &b.id, &a.id, &b.id].map(String::clone));
}

#[test]
fn least_used_prefers_the_key_with_fewest_uses() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    let b = add(&pool, "openai", "sk-bbbb-1111");
    pool.set_strategy("openai", Strategy::LeastUsed).unwrap();
    assert_eq!(pool.pick("openai", 1).unwrap().id, a.id);
    assert_eq!(pool.pick("openai", 2).unwrap().id, b.id);
    assert_eq!(pool.pick("openai", 3).unwrap().id, a.id);
    let uses: Vec<u64> = keys_of(&pool, "openai", 4).iter().map(|k| k.uses).collect();
    assert_eq!(uses, [2, 1]);
}

#[test]
fn random_picks_only_ready_keys() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    let b = add(&pool, "openai", "sk-bbbb-1111");
    pool.set_strategy("openai", Strategy::Random).unwrap();
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::Billing, Some(402), None),
        1,
    );
    for now in 2..20 {
        assert_eq!(pool.pick("openai", now).unwrap().id, b.id);
    }
    let pool = memory_pool();
    for n in 0..4 {
        add(&pool, "openai", &format!("sk-key-{n}-0000"));
    }
    pool.set_strategy("openai", Strategy::Random).unwrap();
    let distinct: std::collections::BTreeSet<String> = (0..64)
        .map(|n| pool.pick("openai", 1_000 + n * 7).unwrap().id)
        .collect();
    assert!(distinct.len() > 1, "random never varied");
}

#[test]
fn rest_times_follow_the_rules() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    let b = add(&pool, "openai", "sk-bbbb-1111");

    // 401 with another ready key -> 5 minutes
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::AuthFailed, Some(401), None),
        1_000,
    );
    assert_eq!(resting_until(&pool, "openai", &a.id), Some(1_000 + 300_000));

    // b is now the only usable key, so even a 429 rests it just 60 s
    pool.report(
        "openai",
        &b.id,
        failure(ModelFailureKind::RateLimited, Some(429), None),
        2_000,
    );
    assert_eq!(resting_until(&pool, "openai", &b.id), Some(2_000 + 60_000));

    // 429 with 2 keys -> 1 hour
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    add(&pool, "openai", "sk-bbbb-1111");
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::RateLimited, Some(429), None),
        5_000,
    );
    assert_eq!(resting_until(&pool, "openai", &a.id), Some(5_000 + HOUR_MS));

    // 429 on the only key -> 60 s
    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::RateLimited, Some(429), None),
        7_000,
    );
    assert_eq!(
        resting_until(&pool, "openai", &only.id),
        Some(7_000 + 60_000)
    );

    // billing on the only key -> 1 hour
    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::Billing, Some(402), None),
        9_000,
    );
    assert_eq!(
        resting_until(&pool, "openai", &only.id),
        Some(9_000 + HOUR_MS)
    );

    // 401 on the only key -> still 5 minutes
    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::AuthFailed, Some(401), None),
        8_000,
    );
    assert_eq!(
        resting_until(&pool, "openai", &only.id),
        Some(8_000 + 300_000)
    );

    // the provider's retry time wins for a rate limit
    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(12)),
        11_000,
    );
    assert_eq!(
        resting_until(&pool, "openai", &only.id),
        Some(11_000 + 12_000)
    );
}

#[test]
fn a_retry_hint_never_cuts_a_billing_or_auth_rest() {
    // A 429 insufficient_quota carries bucket-reset headers (~1 s); being out
    // of credit still rests an hour, and a rejected key five minutes.
    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::Billing, Some(429), Some(1)),
        1_000,
    );
    assert_eq!(
        resting_until(&pool, "openai", &only.id),
        Some(1_000 + HOUR_MS)
    );

    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::AuthFailed, Some(401), Some(1)),
        1_000,
    );
    assert_eq!(
        resting_until(&pool, "openai", &only.id),
        Some(1_000 + 300_000)
    );
}

#[test]
fn pick_untried_skips_tried_keys_even_when_one_woke_up() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    let b = add(&pool, "openai", "sk-bbbb-1111");
    let c = add(&pool, "openai", "sk-cccc-2222");
    // a rests one second, then b fails after a's rest is over.
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(1)),
        0,
    );
    pool.report(
        "openai",
        &b.id,
        failure(ModelFailureKind::RateLimited, Some(429), None),
        2_000,
    );
    // Fill-first would hand out a again.
    assert_eq!(pool.pick("openai", 2_000).unwrap().id, a.id);
    let tried = [a.id.clone(), b.id.clone()];
    assert_eq!(pool.pick_untried("openai", &tried, 2_000).unwrap().id, c.id);
    let tried = [a.id.clone(), b.id.clone(), c.id.clone()];
    assert_eq!(pool.pick_untried("openai", &tried, 2_000), None);
}

#[test]
fn resting_says_why_and_until_when() {
    let env = BTreeMap::from([("openai".to_owned(), "env-key-0000".to_owned())]);
    let pool = KeyPool::open(None, &env).unwrap();
    assert_eq!(pool.resting_state("openai", 0), None, "the key is ready");

    pool.report(
        "openai",
        ENV_KEY_ID,
        failure(ModelFailureKind::Billing, Some(402), None),
        60_000,
    );
    assert_eq!(
        pool.resting_state("openai", 120_000),
        Some((ModelFailureKind::Billing, 60_000 + HOUR_MS))
    );

    let pool = KeyPool::open(None, &env).unwrap();
    pool.report(
        "openai",
        ENV_KEY_ID,
        failure(ModelFailureKind::AuthFailed, Some(401), None),
        0,
    );
    assert_eq!(
        pool.resting_state("openai", 1_000),
        Some((ModelFailureKind::AuthFailed, 300_000))
    );
    assert_eq!(pool.resting_state("anthropic", 1_000), None);

    // Mixed causes: the cause most keys rest for wins.
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    let b = add(&pool, "openai", "sk-bbbb-1111");
    let c = add(&pool, "openai", "sk-cccc-2222");
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(30)),
        0,
    );
    pool.report(
        "openai",
        &b.id,
        failure(ModelFailureKind::Billing, Some(402), None),
        0,
    );
    pool.report(
        "openai",
        &c.id,
        failure(ModelFailureKind::Billing, Some(402), None),
        0,
    );
    assert_eq!(
        pool.resting_state("openai", 1_000),
        Some((ModelFailureKind::Billing, 30_000))
    );
}

#[test]
fn rest_end_saturates_instead_of_overflowing() {
    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(u64::MAX)),
        u64::MAX - 5,
    );
    assert_eq!(resting_until(&pool, "openai", &only.id), Some(u64::MAX));
}

#[test]
fn retry_after_is_capped_at_a_day() {
    let pool = memory_pool();
    let only = add(&pool, "openai", "sk-only-0000");
    pool.report(
        "openai",
        &only.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(u64::MAX)),
        1_000,
    );
    assert_eq!(
        resting_until(&pool, "openai", &only.id),
        Some(1_000 + 24 * HOUR_MS)
    );
}

#[test]
fn failures_that_do_not_rotate_keys_leave_the_key_ready() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    for kind in [
        ModelFailureKind::Overloaded,
        ModelFailureKind::Timeout,
        ModelFailureKind::BadRequest,
    ] {
        pool.report("openai", &a.id, failure(kind, None, None), 1);
    }
    assert_eq!(pool.pick("openai", 2).unwrap().id, a.id);
}

#[test]
fn a_rested_key_returns_after_its_rest() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(10)),
        1_000,
    );
    assert!(pool.pick("openai", 10_999).is_none());
    assert_eq!(keys_of(&pool, "openai", 10_999)[0].status, "resting");
    assert_eq!(pool.pick("openai", 11_000).unwrap().id, a.id);
    assert_eq!(keys_of(&pool, "openai", 11_000)[0].status, "ready");
}

#[test]
fn adding_or_removing_keys_never_wakes_a_resting_key() {
    let pool = memory_pool();
    let a = add(&pool, "openai", "sk-aaaa-0000");
    let b = add(&pool, "openai", "sk-bbbb-1111");
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::RateLimited, Some(429), None),
        1_000,
    );
    let before = resting_until(&pool, "openai", &a.id);
    pool.add("openai", None, "sk-cccc-2222".into(), 2_000)
        .unwrap();
    pool.remove("openai", &b.id).unwrap();
    assert_eq!(resting_until(&pool, "openai", &a.id), before);
    assert_ne!(pool.pick("openai", 3_000).unwrap().id, a.id);
}

#[test]
fn env_key_is_listed_first_never_saved_and_cannot_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider-keys.json");
    let env = BTreeMap::from([("openai".to_owned(), "sk-env-key-9999".to_owned())]);
    let pool = KeyPool::open(Some(path.clone()), &env).unwrap();
    let added = pool
        .add("openai", Some("work".into()), "sk-file-key-1234".into(), 5)
        .unwrap();
    let keys = keys_of(&pool, "openai", 5);
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].id, "env");
    assert_eq!(keys[0].source, KeySource::Env);
    assert_eq!(keys[1].id, added.id);
    assert_eq!(keys[1].source, KeySource::File);
    assert_eq!(keys[1].label.as_deref(), Some("work"));
    assert_eq!(pool.pick("openai", 6).unwrap().key, "sk-env-key-9999");
    assert_eq!(pool.remove("openai", "env"), Err(KeyError::EnvKey));
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("sk-env-key-9999"));
    assert!(saved.contains("sk-file-key-1234"));
    // A duplicate of the env key is rejected too.
    assert_eq!(
        pool.add("openai", None, "sk-env-key-9999".into(), 7)
            .unwrap_err(),
        KeyError::Duplicate
    );
}

#[cfg(unix)]
#[test]
fn file_is_0600_and_survives_reopen() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("provider-keys.json");
    let pool = KeyPool::open(Some(path.clone()), &BTreeMap::new()).unwrap();
    let a = pool
        .add("openai", Some("one".into()), "sk-aaaa-0000".into(), 10)
        .unwrap();
    let b = add(&pool, "anthropic", "sk-ant-bbbb-1111");
    pool.set_strategy("openai", Strategy::RoundRobin).unwrap();
    pool.report(
        "openai",
        &a.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(600)),
        1_000,
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!dir
        .path()
        .join("nested")
        .join("provider-keys.json.tmp")
        .exists());

    let reopened = KeyPool::open(Some(path), &BTreeMap::new()).unwrap();
    let openai = keys_of(&reopened, "openai", 1_001);
    assert_eq!(openai.len(), 1);
    assert_eq!(openai[0].id, a.id);
    assert_eq!(openai[0].label.as_deref(), Some("one"));
    assert_eq!(openai[0].status, "resting");
    assert_eq!(openai[0].resting_until_ms, Some(1_000 + 600_000));
    assert!(openai[0].last_error.is_some());
    assert_eq!(keys_of(&reopened, "anthropic", 0)[0].id, b.id);
    assert_eq!(
        reopened.pick("anthropic", 1).unwrap().key,
        "sk-ant-bbbb-1111"
    );
    let strategies: BTreeMap<String, Strategy> = reopened
        .list(0)
        .into_iter()
        .map(|view| (view.id, view.strategy))
        .collect();
    assert_eq!(strategies["openai"], Strategy::RoundRobin);
    assert_eq!(strategies["anthropic"], Strategy::FillFirst);
    assert!(reopened.list(0).iter().all(|view| view.persisted));
}

#[test]
fn a_corrupt_key_file_is_an_error_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider-keys.json");
    std::fs::write(&path, "{not json").unwrap();
    assert!(KeyPool::open(Some(path.clone()), &BTreeMap::new()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json");
}

#[test]
fn short_keys_show_at_most_their_last_four() {
    assert_eq!(mask_key("abcdefghijklmnop"), "…mnop");
    assert_eq!(mask_key("abcdefghijklmnopqrs"), "…pqrs");
    assert_eq!(mask_key("abcd1234"), "…34");
    assert_eq!(mask_key("sk-abcdefghijklmnopq"), "sk-abc…nopq");
}

#[test]
fn keys_are_masked_in_views() {
    let pool = memory_pool();
    let view = add(&pool, "openai", "sk-abcdefghijklmnopwxyz");
    assert_eq!(view.masked, "sk-abc…wxyz");
    assert_ne!(view.masked, "sk-abcdefghijklmnopwxyz");
    let short = add(&pool, "openai", "abcd1234");
    assert_ne!(short.masked, "abcd1234");
    assert!(!short.masked.contains("abcd1"));
    let tiny = add(&pool, "openai", "ab");
    assert!(!tiny.masked.contains("ab"));
    let json = serde_json::to_string(&pool.list(0)).unwrap();
    assert!(!json.contains("sk-abcdefghijklmnopwxyz"));
}

#[test]
fn add_rejects_empty_long_and_duplicate_keys() {
    let pool = memory_pool();
    assert_eq!(
        pool.add("openai", None, "   ".into(), 0).unwrap_err(),
        KeyError::Empty
    );
    assert_eq!(
        pool.add("openai", None, "k".repeat(2_001), 0).unwrap_err(),
        KeyError::TooLong
    );
    assert!(pool.add("openai", None, "k".repeat(2_000), 0).is_ok());
    add(&pool, "openai", "sk-dup-0000");
    assert_eq!(
        pool.add("openai", None, " sk-dup-0000 ".into(), 0)
            .unwrap_err(),
        KeyError::Duplicate
    );
    // The same key under another provider is not a duplicate.
    assert!(pool.add("anthropic", None, "sk-dup-0000".into(), 0).is_ok());
    assert_eq!(
        pool.add("not-a-provider", None, "sk-x".into(), 0)
            .unwrap_err(),
        KeyError::Unknown
    );
    assert_eq!(pool.remove("openai", "nope"), Err(KeyError::Unknown));
    assert_eq!(pool.remove("not-a-provider", "x"), Err(KeyError::Unknown));
}

#[test]
fn without_a_file_added_keys_work_but_are_not_persisted() {
    let pool = memory_pool();
    let view = add(&pool, "openai", "sk-aaaa-0000");
    assert_eq!(pool.pick("openai", 1).unwrap().id, view.id);
    let listed = pool.list(0);
    assert!(!listed.is_empty());
    assert!(listed.iter().all(|provider| !provider.persisted));
}

#[test]
fn a_failed_save_rolls_the_add_back() {
    let dir = tempfile::tempdir().unwrap();
    // The key file's folder does not exist when the pool opens, then a regular
    // file takes its place, so the write cannot succeed.
    let folder = dir.path().join("keys");
    let pool = KeyPool::open(Some(folder.join("provider-keys.json")), &BTreeMap::new()).unwrap();
    std::fs::write(&folder, "x").unwrap();
    assert!(matches!(
        pool.add("openai", None, "sk-aaaa-0000".into(), 0),
        Err(KeyError::Io(_))
    ));
    assert!(keys_of(&pool, "openai", 0).is_empty());
}

#[test]
fn debug_output_never_contains_a_raw_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider-keys.json");
    let raw = "sk-raw-secret-key-123456";
    let env_raw = "sk-env-secret-key-654321";
    let env = BTreeMap::from([("anthropic".to_owned(), env_raw.to_owned())]);
    let pool = KeyPool::open(Some(path.clone()), &env).unwrap();
    add(&pool, "openai", raw);
    let picked = pool.pick("openai", 1).unwrap();
    assert_eq!(picked.key, raw);

    let stored = store::load(&path).unwrap();
    for text in [
        format!("{picked:?}"),
        format!("{pool:?}"),
        format!("{:#?}", pool.inner.lock().unwrap()),
        format!("{stored:?}"),
        format!("{:#?}", stored),
    ] {
        assert!(!text.contains(raw), "raw key in {text}");
        assert!(!text.contains(env_raw), "raw env key in {text}");
    }
    assert!(format!("{picked:?}").contains("sk-raw…3456"));
}

#[test]
fn try_now_wakes_a_resting_key_including_the_env_key() {
    let env = BTreeMap::from([("openai".to_owned(), "sk-env-key-9999".to_owned())]);
    let pool = KeyPool::open(None, &env).unwrap();
    pool.report(
        "openai",
        ENV_KEY_ID,
        failure(ModelFailureKind::Billing, Some(402), None),
        0,
    );
    assert_eq!(pool.pick("openai", 1), None);

    let view = pool.wake("openai", ENV_KEY_ID, 2).unwrap();

    assert_eq!(view.status, "ready");
    assert_eq!(view.resting_until_ms, None);
    assert_eq!(view.last_error, None);
    assert_eq!(pool.pick("openai", 3).unwrap().id, ENV_KEY_ID);
}

#[test]
fn a_woken_file_key_stays_awake_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider-keys.json");
    let pool = KeyPool::open(Some(path.clone()), &BTreeMap::new()).unwrap();
    let key = add(&pool, "openai", "sk-file-key-1234");
    pool.report(
        "openai",
        &key.id,
        failure(ModelFailureKind::Billing, Some(402), None),
        0,
    );

    pool.wake("openai", &key.id, 1).unwrap();

    let reopened = KeyPool::open(Some(path), &BTreeMap::new()).unwrap();
    let keys = keys_of(&reopened, "openai", 2);
    assert_eq!(keys[0].status, "ready");
    assert_eq!(keys[0].last_error, None);
}

#[test]
fn waking_an_unknown_key_or_provider_is_unknown() {
    let pool = memory_pool();
    add(&pool, "openai", "sk-file-key-1234");
    assert_eq!(
        pool.wake("openai", "key_missing", 0),
        Err(KeyError::Unknown)
    );
    assert_eq!(pool.wake("nobody", ENV_KEY_ID, 0), Err(KeyError::Unknown));
}

#[test]
fn a_late_failure_on_a_resting_key_only_extends_its_rest() {
    let pool = memory_pool();
    let key = add(&pool, "openai", "sk-first-0000");
    add(&pool, "openai", "sk-second-111");
    pool.report(
        "openai",
        &key.id,
        failure(ModelFailureKind::Billing, Some(402), None),
        0,
    );
    // An in-flight call on the same key fails later with a short rate limit.
    pool.report(
        "openai",
        &key.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(30)),
        10,
    );
    assert_eq!(resting_until(&pool, "openai", &key.id), Some(HOUR_MS));
    assert_eq!(
        keys_of(&pool, "openai", 20)[0].last_error.as_deref(),
        Some("out of credit")
    );

    // A longer rest still wins.
    let other = add(&pool, "openai", "sk-third-2222");
    pool.report(
        "openai",
        &other.id,
        failure(ModelFailureKind::RateLimited, Some(429), Some(30)),
        0,
    );
    pool.report(
        "openai",
        &other.id,
        failure(ModelFailureKind::Billing, Some(402), None),
        10,
    );
    assert_eq!(
        resting_until(&pool, "openai", &other.id),
        Some(10 + HOUR_MS)
    );
}
