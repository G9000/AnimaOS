//! The daemon's side of the external memory plug-in point
//! (`anima_memory::ExternalMemoryProvider`): the id registry, the boot choice,
//! the process-wide handle, and the background `sync_turn` / `on_memory_write`
//! dispatch. The local store never depends on the provider: every call here is
//! best effort, bounded, and logged by provider id and error only (never memory
//! or turn text).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anima_memory::{
    ExternalMemoryProvider, Memory, MemoryWriteAction, MemoryWriteEvent, NoExternalMemory,
    ProviderStatus, TurnRecord,
};
use tracing::warn;

pub(crate) type SharedExternalMemory = Arc<dyn ExternalMemoryProvider>;

pub(crate) const MEMORY_PROVIDER_ENV: &str = "ANIMAOS_RS_MEMORY_PROVIDER";
const DEFAULT_PROVIDER: &str = "none";
/// How long one background `sync_turn` or `on_memory_write` call may run.
const BACKGROUND_CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// The provider ids this daemon can build. A later task registers a real one
/// (for example an HTTP adapter or Mem0) here and in [`from_id`].
pub(crate) fn known_ids() -> &'static [&'static str] {
    &[DEFAULT_PROVIDER]
}

pub(crate) fn from_id(id: &str) -> Result<SharedExternalMemory, String> {
    match id {
        DEFAULT_PROVIDER => Ok(Arc::new(NoExternalMemory)),
        other => Err(format!(
            "unknown memory provider '{other}'; known: {}",
            known_ids().join(", ")
        )),
    }
}

/// Reads `ANIMAOS_RS_MEMORY_PROVIDER` (default `none`). An unknown id is a
/// boot error that lists the known ids.
pub(crate) fn from_env() -> Result<SharedExternalMemory, String> {
    from_value(std::env::var(MEMORY_PROVIDER_ENV).ok().as_deref())
}

fn from_value(value: Option<&str>) -> Result<SharedExternalMemory, String> {
    let id = value.map(str::trim).filter(|id| !id.is_empty());
    from_id(id.unwrap_or(DEFAULT_PROVIDER))
}

/// Events a full queue holds before new ones are dropped.
const QUEUE_CAPACITY: usize = 1_024;
/// How long `status()` may take before the provider reports unavailable.
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const STATUS_TIMED_OUT: &str = "status check timed out";
const PROVIDER_PANICKED: &str = "provider panicked";

/// Runs one provider call so that a panic inside it (or in the call that
/// builds its future) comes back as `Err` instead of unwinding the caller.
async fn guarded<T>(call: impl std::future::Future<Output = T>) -> std::thread::Result<T> {
    futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(call)).await
}

/// One call for the single worker, in the order it was queued.
enum JobPayload {
    Turn(TurnRecord),
    Write(MemoryWriteEvent),
}

struct Job {
    payload: JobPayload,
    owner: Option<crate::profile_owner::ProfileOwner>,
}

/// The active provider and the queue to its one worker. Mirror events and
/// turn records are queued here in order and sent one at a time, so a Remove
/// can never overtake the Add before it. Producers never wait: a full queue
/// drops the event with a warning.
pub(crate) struct Active {
    provider: SharedExternalMemory,
    queue: tokio::sync::mpsc::Sender<Job>,
    status_timeout: Duration,
    dropped: std::sync::atomic::AtomicUsize,
}

