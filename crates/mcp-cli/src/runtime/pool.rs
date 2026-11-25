//! The helper's pool of live server sessions.
//!
//! One [`Session`] per server id, opened lazily and shared by all calls of a run. The pool
//! keeps one `OnceCell` per id and releases its lock before awaiting the connect, so two
//! concurrent first calls to one server produce one initialize and no lock is held across an
//! await.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use mcp_execution_core::untrusted::{MAX_UNTRUSTED_FIELD_LEN, sanitize_untrusted_text};
use mcp_execution_core::{Error as CoreError, ResourceKind, ServerId, ToolName};
use mcp_execution_introspector::{
    CallTimeout, Introspector, ServerText, Session, ToolCall, ToolCallOutcome,
};
use tokio::sync::OnceCell;

use super::journal::CallStatus;
use super::protocol::{ErrorBody, ErrorKind, Request, ResponseBody};
use crate::commands::common::{ServerSource, resolve_server_config};

/// Most servers a run may have open at once.
pub const MAX_POOLED_SERVERS: usize = 16;

/// Most calls in flight at once; further requests queue.
pub const MAX_INFLIGHT_CALLS: usize = 16;

type Cell = Arc<OnceCell<Session>>;

/// Lazily opened sessions, one per server id.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::pool::SessionPool;
/// use mcp_execution_introspector::CallTimeout;
///
/// let pool = SessionPool::new(CallTimeout::new(std::time::Duration::from_secs(60)).unwrap());
/// # let _ = pool;
/// ```
#[derive(Debug)]
pub struct SessionPool {
    call_timeout: CallTimeout,
    introspector: Introspector,
    cells: Mutex<HashMap<ServerId, Cell>>,
}

impl SessionPool {
    /// Creates an empty pool whose calls use `call_timeout` unless narrowed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::pool::SessionPool;
    /// use mcp_execution_introspector::CallTimeout;
    ///
    /// let timeout = CallTimeout::new(std::time::Duration::from_secs(60)).unwrap();
    /// let _pool = SessionPool::new(timeout);
    /// ```
    #[must_use]
    pub fn new(call_timeout: CallTimeout) -> Self {
        Self {
            call_timeout,
            introspector: Introspector::new(),
            cells: Mutex::new(HashMap::new()),
        }
    }

    /// Performs one call and maps every outcome, including failures, onto a [`CallReply`].
    ///
    /// The server is opened on first use from `~/.claude/mcp.json`. Runner-written text is in
    /// `message`; server text only in `server_message`, sanitized and capped.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::pool::SessionPool;
    /// # use mcp_execution_cli::runtime::protocol::Request;
    /// # async fn example(pool: &SessionPool, request: Request) {
    /// let reply = pool.call(request).await;
    /// println!("{:?}", reply.status);
    /// # }
    /// ```
    pub async fn call(&self, request: Request) -> CallReply {
        let Request {
            server,
            tool,
            arguments,
            result: result_source,
            ..
        } = request;
        let result = async {
            let cell = self.cell_for(&server)?;
            let session = self.open(&cell, &server).await?;
            let outcome = session
                .call_tool(
                    ToolCall::new(&tool, arguments, result_source),
                    self.call_timeout,
                )
                .await;
            if session.is_poisoned() {
                self.evict_if_current(&server, &cell);
            }
            outcome
        }
        .await;
        match result {
            Ok(outcome) => reply_of(&server, &tool, outcome),
            Err(error) => failure_reply(&error),
        }
    }

    /// Closes every open session, killing the server processes it owns.
    ///
    /// A session still referenced by a call that has not finished is dropped instead, which
    /// also kills its child.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::pool::SessionPool;
    /// # async fn example(pool: &SessionPool) {
    /// pool.close().await;
    /// # }
    /// ```
    pub async fn close(&self) {
        let cells: Vec<Cell> = self.lock().drain().map(|(_, cell)| cell).collect();
        for cell in cells {
            if let Some(session) = Arc::into_inner(cell).and_then(OnceCell::into_inner) {
                session.close().await;
            }
        }
    }

