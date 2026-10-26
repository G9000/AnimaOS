//! Bounded SQLite persistence for the portable execution store.
mod admission;
mod budget;
mod schema;
pub use budget::SqliteBudgetPort;
mod store;
mod worker;

use anima_core::{
    ExecutionClock, ExecutionStoreError, InMemoryExecutionStore, PersistenceProtection,
};
use std::{path::Path, sync::Arc};
use tokio::sync::{mpsc, oneshot};
use worker::{unavailable, Job, Worker};

/// Clones share one bounded worker. Independent opens serialize through SQLite.
#[derive(Clone)]
pub struct SqliteExecutionStore {
    sender: mpsc::Sender<Job>,
    store_id: uuid::Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anima_core::{
        AuthoritativePolicyChange, AuthoritativePolicyState, ExecutionStore, ManualExecutionClock,
        PersistenceSnapshotSealKey,
    };
    use futures::{executor::block_on, poll};
    use std::task::Poll;
    use uuid::Uuid;

    #[tokio::test]
    async fn cancelled_reply_does_not_abandon_an_admitted_write_and_shutdown_drains() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cancel.sqlite");
        let protection =
            PersistenceProtection::payload_free(PersistenceSnapshotSealKey::new([1; 32]).unwrap())
                .unwrap();
        let clock = Arc::new(ManualExecutionClock::default());
        let store = SqliteExecutionStore::open(&path, protection.clone(), clock.clone())
            .await
            .unwrap();
        let (entered, waiting) = oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        store
            .sender
            .send(Job::Operation(Box::new(move |_| {
                let _ = entered.send(());
                released.recv().unwrap();
            })))
            .await
            .unwrap();
        waiting.await.unwrap();
        let owner = Uuid::new_v4();
        let mut write = Box::pin(store.access(true, move |reference| {
            block_on(
                reference.apply_authoritative_policy(
                    owner,
                    AuthoritativePolicyChange::create(
                        AuthoritativePolicyState::active(owner, "cancelled-caller", 1, 1, None)
                            .unwrap(),
                    ),
                ),
            )
        }));
        assert!(matches!(poll!(&mut write), Poll::Pending));
        assert_eq!(store.sender.capacity(), 31);
        drop(write);
        release.send(()).unwrap();
        store.shutdown().await.unwrap();
        assert!(store
            .load_authoritative_policy(owner, "cancelled-caller", 1)
            .await
            .is_err());
        let reopened = SqliteExecutionStore::open(&path, protection, clock)
            .await
            .unwrap();
        assert!(reopened
            .load_authoritative_policy(owner, "cancelled-caller", 1)
            .await
            .unwrap()
            .is_some());
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn every_concurrent_shutdown_waits_for_the_entire_drain() {
        let temp = tempfile::tempdir().unwrap();
        let store = SqliteExecutionStore::open(
            temp.path().join("shutdown.sqlite"),
            PersistenceProtection::payload_free(PersistenceSnapshotSealKey::new([2; 32]).unwrap())
                .unwrap(),
            Arc::new(ManualExecutionClock::default()),
        )
        .await
        .unwrap();
        let (entered, waiting) = oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        store
            .sender
            .send(Job::Operation(Box::new(move |_| {
                entered.send(()).unwrap();
                released.recv().unwrap();
            })))
            .await
            .unwrap();
        waiting.await.unwrap();
        let (first, mut first_done) = oneshot::channel();
        let (second, mut second_done) = oneshot::channel();
        store.sender.send(Job::Shutdown(first)).await.unwrap();
        store.sender.send(Job::Shutdown(second)).await.unwrap();
        let (tail, tail_entered) = oneshot::channel();
        let (finish, finished) = std::sync::mpsc::channel();
        store
            .sender
            .send(Job::Operation(Box::new(move |_| {
                tail.send(()).unwrap();
                finished.recv().unwrap();
            })))
            .await
            .unwrap();
        release.send(()).unwrap();
        tail_entered.await.unwrap();
        let first_pending = matches!(
            first_done.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        let second_pending = matches!(
            second_done.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        );
        finish.send(()).unwrap();
        assert!(
            first_pending && second_pending,
            "shutdown replied before tail retired"
        );
        first_done.await.unwrap();
        second_done.await.unwrap();
    }
}

impl SqliteExecutionStore {
    /// Stable identity of this committed store, for host cross-store bindings.
    pub fn store_id(&self) -> uuid::Uuid {
        self.store_id
    }

    pub async fn open(
        path: impl AsRef<Path>,
        protection: PersistenceProtection,
        clock: Arc<dyn ExecutionClock>,
    ) -> Result<Self, ExecutionStoreError> {
        let path = path.as_ref().to_owned();
        let (sender, mut receiver) = mpsc::channel::<Job>(32);
        let (ready, opened) = oneshot::channel();
        std::thread::Builder::new()
            .name("anima-execution-sqlite".into())
            .spawn(move || {
                let mut worker = match Worker::open(&path, protection, clock) {
                    Ok(worker) => worker,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(worker.store_id)).is_err() {
                    return;
                }
                while let Some(job) = receiver.blocking_recv() {
                    match job {
                        Job::Operation(operation) => operation(&mut worker),
                        Job::Shutdown(done) => {
                            receiver.close();
                            let mut shutdowns = vec![done];
                            while let Some(job) = receiver.blocking_recv() {
                                match job {
                                    Job::Operation(operation) => operation(&mut worker),
                                    Job::Shutdown(other) => shutdowns.push(other),
                                }
                            }
                            drop(worker);
                            for done in shutdowns {
                                let _ = done.send(());
                            }
                            return;
                        }
                    }
                }
            })
            .map_err(|_| unavailable())?;
        let store_id = opened.await.map_err(|_| unavailable())??;
        Ok(Self { sender, store_id })
    }

    /// Closes admission for all clones and drains already admitted writes.
    pub async fn shutdown(&self) -> Result<(), ExecutionStoreError> {
        let (done, finished) = oneshot::channel();
        self.sender
            .send(Job::Shutdown(done))
            .await
            .map_err(|_| unavailable())?;
        finished.await.map_err(|_| unavailable())
    }

    async fn access<T: Send + 'static>(
        &self,
        write: bool,
        operation: impl FnOnce(&InMemoryExecutionStore) -> Result<T, ExecutionStoreError>
            + Send
            + 'static,
    ) -> Result<T, ExecutionStoreError> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Job::Operation(Box::new(move |worker| {
                // Custody survives cancellation of the reply receiver.
                let value = worker.transaction(write, operation);
                let _ = reply.send(value);
            })))
            .await
            .map_err(|_| unavailable())?;
        result.await.map_err(|_| unavailable())?
    }
}
