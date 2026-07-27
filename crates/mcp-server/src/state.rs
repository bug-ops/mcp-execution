//! State management for pending generation sessions.
//!
//! The `StateManager` stores temporary session data between `introspect_server`
//! and `save_categorized_tools` calls. Sessions expire after 30 minutes and
//! are cleaned up lazily on each operation.

use crate::clock::{Clock, SystemClock};
use crate::types::PendingGeneration;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Maximum number of concurrent pending generation sessions (denial-of-service protection,
/// CWE-400).
///
/// Sessions are only ever swept lazily (as a side effect of [`StateManager::store`]/
/// [`StateManager::take`]), so without a hard ceiling a caller who repeatedly calls
/// `introspect_server` without ever following up with `save_categorized_tools` could grow
/// this table without bound. This is a structural backstop against unbounded `HashMap` growth
/// (and the iteration cost of sweeping it) independent of session size; [`MAX_TOTAL_PENDING_BYTES`]
/// below is what actually bounds memory, since a session's size can vary by orders of
/// magnitude depending on the introspected server's tool count.
pub const MAX_PENDING_SESSIONS: usize = 1000;

/// Approximate upper bound on a single session's in-memory footprint: up to
/// `mcp_execution_introspector::MAX_TOOL_COUNT` tools, each up to `MAX_TOOL_NAME_LEN` +
/// `MAX_TOOL_DESCRIPTION_LEN` + two independently-bounded schemas (`input_schema` and
/// `output_schema`, each up to `MAX_SCHEMA_SIZE_BYTES` — see `mcp_execution_introspector`'s
/// `build_tool_info`). Used only to derive [`MAX_TOTAL_PENDING_BYTES`] below.
const MAX_SINGLE_SESSION_BYTES: usize = mcp_execution_introspector::MAX_TOOL_COUNT
    * (mcp_execution_introspector::MAX_TOOL_NAME_LEN
        + mcp_execution_introspector::MAX_TOOL_DESCRIPTION_LEN
        + mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES
        + mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES);

/// Maximum combined approximate memory footprint of every pending session at once
/// (denial-of-service protection, CWE-400).
///
/// [`MAX_PENDING_SESSIONS`] alone does not bound memory: per-item caps on tool count/size
/// multiply into a per-session footprint that can itself be hundreds of megabytes (this
/// module's `MAX_SINGLE_SESSION_BYTES`), so a count-only cap of 1000 sessions could still reach
/// hundreds of gigabytes in the worst case (issue #198 S1). This budget is checked in
/// addition to the count cap and is the one that actually matters for memory: set to four
/// times `MAX_SINGLE_SESSION_BYTES`, generous enough for a few concurrent large introspections
/// without silently capping realistic multi-server usage, while categorically ruling out the
/// unbounded multiplication a pure count cap allows.
pub const MAX_TOTAL_PENDING_BYTES: usize = MAX_SINGLE_SESSION_BYTES * 4;

/// Errors from [`StateManager::store`].
#[derive(Debug, Error)]
pub enum StateError {
    /// The pending-session table is already at its configured capacity.
    #[error("too many pending generation sessions: at capacity limit of {limit}")]
    AtCapacity {
        /// The configured maximum (`MAX_PENDING_SESSIONS`).
        limit: usize,
    },

    /// Storing this session would push the aggregate approximate memory footprint of all
    /// pending sessions past [`MAX_TOTAL_PENDING_BYTES`].
    #[error(
        "pending generation sessions would exceed the aggregate memory budget of {limit} bytes"
    )]
    MemoryBudgetExceeded {
        /// The configured maximum (`MAX_TOTAL_PENDING_BYTES`).
        limit: usize,
    },
}

/// Estimates a [`PendingGeneration`]'s in-memory footprint from its serialized
/// [`ServerInfo`](mcp_execution_introspector::ServerInfo) size.
///
/// This is an approximation (it ignores `config`/`output_dir_override`, both small relative to
/// `server_info`'s tool list), used only to enforce [`MAX_TOTAL_PENDING_BYTES`]. A serialization
/// failure is treated as exceeding any bound, rather than silently under-counting a session
/// that could not be measured.
///
/// `serde_json::to_vec`'s `Serialize` implementation for `Value` (nested inside `server_info`'s
/// tool schemas) is unconditionally recursive with no depth limit of its own — the same class
/// of unguarded recursion `mcp-execution-codegen`'s `MAX_SCHEMA_RECURSION_DEPTH` defends
/// against (issue #303). It isn't guarded separately here for the same reason it isn't guarded
/// in `mcp-execution-introspector`'s `build_tool_info`, which is what originally built every
/// schema in `server_info`: `serde_json`'s deserializer already enforces its own default
/// recursion limit while parsing a `tools/list` response, so nothing reaching this function was
/// ever deep enough to threaten a recursive serialize.
fn estimate_size_bytes(generation: &PendingGeneration) -> usize {
    serde_json::to_vec(&generation.server_info).map_or(usize::MAX, |bytes| bytes.len())
}

