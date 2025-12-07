//! State management for pending generation sessions.
//!
//! The `StateManager` stores temporary session data between `introspect_server`
//! and `save_categorized_tools` calls. Sessions expire after 30 minutes and
//! are cleaned up lazily on each operation.

use crate::types::PendingGeneration;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// State manager for pending generation sessions.
///
/// Uses an in-memory `HashMap` protected by `RwLock` for thread-safe access.
/// Sessions expire after 30 minutes and are cleaned up lazily.
///
/// # Examples
///
/// ```
/// use mcp_server::state::StateManager;
/// use mcp_server::types::PendingGeneration;
/// use mcp_core::{ServerId, ServerConfig};
/// use mcp_introspector::ServerInfo;
/// use std::path::PathBuf;
///
/// # async fn example() {
/// let state = StateManager::new();
///
/// # let server_info = ServerInfo {
/// #     id: ServerId::new("test"),
/// #     name: "Test".to_string(),
/// #     version: "1.0.0".to_string(),
/// #     capabilities: mcp_introspector::ServerCapabilities {
/// #         supports_tools: true,
/// #         supports_resources: false,
/// #         supports_prompts: false,
/// #     },
/// #     tools: vec![],
/// # };
/// let pending = PendingGeneration::new(
///     ServerId::new("github"),
///     server_info,
///     ServerConfig::builder().command("npx".to_string()).build(),
///     PathBuf::from("/tmp/output"),
/// );
///
/// // Store and get session ID
/// let session_id = state.store(pending).await;
///
/// // Retrieve session data
/// let retrieved = state.take(session_id).await;
/// assert!(retrieved.is_some());
/// # }
/// ```
#[derive(Debug, Default)]
pub struct StateManager {
    pending: Arc<RwLock<HashMap<Uuid, PendingGeneration>>>,
}

impl StateManager {
    /// Creates a new state manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a pending generation and returns a session ID.
    ///
    /// This operation also performs lazy cleanup of expired sessions.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_server::state::StateManager;
    /// # use mcp_server::types::PendingGeneration;
    /// # use mcp_core::{ServerId, ServerConfig};
    /// # use mcp_introspector::ServerInfo;
    /// # use std::path::PathBuf;
    ///
    /// # async fn example(pending: PendingGeneration) {
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await;
    /// # }
    /// ```
    pub async fn store(&self, generation: PendingGeneration) -> Uuid {
        let session_id = Uuid::new_v4();
        let mut pending = self.pending.write().await;

        // Clean up expired sessions
        pending.retain(|_, g| !g.is_expired());

        pending.insert(session_id, generation);
        session_id
    }

    /// Retrieves and removes a pending generation.
    ///
    /// Returns `None` if the session is not found or has expired.
    /// This operation also performs lazy cleanup of expired sessions.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_server::state::StateManager;
    /// # use mcp_server::types::PendingGeneration;
    /// # use mcp_core::{ServerId, ServerConfig};
    /// # use mcp_introspector::ServerInfo;
    /// # use std::path::PathBuf;
    ///
    /// # async fn example(pending: PendingGeneration) {
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await;
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
        let generation = {
            let mut pending = self.pending.write().await;

            // Clean up expired sessions
            pending.retain(|_, g| !g.is_expired());

            pending.remove(&session_id)?
        };

        // Verify not expired (lock already released)
        if generation.is_expired() {
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
    /// use mcp_server::state::StateManager;
    /// # use mcp_server::types::PendingGeneration;
    /// # use mcp_core::{ServerId, ServerConfig};
    /// # use mcp_introspector::ServerInfo;
    /// # use std::path::PathBuf;
    ///
    /// # async fn example(pending: PendingGeneration) {
    /// let state = StateManager::new();
    /// let session_id = state.store(pending).await;
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
        let pending = self.pending.read().await;
        pending
            .get(&session_id)
            .filter(|g| !g.is_expired())
            .cloned()
    }

    /// Returns the current pending session count (excluding expired).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_server::state::StateManager;
    ///
    /// # async fn example() {
    /// let state = StateManager::new();
    /// assert_eq!(state.pending_count().await, 0);
    /// # }
    /// ```
    pub async fn pending_count(&self) -> usize {
        let pending = self.pending.read().await;
        pending.values().filter(|g| !g.is_expired()).count()
    }

    /// Cleans up all expired sessions.
    ///
    /// Returns the number of sessions that were removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_server::state::StateManager;
    ///
    /// # async fn example() {
    /// let state = StateManager::new();
    /// let removed = state.cleanup_expired().await;
    /// assert_eq!(removed, 0);
    /// # }
    /// ```
    pub async fn cleanup_expired(&self) -> usize {
        let mut pending = self.pending.write().await;
        let before = pending.len();
        pending.retain(|_, g| !g.is_expired());
        before - pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PendingGeneration;
    use chrono::{Duration, Utc};
    use mcp_core::{ServerConfig, ServerId, ToolName};
    use mcp_introspector::ServerInfo;
    use std::path::PathBuf;

    fn create_test_pending() -> PendingGeneration {
        use mcp_introspector::{ServerCapabilities, ToolInfo};

        let server_id = ServerId::new("test");
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
                name: ToolName::new("test_tool"),
                description: "Test tool".to_string(),
                input_schema: serde_json::json!({}),
                output_schema: None,
            }],
        };
        let config = ServerConfig::builder().command("echo".to_string()).build();
        let output_dir = PathBuf::from("/tmp/test");

        PendingGeneration::new(server_id, server_info, config, output_dir)
    }

    fn create_expired_pending() -> PendingGeneration {
        let mut pending = create_test_pending();
        pending.expires_at = Utc::now() - Duration::hours(1);
        pending
    }

    #[tokio::test]
    async fn test_store_and_retrieve() {
        let state = StateManager::new();
        let pending = create_test_pending();

        let session_id = state.store(pending.clone()).await;
        let retrieved = state.take(session_id).await;

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.server_id, pending.server_id);
    }

    #[tokio::test]
    async fn test_take_removes_session() {
        let state = StateManager::new();
        let pending = create_test_pending();

        let session_id = state.store(pending).await;

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

        let session_id = state.store(pending).await;

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

        let session_id = state.store(pending).await;

        // Should return None because expired
        let retrieved = state.take(session_id).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_pending_count() {
        let state = StateManager::new();

        assert_eq!(state.pending_count().await, 0);

        let session_id = state.store(create_test_pending()).await;
        assert_eq!(state.pending_count().await, 1);

        state.take(session_id).await;
        assert_eq!(state.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let state = StateManager::new();

        // Add valid session
        state.store(create_test_pending()).await;

        // Add expired session
        state.store(create_expired_pending()).await;

        assert_eq!(state.pending_count().await, 1); // Only valid session counts

        let removed = state.cleanup_expired().await;
        assert_eq!(removed, 1); // One expired session removed
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
                pending.server_id = ServerId::new(&format!("server-{i}"));
                state_clone.store(pending).await
            }));
        }

        // Wait for all operations to complete
        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(state.pending_count().await, 10);
    }

    #[tokio::test]
    async fn test_lazy_cleanup_on_store() {
        let state = StateManager::new();

        // Store expired session directly
        {
            let mut pending = state.pending.write().await;
            pending.insert(Uuid::new_v4(), create_expired_pending());
        }

        // Store new session triggers cleanup
        state.store(create_test_pending()).await;

        // Only the new session should remain
        assert_eq!(state.pending_count().await, 1);
    }
}
