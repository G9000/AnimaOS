use anima_core::{execution_limits::*, *};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct DurableControl {
    action: Arc<AtomicU8>,
    cancel: CancelSignal,
}
impl DurableControl {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stop(&self) {
        self.action.store(2, Ordering::SeqCst);
        self.cancel.cancel();
    }
    pub fn pause(&self) {
        self.action.store(1, Ordering::SeqCst);
    }
    pub fn resume(&self) {
        self.action.store(3, Ordering::SeqCst);
    }
    /// Restores the existing scope/deadline. Never creates replacement allowance.
    pub fn execution_context(
        &self,
        scope: &ExecutionScope,
        run: Uuid,
        port: Arc<dyn ExecutionBudgetPort>,
        clock: Arc<dyn ExecutionClock>,
    ) -> Result<ExecutionContext, String> {
        scope.validate()?;
        if run.is_nil() {
            return Err("invalid execution run".into());
        }
        Ok(ExecutionContext {
            scope_id: scope.id.clone(),
            run_id: run.to_string(),
            port,
            clock,
            cancel: self.cancel.clone(),
            deadline_at_ms: scope.deadline_at_ms,
            failures: Arc::new(Mutex::new(None)),
            secondary_cancel: None,
        })
    }
}
impl EngineControlSignal for DurableControl {
    fn at_boundary(&self) -> EngineBoundaryAction {
        match self.action.load(Ordering::SeqCst) {
            1 => EngineBoundaryAction::Pause,
            2 => EngineBoundaryAction::Cancel,
            3 => EngineBoundaryAction::Resume,
            _ => EngineBoundaryAction::Continue,
        }
    }
}
