//! State storage trait.
//!
//! This module defines the `StateStorage` trait for persistent state management
//! across WASM execution sessions.

use crate::{Result, SessionId};
use async_trait::async_trait;
use serde_json::Value;

/// Provides persistent state storage for WASM executions.
///
/// Implementations of this trait allow WASM code to store and retrieve
/// state across multiple execution sessions, enabling stateful workflows.
///
/// # Type Safety
///
/// All implementations must be `Send + Sync` to work with Tokio's async runtime.
///
/// # Examples
///
/// ```
/// use mcp_core::traits::StateStorage;
/// use mcp_core::{SessionId, Result};
/// use async_trait::async_trait;
/// use serde_json::Value;
/// use std::collections::HashMap;
///
/// struct MemoryState {
///     storage: HashMap<String, HashMap<String, Value>>,
/// }
///
/// impl MemoryState {
///     fn new() -> Self {
///         Self {
///             storage: HashMap::new(),
///         }
///     }
/// }
///
/// #[async_trait]
/// impl StateStorage for MemoryState {
///     async fn get(&self, session: &SessionId, key: &str) -> Result<Option<Value>> {
///         Ok(self.storage
///             .get(session.as_str())
///             .and_then(|s| s.get(key))
///             .cloned())
///     }
///
///     async fn set(&mut self, session: SessionId, key: String, value: Value) -> Result<()> {
///         self.storage
///             .entry(session.into_inner())
///             .or_insert_with(HashMap::new)
///             .insert(key, value);
///         Ok(())
///     }
///
///     async fn remove(&mut self, session: &SessionId, key: &str) -> Result<()> {
///         if let Some(state) = self.storage.get_mut(session.as_str()) {
///             state.remove(key);
///         }
///         Ok(())
///     }
///
///     async fn clear_session(&mut self, session: &SessionId) -> Result<()> {
///         self.storage.remove(session.as_str());
///         Ok(())
///     }
///
///     async fn list_keys(&self, session: &SessionId) -> Result<Vec<String>> {
///         Ok(self.storage
///             .get(session.as_str())
///             .map(|s| s.keys().cloned().collect())
///             .unwrap_or_default())
///     }
/// }
/// ```
#[async_trait]
pub trait StateStorage: Send + Sync {
    /// Retrieves a state value for a session.
    ///
    /// Returns `Ok(Some(value))` if the key exists for this session,
    /// `Ok(None)` if the key is not found.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails (e.g., I/O error,
    /// deserialization error).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::StateStorage;
    /// # use mcp_core::{SessionId, Result};
    /// # async fn example(storage: &impl StateStorage) -> Result<()> {
    /// let session = SessionId::new("session-123");
    /// if let Some(value) = storage.get(&session, "counter").await? {
    ///     println!("Counter: {}", value);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    async fn get(&self, session: &SessionId, key: &str) -> Result<Option<Value>>;

    /// Stores a state value for a session.
    ///
    /// If the key already exists for this session, the value is overwritten.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails (e.g., I/O error,
    /// serialization error, out of space).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::StateStorage;
    /// # use mcp_core::{SessionId, Result};
    /// # use serde_json::json;
    /// # async fn example(storage: &mut impl StateStorage) -> Result<()> {
    /// let session = SessionId::new("session-123");
    /// storage.set(session, "counter".to_string(), json!(42)).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn set(&mut self, session: SessionId, key: String, value: Value) -> Result<()>;

    /// Removes a state value for a session.
    ///
    /// If the key does not exist, this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::StateStorage;
    /// # use mcp_core::{SessionId, Result};
    /// # async fn example(storage: &mut impl StateStorage) -> Result<()> {
    /// let session = SessionId::new("session-123");
    /// storage.remove(&session, "counter").await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn remove(&mut self, session: &SessionId, key: &str) -> Result<()>;

    /// Clears all state for a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::StateStorage;
    /// # use mcp_core::{SessionId, Result};
    /// # async fn example(storage: &mut impl StateStorage) -> Result<()> {
    /// let session = SessionId::new("session-123");
    /// storage.clear_session(&session).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn clear_session(&mut self, session: &SessionId) -> Result<()>;

    /// Lists all keys for a session.
    ///
    /// Returns an empty vector if the session has no state.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::StateStorage;
    /// # use mcp_core::{SessionId, Result};
    /// # async fn example(storage: &impl StateStorage) -> Result<()> {
    /// let session = SessionId::new("session-123");
    /// let keys = storage.list_keys(&session).await?;
    /// println!("Session has {} keys", keys.len());
    /// # Ok(())
    /// # }
    /// ```
    async fn list_keys(&self, session: &SessionId) -> Result<Vec<String>>;

