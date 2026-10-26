use crate::{
    worker::{Job, Worker},
    SqliteExecutionStore,
};
use anima_core::execution_limits::*;
use async_trait::async_trait;
use futures::{future::BoxFuture, FutureExt};
use rusqlite::{params, OptionalExtension, Transaction};
use tokio::sync::oneshot;
use uuid::Uuid;

/// Host-bound access to one durable allowance shared by all child runs.
#[derive(Clone)]
pub struct SqliteBudgetPort {
    store: SqliteExecutionStore,
    owner: Uuid,
    scope: String,
}

fn load(tx: &Transaction<'_>, owner: Uuid, id: &str) -> Result<Option<ExecutionScope>, String> {
    let payload: Option<String> = tx
        .query_row(
            "SELECT payload FROM execution_allowances WHERE owner_id=?1 AND scope_id=?2",
            params![owner.to_string(), id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "execution allowance unavailable")?;
    payload
        .map(|payload| {
            serde_json::from_str::<ExecutionScope>(&payload)
                .map_err(|_| "execution allowance corrupt".into())
        })
        .transpose()
}
fn save(tx: &Transaction<'_>, owner: Uuid, scope: &ExecutionScope) -> Result<(), String> {
    scope.validate()?;
    let payload = serde_json::to_string(scope).map_err(|_| "execution allowance corrupt")?;
    tx.execute("INSERT INTO execution_allowances VALUES (?1,?2,?3) ON CONFLICT(owner_id,scope_id) DO UPDATE SET payload=excluded.payload",params![owner.to_string(),scope.id,payload]).map_err(|_|"execution allowance commit failed")?;
    Ok(())
}

impl SqliteExecutionStore {
    pub(crate) async fn database<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Worker) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(Job::Operation(Box::new(move |worker| {
                let value = operation(worker);
                let _ = reply.send(value);
            })))
            .await
            .map_err(|_| "execution storage unavailable")?;
        result.await.map_err(|_| "execution storage unavailable")?
    }
    pub async fn register_budget(
        &self,
        owner: Uuid,
        scope: ExecutionScope,
    ) -> Result<SqliteBudgetPort, String> {
        if owner.is_nil() {
            return Err("invalid allowance owner".into());
        }
        scope.validate()?;
        let id = scope.id.clone();
        self.database(move |worker| {
            worker.database(|tx, _| {
                if let Some(current) = load(tx, owner, &scope.id)? {
                    if current.root_run_id != scope.root_run_id
                        || current.limits != scope.limits
                        || current.created_at_ms != scope.created_at_ms
                        || current.deadline_at_ms != scope.deadline_at_ms
                    {
                        return Err("execution allowance admission conflicts".into());
                    }
                } else {
                    if !scope.reservations.is_empty() || scope.version != 0 {
                        return Err("new allowance cannot import authority".into());
                    }
                    save(tx, owner, &scope)?;
                }
                Ok(())
            })
        })
        .await?;
        Ok(SqliteBudgetPort {
            store: self.clone(),
            owner,
            scope: id,
        })
    }
    pub async fn bind_budget(&self, owner: Uuid, id: &str) -> Result<SqliteBudgetPort, String> {
        let port = SqliteBudgetPort {
            store: self.clone(),
            owner,
            scope: id.into(),
        };
        port.snapshot().await?;
        Ok(port)
    }
}

impl SqliteBudgetPort {
    pub async fn snapshot(&self) -> Result<ExecutionScope, String> {
        let owner = self.owner;
        let scope = self.scope.clone();
        self.store
            .database(move |worker| {
                worker.database(|tx, _| {
                    load(tx, owner, &scope)?.ok_or("execution allowance unavailable".into())
                })
            })
            .await
    }
    fn check(&self, id: &str) -> Result<(), String> {
        if id == self.scope {
            Ok(())
        } else {
            Err("execution allowance scope denied".into())
        }
    }
}

#[async_trait]
impl ExecutionBudgetPort for SqliteBudgetPort {
    async fn reservation(
        &self,
        id: &str,
        reservation_id: &str,
    ) -> Result<Option<ExecutionReservation>, String> {
        self.check(id)?;
        Ok(self
            .snapshot()
            .await?
            .reservations
            .get(reservation_id)
            .cloned())
    }
    fn authority_binding(&self) -> Option<ExecutionBudgetAuthority> {
        Some(ExecutionBudgetAuthority {
            owner_id: self.owner.to_string(),
            store_id: self.store.store_id.to_string(),
            scope_id: self.scope.clone(),
        })
    }
    async fn reserve(
        &self,
        id: &str,
        request: ReservationRequest,
    ) -> Result<ReservationAdmission, String> {
        self.check(id)?;
        let owner = self.owner;
        let id = self.scope.clone();
        // A rejected reservation still commits the observed clock floor. Candidate
        // changes remain local until the portable ledger accepts them.
        self.store
            .database(move |worker| {
                worker.database(|tx, now| {
                    let mut scope =
                        load(tx, owner, &id)?.ok_or("execution allowance unavailable")?;
                    let result = scope.reserve(request, now);
                    if result.is_ok() {
                        save(tx, owner, &scope)?;
                    }
                    Ok(result)
                })
            })
            .await?
    }
    fn submit_settlement(
        &self,
        id: &str,
        settlement: ReservationSettlement,
    ) -> BoxFuture<'static, Result<(), String>> {
        if let Err(error) = self.check(id) {
            return futures::future::ready(Err(error)).boxed();
        }
        let owner = self.owner;
        let id = self.scope.clone();
        let (reply, result) = oneshot::channel();
        let job = Job::Operation(Box::new(move |worker| {
            let value = worker.database(|tx, _| {
                let mut scope = load(tx, owner, &id)?.ok_or("execution allowance unavailable")?;
                scope.settle(settlement)?;
                save(tx, owner, &scope)
            });
            let _ = reply.send(value);
        }));
        // Submission, not polling the returned future, transfers custody. Queue
        // saturation is explicit; unknown/in-flight charges remain conservative.
        if self.store.sender.try_send(job).is_err() {
            return futures::future::ready(Err("execution settlement queue unavailable".into()))
                .boxed();
        }
        async move { result.await.map_err(|_| "execution storage unavailable")? }.boxed()
    }
    async fn allowance(&self, id: &str) -> Result<ExecutionAllowance, String> {
        self.check(id)?;
        let owner = self.owner;
        let id = self.scope.clone();
        self.store
            .database(move |worker| {
                worker.database(|tx, now| {
                    Ok(load(tx, owner, &id)?
                        .ok_or("execution allowance unavailable")?
                        .allowance(now))
                })
            })
            .await?
    }
}
