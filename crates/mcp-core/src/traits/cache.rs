//! Cache provider trait.
//!
//! This module defines the `CacheProvider` trait for caching tool call results
//! to reduce redundant calls to MCP servers.

use crate::{CacheKey, Result};
use async_trait::async_trait;
use serde_json::Value;

/// Provides caching capabilities for tool call results.
///
/// Implementations of this trait store and retrieve cached results,
/// reducing redundant calls to MCP servers and improving performance.
///
/// # Type Safety
///
/// All implementations must be `Send + Sync` to work with Tokio's async runtime.
///
/// # Examples
///
/// ```
/// use mcp_core::traits::CacheProvider;
/// use mcp_core::{CacheKey, Result, Error};
/// use async_trait::async_trait;
/// use serde_json::Value;
/// use std::collections::HashMap;
///
/// struct MemoryCache {
///     cache: HashMap<String, Value>,
/// }
///
/// impl MemoryCache {
///     fn new() -> Self {
///         Self {
///             cache: HashMap::new(),
///         }
///     }
/// }
///
/// #[async_trait]
/// impl CacheProvider for MemoryCache {
///     async fn get(&self, key: &CacheKey) -> Result<Option<Value>> {
///         Ok(self.cache.get(key.as_str()).cloned())
///     }
///
///     async fn set(&mut self, key: CacheKey, value: Value) -> Result<()> {
///         self.cache.insert(key.into_inner(), value);
///         Ok(())
///     }
///
///     async fn remove(&mut self, key: &CacheKey) -> Result<()> {
///         self.cache.remove(key.as_str());
///         Ok(())
///     }
///
///     async fn clear(&mut self) -> Result<()> {
///         self.cache.clear();
///         Ok(())
///     }
///
///     async fn contains(&self, key: &CacheKey) -> Result<bool> {
///         Ok(self.cache.contains_key(key.as_str()))
///     }
/// }
/// ```
#[async_trait]
pub trait CacheProvider: Send + Sync {
    /// Retrieves a cached value by key.
    ///
    /// Returns `Ok(Some(value))` if the key exists in the cache,
    /// `Ok(None)` if the key is not found.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache operation fails (e.g., I/O error,
    /// deserialization error).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CacheProvider;
    /// # use mcp_core::{CacheKey, Result};
    /// # async fn example(cache: &impl CacheProvider) -> Result<()> {
    /// let key = CacheKey::new("my-key");
    /// if let Some(value) = cache.get(&key).await? {
    ///     println!("Cache hit: {}", value);
    /// } else {
    ///     println!("Cache miss");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    async fn get(&self, key: &CacheKey) -> Result<Option<Value>>;

    /// Stores a value in the cache.
    ///
    /// If the key already exists, the value is overwritten.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache operation fails (e.g., I/O error,
    /// serialization error, out of space).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CacheProvider;
    /// # use mcp_core::{CacheKey, Result};
    /// # use serde_json::json;
    /// # async fn example(cache: &mut impl CacheProvider) -> Result<()> {
    /// let key = CacheKey::new("my-key");
    /// let value = json!({"result": "success"});
    /// cache.set(key, value).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn set(&mut self, key: CacheKey, value: Value) -> Result<()>;

    /// Removes a value from the cache.
    ///
    /// If the key does not exist, this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CacheProvider;
    /// # use mcp_core::{CacheKey, Result};
    /// # async fn example(cache: &mut impl CacheProvider) -> Result<()> {
    /// let key = CacheKey::new("my-key");
    /// cache.remove(&key).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn remove(&mut self, key: &CacheKey) -> Result<()>;

    /// Clears all entries from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CacheProvider;
    /// # use mcp_core::Result;
    /// # async fn example(cache: &mut impl CacheProvider) -> Result<()> {
    /// cache.clear().await?;
    /// println!("Cache cleared");
    /// # Ok(())
    /// # }
    /// ```
    async fn clear(&mut self) -> Result<()>;