    /// Checks if a key exists for a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::StateStorage;
    /// # use mcp_core::{SessionId, Result};
    /// # async fn example(storage: &impl StateStorage) -> Result<()> {
    /// let session = SessionId::new("session-123");
    /// if storage.contains(&session, "counter").await? {
    ///     println!("Counter exists");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    async fn contains(&self, session: &SessionId, key: &str) -> Result<bool> {
        Ok(self.get(session, key).await?.is_some())
    }

    /// Returns the number of keys in a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::StateStorage;
    /// # use mcp_core::{SessionId, Result};
    /// # async fn example(storage: &impl StateStorage) -> Result<()> {
    /// let session = SessionId::new("session-123");
    /// let count = storage.count(&session).await?;
    /// println!("Session has {} values", count);
    /// # Ok(())
    /// # }
    /// ```
    async fn count(&self, session: &SessionId) -> Result<usize> {
        Ok(self.list_keys(session).await?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    struct TestState {
        data: HashMap<String, HashMap<String, Value>>,
    }

    impl TestState {
        fn new() -> Self {
            Self {
                data: HashMap::new(),
            }
        }
    }

    #[async_trait]
    impl StateStorage for TestState {
        async fn get(&self, session: &SessionId, key: &str) -> Result<Option<Value>> {
            Ok(self
                .data
                .get(session.as_str())
                .and_then(|s| s.get(key))
                .cloned())
        }

        async fn set(&mut self, session: SessionId, key: String, value: Value) -> Result<()> {
            self.data
                .entry(session.into_inner())
                .or_insert_with(HashMap::new)
                .insert(key, value);
            Ok(())
        }

        async fn remove(&mut self, session: &SessionId, key: &str) -> Result<()> {
            if let Some(state) = self.data.get_mut(session.as_str()) {
                state.remove(key);
            }
            Ok(())
        }

        async fn clear_session(&mut self, session: &SessionId) -> Result<()> {
            self.data.remove(session.as_str());
            Ok(())
        }

        async fn list_keys(&self, session: &SessionId) -> Result<Vec<String>> {
            Ok(self
                .data
                .get(session.as_str())
                .map(|s| s.keys().cloned().collect())
                .unwrap_or_default())
        }
    }

    #[tokio::test]
    async fn test_state_set_and_get() {
        let mut state = TestState::new();
        let session = SessionId::new("test-session");

        state
            .set(session.clone(), "key".to_string(), json!("value"))
            .await
            .unwrap();

        let value = state.get(&session, "key").await.unwrap();
        assert_eq!(value, Some(json!("value")));
    }

    #[tokio::test]
    async fn test_state_get_missing() {
        let state = TestState::new();
        let session = SessionId::new("test-session");

        let value = state.get(&session, "missing").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_state_remove() {
        let mut state = TestState::new();
        let session = SessionId::new("test-session");

        state
            .set(session.clone(), "key".to_string(), json!("value"))
            .await
            .unwrap();

        assert!(state.contains(&session, "key").await.unwrap());

        state.remove(&session, "key").await.unwrap();
        assert!(!state.contains(&session, "key").await.unwrap());
    }

    #[tokio::test]
    async fn test_state_clear_session() {
        let mut state = TestState::new();
        let session = SessionId::new("test-session");

        state
            .set(session.clone(), "k1".to_string(), json!("v1"))
            .await
            .unwrap();
        state
            .set(session.clone(), "k2".to_string(), json!("v2"))
            .await
            .unwrap();

        assert_eq!(state.count(&session).await.unwrap(), 2);

        state.clear_session(&session).await.unwrap();
        assert_eq!(state.count(&session).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_state_list_keys() {
        let mut state = TestState::new();
        let session = SessionId::new("test-session");

        state
            .set(session.clone(), "key1".to_string(), json!("v1"))
            .await
            .unwrap();
        state
            .set(session.clone(), "key2".to_string(), json!("v2"))
            .await
            .unwrap();

        let keys = state.list_keys(&session).await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
    }

    #[tokio::test]
    async fn test_state_contains() {
        let mut state = TestState::new();
        let session = SessionId::new("test-session");

        assert!(!state.contains(&session, "key").await.unwrap());

        state
            .set(session.clone(), "key".to_string(), json!("value"))
            .await
            .unwrap();

        assert!(state.contains(&session, "key").await.unwrap());
    }

    #[tokio::test]
    async fn test_state_count() {
        let mut state = TestState::new();
        let session = SessionId::new("test-session");

        assert_eq!(state.count(&session).await.unwrap(), 0);

        state
            .set(session.clone(), "k1".to_string(), json!("v1"))
            .await
            .unwrap();
        assert_eq!(state.count(&session).await.unwrap(), 1);

        state
            .set(session.clone(), "k2".to_string(), json!("v2"))
            .await
            .unwrap();
        assert_eq!(state.count(&session).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_state_session_isolation() {
        let mut state = TestState::new();
        let session1 = SessionId::new("session-1");
        let session2 = SessionId::new("session-2");

        state
            .set(session1.clone(), "key".to_string(), json!("value1"))
            .await
            .unwrap();
        state
            .set(session2.clone(), "key".to_string(), json!("value2"))
            .await
            .unwrap();

        let val1 = state.get(&session1, "key").await.unwrap();
        let val2 = state.get(&session2, "key").await.unwrap();

        assert_eq!(val1, Some(json!("value1")));
        assert_eq!(val2, Some(json!("value2")));
    }

    #[tokio::test]
    async fn test_state_overwrite() {
        let mut state = TestState::new();
        let session = SessionId::new("test-session");

        state
            .set(session.clone(), "key".to_string(), json!("old"))
            .await
            .unwrap();
        state
            .set(session.clone(), "key".to_string(), json!("new"))
            .await
            .unwrap();

        let value = state.get(&session, "key").await.unwrap();
        assert_eq!(value, Some(json!("new")));
    }
}
