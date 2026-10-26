use crate::SqliteExecutionStore;
use anima_core::*;
use uuid::Uuid;

#[async_trait::async_trait]
impl ExecutionStore for SqliteExecutionStore {
    async fn load_session_version(
        &self,
        owner_id: Uuid,
        session_id: Uuid,
    ) -> Result<Option<u64>, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(reference.load_session_version(owner_id, session_id))
        })
        .await
    }

    async fn load_completed_output(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<Content>, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(reference.load_completed_output(owner_id, run_id))
        })
        .await
    }

    async fn apply_authoritative_policy(
        &self,
        owner_id: Uuid,
        change: AuthoritativePolicyChange,
    ) -> Result<AuthoritativePolicyState, ExecutionStoreError> {
        self.access(true, move |reference| {
            futures::executor::block_on(async move {
                reference.apply_authoritative_policy(owner_id, change).await
            })
        })
        .await
    }

    async fn load_authoritative_policy(
        &self,
        owner_id: Uuid,
        agent_definition_id: &str,
        agent_definition_version: u32,
    ) -> Result<Option<AuthoritativePolicyState>, ExecutionStoreError> {
        let agent_definition_id = agent_definition_id.to_owned();
        self.access(false, move |reference| {
            futures::executor::block_on(async move {
                reference
                    .load_authoritative_policy(
                        owner_id,
                        &agent_definition_id,
                        agent_definition_version,
                    )
                    .await
            })
        })
        .await
    }

    async fn apply_authoritative_grant(
        &self,
        owner_id: Uuid,
        change: AuthoritativeGrantChange,
    ) -> Result<AuthoritativeGrantState, ExecutionStoreError> {
        self.access(true, move |reference| {
            futures::executor::block_on(async move {
                reference.apply_authoritative_grant(owner_id, change).await
            })
        })
        .await
    }

    async fn load_authoritative_grant(
        &self,
        owner_id: Uuid,
        authority_key: &GrantAuthorityKey,
    ) -> Result<Option<AuthoritativeGrantState>, ExecutionStoreError> {
        let authority_key = authority_key.clone();
        self.access(false, move |reference| {
            futures::executor::block_on(async move {
                reference
                    .load_authoritative_grant(owner_id, &authority_key)
                    .await
            })
        })
        .await
    }

    async fn create_run(
        &self,
        owner_id: Uuid,
        request: CreateRun,
    ) -> Result<StoredRun, ExecutionStoreError> {
        self.access(true, move |reference| {
            futures::executor::block_on(
                async move { reference.create_run(owner_id, request).await },
            )
        })
        .await
    }

    async fn acquire_lease(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        expected_run_version: u64,
        duration_ms: u64,
    ) -> Result<ExecutionLease, ExecutionStoreError> {
        self.access(true, move |reference| {
            futures::executor::block_on(async move {
                reference
                    .acquire_lease(owner_id, run_id, expected_run_version, duration_ms)
                    .await
            })
        })
        .await
    }

    async fn renew_lease(
        &self,
        owner_id: Uuid,
        lease: ExecutionLease,
        duration_ms: u64,
    ) -> Result<ExecutionLease, ExecutionStoreError> {
        self.access(true, move |reference| {
            futures::executor::block_on(async move {
                reference.renew_lease(owner_id, lease, duration_ms).await
            })
        })
        .await
    }

    async fn commit_execution(
        &self,
        owner_id: Uuid,
        commit: ExecutionCommit,
    ) -> Result<ExecutionCommitOutcome, ExecutionStoreError> {
        self.access(true, move |reference| {
            futures::executor::block_on(async move {
                reference.commit_execution(owner_id, commit).await
            })
        })
        .await
    }

    async fn load_run(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<StoredRun>, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(async move { reference.load_run(owner_id, run_id).await })
        })
        .await
    }

    async fn load_checkpoint(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<(u64, CheckpointV1)>, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(
                async move { reference.load_checkpoint(owner_id, run_id).await },
            )
        })
        .await
    }

    async fn load_steps_page(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        page: StoreReadPage,
    ) -> Result<StoreHistoryPage<ExecutionStep>, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(async move {
                reference.load_steps_page(owner_id, run_id, page).await
            })
        })
        .await
    }

    async fn load_attempts_page(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        page: StoreReadPage,
    ) -> Result<StoreHistoryPage<InvocationAttemptRecord>, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(async move {
                reference.load_attempts_page(owner_id, run_id, page).await
            })
        })
        .await
    }

    async fn load_durable_result(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        logical_invocation_id: Uuid,
    ) -> Result<Option<DurableCapabilityResult>, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(async move {
                reference
                    .load_durable_result(owner_id, run_id, logical_invocation_id)
                    .await
            })
        })
        .await
    }

    async fn replay_events(
        &self,
        owner_id: Uuid,
        run_id: Uuid,
        page: StoreReadPage,
    ) -> Result<EventReplayPage, ExecutionStoreError> {
        self.access(false, move |reference| {
            futures::executor::block_on(async move {
                reference.replay_events(owner_id, run_id, page).await
            })
        })
        .await
    }
    async fn retry_run(
        &self,
        owner_id: Uuid,
        request: RetryRun,
    ) -> Result<RetryRunOutcome, ExecutionStoreError> {
        self.access(true, move |reference| {
            futures::executor::block_on(reference.retry_run(owner_id, request))
        })
        .await
    }
}