    /// Checks if a key exists in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CacheProvider;
    /// # use mcp_core::{CacheKey, Result};
    /// # async fn example(cache: &impl CacheProvider) -> Result<()> {
    /// let key = CacheKey::new("my-key");
    /// if cache.contains(&key).await? {
    ///     println!("Key exists");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    async fn contains(&self, key: &CacheKey) -> Result<bool>;

    /// Returns the number of entries in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache operation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_core::traits::CacheProvider;
    /// # use mcp_core::Result;
    /// # async fn example(cache: &impl CacheProvider) -> Result<()> {
    /// let size = cache.size().await?;
    /// println!("Cache contains {} entries", size);
    /// # Ok(())
    /// # }
    /// ```
    async fn size(&self) -> Result<usize> {
        // Default implementation (can be overridden for efficiency)
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    struct TestCache {
        data: HashMap<String, Value>,
    }

    impl TestCache {
        fn new() -> Self {
            Self {
                data: HashMap::new(),
            }
        }
    }

    #[async_trait]
    impl CacheProvider for TestCache {
        async fn get(&self, key: &CacheKey) -> Result<Option<Value>> {
            Ok(self.data.get(key.as_str()).cloned())
        }

        async fn set(&mut self, key: CacheKey, value: Value) -> Result<()> {
            self.data.insert(key.into_inner(), value);
            Ok(())
        }

        async fn remove(&mut self, key: &CacheKey) -> Result<()> {
            self.data.remove(key.as_str());
            Ok(())
        }

        async fn clear(&mut self) -> Result<()> {
            self.data.clear();
            Ok(())
        }

        async fn contains(&self, key: &CacheKey) -> Result<bool> {
            Ok(self.data.contains_key(key.as_str()))
        }

        async fn size(&self) -> Result<usize> {
            Ok(self.data.len())
        }
    }

    #[tokio::test]
    async fn test_cache_set_and_get() {
        let mut cache = TestCache::new();
        let key = CacheKey::new("test-key");
        let value = json!({"result": "success"});

        cache.set(key.clone(), value.clone()).await.unwrap();
        let retrieved = cache.get(&key).await.unwrap();

        assert_eq!(retrieved, Some(value));
    }

    #[tokio::test]
    async fn test_cache_get_missing() {
        let cache = TestCache::new();
        let key = CacheKey::new("missing");

        let result = cache.get(&key).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_cache_contains() {
        let mut cache = TestCache::new();
        let key = CacheKey::new("test");

        assert!(!cache.contains(&key).await.unwrap());

        cache.set(key.clone(), json!("value")).await.unwrap();
        assert!(cache.contains(&key).await.unwrap());
    }

    #[tokio::test]
    async fn test_cache_remove() {
        let mut cache = TestCache::new();
        let key = CacheKey::new("test");

        cache.set(key.clone(), json!("value")).await.unwrap();
        assert!(cache.contains(&key).await.unwrap());

        cache.remove(&key).await.unwrap();
        assert!(!cache.contains(&key).await.unwrap());
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let mut cache = TestCache::new();

        cache
            .set(CacheKey::new("key1"), json!("value1"))
            .await
            .unwrap();
        cache
            .set(CacheKey::new("key2"), json!("value2"))
            .await
            .unwrap();

        assert_eq!(cache.size().await.unwrap(), 2);

        cache.clear().await.unwrap();
        assert_eq!(cache.size().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_cache_size() {
        let mut cache = TestCache::new();

        assert_eq!(cache.size().await.unwrap(), 0);

        cache.set(CacheKey::new("k1"), json!("v1")).await.unwrap();
        assert_eq!(cache.size().await.unwrap(), 1);

        cache.set(CacheKey::new("k2"), json!("v2")).await.unwrap();
        assert_eq!(cache.size().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_cache_overwrite() {
        let mut cache = TestCache::new();
        let key = CacheKey::new("key");

        cache.set(key.clone(), json!("old")).await.unwrap();
        cache.set(key.clone(), json!("new")).await.unwrap();

        let value = cache.get(&key).await.unwrap();
        assert_eq!(value, Some(json!("new")));
    }
}
