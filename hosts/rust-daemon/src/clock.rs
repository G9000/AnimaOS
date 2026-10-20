//! The owner's wall clock. Agents read times the owner mentions ("remind me
//! at 7 p.m.") on it, and automations without an explicit time zone run on
//! it. The owner's zone comes from their web console; until it has reported
//! one, the host's zone stands in, which is wrong when the daemon runs on a
//! server elsewhere.

use std::sync::{Arc, RwLock};

use anima_core::{AgentRuntime, Message, Provider, ProviderResult};
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use chrono_tz::Tz;

/// The host's IANA time zone: `TZ` when it names a known zone, else the
/// operating system's zone, else UTC.
pub(crate) fn host_time_zone() -> Tz {
    resolve_time_zone(
        std::env::var("TZ").ok().as_deref(),
        iana_time_zone::get_timezone().ok().as_deref(),
    )
}

fn resolve_time_zone(tz_env: Option<&str>, system: Option<&str>) -> Tz {
    [
        tz_env.map(|zone| zone.trim().trim_start_matches(':')),
        system,
    ]
    .into_iter()
    .flatten()
    .find_map(|zone| zone.trim().parse::<Tz>().ok())
    .unwrap_or(Tz::UTC)
}

/// The owner's IANA time zone as last reported by their web console; one
/// handle shared by the daemon state, the clock provider and tool contexts.
#[derive(Clone, Default)]
pub(crate) struct OwnerTimeZone(Arc<RwLock<Option<Tz>>>);

impl OwnerTimeZone {
    pub(crate) fn get(&self) -> Option<Tz> {
        *self
            .0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records `zone`; true when it differs from the one held.
    pub(crate) fn set(&self, zone: Option<Tz>) -> bool {
        let mut held = self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed = *held != zone;
        *held = zone;
        changed
    }

    /// The owner's zone, else the host's.
    pub(crate) fn effective(&self) -> Tz {
        self.get().unwrap_or_else(host_time_zone)
    }
}

/// `at` on `zone`'s wall clock, e.g.
/// `Monday 2026-10-19T19:00:00+08:00 (Asia/Kuala_Lumpur)`.
pub(crate) fn local_time_text(at: DateTime<Utc>, zone: Tz) -> String {
    let local = at.with_timezone(&zone);
    format!(
        "{} {} ({})",
        local.format("%A"),
        local.to_rfc3339_opts(SecondsFormat::Secs, false),
        zone.name()
    )
}

/// The owner's current local time and time zone, read once per run.
struct LocalTimeProvider {
    owner: OwnerTimeZone,
}

#[async_trait]
impl Provider for LocalTimeProvider {
    fn name(&self) -> &str {
        "local_time"
    }
    fn description(&self) -> &str {
        "The owner's current local time and time zone"
    }
    async fn get(&self, _: &AgentRuntime, _: &Message) -> Result<ProviderResult, String> {
        Ok(ProviderResult {
            text: local_time_context(self.owner.get(), host_time_zone(), Utc::now()),
            metadata: None,
        })
    }
}

fn local_time_context(owner: Option<Tz>, host: Tz, now: DateTime<Utc>) -> String {
    let zone = owner.unwrap_or(host);
    let clock = match owner {
        Some(_) => format!("It is now {} in the owner's time zone.", local_time_text(now, zone)),
        None => format!(
            "It is now {} on this host's clock. The owner's own time zone is not known yet, so name the zone whenever you confirm a time with them.",
            local_time_text(now, zone)
        ),
    };
    format!(
        "{clock} Read times the owner mentions, like \"7 p.m.\" or \"tomorrow morning\", on this clock unless they name another zone; pass timeZone \"{}\" for automations, and give one-time runs as \"at <RFC 3339 time>\" with this offset.",
        zone.name()
    )
}

pub(crate) fn local_time_provider(owner: OwnerTimeZone) -> Arc<dyn Provider> {
    Arc::new(LocalTimeProvider { owner })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn tz_wins_over_the_system_zone_and_utc_is_the_fallback() {
        assert_eq!(
            resolve_time_zone(Some(":Europe/London"), Some("Asia/Kuala_Lumpur")),
            Tz::Europe__London
        );
        assert_eq!(
            resolve_time_zone(Some("not/a zone"), Some("Asia/Kuala_Lumpur")),
            Tz::Asia__Kuala_Lumpur
        );
        assert_eq!(resolve_time_zone(None, Some("Nowhere/Else")), Tz::UTC);
        assert_eq!(resolve_time_zone(None, None), Tz::UTC);
    }

    #[test]
    fn the_owner_zone_wins_and_a_change_is_reported_once() {
        let owner = OwnerTimeZone::default();
        assert_eq!(owner.get(), None);
        assert!(owner.set(Some(Tz::America__New_York)));
        assert!(!owner.set(Some(Tz::America__New_York)));
        assert_eq!(owner.effective(), Tz::America__New_York);
        assert!(owner.clone().set(None));
        assert_eq!(owner.get(), None);
    }

    #[test]
    fn the_context_says_whose_clock_it_reads() {
        let now = Utc.with_ymd_and_hms(2026, 10, 19, 11, 0, 0).unwrap();
        let owned = local_time_context(Some(Tz::America__New_York), Tz::Asia__Singapore, now);
        assert!(owned.starts_with(
            "It is now Monday 2026-10-19T07:00:00-04:00 (America/New_York) in the owner's time zone."
        ));
        assert!(owned.contains("pass timeZone \"America/New_York\""));
        let fallback = local_time_context(None, Tz::Asia__Singapore, now);
        assert!(fallback.starts_with(
            "It is now Monday 2026-10-19T19:00:00+08:00 (Asia/Singapore) on this host's clock."
        ));
        assert!(fallback.contains("not known yet"));
    }

    #[test]
    fn local_time_names_the_weekday_offset_and_zone() {
        let at = Utc.with_ymd_and_hms(2026, 10, 19, 11, 0, 0).unwrap();
        assert_eq!(
            local_time_text(at, Tz::Asia__Kuala_Lumpur),
            "Monday 2026-10-19T19:00:00+08:00 (Asia/Kuala_Lumpur)"
        );
    }
}