/// The pending-session table itself, tracking a running approximate byte total alongside the
/// entries so [`MAX_TOTAL_PENDING_BYTES`] can be checked in O(1) rather than re-measuring every
/// session on each call.
#[derive(Debug, Default)]
struct PendingTable {
    entries: HashMap<Uuid, PendingEntry>,
    total_bytes: usize,
}

/// A single stored session alongside its precomputed size, so removing it can decrement
/// [`PendingTable::total_bytes`] without re-measuring.
#[derive(Debug)]
struct PendingEntry {
    generation: PendingGeneration,
    size_bytes: usize,
}

impl PendingTable {
    /// Removes every expired entry, keeping `total_bytes` consistent with what remains.
    fn sweep_expired(&mut self, clock: &dyn Clock) {
        let total_bytes = &mut self.total_bytes;
        self.entries.retain(|_, entry| {
            let keep = !entry.generation.is_expired(clock);
            if !keep {
                *total_bytes -= entry.size_bytes;
            }
            keep
        });
    }
}

/// State manager for pending generation sessions.
///
/// Uses an in-memory `HashMap` protected by `RwLock` for thread-safe access.
/// Sessions expire after 30 minutes and are cleaned up lazily.
///
/// # Examples
///
/// ```
/// use mcp_execution_server::state::StateManager;
/// use mcp_execution_server::types::PendingGeneration;
/// use mcp_execution_server::clock::SystemClock;
/// use mcp_execution_core::{ServerId, ServerConfig};
/// use mcp_execution_introspector::ServerInfo;
///
/// # async fn example() {
/// let state = StateManager::new();
///
/// # let server_info = ServerInfo {
/// #     id: ServerId::new("test").unwrap(),
/// #     name: "Test".to_string(),
/// #     version: "1.0.0".to_string(),
/// #     capabilities: mcp_execution_introspector::ServerCapabilities {
/// #         supports_tools: true,
/// #         supports_resources: false,
/// #         supports_prompts: false,
/// #     },
/// #     tools: vec![],
/// # };
/// let pending = PendingGeneration::new(
///     ServerId::new("github").unwrap(),
///     server_info,
///     ServerConfig::builder().command("npx".to_string()).build().unwrap(),
///     None,
///     &SystemClock,
/// );
///
/// // Store and get session ID
/// let session_id = state.store(pending).await.unwrap();
///
/// // Retrieve session data
/// let retrieved = state.take(session_id).await;
/// assert!(retrieved.is_some());
/// # }
/// ```
#[derive(Debug)]
pub struct StateManager {
    pending: Arc<RwLock<PendingTable>>,
    clock: Arc<dyn Clock>,
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StateManager {
    /// Creates a new state manager using the real system clock.
    #[must_use]
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    /// Creates a new state manager backed by a custom clock.
    ///
    /// Used in tests to inject a fake clock so session expiry can be
    /// exercised deterministically.
    #[must_use]
    pub(crate) fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self {
            pending: Arc::new(RwLock::new(PendingTable::default())),
            clock,
        }
    }

