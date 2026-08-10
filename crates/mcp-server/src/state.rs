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

    /// Checks whether adding `size_bytes` worth of new entry data would stay within
    /// [`MAX_PENDING_SESSIONS`]/[`MAX_TOTAL_PENDING_BYTES`], without mutating anything.
    ///
    /// Shared by [`StateManager::store`] and [`StateManager::restore`] so both insertion paths
    /// enforce the exact same CWE-400 bounds — `restore` re-inserting a session it previously
    /// removed must not be able to silently bypass the caps a fresh `store` would be held to
    /// (issue #379 S1: without this, a caller could free up budget with concurrent `store`s
    /// during another session's checkout window, then have that session's `restore` land on top
    /// of the refilled budget).
    fn check_capacity(&self, size_bytes: usize) -> Result<(), StateError> {
        if self.entries.len() >= MAX_PENDING_SESSIONS {
            return Err(StateError::AtCapacity {
                limit: MAX_PENDING_SESSIONS,
            });
        }

        if self.total_bytes.saturating_add(size_bytes) > MAX_TOTAL_PENDING_BYTES {
            return Err(StateError::MemoryBudgetExceeded {
                limit: MAX_TOTAL_PENDING_BYTES,
            });
        }

        Ok(())
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
            table.check_capacity(size_bytes)?;

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

    /// Validates a pending generation in place and consumes it only if validation succeeds.
    ///
    /// `validate` runs against the live entry while the write lock is held, so a failed
    /// validation (e.g. a retried call with a bad tool name) never pays [`PendingGeneration`]'s
    /// clone cost the way a `get`-then-`take` pattern would (issue #378) — [`Self::get`] deep
    /// clones the entire session, including every introspected tool's schema, up to
    /// `MAX_SINGLE_SESSION_BYTES` worth of data per call.
    ///
    /// Returns `None` if the session is not found or has expired — the same miss reported by
    /// [`Self::get`]/[`Self::take`]. Returns `Some(Err(e))` if `validate` rejected the entry,
    /// leaving the session in place at its original expiry so the caller can retry with the same
    /// `session_id`. Returns `Some(Ok((generation, size_bytes, value)))` if `validate` accepted
    /// it: the entry is removed from the table and its owned [`PendingGeneration`] is handed back
    /// alongside its already-known `size_bytes` (the exact value this call's own removal just
    /// subtracted from the table's running total — nothing is re-derived) and `validate`'s
    /// output, so a caller that needs the session's data for further (fallible) work downstream
    /// of validation can restore both to `Self::restore` without re-fetching, re-cloning, or
    /// re-serializing anything.
    ///
    /// `validate` must be synchronous and touch only its `&PendingGeneration` argument — it runs
    /// while the table's write lock is held, so anything slower (I/O, another lock) would block
    /// every other session in the table for its duration.
    ///
    /// # Panics
    ///
    /// Never in practice: the internal `expect` on the successful-validation path removes the
    /// same entry that was just looked up moments earlier under the same continuously-held write
    /// lock, so it cannot have disappeared in between.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::state::StateManager;
    /// # use mcp_execution_server::types::PendingGeneration;
    /// # use mcp_execution_core::{ServerId, ServerConfig};
    /// # use mcp_execution_introspector::ServerInfo;
    ///
    /// # async fn example(pending: PendingGeneration) {
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await.unwrap();
    ///
    /// // A failed validation leaves the session in place.
    /// let rejected = state
    ///     .take_if(session_id, |_generation| Err::<(), _>("bad input"))
    ///     .await;
    /// assert!(matches!(rejected, Some(Err("bad input"))));
    /// assert!(state.get(session_id).await.is_some());
    ///
    /// // A successful validation consumes the session and hands back all three parts.
    /// let accepted = state
    ///     .take_if(session_id, |generation| Ok::<_, &str>(generation.server_id.clone()))
    ///     .await;
    /// assert!(accepted.is_some());
    /// let (generation, _size_bytes, server_id) = accepted.unwrap().unwrap();
    /// assert_eq!(generation.server_id, server_id);
    /// assert!(state.get(session_id).await.is_none());
    /// # }
    /// ```
    pub async fn take_if<T, E>(
        &self,
        session_id: Uuid,
        validate: impl FnOnce(&PendingGeneration) -> Result<T, E>,
    ) -> Option<Result<(PendingGeneration, usize, T), E>> {
        let mut table = self.pending.write().await;
        table.sweep_expired(self.clock.as_ref());

        let entry = table.entries.get(&session_id)?;
        if entry.generation.is_expired(self.clock.as_ref()) {
            return None;
        }

        let outcome = match validate(&entry.generation) {
            Ok(value) => {
                let entry = table
                    .entries
                    .remove(&session_id)
                    .expect("entry was just looked up above under the same held write lock");
                table.total_bytes -= entry.size_bytes;
                Ok((entry.generation, entry.size_bytes, value))
            }
            Err(e) => Err(e),
        };
        drop(table);
        Some(outcome)
    }

    /// Re-inserts a previously-[`Self::take`]n or [`Self::take_if`]-consumed session under its
    /// original `session_id`, preserving its original `expires_at` rather than granting a fresh
    /// TTL.
    ///
    /// Used to undo a consuming removal when the work the caller intended to do with the session
    /// afterward fails for a transient, retriable reason (e.g. a downstream I/O error during
    /// export) — so the caller isn't forced back to re-establishing the session from scratch for
    /// a failure that has nothing to do with the session's own validity (issue #379). `size_bytes`
    /// must be the value [`Self::take_if`] (or [`estimate_size_bytes`]) previously computed for
    /// `generation`, so re-inserting it never re-serializes the session to re-derive a size this
    /// call's caller already had (issue #378 S2).
    ///
    /// Enforces the exact same [`MAX_PENDING_SESSIONS`]/[`MAX_TOTAL_PENDING_BYTES`] bounds as
    /// [`Self::store`] (issue #379 S1): without this, a session checked out via `take_if` is
    /// briefly unaccounted for while its caller's own pipeline runs, and a client could exploit
    /// that window — filling the freed budget with fresh `store`s, then having this call land on
    /// top of it — to park the table above its configured caps for the remainder of the original
    /// session's TTL. A caller whose `restore` fails this way has no better option than the
    /// session genuinely being lost: silently dropping the cap instead would defeat the exact
    /// protection [`StateError`] exists to provide.
    ///
    /// `pub(crate)`, not `pub`: unlike `store`/`take`/`get`, nothing outside this crate can call
    /// it correctly — a caller needs `size_bytes` to already be the exact value this same
    /// `generation` was measured at, a value [`estimate_size_bytes`] (private) is the only way to
    /// produce, and passing an arbitrary `session_id` bypasses `store`'s minted-`Uuid` invariant.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::AtCapacity`] or [`StateError::MemoryBudgetExceeded`] under the same
    /// conditions as [`Self::store`], in which case `generation` is dropped rather than
    /// re-inserted — the session is genuinely lost, exactly as a fresh `store` under the same
    /// pressure would be rejected rather than silently exceeding the configured caps.
    ///
    /// # Examples
    ///
    /// Illustrative only (`ignore`d, not compiled): `restore` is `pub(crate)`, so it cannot be
    /// called from a doc-test, which compiles as a separate external crate.
    ///
    /// ```ignore
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await.unwrap();
    ///
    /// let (taken, size_bytes, ()) = state
    ///     .take_if(session_id, |_generation| Ok::<_, &str>(()))
    ///     .await
    ///     .unwrap()
    ///     .unwrap();
    /// assert!(state.get(session_id).await.is_none());
    ///
    /// // Simulated downstream failure: hand the session back.
    /// state.restore(session_id, taken, size_bytes).await.unwrap();
    /// assert!(state.get(session_id).await.is_some());
    /// ```
    pub(crate) async fn restore(
        &self,
        session_id: Uuid,
        generation: PendingGeneration,
        size_bytes: usize,
    ) -> Result<(), StateError> {
        let mut table = self.pending.write().await;
        table.sweep_expired(self.clock.as_ref());
        table.check_capacity(size_bytes)?;

        if let Some(previous) = table.entries.insert(
            session_id,
            PendingEntry {
                generation,
                size_bytes,
            },
        ) {
            table.total_bytes -= previous.size_bytes;
        }
        table.total_bytes += size_bytes;
        drop(table);

        Ok(())
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

    /// Test-only hook to seed the table's running byte total directly, without paying for a
    /// multi-hundred-megabyte allocation to organically reach [`MAX_TOTAL_PENDING_BYTES`] (this
    /// module's own `test_store_rejects_when_would_exceed_memory_budget` does the same via direct
    /// field access; `service.rs`'s tests need this instead, since `pending` is private to this
    /// module).
    #[cfg(test)]
    pub(crate) async fn set_total_bytes_for_test(&self, total_bytes: usize) {
        self.pending.write().await.total_bytes = total_bytes;
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

    // ── take_if / restore (issues #378, #379) ────────────────────────────────

    #[tokio::test]
    async fn test_take_if_success_consumes_session_and_returns_value() {
        let state = StateManager::new();
        let pending = create_test_pending();
        let session_id = state.store(pending.clone()).await.unwrap();

        let result = state
            .take_if(session_id, |generation| {
                Ok::<_, &str>(generation.server_id.clone())
            })
            .await;

        let (generation, size_bytes, server_id) =
            result.expect("session was present").expect("Ok result");
        assert_eq!(generation.server_id, pending.server_id);
        assert_eq!(server_id, pending.server_id);
        assert_eq!(size_bytes, estimate_size_bytes(&pending));

        // Consumed: no longer retrievable.
        assert!(state.get(session_id).await.is_none());
        assert_eq!(state.pending.read().await.total_bytes, 0);
    }

    #[tokio::test]
    async fn test_take_if_validation_failure_retains_session_without_cloning() {
        let state = StateManager::new();
        let pending = create_test_pending();
        let session_id = state.store(pending).await.unwrap();
        let total_bytes_before = state.pending.read().await.total_bytes;

        let first = state
            .take_if(session_id, |_generation| Err::<(), _>("bad input"))
            .await;
        assert!(matches!(first, Some(Err("bad input"))));

        // Session is still present, at the same accounted size, for a repeated
        // clone-free retry against the same live entry.
        assert!(state.get(session_id).await.is_some());
        assert_eq!(state.pending.read().await.total_bytes, total_bytes_before);

        let second = state
            .take_if(session_id, |_generation| Err::<(), _>("bad input again"))
            .await;
        assert!(matches!(second, Some(Err("bad input again"))));
        assert!(state.get(session_id).await.is_some());
    }

    #[tokio::test]
    async fn test_take_if_missing_session_returns_none() {
        let state = StateManager::new();
        let result = state
            .take_if(Uuid::new_v4(), |_generation| Ok::<_, &str>(()))
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_take_if_expired_session_returns_none_and_does_not_call_validate() {
        let state = StateManager::new();
        let session_id = state.store(create_expired_pending()).await.unwrap();

        let mut validate_called = false;
        let result = state
            .take_if(session_id, |_generation| {
                validate_called = true;
                Ok::<_, &str>(())
            })
            .await;

        assert!(result.is_none());
        assert!(!validate_called);
    }

    #[tokio::test]
    async fn test_restore_reinserts_under_same_session_id_and_is_retryable() {
        let state = StateManager::new();
        let pending = create_test_pending();
        let session_id = state.store(pending.clone()).await.unwrap();

        // Simulate `save_categorized_tools` consuming the session via `take_if`
        // and then hitting a transient downstream failure (codegen/export).
        let (generation, size_bytes, ()) = state
            .take_if(session_id, |_generation| Ok::<_, &str>(()))
            .await
            .unwrap()
            .unwrap();
        assert!(state.get(session_id).await.is_none());

        state
            .restore(session_id, generation, size_bytes)
            .await
            .unwrap();

        // Retryable: the same session_id is usable again, with its original data intact.
        let retried = state.get(session_id).await;
        assert!(retried.is_some());
        assert_eq!(retried.unwrap().server_id, pending.server_id);
        assert_eq!(
            state.pending.read().await.total_bytes,
            estimate_size_bytes(&pending)
        );
    }

    /// Issue #379 S1 — `restore` must not be able to push the table above the same
    /// `MAX_TOTAL_PENDING_BYTES` budget a fresh `store` is held to, even though the session being
    /// restored was briefly unaccounted for during its checkout window. Simulates the exploit the
    /// critic flagged: another session fills the budget back up while this one is checked out via
    /// `take_if`, so by the time `restore` runs there is no room left for it.
    #[tokio::test]
    async fn test_restore_rejects_when_would_exceed_memory_budget() {
        let state = StateManager::new();
        let pending = create_test_pending();
        let session_id = state.store(pending).await.unwrap();

        let (generation, size_bytes, ()) = state
            .take_if(session_id, |_generation| Ok::<_, &str>(()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.pending.read().await.total_bytes, 0);

        // Another concurrent session refills the entire budget while this one is checked out.
        {
            let mut table = state.pending.write().await;
            table.total_bytes = MAX_TOTAL_PENDING_BYTES;
        }

        let result = state.restore(session_id, generation, size_bytes).await;

        assert!(
            matches!(result, Err(StateError::MemoryBudgetExceeded { limit }) if limit == MAX_TOTAL_PENDING_BYTES)
        );
        // The rejected session must not have been silently inserted anyway.
        assert!(state.get(session_id).await.is_none());
    }

    /// Same invariant as the byte-budget test above, but for the session-count cap.
    #[tokio::test]
    async fn test_restore_rejects_when_at_capacity() {
        let state = StateManager::new();
        let pending = create_test_pending();
        let session_id = state.store(pending).await.unwrap();

        let (generation, size_bytes, ()) = state
            .take_if(session_id, |_generation| Ok::<_, &str>(()))
            .await
            .unwrap()
            .unwrap();

        // Fill the table to capacity while this session is checked out.
        for _ in 0..MAX_PENDING_SESSIONS {
            state.store(create_test_pending()).await.unwrap();
        }

        let result = state.restore(session_id, generation, size_bytes).await;

        assert!(
            matches!(result, Err(StateError::AtCapacity { limit }) if limit == MAX_PENDING_SESSIONS)
        );
        assert!(state.get(session_id).await.is_none());
    }

    /// Issue #387 gap 1 — `restore` re-inserts a session under its *original* `expires_at`
    /// rather than granting a fresh TTL. Correct by construction (`restore` never touches that
    /// field), but nothing previously exercised it: advances the shared clock past the session's
    /// original expiry between checkout and `restore`, then proves the restored session is
    /// immediately treated as expired by both `get` and `take_if` instead of being resurrected.
    #[tokio::test]
    async fn test_restore_does_not_extend_ttl_past_original_expiry() {
        let start = chrono::Utc::now();
        let clock = Arc::new(TestClock::new(start));
        let state = StateManager::with_clock(Arc::clone(&clock) as Arc<dyn Clock>);

        let pending = create_test_pending_with_clock(clock.as_ref());
        let session_id = state.store(pending).await.unwrap();

        let (generation, size_bytes, ()) = state
            .take_if(session_id, |_generation| Ok::<_, &str>(()))
            .await
            .unwrap()
            .unwrap();

        // Advance the clock past the session's original expiry before restoring it.
        clock.set(
            start
                + chrono::Duration::minutes(PendingGeneration::DEFAULT_TIMEOUT_MINUTES)
                + chrono::Duration::seconds(1),
        );

        state
            .restore(session_id, generation, size_bytes)
            .await
            .unwrap();

        assert!(
            state.get(session_id).await.is_none(),
            "restore must not resurrect a session past its original expires_at"
        );
        let take_if_after_restore = state
            .take_if(session_id, |_generation| Ok::<_, &str>(()))
            .await;
        assert!(
            take_if_after_restore.is_none(),
            "an expired restored session must not be handed to take_if's validate closure"
        );
    }

    /// Issue #387 gap 2 - the write lock `take_if` holds across its whole validate-then-remove
    /// sequence is what makes exactly one racing caller succeed; only `store`'s concurrency path
    /// (`test_concurrent_access` above) had a regression test for this.
    ///
    /// Deliberately runs on a multi-thread runtime, an explicit, justified exception to this
    /// project's no-`multi_thread`-by-default convention: under the default `current_thread`
    /// runtime, spawned tasks are cooperatively scheduled on a single OS thread and can never
    /// actually run in parallel inside `take_if`'s critical section, so a `current_thread`
    /// version of this test is structurally incapable of detecting a broken (non-atomic)
    /// `take_if`, no matter how many times it's repeated - only `multi_thread` gives concurrent
    /// callers a chance to genuinely interleave there. Even under `multi_thread`, a single round
    /// of this particular race is an unreliable detector, so it's repeated many times within the
    /// test rather than run once; the whole test still completes in a few milliseconds, so the
    /// extra rounds are effectively free.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_take_if_same_session_exactly_one_succeeds() {
        let state = Arc::new(StateManager::new());

        for _ in 0..200 {
            let session_id = state.store(create_test_pending()).await.unwrap();

            let mut handles = vec![];
            for _ in 0..10 {
                let state_clone = Arc::clone(&state);
                handles.push(tokio::spawn(async move {
                    state_clone
                        .take_if(session_id, |_generation| Ok::<_, &str>(()))
                        .await
                }));
            }

            let mut successes = 0;
            let mut misses = 0;
            for handle in handles {
                match handle.await.unwrap() {
                    Some(Ok(_)) => successes += 1,
                    None => misses += 1,
                    Some(Err(e)) => panic!("validate always returns Ok in this test, got {e:?}"),
                }
            }

            assert_eq!(successes, 1, "exactly one racing take_if must succeed");
            assert_eq!(
                misses, 9,
                "every other racing take_if must observe the session as gone"
            );
            assert!(state.get(session_id).await.is_none());
        }
    }
}
