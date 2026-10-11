//! Signal registration failures never masquerade as a stop request.
use std::future::Future;
use tracing::{info, warn};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShutdownReason {
    CtrlC,
    Terminate,
}

async fn valid_signal<F, E>(signal: F)
where
    F: Future<Output = Result<(), E>>,
{
    if signal.await.is_err() {
        warn!("failed to install shutdown signal handler; waiting for another signal");
        std::future::pending::<()>().await;
    }
}

pub(super) async fn wait_for_shutdown<C, T, E, U>(ctrl_c: C, terminate: T) -> ShutdownReason
where
    C: Future<Output = Result<(), E>>,
    T: Future<Output = Result<(), U>>,
{
    tokio::select! {
        _ = valid_signal(ctrl_c) => ShutdownReason::CtrlC,
        _ = valid_signal(terminate) => ShutdownReason::Terminate,
    }
}

pub(super) const SHUTDOWN_BUDGET: std::time::Duration = std::time::Duration::from_secs(40);

/// Bounds server completion too: long HTTP handlers cannot extend the stop budget.
/// The timer is supplied only once shutdown actually starts.
pub(super) async fn run_server_until_shutdown<S, T, D>(
    server: S,
    start_deadline: T,
) -> Option<S::Output>
where
    S: Future,
    T: Future<Output = D>,
    D: Future<Output = ()>,
{
    tokio::pin!(server);
    let deadline = tokio::select! {
        biased;
        result = &mut server => return Some(result),
        deadline = start_deadline => deadline,
    };
    tokio::select! {
        biased;
        result = &mut server => Some(result),
        _ = deadline => None,
    }
}

/// Bounds the complete workflow without aborting or relabeling spawned work.
pub(super) async fn run_shutdown_until<D, W>(deadline: D, workflow: W) -> bool
where
    D: Future<Output = ()>,
    W: Future<Output = ()>,
{
    tokio::select! {
        biased;
        _ = workflow => true,
        _ = deadline => false,
    }
}

/// Producer admission closes before any admitted-work drain is polled.
pub(super) async fn producer_first_shutdown<C, D>(close_producers: C, drain: D)
where
    C: Future<Output = ()>,
    D: Future<Output = ()>,
{
    close_producers.await;
    drain.await;
}

pub(super) async fn shutdown_signal() {
    #[cfg(unix)]
    let terminate = async {
        let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        signal
            .recv()
            .await
            .ok_or_else(|| std::io::Error::other("signal stream closed"))
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<Result<(), std::io::Error>>();
    let reason = wait_for_shutdown(tokio::signal::ctrl_c(), terminate).await;
    info!(?reason, "shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn overall_budget_also_bounds_server_and_http_completion() {
        let blocked_server = std::future::pending::<Result<(), ()>>();
        assert!(run_server_until_shutdown(
            blocked_server,
            std::future::ready(std::future::ready(()))
        )
        .await
        .is_none());
        let served = std::future::ready(Ok::<(), ()>(()));
        assert_eq!(
            run_server_until_shutdown(served, std::future::pending::<std::future::Ready<()>>())
                .await,
            Some(Ok(()))
        );
        // A completed producer/workflow does not excuse a blocked HTTP handler.
        let workflow = run_shutdown_until(std::future::pending(), std::future::ready(()));
        assert!(workflow.await);
        assert!(run_server_until_shutdown(
            std::future::pending::<()>(),
            std::future::ready(std::future::ready(()))
        )
        .await
        .is_none());
    }

    #[tokio::test]
    async fn overall_budget_bounds_blocked_producer_close_and_drain() {
        assert_eq!(SHUTDOWN_BUDGET, std::time::Duration::from_secs(40));
        for blocked_close in [true, false] {
            let workflow = producer_first_shutdown(
                async {
                    if blocked_close {
                        std::future::pending::<()>().await;
                    }
                },
                std::future::pending::<()>(),
            );
            assert!(!run_shutdown_until(std::future::ready(()), workflow).await);
        }
        assert!(run_shutdown_until(std::future::pending::<()>(), std::future::ready(())).await);
    }

    #[tokio::test]
    async fn producer_close_finishes_before_admitted_work_is_drained() {
        use std::sync::{Arc, Mutex};
        let closed = Arc::new(Mutex::new(Vec::new()));
        let producer_state = Arc::clone(&closed);
        let drained = Arc::clone(&closed);
        producer_first_shutdown(
            async move {
                for producer in ["admission", "jobs", "scheduler", "scanner", "connectors"] {
                    producer_state.lock().unwrap().push(producer);
                }
            },
            async move {
                assert_eq!(
                    *drained.lock().unwrap(),
                    ["admission", "jobs", "scheduler", "scanner", "connectors"]
                );
            },
        )
        .await;
    }

    #[tokio::test]
    async fn injected_signals_select_the_same_stop_path() {
        assert_eq!(
            wait_for_shutdown(
                std::future::ready(Ok::<_, ()>(())),
                std::future::pending::<Result<(), ()>>()
            )
            .await,
            ShutdownReason::CtrlC
        );
        assert_eq!(
            wait_for_shutdown(
                std::future::pending::<Result<(), ()>>(),
                std::future::ready(Ok::<_, ()>(()))
            )
            .await,
            ShutdownReason::Terminate
        );
    }
    #[tokio::test]
    async fn registration_failure_does_not_stop_a_healthy_daemon() {
        let wait = wait_for_shutdown(
            std::future::ready(Err::<(), _>(())),
            std::future::pending::<Result<(), ()>>(),
        );
        tokio::pin!(wait);
        assert!(matches!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(wait.as_mut().poll(cx))).await,
            std::task::Poll::Pending
        ));
    }
    #[tokio::test]
    async fn failed_ctrl_c_registration_still_allows_sigterm() {
        assert_eq!(
            wait_for_shutdown(
                std::future::ready(Err::<(), _>(())),
                std::future::ready(Ok::<_, ()>(()))
            )
            .await,
            ShutdownReason::Terminate
        );
    }
}