    /// Stores a pending generation and returns a session ID.
    ///
    /// This operation also performs lazy cleanup of expired sessions.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::AtCapacity`] if the pending-session table is already at
    /// [`MAX_PENDING_SESSIONS`] after expired sessions have been swept, or
    /// [`StateError::MemoryBudgetExceeded`] if storing this session would push the aggregate
    /// approximate memory footprint of all pending sessions past [`MAX_TOTAL_PENDING_BYTES`].
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::state::StateManager;
    /// # use mcp_execution_server::types::PendingGeneration;
    /// # use mcp_execution_core::{ServerId, ServerConfig};
    /// # use mcp_execution_introspector::ServerInfo;
    /// # use std::path::PathBuf;
    ///
    /// # async fn example(pending: PendingGeneration) {
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await.unwrap();
    /// # }
    /// ```
    pub async fn store(&self, generation: PendingGeneration) -> Result<Uuid, StateError> {
        let session_id = Uuid::new_v4();
        let size_bytes = estimate_size_bytes(&generation);

        {
            let mut table = self.pending.write().await;
            table.sweep_expired(self.clock.as_ref());

            if table.entries.len() >= MAX_PENDING_SESSIONS {
                return Err(StateError::AtCapacity {
                    limit: MAX_PENDING_SESSIONS,
                });
            }

            if table.total_bytes.saturating_add(size_bytes) > MAX_TOTAL_PENDING_BYTES {
                return Err(StateError::MemoryBudgetExceeded {
                    limit: MAX_TOTAL_PENDING_BYTES,
                });
            }

            table.entries.insert(
                session_id,
                PendingEntry {
                    generation,
                    size_bytes,
                },
            );
            table.total_bytes += size_bytes;
        }

        Ok(session_id)
    }

    /// Retrieves and removes a pending generation.
    ///
    /// Returns `None` if the session is not found or has expired.
    /// This operation also performs lazy cleanup of expired sessions.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::state::StateManager;
    /// # use mcp_execution_server::types::PendingGeneration;
    /// # use mcp_execution_core::{ServerId, ServerConfig};
    /// # use mcp_execution_introspector::ServerInfo;
    /// # use std::path::PathBuf;
    ///
    /// # async fn example(pending: PendingGeneration) {
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await.unwrap();
    ///
    /// let retrieved = state.take(session_id).await;
    /// assert!(retrieved.is_some());
    ///
    /// // Second take returns None (already removed)
    /// let second = state.take(session_id).await;
    /// assert!(second.is_none());
    /// # }
    /// ```
    pub async fn take(&self, session_id: Uuid) -> Option<PendingGeneration> {
        let mut table = self.pending.write().await;
        table.sweep_expired(self.clock.as_ref());

        let entry = table.entries.remove(&session_id)?;
        table.total_bytes -= entry.size_bytes;
        drop(table);
        let generation = entry.generation;

        // Verify not expired (lock already released)
        if generation.is_expired(self.clock.as_ref()) {
            return None;
        }

        Some(generation)
    }

    /// Gets a pending generation without removing it.
    ///
    /// Returns `None` if the session is not found or has expired.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::state::StateManager;
    /// # use mcp_execution_server::types::PendingGeneration;
    /// # use mcp_execution_core::{ServerId, ServerConfig};
    /// # use mcp_execution_introspector::ServerInfo;
    /// # use std::path::PathBuf;
    ///
    /// # async fn example(pending: PendingGeneration) {
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await.unwrap();
    ///
    /// // Get without removing
    /// let peeked = state.get(session_id).await;
    /// assert!(peeked.is_some());
    ///
    /// // Still available
    /// let peeked_again = state.get(session_id).await;
    /// assert!(peeked_again.is_some());
    /// # }
    /// ```
    pub async fn get(&self, session_id: Uuid) -> Option<PendingGeneration> {
        let table = self.pending.read().await;
        let clock = self.clock.as_ref();
        table
            .entries
            .get(&session_id)
            .filter(|entry| !entry.generation.is_expired(clock))
            .map(|entry| entry.generation.clone())
    }

    /// Returns the current pending session count (excluding expired).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::state::StateManager;
    ///
    /// # async fn example() {
    /// let state = StateManager::new();
    /// assert_eq!(state.pending_count().await, 0);
    /// # }
    /// ```
    pub async fn pending_count(&self) -> usize {
        let table = self.pending.read().await;
        let clock = self.clock.as_ref();
        table
            .entries
            .values()
            .filter(|entry| !entry.generation.is_expired(clock))
            .count()
    }

