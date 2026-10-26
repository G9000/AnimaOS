use anima_core::*;
use anima_execution_store_sqlite::SqliteExecutionStore;
use std::{sync::Arc, time::Instant};
use uuid::Uuid;

fn committed(connection: &rusqlite::Connection) -> (i64, Vec<u8>) {
    connection
        .query_row(
            "SELECT generation,snapshot FROM execution_authority WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

fn request(owner: Uuid, input: &str) -> (Uuid, CreateRun) {
    let session = Session::new(
        Uuid::new_v4(),
        "capacity",
        1,
        SessionConcurrencyPolicy::Serial,
    )
    .unwrap();
    let run = Run::queued(Uuid::new_v4(), session.id(), "capacity", 1).unwrap();
    let id = run.id();
    let request = CreateRun::new_for_owner(
        owner,
        session.clone(),
        run,
        0,
        SessionConcurrencyPolicy::Serial,
    )
    .with_initial_input(DurableRunInput::text(owner, session.id(), id, input).unwrap())
    .unwrap();
    (id, request)
}

/// A local debug-build fixture measurement, not a product latency promise.
#[tokio::test]
async fn snapshot_capacity_failure_preserves_the_last_generation_and_all_runs() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("capacity.sqlite");
    let clock = Arc::new(ManualExecutionClock::new(1_000));
    let protection = PersistenceProtection::allow_model_payloads(
        PersistenceSnapshotSealKey::new([89; 32]).unwrap(),
        vec![],
    )
    .unwrap();
    let store = SqliteExecutionStore::open(&path, protection.clone(), clock.clone())
        .await
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let owner = Uuid::new_v4();
    let input = "capacity-fixture".repeat(2048);
    assert_eq!(input.len(), 32 * 1024);
    let started = Instant::now();
    let mut durations = Vec::new();
    let mut accepted = Vec::new();
    let mut previous = committed(&connection);
    // Prepare a valid near-capacity image through the public reference adapter and
    // install it as fixture data. Only the final SQLite admission ramp is timed;
    // every measured operation still restores/updates the real disk-backed adapter.
    // This avoids hundreds of costly debug full-snapshot exports during setup.
    let seed = InMemoryExecutionStore::with_clock(clock.clone());
    let (id, initial) = request(owner, &input);
    seed.create_run(owner, initial).await.unwrap();
    accepted.push(id);
    let first = seed.export_snapshot(&protection).await.unwrap();
    let per_run = first.as_bytes().len() - previous.1.len();
    let seed_count =
        ((ExecutionStoreSnapshot::maximum_bytes() - previous.1.len()) * 97 / 100 / per_run)
            .clamp(1, 900);
    for _ in 1..seed_count {
        let (id, initial) = request(owner, &input);
        seed.create_run(owner, initial).await.unwrap();
        accepted.push(id);
    }
    let snapshot = seed.export_snapshot(&protection).await.unwrap();
    connection
        .execute(
            "UPDATE execution_authority SET generation=?1,snapshot=?2 WHERE singleton=1",
            rusqlite::params![seed_count as i64, snapshot.as_bytes()],
        )
        .unwrap();
    previous = committed(&connection);
    let mut rejected = None;
    for _ in 0..1000 {
        let (id, request) = request(owner, &input);
        let call = Instant::now();
        let result = store.create_run(owner, request.clone()).await;
        durations.push(call.elapsed());
        match result {
            Ok(_) => {
                accepted.push(id);
                let next = committed(&connection);
                assert_eq!(next.0, previous.0 + 1);
                assert!(next.1.len() > previous.1.len());
                previous = next;
            }
            Err(error) => {
                // The portable snapshot currently reports its size bound as InvalidRequest.
                assert_eq!(error.code(), ExecutionStoreErrorCode::InvalidRequest);
                assert_eq!(committed(&connection), previous);
                rejected = Some((id, request));
                break;
            }
        }
    }
    let (rejected, rejected_request) =
        rejected.expect("capacity must fail within the 1000-operation watchdog");
    assert!(!accepted.is_empty());
    assert!(store.load_run(owner, rejected).await.unwrap().is_none());
    for id in [accepted[0], *accepted.last().unwrap()] {
        assert_eq!(
            store
                .load_run(owner, id)
                .await
                .unwrap()
                .unwrap()
                .initial_input()
                .text_value(),
            input
        );
    }
    // Inspect the real committed disk image once, so verifying every retained run does not
    // perform hundreds of additional full-snapshot SQLite reads near the capacity bound.
    let image = ExecutionStoreSnapshot::from_bytes(previous.1.clone(), &protection).unwrap();
    let reference = InMemoryExecutionStore::from_snapshot(&image, &protection, clock).unwrap();
    for id in &accepted {
        assert_eq!(
            reference
                .load_run(owner, *id)
                .await
                .unwrap()
                .unwrap()
                .initial_input()
                .text_value(),
            input
        );
    }
    assert!(reference.load_run(owner, rejected).await.unwrap().is_none());
    assert!(previous.1.len() <= ExecutionStoreSnapshot::maximum_bytes());
    assert!(previous.1.len() > ExecutionStoreSnapshot::maximum_bytes() * 99 / 100);
    // The same request is valid in the reference state machine; only exporting
    // the enlarged image fails, distinguishing capacity from request rejection.
    reference.create_run(owner, rejected_request).await.unwrap();
    assert_eq!(
        reference
            .export_snapshot(&protection)
            .await
            .unwrap_err()
            .code(),
        ExecutionStoreErrorCode::InvalidRequest
    );
    assert_eq!(committed(&connection), previous);
    durations.sort_unstable();
    let percentile = |percent: usize| {
        // Nearest-rank percentiles keep the small fixture's p95 conservative.
        let rank = (durations.len() * percent).div_ceil(100).saturating_sub(1);
        durations[rank].as_secs_f64() * 1000.0
    };
    println!(
        "LOCAL_SQLITE_CAPACITY_FIXTURE input_bytes={} seeded_runs={} committed_runs={} snapshot_bytes={} limit_bytes={} measured_near_capacity_calls={} p50_ms={:.3} p95_ms={:.3} max_ms={:.3} elapsed_s={:.3} profile=debug fixture=temporary-local-no-provider",
        input.len(), seed_count, accepted.len(), previous.1.len(), ExecutionStoreSnapshot::maximum_bytes(),
        durations.len(), percentile(50), percentile(95), percentile(100), started.elapsed().as_secs_f64()
    );
    store.shutdown().await.unwrap();
}
