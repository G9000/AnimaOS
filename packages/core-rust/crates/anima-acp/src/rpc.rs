//! JSON-RPC 2.0 over newline-delimited stdio: outgoing requests and their
//! pending replies. Incoming dispatch lives in `session.rs`.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
use tokio::sync::{mpsc, oneshot};

pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;
pub(crate) const INTERNAL_ERROR: i64 = -32603;
pub(crate) const AUTH_REQUIRED: i64 = -32000;
pub(crate) const RESOURCE_NOT_FOUND: i64 = -32002;

#[derive(Debug)]
pub(crate) enum CallError {
    Rpc {
        code: i64,
        message: String,
    },
    /// The agent's stdout closed (or the process exited) before a reply.
    Closed,
}

/// Runs inline on the reader task, so a reply is handled before any later
/// line (or the agent's exit) is.
pub(crate) type Reply = Box<dyn FnOnce(Result<Value, CallError>) + Send>;

pub(crate) struct Rpc {
    out: mpsc::UnboundedSender<String>,
    next_id: AtomicU64,
    /// None once the connection closed, so no request can wait forever.
    pending: Mutex<Option<HashMap<u64, Reply>>>,
}

impl Rpc {
    pub(crate) fn new(out: mpsc::UnboundedSender<String>) -> Self {
        Self {
            out,
            next_id: AtomicU64::new(1),
            pending: Mutex::new(Some(HashMap::new())),
        }
    }

    pub(crate) async fn request(&self, method: &str, params: Value) -> Result<Value, CallError> {
        let (tx, rx) = oneshot::channel();
        self.call(
            method,
            params,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );
        rx.await.unwrap_or(Err(CallError::Closed))
    }

    /// Sends a request whose reply goes to `reply`. A closed connection
    /// answers `Closed` at once.
    pub(crate) fn call(&self, method: &str, params: Value, reply: Reply) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            let Some(map) = pending.as_mut() else {
                drop(pending);
                reply(Err(CallError::Closed));
                return;
            };
            map.insert(id, reply);
        }
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if self.out.send(line.to_string()).is_err() {
            if let Some(reply) = self.take(id) {
                reply(Err(CallError::Closed));
            }
        }
    }

    pub(crate) fn notify(&self, method: &str, params: Value) -> bool {
        let line = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.out.send(line.to_string()).is_ok()
    }

    pub(crate) fn respond(&self, id: Value, result: Result<Value, (i64, String)>) {
        let line = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        let _ = self.out.send(line.to_string());
    }

    /// Routes a reply from the agent to its waiting request.
    pub(crate) fn resolve(&self, message: &Value) {
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return;
        };
        let Some(reply) = self.take(id) else {
            return;
        };
        let result = match message.get("error") {
            Some(error) => Err(CallError::Rpc {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            }),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        reply(result);
    }

    /// Fails every waiting request and refuses new ones.
    pub(crate) fn close(&self) {
        let drained = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        for (_, reply) in drained.into_iter().flatten() {
            reply(Err(CallError::Closed));
        }
    }

    fn take(&self, id: u64) -> Option<Reply> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()?
            .remove(&id)
    }
}
