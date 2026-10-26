//! Common persistent-profile checks. Fixtures must use their real host boundary.
//! This basic contract supplements, rather than certifies, each host's control,
//! provider, approval, process-death, effect, migration, and domain acceptance.
use anima_core::MemoryRecord;
use async_trait::async_trait;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostReply {
    pub run_id: String,
    pub individual_id: String,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedPreference {
    pub memory_id: String,
    pub source_event_id: String,
}

#[async_trait]
pub trait PersistentHostFixture: Send {
    async fn submit(&mut self, request_id: &str, text: &str) -> Result<HostReply, String>;
    async fn accept_preference(
        &mut self,
        source_event_id: &str,
        preference: &str,
    ) -> Result<AcceptedPreference, String>;
    async fn recall(&mut self, query: &str) -> Result<Vec<MemoryRecord>, String>;
    /// Reopen the same host data and identities. Physical process recovery is a
    /// separate required test and cannot be inferred from this method's name.
    async fn reopen(&mut self) -> Result<(), String>;
}

pub async fn assert_persistent_host_basics<F: PersistentHostFixture>(
    fixture: &mut F,
) -> Result<(), String> {
    let first = fixture.submit("shared-contract-hello", "Hello").await?;
    if first.run_id.is_empty() || first.individual_id.is_empty() || first.text.is_empty() {
        return Err("host reply lacks durable identity/output".into());
    }
    if fixture.submit("shared-contract-hello", "Hello").await? != first {
        return Err("exact request retry changed committed output or identity".into());
    }
    if fixture
        .submit("shared-contract-hello", "A conflicting payload")
        .await
        .is_ok()
    {
        return Err("host accepted conflicting request payload".into());
    }
    let preference = fixture
        .accept_preference("shared-contract-preference", "prefers jasmine tea")
        .await?;
    if preference.memory_id.is_empty() || preference.source_event_id.is_empty() {
        return Err("accepted preference lacks source identity".into());
    }
    if fixture
        .accept_preference("shared-contract-preference", "prefers jasmine tea")
        .await?
        != preference
    {
        return Err("preference retry changed memory/source identity".into());
    }
    if fixture
        .accept_preference("shared-contract-preference", "prefers coffee instead")
        .await
        .is_ok()
    {
        return Err("host accepted conflicting source event".into());
    }
    let before = fixture.recall("jasmine").await?;
    let record = before
        .iter()
        .find(|record| {
            record.id() == preference.memory_id
                && record.content().contains("jasmine")
                && record.provenance().source_identity() == preference.source_event_id
        })
        .ok_or("accepted preference is not visible with authenticated source")?
        .clone();
    fixture.reopen().await?;
    if fixture.submit("shared-contract-hello", "Hello").await? != first {
        return Err("reopen regenerated or changed an exact retry".into());
    }
    let next = fixture
        .submit("shared-contract-next-session", "Hello again")
        .await?;
    if next.individual_id != first.individual_id || next.run_id == first.run_id {
        return Err("new activity changed individual identity or reused a run".into());
    }
    let after = fixture.recall("jasmine").await?;
    if !after.contains(&record) {
        return Err("reopen lost or changed canonical preference memory".into());
    }
    Ok(())
}

/// A host-selected identity or owner/world partition, never model-selected authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtherHostScope {
    Individual,
    Partition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreferenceMutation {
    pub memory_id: String,
    /// The host's complete response, kept opaque so exact replay must preserve it.
    pub receipt: String,
}

/// Additional host-boundary acceptance for private memory lifecycle behavior.
/// This remains separate so the basic fixture contract does not imply coverage.
#[async_trait]
pub trait PersistentHostLifecycleFixture: PersistentHostFixture {
    fn captured_requests(&self) -> Vec<anima_core::ModelGenerateRequest>;
    async fn submit_as(
        &mut self,
        scope: OtherHostScope,
        request_id: &str,
        text: &str,
    ) -> Result<HostReply, String>;
    async fn correct_preference(
        &mut self,
        source_event_id: &str,
        original: &MemoryRecord,
        replacement: &str,
    ) -> Result<PreferenceMutation, String>;
    async fn forget_preference(
        &mut self,
        source_event_id: &str,
        memory_id: &str,
        expected_revision: u64,
    ) -> Result<PreferenceMutation, String>;
    async fn forget_as_other(
        &mut self,
        scope: OtherHostScope,
        memory_id: &str,
        expected_revision: u64,
    ) -> Result<Option<PreferenceMutation>, String>;
}

fn assert_new_model_requests<F: PersistentHostLifecycleFixture>(
    fixture: &F,
    before: usize,
    required: Option<&str>,
    forbidden: &[&str],
    context: &str,
) -> Result<(), String> {
    let requests = fixture.captured_requests();
    let fresh = requests
        .get(before..)
        .ok_or("model capture lost prior requests")?;
    if fresh.is_empty() {
        return Err(format!("{context}: no physical model request was captured"));
    }
    // Hosts may use multiple stages per turn. Inspect every new request, including
    // messages and system text, rather than letting a clean final call mask a leak.
    let payloads: Vec<_> = fresh.iter().map(|request| format!("{request:?}")).collect();
    if forbidden
        .iter()
        .any(|marker| payloads.iter().any(|payload| payload.contains(marker)))
    {
        return Err(format!(
            "{context}: private or inactive preference leaked into a model request"
        ));
    }
    if required.is_some_and(|marker| !payloads.iter().any(|payload| payload.contains(marker))) {
        return Err(format!(
            "{context}: fresh model requests omitted the canonical preference"
        ));
    }
    Ok(())
}

pub async fn assert_persistent_host_lifecycle<F: PersistentHostLifecycleFixture>(
    fixture: &mut F,
) -> Result<(), String> {
    const OLD: &str = "jasmine-private-orchid4827";
    const NEW: &str = "sesame-private-orchid4827";
    // Keep the query relevant without supplying either private marker.
    const NEUTRAL: &str = "What tea do I prefer?";
    const OLD_TEXT: &str = "I prefer jasmine tea: jasmine-private-orchid4827";
    const NEW_TEXT: &str = "I prefer sesame tea: sesame-private-orchid4827";
    let accepted = fixture
        .accept_preference("shared-lifecycle-original", OLD_TEXT)
        .await?;
    let original = fixture
        .recall("jasmine")
        .await?
        .into_iter()
        .find(|record| record.id() == accepted.memory_id && record.content().contains(OLD))
        .ok_or("private preference missing from canonical recall")?;
    let before = fixture.captured_requests().len();
    let completed = fixture
        .submit("shared-lifecycle-completed", NEUTRAL)
        .await?;
    assert_new_model_requests(fixture, before, Some(OLD), &[], "own individual")?;

    for (scope, request_id, context) in [
        (
            OtherHostScope::Individual,
            "shared-lifecycle-other-individual",
            "other individual",
        ),
        (
            OtherHostScope::Partition,
            "shared-lifecycle-other-partition",
            "other partition",
        ),
    ] {
        let before = fixture.captured_requests().len();
        fixture.submit_as(scope, request_id, NEUTRAL).await?;
        assert_new_model_requests(fixture, before, None, &[OLD, NEW], context)?;
        if fixture
            .forget_as_other(scope, original.id(), original.revision())
            .await?
            .is_some()
        {
            return Err(format!("{context}: host accepted foreign forgetting"));
        }
    }
    if !fixture.recall("jasmine").await?.contains(&original) {
        return Err("foreign forgetting changed canonical private memory".into());
    }

    let correction = fixture
        .correct_preference("shared-lifecycle-correction", &original, NEW_TEXT)
        .await?;
    if correction.memory_id.is_empty()
        || correction.receipt.is_empty()
        || correction.memory_id == original.id()
    {
        return Err("correction lacks a distinct replacement identity or receipt".into());
    }
    if fixture
        .correct_preference("shared-lifecycle-correction", &original, NEW_TEXT)
        .await?
        != correction
    {
        return Err("exact correction retry changed the host receipt".into());
    }
    if fixture
        .recall("jasmine")
        .await?
        .iter()
        .any(|record| record.id() == original.id() || record.content().contains(OLD))
    {
        return Err("correction retained old canonical recall".into());
    }
    let replacement = fixture
        .recall("sesame")
        .await?
        .into_iter()
        .find(|record| {
            record.id() == correction.memory_id
                && record.content().contains(NEW)
                && record.corrects() == Some(original.id())
        })
        .ok_or("replacement lacks canonical correction lineage")?;
    let before = fixture.captured_requests().len();
    fixture
        .submit("shared-lifecycle-after-correction", NEUTRAL)
        .await?;
    assert_new_model_requests(fixture, before, Some(NEW), &[OLD], "after correction")?;

    let forgotten = fixture
        .forget_preference(
            "shared-lifecycle-forget",
            replacement.id(),
            replacement.revision(),
        )
        .await?;
    if forgotten.memory_id != replacement.id() || forgotten.receipt.is_empty() {
        return Err("forgetting lacks the affected memory identity or receipt".into());
    }
    if fixture
        .forget_preference(
            "shared-lifecycle-forget",
            replacement.id(),
            replacement.revision(),
        )
        .await?
        != forgotten
    {
        return Err("exact forgetting retry changed the host receipt".into());
    }
    if fixture
        .forget_preference(
            "shared-lifecycle-forget",
            replacement.id(),
            replacement.revision() + 1,
        )
        .await
        .is_ok()
    {
        return Err("host accepted changed revision under the same forgetting source".into());
    }
    fixture.reopen().await?;
    let before = fixture.captured_requests().len();
    if fixture
        .correct_preference("shared-lifecycle-correction", &original, NEW_TEXT)
        .await?
        != correction
    {
        return Err("reopen changed an exact correction receipt replay".into());
    }
    if fixture
        .forget_preference(
            "shared-lifecycle-forget",
            replacement.id(),
            replacement.revision(),
        )
        .await?
        != forgotten
    {
        return Err("reopen changed an exact forgetting receipt replay".into());
    }
    if fixture.captured_requests().len() != before {
        return Err("reopened memory receipt replay dispatched another model call".into());
    }
    for query in ["jasmine", "sesame"] {
        if fixture.recall(query).await?.iter().any(|record| {
            record.id() == original.id()
                || record.id() == replacement.id()
                || record.content().contains(OLD)
                || record.content().contains(NEW)
        }) {
            return Err("reopen restored corrected or forgotten canonical recall".into());
        }
    }
    let before = fixture.captured_requests().len();
    fixture
        .submit("shared-lifecycle-after-reopen", NEUTRAL)
        .await?;
    assert_new_model_requests(
        fixture,
        before,
        None,
        &[OLD, NEW],
        "after forgetting and reopen",
    )?;
    let before = fixture.captured_requests().len();
    if fixture
        .submit("shared-lifecycle-completed", NEUTRAL)
        .await?
        != completed
    {
        return Err("memory lifecycle changed an exact completed reply replay".into());
    }
    if fixture.captured_requests().len() != before {
        return Err("exact completed reply replay dispatched another model call".into());
    }
    Ok(())
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use anima_core::*;

    struct LeakingFixture {
        record: Option<MemoryRecord>,
        calls: Vec<ModelGenerateRequest>,
    }
    impl LeakingFixture {
        fn capture(&mut self, system: &str) {
            self.calls.push(ModelGenerateRequest {
                system: system.into(),
                messages: vec![],
                temperature: None,
                max_tokens: None,
            });
        }
        fn reply() -> HostReply {
            HostReply {
                run_id: "run".into(),
                individual_id: "individual".into(),
                text: "hello".into(),
            }
        }
    }
    #[async_trait]
    impl PersistentHostFixture for LeakingFixture {
        async fn submit(&mut self, _: &str, _: &str) -> Result<HostReply, String> {
            let text = self.record.as_ref().unwrap().content().to_owned();
            self.capture(&text);
            Ok(Self::reply())
        }
        async fn accept_preference(
            &mut self,
            source: &str,
            text: &str,
        ) -> Result<AcceptedPreference, String> {
            let write = MemoryWrite::new(
                "private-memory",
                MemoryKind::Preference,
                MemoryScope::agent("individual").unwrap(),
                text,
                1.0,
                MemoryProvenance::new("accepted-user-statement", source, 0).unwrap(),
            )
            .unwrap();
            self.record = Some(MemoryRecord::from_write(write, 1).unwrap());
            Ok(AcceptedPreference {
                memory_id: "private-memory".into(),
                source_event_id: source.into(),
            })
        }
        async fn recall(&mut self, _: &str) -> Result<Vec<MemoryRecord>, String> {
            Ok(self.record.iter().cloned().collect())
        }
        async fn reopen(&mut self) -> Result<(), String> {
            Err("unexpected reopen".into())
        }
    }
    #[async_trait]
    impl PersistentHostLifecycleFixture for LeakingFixture {
        fn captured_requests(&self) -> Vec<ModelGenerateRequest> {
            self.calls.clone()
        }
        async fn submit_as(
            &mut self,
            _: OtherHostScope,
            _: &str,
            _: &str,
        ) -> Result<HostReply, String> {
            let leaked = self.record.as_ref().unwrap().content().to_owned();
            self.capture(&leaked);
            self.capture("clean final stage");
            Ok(Self::reply())
        }
        async fn correct_preference(
            &mut self,
            _: &str,
            _: &MemoryRecord,
            _: &str,
        ) -> Result<PreferenceMutation, String> {
            Err("unexpected correction".into())
        }
        async fn forget_preference(
            &mut self,
            _: &str,
            _: &str,
            _: u64,
        ) -> Result<PreferenceMutation, String> {
            Err("unexpected forgetting".into())
        }
        async fn forget_as_other(
            &mut self,
            _: OtherHostScope,
            _: &str,
            _: u64,
        ) -> Result<Option<PreferenceMutation>, String> {
            Ok(None)
        }
    }
    #[tokio::test]
    async fn lifecycle_suite_rejects_leak_in_first_of_two_model_requests() {
        let mut fixture = LeakingFixture {
            record: None,
            calls: vec![],
        };
        let error = assert_persistent_host_lifecycle(&mut fixture)
            .await
            .unwrap_err();
        assert!(
            error.contains("other individual: private or inactive preference leaked"),
            "{error}"
        );
        assert_eq!(fixture.calls.last().unwrap().system, "clean final stage");
    }
}