    /// Cleans up all expired sessions.
    ///
    /// Returns the number of sessions that were removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::state::StateManager;
    ///
    /// # async fn example() {
    /// let state = StateManager::new();
    /// let removed = state.cleanup_expired().await;
    /// assert_eq!(removed, 0);
    /// # }
    /// ```
    pub async fn cleanup_expired(&self) -> usize {
        let mut table = self.pending.write().await;
        let before = table.entries.len();
        table.sweep_expired(self.clock.as_ref());
        before - table.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{SystemClock, TestClock};
    use crate::types::PendingGeneration;
    use mcp_execution_core::{ServerConfig, ServerId, ToolName};
    use mcp_execution_introspector::ServerInfo;

    fn create_test_pending() -> PendingGeneration {
        create_test_pending_with_clock(&SystemClock)
    }

    fn create_test_pending_with_clock(clock: &dyn Clock) -> PendingGeneration {
        use mcp_execution_introspector::{ServerCapabilities, ToolInfo};

        let server_id = ServerId::new("test").unwrap();
        let server_info = ServerInfo {
            id: server_id.clone(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new("test_tool").unwrap(),
                description: "Test tool".to_string(),
                input_schema: serde_json::json!({}),
                output_schema: None,
            }],
        };
        let config = ServerConfig::builder()
            .command("echo".to_string())
            .build()
            .unwrap();

        PendingGeneration::new(server_id, server_info, config, None, clock)
    }

    /// Builds an already-expired pending generation by constructing it with a
    /// clock fixed an hour in the past, instead of rewinding `expires_at`
    /// after construction.
    fn create_expired_pending() -> PendingGeneration {
        let past_clock = TestClock::new(chrono::Utc::now() - chrono::Duration::hours(1));
        create_test_pending_with_clock(&past_clock)
    }