impl Active {
    /// Starts the worker on the current runtime. `None` outside a runtime.
    fn start(
        provider: SharedExternalMemory,
        capacity: usize,
        call_timeout: Duration,
        status_timeout: Duration,
    ) -> Option<Arc<Self>> {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            warn!(provider = %provider.id(), "external memory provider not started: no runtime");
            return None;
        };
        let (queue, mut jobs) = tokio::sync::mpsc::channel(capacity);
        let worker = Arc::clone(&provider);
        // The worker ends when the last sender (the `Active`) is dropped. It
        // holds the provider, not the `Active`, so there is no cycle. Each call
        // is guarded on its own, so a panicking one never stops the worker.
        runtime.spawn(async move {
            while let Some(Job { payload, owner }) = jobs.recv().await {
                if let Some(owner) = &owner {
                    if owner.verify_owned().await.is_err() {
                        warn!(provider = %worker.id(), "external memory job refused after profile ownership loss");
                        continue;
                    }
                }
                let (call, outcome) = match payload {
                    JobPayload::Turn(turn) => (
                        "sync_turn",
                        tokio::time::timeout(
                            call_timeout,
                            guarded(async { worker.sync_turn(turn).await }),
                        )
                        .await,
                    ),
                    JobPayload::Write(event) => (
                        "on_memory_write",
                        tokio::time::timeout(
                            call_timeout,
                            guarded(async { worker.on_memory_write(event).await }),
                        )
                        .await,
                    ),
                };
                // The job retains its authority through the remote call. If
                // ownership was lost after dispatch, its outcome remains an
                // external effect requiring review; no rollback is claimed.
                if owner.as_ref().is_some_and(|owner| owner.ensure_owned().is_err()) {
                    warn!(provider = %worker.id(), call, "external memory outcome requires review after profile ownership loss");
                }
                match outcome {
                    Ok(Ok(Ok(()))) => {}
                    Ok(Ok(Err(error))) => {
                        warn!(provider = %worker.id(), call, error = %error, "external memory call failed")
                    }
                    Ok(Err(_)) => warn!(provider = %worker.id(), call, "external memory call panicked"),
                    Err(_) => warn!(provider = %worker.id(), call, "external memory call timed out"),
                }
            }
        });
        Some(Arc::new(Self {
            provider,
            queue,
            status_timeout,
            dropped: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    pub(crate) fn provider(&self) -> &SharedExternalMemory {
        &self.provider
    }

    /// Queues `jobs` in order without waiting; whatever does not fit is
    /// dropped, counted, and logged by provider id and count only.
    fn enqueue(&self, jobs: impl IntoIterator<Item = Job>) {
        let mut dropped = 0;
        for job in jobs {
            if self.queue.try_send(job).is_err() {
                dropped += 1;
            }
        }
        if dropped > 0 {
            self.dropped
                .fetch_add(dropped, std::sync::atomic::Ordering::Relaxed);
            warn!(provider = %self.provider.id(), dropped, "external memory queue full or closed; events dropped");
        }
    }
}

// The one provider of this process, set once at boot. `MemoryMutation` has no
// state access, so the dispatch reaches the provider through this handle. It
// only ever holds a provider other than `none`: absence means `none`, which
// makes the default path a single atomic load.
static ACTIVE: OnceLock<Arc<Active>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    // A test's own provider and worker. Every test thread has its own, so
    // fakes never leak across parallel tests. The worker is spawned by
    // `inject` on the test's runtime and only ever calls that provider.
    static TEST_PROVIDER: std::cell::RefCell<Option<Arc<Active>>> =
        const { std::cell::RefCell::new(None) };
    static SNAPSHOT_DIFFS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Makes `provider` the process's provider and starts its worker. A `none`
/// provider installs nothing. Called once at boot, inside the runtime; a
/// second install is ignored.
pub(crate) fn install(provider: SharedExternalMemory) {
    if provider.id() == DEFAULT_PROVIDER {
        return;
    }
    if let Some(active) = Active::start(
        provider,
        QUEUE_CAPACITY,
        BACKGROUND_CALL_TIMEOUT,
        STATUS_TIMEOUT,
    ) {
        let _ = ACTIVE.set(active);
    }
}

/// The active provider, or `None` for `none`.
pub(crate) fn active() -> Option<Arc<Active>> {
    #[cfg(test)]
    if let Some(active) = TEST_PROVIDER.with(|slot| slot.borrow().clone()) {
        return Some(active);
    }
    ACTIVE.get().cloned()
}

/// The readiness route's view of the provider. A provider that does not
/// answer within 2 seconds, or panics, is reported unavailable.
pub(crate) async fn status() -> (String, ProviderStatus) {
    match active() {
        Some(active) => {
            let status = match tokio::time::timeout(
                active.status_timeout,
                guarded(async { active.provider.status().await }),
            )
            .await
            {
                Ok(Ok(status)) => status,
                Ok(Err(_)) => {
                    warn!(provider = %active.provider.id(), "external memory status panicked");
                    ProviderStatus::Unavailable(PROVIDER_PANICKED.into())
                }
                Err(_) => ProviderStatus::Unavailable(STATUS_TIMED_OUT.into()),
            };
            (active.provider.id().to_owned(), status)
        }
        None => (DEFAULT_PROVIDER.to_owned(), ProviderStatus::Ready),
    }
}

/// Queues a finished owner turn for the provider. With none, `turn` is never
/// built.
pub(crate) fn sync_turn(turn: impl FnOnce() -> TurnRecord) {
    sync_turn_with_owner(None, turn);
}

pub(crate) fn sync_turn_with_owner(
    owner: Option<crate::profile_owner::ProfileOwner>,
    turn: impl FnOnce() -> TurnRecord,
) {
    let Some(active) = active() else {
        return;
    };
    active.enqueue([Job {
        payload: JobPayload::Turn(turn()),
        owner,
    }]);
}

/// Queues a persisted memory change for the provider. With no provider
/// (`none`) this returns before `current` runs, so the default path never
/// snapshots or diffs.
pub(crate) fn mirror_persisted(previous: &[Memory], current: impl FnOnce() -> Vec<Memory>) {
    mirror_persisted_with_owner(previous, current, None);
}

pub(crate) fn mirror_persisted_with_owner(
    previous: &[Memory],
    current: impl FnOnce() -> Vec<Memory>,
    owner: Option<crate::profile_owner::ProfileOwner>,
) {
    let Some(active) = active() else {
        return;
    };
    #[cfg(test)]
    SNAPSHOT_DIFFS.with(|count| count.set(count.get() + 1));
    let events = memory_write_events(previous, &current());
    active.enqueue(events.into_iter().map(|event| Job {
        payload: JobPayload::Write(event),
        owner: owner.clone(),
    }));
}

/// Adds, updates (content changed) and removes between two snapshots, by id.
pub(crate) fn memory_write_events(
    previous: &[Memory],
    current: &[Memory],
) -> Vec<MemoryWriteEvent> {
    let before: HashMap<&str, &Memory> = previous
        .iter()
        .map(|memory| (memory.id.as_str(), memory))
        .collect();
    let event = |memory: &Memory, action, content: &str| MemoryWriteEvent {
        agent_id: memory.agent_id.clone(),
        action,
        memory_type: memory.memory_type.as_str().to_owned(),
        memory_id: memory.id.clone(),
        content: content.to_owned(),
    };
    let mut events = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for memory in current {
        seen.insert(memory.id.as_str());
        match before.get(memory.id.as_str()) {
            None => events.push(event(memory, MemoryWriteAction::Add, &memory.content)),
            Some(old) if old.content != memory.content => {
                events.push(event(memory, MemoryWriteAction::Update, &memory.content));
            }
            Some(_) => {}
        }
    }
    for memory in previous {
        if !seen.contains(memory.id.as_str()) {
            events.push(event(memory, MemoryWriteAction::Remove, &memory.content));
        }
    }
    events
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Mutex;

    use anima_memory::{ExternalMemoryError, RecallRequest, RecalledItem};

    use super::*;

    /// Restores the previous test provider (normally none) on drop. Dropping
    /// the injected `Active` also ends its worker.
    pub(crate) struct InjectedProvider {
        previous: Option<Arc<Active>>,
        active: Arc<Active>,
    }

    impl InjectedProvider {
        /// Events this provider's queue has dropped for being full.
        pub(crate) fn dropped(&self) -> usize {
            self.active
                .dropped
                .load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl Drop for InjectedProvider {
        fn drop(&mut self) {
            TEST_PROVIDER.with(|slot| *slot.borrow_mut() = self.previous.take());
        }
    }

    /// Makes `provider` this thread's provider, with its own worker on the
    /// current runtime, until the guard drops.
    pub(crate) fn inject(provider: SharedExternalMemory) -> InjectedProvider {
        inject_with(
            provider,
            QUEUE_CAPACITY,
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
    }

    /// `inject` with a queue capacity and shortened timeouts.
    pub(crate) fn inject_with(
        provider: SharedExternalMemory,
        capacity: usize,
        call_timeout: Duration,
        status_timeout: Duration,
    ) -> InjectedProvider {
        let active = Active::start(provider, capacity, call_timeout, status_timeout)
            .expect("inject runs inside a runtime");
        let previous = TEST_PROVIDER.with(|slot| slot.borrow_mut().replace(Arc::clone(&active)));
        InjectedProvider { previous, active }
    }

    /// A provider that records what it receives and returns canned recall.
    pub(crate) struct FakeProvider {
        pub(crate) id: String,
        pub(crate) items: Vec<RecalledItem>,
        pub(crate) recall_error: bool,
        /// `recall` waits this long before answering.
        pub(crate) recall_delay: Option<Duration>,
        pub(crate) unavailable: Option<String>,
        /// `status` waits this long before answering.
        pub(crate) status_delay: Option<Duration>,
        /// An `on_memory_write` for this content never returns.
        pub(crate) hang_write_content: Option<String>,
        /// `recall`, `status` and `sync_turn` panic.
        pub(crate) panics: bool,
        /// An `on_memory_write` for this content panics.
        pub(crate) panic_write_content: Option<String>,
        pub(crate) recall_requests: Mutex<Vec<RecallRequest>>,
        pub(crate) turns: Mutex<Vec<TurnRecord>>,
        pub(crate) writes: Mutex<Vec<MemoryWriteEvent>>,
    }

    impl FakeProvider {
        pub(crate) fn new(id: &str) -> Self {
            Self {
                id: id.into(),
                items: Vec::new(),
                recall_error: false,
                recall_delay: None,
                unavailable: None,
                status_delay: None,
                hang_write_content: None,
                panics: false,
                panic_write_content: None,
                recall_requests: Mutex::default(),
                turns: Mutex::default(),
                writes: Mutex::default(),
            }
        }

        pub(crate) fn with_items(id: &str, texts: &[&str]) -> Self {
            Self {
                items: texts
                    .iter()
                    .map(|text| RecalledItem {
                        text: (*text).into(),
                        score: 0.5,
                        source_id: None,
                    })
                    .collect(),
                ..Self::new(id)
            }
        }

        /// Lets background tasks run until `ready` holds (or fails after 2s).
        pub(crate) async fn wait_until(&self, ready: impl Fn(&Self) -> bool) {
            for _ in 0..200 {
                if ready(self) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("the provider did not receive the expected calls");
        }
    }

    #[async_trait::async_trait]
    impl ExternalMemoryProvider for FakeProvider {
        fn id(&self) -> &str {
            &self.id
        }

        async fn recall(
            &self,
            request: RecallRequest,
        ) -> Result<Vec<RecalledItem>, ExternalMemoryError> {
            self.recall_requests.lock().unwrap().push(request);
            assert!(!self.panics, "fake recall panic");
            if let Some(delay) = self.recall_delay {
                tokio::time::sleep(delay).await;
            }
            if self.recall_error {
                return Err(ExternalMemoryError::new("fake recall failure"));
            }
            Ok(self.items.clone())
        }

        async fn sync_turn(&self, turn: TurnRecord) -> Result<(), ExternalMemoryError> {
            self.turns.lock().unwrap().push(turn);
            assert!(!self.panics, "fake sync_turn panic");
            Ok(())
        }

        async fn on_memory_write(
            &self,
            event: MemoryWriteEvent,
        ) -> Result<(), ExternalMemoryError> {
            // Recorded on entry, so a call that hangs is still seen.
            let hang = self.hang_write_content.as_deref() == Some(event.content.as_str());
            let panics = self.panic_write_content.as_deref() == Some(event.content.as_str());
            self.writes.lock().unwrap().push(event);
            assert!(!panics, "fake on_memory_write panic");
            if hang {
                std::future::pending::<()>().await;
            }
            Ok(())
        }

        async fn status(&self) -> ProviderStatus {
            assert!(!self.panics, "fake status panic");
            if let Some(delay) = self.status_delay {
                tokio::time::sleep(delay).await;
            }
            match &self.unavailable {
                Some(reason) => ProviderStatus::Unavailable(reason.clone()),
                None => ProviderStatus::Ready,
            }
        }
    }

    /// How many snapshot diffs this thread's dispatch has run.
    pub(crate) fn snapshot_diffs() -> usize {
        SNAPSHOT_DIFFS.with(std::cell::Cell::get)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod profile_ownership_tests {
    use super::*;
    use crate::profile_owner::ProfileOwner;
    use test_support::{inject_with, FakeProvider};

    fn turn(text: &str) -> TurnRecord {
        TurnRecord {
            agent_id: "owner".into(),
            session_id: "session".into(),
            user_text: text.into(),
            reply_text: "reply".into(),
            at_ms: 1,
        }
    }
    fn event(content: &str) -> MemoryWriteEvent {
        MemoryWriteEvent {
            agent_id: "owner".into(),
            action: MemoryWriteAction::Add,
            memory_type: "fact".into(),
            memory_id: content.into(),
            content: content.into(),
        }
    }

    #[tokio::test]
    async fn profile_owner_queued_remote_write_and_turn_are_refused_after_loss() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.json");
        let owner = ProfileOwner::acquire([path.clone()]).unwrap();
        let mut fake = FakeProvider::new("profile-owner-queue");
        fake.hang_write_content = Some("hold".into());
        let fake = Arc::new(fake);
        let _injected = inject_with(
            fake.clone(),
            8,
            Duration::from_millis(200),
            Duration::from_secs(1),
        );
        active().unwrap().enqueue([Job {
            payload: JobPayload::Write(event("hold")),
            owner: None,
        }]);
        fake.wait_until(|provider| !provider.writes.lock().unwrap().is_empty())
            .await;
        let mut manager = anima_memory::MemoryManager::new();
        let memory = manager
            .add(anima_memory::NewMemory {
                agent_id: "owner".into(),
                agent_name: "Owner".into(),
                memory_type: anima_memory::MemoryType::Fact,
                content: "must not dispatch".into(),
                importance: 0.5,
                tags: None,
                scope: None,
                room_id: None,
                world_id: None,
                session_id: None,
            })
            .unwrap();
        mirror_persisted_with_owner(&[], || vec![memory], Some(owner.clone()));
        sync_turn_with_owner(Some(owner.clone()), || turn("must not dispatch"));
        owner.mark_lost();
        drop(owner);
        sync_turn(|| turn("barrier"));
        fake.wait_until(|provider| !provider.turns.lock().unwrap().is_empty())
            .await;
        assert_eq!(
            fake.writes.lock().unwrap().len(),
            1,
            "a revoked queued memory write reached the provider"
        );
        assert_eq!(fake.turns.lock().unwrap()[0].user_text, "barrier");
        ProfileOwner::acquire([path]).unwrap();
    }

    #[tokio::test]
    async fn profile_owner_admitted_remote_job_retains_lease_through_await() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.json");
        let owner = ProfileOwner::acquire([path.clone()]).unwrap();
        let mut fake = FakeProvider::new("profile-owner-inflight");
        fake.hang_write_content = Some("hold".into());
        let fake = Arc::new(fake);
        let injected = inject_with(
            fake.clone(),
            8,
            Duration::from_millis(300),
            Duration::from_secs(1),
        );
        active().unwrap().enqueue([Job {
            payload: JobPayload::Write(event("hold")),
            owner: Some(owner),
        }]);
        fake.wait_until(|provider| !provider.writes.lock().unwrap().is_empty())
            .await;
        drop(injected);
        assert!(
            ProfileOwner::acquire([path.clone()]).is_err(),
            "a dispatched provider future released its profile authority"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if ProfileOwner::acquire([path.clone()]).is_ok() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "completed provider job leaked ownership"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