    /// Opens the session of `cell` once; on failure drops the map entry only if it is still
    /// this very cell, so a newer attempt is never evicted.
    async fn open<'a>(&self, cell: &'a Cell, id: &ServerId) -> Result<&'a Session, CoreError> {
        let opened = cell
            .get_or_try_init(|| async {
                let (server_id, config) = resolve_server_config(ServerSource::Config {
                    name: id.to_string(),
                })
                .map_err(|e| CoreError::InvalidArgument(format!("{e:#}")))?;
                self.introspector.connect(server_id, &config).await
            })
            .await;
        if opened.is_err() {
            self.evict_if_current(id, cell);
        }
        opened
    }

    /// Removes the entry of `id` only if it is still `cell`, so a newer attempt survives.
    fn evict_if_current(&self, id: &ServerId, cell: &Cell) {
        let mut cells = self.lock();
        if cells.get(id).is_some_and(|c| Arc::ptr_eq(c, cell)) {
            cells.remove(id);
        }
    }

    fn cell_for(&self, id: &ServerId) -> Result<Cell, CoreError> {
        let mut cells = self.lock();
        if let Some(cell) = cells.get(id) {
            return Ok(Arc::clone(cell));
        }
        if cells.len() >= MAX_POOLED_SERVERS {
            return Err(CoreError::InvalidArgument(format!(
                "more than {MAX_POOLED_SERVERS} servers in one run"
            )));
        }
        let cell = Cell::default();
        cells.insert(id.clone(), Arc::clone(&cell));
        drop(cells);
        Ok(cell)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ServerId, Cell>> {
        self.cells.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn failure_reply(error: &CoreError) -> CallReply {
    let (status, text) = match error {
        CoreError::Timeout { .. } => (CallStatus::TimedOut, error.to_string()),
        CoreError::ResourceLimitExceeded {
            resource: ResourceKind::ResponseLine { .. },
            limit,
            ..
        } => (CallStatus::Failed, format!("result exceeds {limit} bytes")),
        _ => (CallStatus::Failed, error.to_string()),
    };
    CallReply {
        status,
        body: failed(&sanitize_untrusted_text(&text, MAX_UNTRUSTED_FIELD_LEN)),
    }
}

fn failed(message: &str) -> ResponseBody {
    ResponseBody::Error {
        error: ErrorBody {
            kind: ErrorKind::CallFailed,
            message: message.to_string(),
            server_message: None,
            server_message_truncated: None,
        },
    }
}

/// The answer to one call: the protocol body and the status the journal records.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::journal::CallStatus;
/// use mcp_execution_cli::runtime::pool::CallReply;
/// use mcp_execution_cli::runtime::protocol::ResponseBody;
///
/// let reply = CallReply { body: ResponseBody::Empty { empty: true }, status: CallStatus::Empty };
/// assert_eq!(reply.status, CallStatus::Empty);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallReply {
    /// What goes back to the script.
    pub body: ResponseBody,
    /// How the call ended, for the journal.
    pub status: CallStatus,
}

fn reply_of(server: &ServerId, tool: &ToolName, outcome: ToolCallOutcome) -> CallReply {
    let (kind, status, message, text) = match outcome {
        ToolCallOutcome::Value(value) => {
            return CallReply {
                body: ResponseBody::Value { value },
                status: CallStatus::Ok,
            };
        }
        ToolCallOutcome::Empty => {
            return CallReply {
                body: ResponseBody::Empty { empty: true },
                status: CallStatus::Empty,
            };
        }
        ToolCallOutcome::ToolError { server_text } => (
            ErrorKind::ToolError,
            CallStatus::ToolError,
            format!("Tool returned error: {server}.{tool}"),
            server_text,
        ),
        ToolCallOutcome::RpcError { code, server_text } => {
            let code = code.map_or_else(String::new, |c| format!(" (JSON-RPC error code {c})"));
            (
                ErrorKind::RpcError,
                CallStatus::RpcError,
                format!("Tool execution failed: {server}.{tool}{code}"),
                server_text,
            )
        }
        _ => (
            ErrorKind::RpcError,
            CallStatus::RpcError,
            format!("Tool execution failed: {server}.{tool}"),
            None,
        ),
    };
    let (server_message, server_message_truncated) = match text {
        Some(ServerText { text, truncated }) => (Some(text), Some(truncated)),
        None => (None, None),
    };
    CallReply {
        body: ResponseBody::Error {
            error: ErrorBody {
                kind,
                message,
                server_message,
                server_message_truncated,
            },
        },
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::helper_protocol::ResultSource;
    use std::assert_matches;
    use std::time::Duration;

    fn pool() -> SessionPool {
        SessionPool::new(CallTimeout::new(Duration::from_secs(5)).unwrap())
    }

    fn request(server: &str) -> Request {
        Request {
            id: 1,
            server: ServerId::new(server).unwrap(),
            tool: mcp_execution_core::ToolName::new("echo").unwrap(),
            arguments: serde_json::Map::new(),
            result: ResultSource::Content,
        }
    }

    #[tokio::test]
    async fn a_failed_open_leaves_no_entry_so_the_next_call_retries() {
        let pool = pool();

        let first = pool.call(request("no-such-server-for-pool-test")).await;
        assert_matches!(first.body, ResponseBody::Error { .. }, "{first:?}");
        assert_eq!(first.status, CallStatus::Failed);
        assert!(pool.lock().is_empty());

        let second = pool.call(request("no-such-server-for-pool-test")).await;
        assert_matches!(second.body, ResponseBody::Error { .. }, "{second:?}");
    }

    #[test]
    fn the_pool_refuses_more_than_the_server_cap() {
        let pool = pool();
        for index in 0..MAX_POOLED_SERVERS {
            pool.cell_for(&ServerId::new(format!("s{index}")).unwrap())
                .unwrap();
        }

        assert!(
            pool.cell_for(&ServerId::new("one-too-many").unwrap())
                .is_err()
        );
        assert!(pool.cell_for(&ServerId::new("s0").unwrap()).is_ok());
    }

    #[test]
    fn eviction_spares_a_newer_cell_for_the_same_id() {
        let pool = pool();
        let id = ServerId::new("demo").unwrap();
        let stale = pool.cell_for(&id).unwrap();
        pool.lock().remove(&id);
        let newer = pool.cell_for(&id).unwrap();

        pool.evict_if_current(&id, &stale);
        assert!(pool.lock().get(&id).is_some_and(|c| Arc::ptr_eq(c, &newer)));

        pool.evict_if_current(&id, &newer);
        assert!(pool.lock().is_empty());
    }
}