    #[tokio::test]
    async fn test_store_and_retrieve() {
        let state = StateManager::new();
        let pending = create_test_pending();

        let session_id = state.store(pending.clone()).await.unwrap();
        let retrieved = state.take(session_id).await;

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.server_id, pending.server_id);
    }

    #[tokio::test]
    async fn test_take_removes_session() {
        let state = StateManager::new();
        let pending = create_test_pending();

        let session_id = state.store(pending).await.unwrap();

        // First take succeeds
        let first = state.take(session_id).await;
        assert!(first.is_some());

        // Second take returns None
        let second = state.take(session_id).await;
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn test_get_does_not_remove() {
        let state = StateManager::new();
        let pending = create_test_pending();

        let session_id = state.store(pending).await.unwrap();

        // Get multiple times
        let first = state.get(session_id).await;
        assert!(first.is_some());

        let second = state.get(session_id).await;
        assert!(second.is_some());

        // Still available for take
        let taken = state.take(session_id).await;
        assert!(taken.is_some());
    }

    #[tokio::test]
    async fn test_expired_session() {
        let state = StateManager::new();
        let pending = create_expired_pending();

        let session_id = state.store(pending).await.unwrap();

        // Should return None because expired
        let retrieved = state.take(session_id).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_pending_count() {
        let state = StateManager::new();

        assert_eq!(state.pending_count().await, 0);

        let session_id = state.store(create_test_pending()).await.unwrap();
        assert_eq!(state.pending_count().await, 1);

        state.take(session_id).await;
        assert_eq!(state.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let state = StateManager::new();

        // Add valid session
        state.store(create_test_pending()).await.unwrap();

        // Add expired session
        state.store(create_expired_pending()).await.unwrap();

        assert_eq!(state.pending_count().await, 1); // Only valid session counts

        let removed = state.cleanup_expired().await;
        assert_eq!(removed, 1); // One expired session removed
    }

    /// Proves `StateManager` consults the clock it was constructed with (not a
    /// hardcoded `SystemClock`): a session created and stored while the shared
    /// clock is fresh must flip to expired across `get`/`pending_count`/
    /// `cleanup_expired`/`take` once that same clock is moved past the TTL —
    /// real wall-clock time barely advances during the test, so this would fail
    /// if any of those call sites silently used `SystemClock` instead of
    /// `self.clock`.
    #[tokio::test]
    async fn test_shared_clock_drives_expiry() {
        let start = chrono::Utc::now();
        let clock = Arc::new(TestClock::new(start));
        let state = StateManager::with_clock(Arc::clone(&clock) as Arc<dyn Clock>);

        let pending = create_test_pending_with_clock(clock.as_ref());
        let session_id = state.store(pending).await.unwrap();

        // Fresh session is visible while the clock is still within the TTL window.
        assert!(state.get(session_id).await.is_some());
        assert_eq!(state.pending_count().await, 1);

        // Jump the shared clock straight past the 30-minute boundary.
        clock.set(
            start
                + chrono::Duration::minutes(PendingGeneration::DEFAULT_TIMEOUT_MINUTES)
                + chrono::Duration::seconds(1),
        );

        assert!(
            state.get(session_id).await.is_none(),
            "expiry should track the injected clock, not Utc::now()"
        );
        assert_eq!(state.pending_count().await, 0);
        assert_eq!(state.cleanup_expired().await, 1);
        assert!(state.take(session_id).await.is_none());
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let state = Arc::new(StateManager::new());
        let mut handles = vec![];

        // Spawn 10 concurrent store operations
        for i in 0..10 {
            let state_clone = Arc::clone(&state);
            handles.push(tokio::spawn(async move {
                let mut pending = create_test_pending();
                pending.server_id = ServerId::new(format!("server-{i}")).unwrap();
                state_clone.store(pending).await
            }));
        }

        // Wait for all operations to complete
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        assert_eq!(state.pending_count().await, 10);
    }

    #[tokio::test]
    async fn test_lazy_cleanup_on_store() {
        let state = StateManager::new();

        // Store expired session directly
        {
            let generation = create_expired_pending();
            let size_bytes = estimate_size_bytes(&generation);
            let mut table = state.pending.write().await;
            table.entries.insert(
                Uuid::new_v4(),
                PendingEntry {
                    generation,
                    size_bytes,
                },
            );
            table.total_bytes += size_bytes;
        }

        // Store new session triggers cleanup
        state.store(create_test_pending()).await.unwrap();

        // Only the new session should remain
        assert_eq!(state.pending_count().await, 1);
    }

    // ── Resource-exhaustion bounds (issue #198) ──────────────────────────────

    #[tokio::test]
    async fn test_store_rejects_when_at_capacity() {
        let state = StateManager::new();
        for _ in 0..MAX_PENDING_SESSIONS {
            state.store(create_test_pending()).await.unwrap();
        }

        let result = state.store(create_test_pending()).await;

        assert!(result.is_err());
        assert!(
            matches!(result, Err(StateError::AtCapacity { limit }) if limit == MAX_PENDING_SESSIONS)
        );
    }

    #[tokio::test]
    async fn test_store_accepts_up_to_exact_capacity() {
        let state = StateManager::new();
        for _ in 0..MAX_PENDING_SESSIONS - 1 {
            state.store(create_test_pending()).await.unwrap();
        }

        let result = state.store(create_test_pending()).await;

        assert!(result.is_ok());
        assert_eq!(state.pending_count().await, MAX_PENDING_SESSIONS);
    }

    /// #198 S1 — the aggregate memory budget, not just the session count, must reject.
    ///
    /// Building enough real sessions to reach `MAX_TOTAL_PENDING_BYTES` (~1GB) would be a slow,
    /// wasteful multi-hundred-MB allocation for every CI run, so this seeds `total_bytes`
    /// directly to just below the cap instead — precisely exercising the same comparison
    /// `store()` performs without materializing gigabytes of real session data.
    #[tokio::test]
    async fn test_store_rejects_when_would_exceed_memory_budget() {
        let state = StateManager::new();
        {
            let mut table = state.pending.write().await;
            table.total_bytes = MAX_TOTAL_PENDING_BYTES;
        }

        let result = state.store(create_test_pending()).await;

        assert!(result.is_err());
        assert!(
            matches!(result, Err(StateError::MemoryBudgetExceeded { limit }) if limit == MAX_TOTAL_PENDING_BYTES)
        );
    }

    #[tokio::test]
    async fn test_store_accepts_at_exact_memory_budget() {
        let state = StateManager::new();
        let generation = create_test_pending();
        let size_bytes = estimate_size_bytes(&generation);
        {
            let mut table = state.pending.write().await;
            table.total_bytes = MAX_TOTAL_PENDING_BYTES - size_bytes;
        }

        let result = state.store(generation).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_take_decrements_total_bytes() {
        let state = StateManager::new();
        let generation = create_test_pending();
        let size_bytes = estimate_size_bytes(&generation);
        assert!(size_bytes > 0);

        let session_id = state.store(generation).await.unwrap();
        assert_eq!(state.pending.read().await.total_bytes, size_bytes);

        state.take(session_id).await;
        assert_eq!(state.pending.read().await.total_bytes, 0);
    }

    #[tokio::test]
    async fn test_sweep_expired_decrements_total_bytes() {
        let state = StateManager::new();
        state.store(create_expired_pending()).await.unwrap();

        // The expired session was counted at store time...
        assert!(state.pending.read().await.total_bytes > 0);

        // ...and swept back out once `cleanup_expired` runs.
        state.cleanup_expired().await;
        assert_eq!(state.pending.read().await.total_bytes, 0);
    }
}
