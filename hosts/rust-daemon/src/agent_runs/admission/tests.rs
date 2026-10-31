use super::*;

/// Runs `threads` copies of `make_and_drop_lease` in lockstep for
/// `rounds` rounds: every round, all threads rendezvous on `start` before
/// building and immediately dropping a lease, so every round's drops
/// truly race each other rather than merely interleaving loosely.
fn stress_concurrent_drops(
    threads: usize,
    rounds: usize,
    make_and_drop_lease: impl Fn() + Send + Sync + 'static,
) {
    let make_and_drop_lease = Arc::new(make_and_drop_lease);
    let start = Arc::new(std::sync::Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let make_and_drop_lease = Arc::clone(&make_and_drop_lease);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                for _ in 0..rounds {
                    start.wait();
                    make_and_drop_lease();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("racer thread should not panic");
    }
}

#[test]
fn concurrent_lease_drops_on_one_key_leave_no_stale_registry_entry() {
    let coordinator = RunAdmission::default();

    // Many leases on the SAME (agent, room) key, dropped by many threads
    // at once, over and over: a failed try-acquire racing the holder's
    // own release is one instance of this, but any two leases whose
    // drops overlap can hit it. `SessionLease::drop`/`SlotLease::drop`
    // must decide removal atomically with releasing their own reference,
    // or a race here leaks the registry entry forever (nothing else ever
    // revisits a key once every lease on it has dropped).
    let session_coordinator = coordinator.clone();
    stress_concurrent_drops(32, 4000, move || {
        let (key, lock) = session_coordinator.session_lock("racer-agent", "racer-room");
        drop(SessionLease {
            key,
            lock: Some(lock),
            guard: None,
            locks: Arc::clone(&session_coordinator.session_locks),
        });
    });
    assert_eq!(
        coordinator.lock_counts(),
        (0, 0),
        "racing session-lease drops on one key must not leak a stale registry entry"
    );

    let slot_coordinator = coordinator.clone();
    stress_concurrent_drops(32, 4000, move || {
        let slots = slot_coordinator.agent_slot_semaphore("racer-agent", 1);
        drop(SlotLease {
            agent_id: "racer-agent".to_string(),
            slots: Some(slots),
            permit: None,
            registry: Arc::clone(&slot_coordinator.agent_slots),
        });
    });
    assert_eq!(
        coordinator.lock_counts(),
        (0, 0),
        "racing slot-lease drops on one key must not leak a stale registry entry"
    );
}

/// The waiting budget is per agent and try-only, and a unit comes back on
/// every exit of the task holding it, including cancellation and panic.
#[tokio::test]
async fn waiting_budget_is_per_agent_try_only_and_released_on_every_exit() {
    let coordinator = RunAdmission::default();
    let mut units: Vec<_> = (0..MAX_QUEUED_RUNS_PER_AGENT)
        .map(|_| {
            coordinator
                .try_take_waiting_unit("agent-x")
                .expect("within the budget")
        })
        .collect();
    assert!(
        coordinator.try_take_waiting_unit("agent-x").is_none(),
        "a ninth waiter is refused without waiting"
    );
    let other = coordinator
        .try_take_waiting_unit("agent-y")
        .expect("each agent has its own budget");

    let cancelled = {
        let unit = units.pop().unwrap();
        tokio::spawn(async move {
            let _unit = unit;
            std::future::pending::<()>().await;
        })
    };
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    let panicked = {
        let unit = units.pop().unwrap();
        tokio::spawn(async move {
            let _unit = unit;
            panic!("simulated panic in a task holding a waiting unit");
        })
    };
    assert!(panicked.await.unwrap_err().is_panic());
    assert_eq!(
        coordinator.waiting_runs("agent-x"),
        MAX_QUEUED_RUNS_PER_AGENT - 2
    );
    let refill = coordinator
        .try_take_waiting_unit("agent-x")
        .expect("a released unit can be taken again");

    drop((units, refill, other));
    assert!(
        coordinator.waiting_budget.lock().unwrap().is_empty(),
        "an agent with no waiting runs keeps no budget entry"
    );
}
