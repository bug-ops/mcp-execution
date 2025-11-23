//! MCP Bridge: Proxies WASM calls to real MCP servers.
//!
//! This crate implements the bridge between WASM execution environment and
//! real MCP servers using the official rmcp SDK. It provides:
//!
//! - Connection pooling for multiple MCP servers
//! - LRU caching of tool results for performance
//! - Thread-safe async operations
//! - Integration with mcp-core types
//!
//! # Architecture
//!
//! The bridge manages persistent connections to MCP servers and proxies
//! tool calls from WASM code. Results are cached to avoid redundant
//! server calls and improve performance.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_bridge::Bridge;
//! use mcp_core::{ServerId, ToolName};
//! use serde_json::json;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create bridge with 1000-entry cache
//! let bridge = Bridge::new(1000);
//!
//! // Connect to server
//! let server_id = ServerId::new("vkteams-bot");
//! bridge.connect(server_id.clone(), "vkteams-bot-server").await?;
//!
//! // Call tool
//! let params = json!({"chat_id": "123", "text": "Hello"});
//! let result = bridge.call_tool(
//!     &server_id,
//!     &ToolName::new("send_message"),
//!     params
//! ).await?;
//!
//! println!("Result: {:?}", result);
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

use lru::LruCache;
use mcp_core::stats::BridgeStats;
use mcp_core::{CacheKey, Error, Result, ServerId, ToolName};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::Mutex;

/// Connection to an MCP server.
///
/// Wraps an `rmcp` `RunningService` and tracks connection metadata.
struct Connection {
    client: rmcp::service::RunningService<RoleClient, ()>,
    server_id: ServerId,
    call_count: u64,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("client", &"RunningService{..}")
            .field("server_id", &self.server_id)
            .field("call_count", &self.call_count)
            .finish()
    }
}

/// MCP Bridge with connection pooling and caching.
///
/// Manages connections to multiple MCP servers and provides
/// caching for tool results to improve performance.
///
/// # Resource Limits
///
/// The bridge enforces a maximum connection limit (default 100)
/// to prevent resource exhaustion. Use `with_limits()` for custom limits.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, using internal locking for
/// safe concurrent access.
///
/// # Examples
///
/// ```no_run
/// use mcp_bridge::Bridge;
/// use mcp_core::ServerId;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let bridge = Bridge::new(1000);
///
/// // Connect to multiple servers
/// bridge.connect(ServerId::new("server1"), "cmd1").await?;
/// bridge.connect(ServerId::new("server2"), "cmd2").await?;
///
/// let stats = bridge.cache_stats().await;
/// println!("Cache: {}/{}", stats.size, stats.capacity);
///
/// let (current, max) = bridge.connection_limits().await;
/// println!("Connections: {}/{}", current, max);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Bridge {
    connections: Arc<Mutex<HashMap<ServerId, Connection>>>,
    cache: Arc<Mutex<LruCache<CacheKey, Value>>>,
    cache_enabled: bool,
    max_connections: usize,

    // Statistics counters (thread-safe atomics)
    total_tool_calls: Arc<AtomicU32>,
    cache_hits: Arc<AtomicU32>,
    total_connections: Arc<AtomicU32>,
    connection_failures: Arc<AtomicU32>,
}

impl Bridge {
    /// Default maximum number of concurrent connections.
    ///
    /// This limit prevents resource exhaustion from unlimited connection pooling.
    pub const DEFAULT_MAX_CONNECTIONS: usize = 100;

    /// Creates a new bridge with specified cache size and default connection limit.
    ///
    /// Uses `DEFAULT_MAX_CONNECTIONS` (100) as the connection limit.
    /// For custom limits, use `with_limits()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::Bridge;
    ///
    /// let bridge = Bridge::new(1000);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `cache_size` is 0. Use at least 1 for minimal caching.
    #[must_use]
    pub fn new(cache_size: usize) -> Self {
        Self::with_limits(cache_size, Self::DEFAULT_MAX_CONNECTIONS)
    }

    /// Creates a bridge with custom cache size and connection limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::Bridge;
    ///
    /// // Custom limits: 5000 cache entries, 50 max connections
    /// let bridge = Bridge::with_limits(5000, 50);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `cache_size` is 0. Use at least 1 for minimal caching.
    #[must_use]
    pub fn with_limits(cache_size: usize, max_connections: usize) -> Self {
        let cache_size = NonZeroUsize::new(cache_size).expect("Cache size must be greater than 0");

        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            cache: Arc::new(Mutex::new(LruCache::new(cache_size))),
            cache_enabled: true,
            max_connections,
            total_tool_calls: Arc::new(AtomicU32::new(0)),
            cache_hits: Arc::new(AtomicU32::new(0)),
            total_connections: Arc::new(AtomicU32::new(0)),
            connection_failures: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Connects to an MCP server via stdio.
    ///
    /// Creates a new connection to the specified MCP server using
    /// stdio transport. The connection is stored in the pool for
    /// subsequent tool calls.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Maximum connection limit is reached (see `DEFAULT_MAX_CONNECTIONS`)
    /// - Command fails security validation
    /// - The server process cannot be spawned
    /// - The command path is invalid
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_bridge::Bridge;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// bridge.connect(ServerId::new("vkteams-bot"), "vkteams-bot-server").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(&self, server_id: ServerId, command: &str) -> Result<()> {
        tracing::info!("Connecting to MCP server: {}", server_id);

        // Check connection limit
        {
            let connections = self.connections.lock().await;
            if connections.len() >= self.max_connections {
                let len = connections.len();
                drop(connections); // Drop lock early before returning error
                return Err(Error::ConfigError {
                    message: format!(
                        "Connection limit reached ({len}/{}). Disconnect servers before adding more.",
                        self.max_connections
                    ),
                });
            }
        }

        // Validate command for security (prevents command injection)
        mcp_core::validate_command(command)?;

        let transport =
            TokioChildProcess::new(tokio::process::Command::new(command).configure(|_cmd| {}))
                .map_err(|e| {
                    // Track connection failure
                    self.connection_failures.fetch_add(1, Ordering::Relaxed);
                    Error::ConnectionFailed {
                        server: server_id.to_string(),
                        source: Box::new(e),
                    }
                })?;

        // Create client using serve pattern
        let client = ().serve(transport).await.map_err(|e| {
            // Track connection failure
            self.connection_failures.fetch_add(1, Ordering::Relaxed);
            Error::ConnectionFailed {
                server: server_id.to_string(),
                source: Box::new(e),
            }
        })?;

        let connection = Connection {
            client,
            server_id: server_id.clone(),
            call_count: 0,
        };

        self.connections.lock().await.insert(server_id, connection);

        // Track successful connection
        self.total_connections.fetch_add(1, Ordering::Relaxed);

        tracing::info!("Successfully connected to server");

        Ok(())
    }

    /// Calls an MCP tool with caching support.
    ///
    /// Executes a tool on the connected MCP server. Results are
    /// cached using a key derived from server ID, tool name, and
    /// parameters.
    ///
    /// # Caching Behavior
    ///
    /// - Cache key is generated using BLAKE3 hash of inputs
    /// - Cache hits skip server calls entirely
    /// - Cache can be disabled with `disable_cache()`
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Server is not connected (call `connect()` first)
    /// - Tool call fails on the server
    /// - Server returns malformed response
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_bridge::Bridge;
    /// use mcp_core::{ServerId, ToolName};
    /// use serde_json::json;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let server_id = ServerId::new("vkteams-bot");
    ///
    /// bridge.connect(server_id.clone(), "vkteams-bot-server").await?;
    ///
    /// let params = json!({"chat_id": "123", "text": "Hello"});
    /// let result = bridge.call_tool(
    ///     &server_id,
    ///     &ToolName::new("send_message"),
    ///     params
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_tool(
        &self,
        server_id: &ServerId,
        tool_name: &ToolName,
        params: Value,
    ) -> Result<Value> {
        // Track tool call attempt
        self.total_tool_calls.fetch_add(1, Ordering::Relaxed);

        // Check cache first
        if self.cache_enabled {
            let cache_key =
                CacheKey::from_parts(server_id.as_str(), tool_name.as_str(), &params.to_string());

            let cached = self.cache.lock().await.get(&cache_key).cloned();
            if let Some(value) = cached {
                tracing::debug!("Cache hit for {}::{}", server_id, tool_name);
                // Track cache hit
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                return Ok(value);
            }
        }

        // Clone client reference to release lock before async call
        let client = {
            let connections = self.connections.lock().await;
            connections
                .get(server_id)
                .ok_or_else(|| Error::ConnectionFailed {
                    server: server_id.to_string(),
                    source: "Server not connected".into(),
                })?
                .client
                .clone()
        }; // Lock released here - allows concurrent tool calls!

        // Call tool via rmcp WITHOUT holding lock
        tracing::debug!("Calling tool {}::{}", server_id, tool_name);

        // Convert JSON params to arguments map
        let arguments = params.as_object().cloned();

        let tool_result = client
            .call_tool(rmcp::model::CallToolRequestParam {
                name: std::borrow::Cow::Owned(tool_name.as_str().to_owned()),
                arguments,
            })
            .await
            .map_err(|e| Error::ExecutionError {
                message: format!("Tool call failed: {e}"),
                source: Some(Box::new(e)),
            })?;

        // Update call count after successful call
        {
            let mut connections = self.connections.lock().await;
            if let Some(connection) = connections.get_mut(server_id) {
                connection.call_count += 1;
            }
        }

        // Convert tool result to JSON
        let result = serde_json::to_value(&tool_result).map_err(|e| Error::SerializationError {
            message: "Failed to serialize tool result".into(),
            source: Some(e),
        })?;

        // Cache result
        if self.cache_enabled {
            let cache_key =
                CacheKey::from_parts(server_id.as_str(), tool_name.as_str(), &params.to_string());
            self.cache.lock().await.put(cache_key, result.clone());
        }

        tracing::debug!("Tool call successful");

        Ok(result)
    }

    /// Disables result caching.
    ///
    /// After calling this method, tool results will not be cached
    /// and cache lookups will be skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::Bridge;
    ///
    /// let mut bridge = Bridge::new(1000);
    /// bridge.disable_cache();
    /// ```
    pub fn disable_cache(&mut self) {
        self.cache_enabled = false;
        tracing::info!("Cache disabled");
    }

    /// Enables result caching.
    ///
    /// Re-enables caching after it was disabled with `disable_cache()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::Bridge;
    ///
    /// let mut bridge = Bridge::new(1000);
    /// bridge.disable_cache();
    /// bridge.enable_cache();
    /// ```
    pub fn enable_cache(&mut self) {
        self.cache_enabled = true;
        tracing::info!("Cache enabled");
    }

    /// Clears the result cache.
    ///
    /// Removes all cached tool results. Useful for testing or
    /// when server state has changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::Bridge;
    ///
    /// # async fn example() {
    /// let bridge = Bridge::new(1000);
    /// bridge.clear_cache().await;
    /// # }
    /// ```
    pub async fn clear_cache(&self) {
        self.cache.lock().await.clear();
        tracing::info!("Cache cleared");
    }

    /// Gets cache statistics.
    ///
    /// Returns current cache size and capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::Bridge;
    ///
    /// # async fn example() {
    /// let bridge = Bridge::new(1000);
    /// let stats = bridge.cache_stats().await;
    /// println!("Cache: {}/{}", stats.size, stats.capacity);
    /// # }
    /// ```
    pub async fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.lock().await;
        CacheStats {
            size: cache.len(),
            capacity: cache.cap().get(),
        }
    }

    /// Gets connection statistics for a server.
    ///
    /// Returns the number of tool calls made through this connection.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_bridge::Bridge;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let server_id = ServerId::new("test");
    ///
    /// bridge.connect(server_id.clone(), "test-cmd").await?;
    ///
    /// let count = bridge.connection_call_count(&server_id).await;
    /// println!("Calls: {}", count.unwrap_or(0));
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connection_call_count(&self, server_id: &ServerId) -> Option<u64> {
        let connections = self.connections.lock().await;
        connections.get(server_id).map(|conn| conn.call_count)
    }

    /// Disconnects from a server.
    ///
    /// Removes the connection from the pool. Does nothing if
    /// the server was not connected.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_bridge::Bridge;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let server_id = ServerId::new("test");
    ///
    /// bridge.connect(server_id.clone(), "test-cmd").await?;
    /// bridge.disconnect(&server_id).await;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn disconnect(&self, server_id: &ServerId) {
        self.connections.lock().await.remove(server_id);
        tracing::info!("Disconnected from server: {}", server_id);
    }

    /// Returns the number of active connections.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_bridge::Bridge;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    ///
    /// assert_eq!(bridge.connection_count().await, 0);
    ///
    /// bridge.connect(ServerId::new("s1"), "cmd1").await?;
    /// assert_eq!(bridge.connection_count().await, 1);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connection_count(&self) -> usize {
        self.connections.lock().await.len()
    }

    /// Returns current and maximum connection counts.
    ///
    /// Useful for monitoring connection pool usage and preventing
    /// hitting the connection limit.
    ///
    /// # Returns
    ///
    /// A tuple of `(current, max)` where:
    /// - `current`: Number of active connections
    /// - `max`: Maximum allowed connections
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::Bridge;
    ///
    /// # async fn example() {
    /// let bridge = Bridge::new(1000);
    /// let (current, max) = bridge.connection_limits().await;
    /// println!("Connections: {}/{}", current, max);
    ///
    /// let usage_percent = (current as f64 / max as f64) * 100.0;
    /// if usage_percent > 80.0 {
    ///     println!("Warning: Connection pool {}% full", usage_percent as u32);
    /// }
    /// # }
    /// ```
    pub async fn connection_limits(&self) -> (usize, usize) {
        let current = self.connections.lock().await.len();
        (current, self.max_connections)
    }

    /// Collects current bridge statistics.
    ///
    /// Returns a snapshot of bridge performance and connection metrics,
    /// including:
    /// - Total tool calls processed
    /// - Cache hit count and hit rate
    /// - Active and total connections
    /// - Connection failure count
    ///
    /// # Performance
    ///
    /// This operation is O(1) and completes in <1ms. It reads atomic
    /// counters and acquires a single lock to count active connections.
    ///
    /// # Thread Safety
    ///
    /// Safe to call concurrently from multiple tasks. Statistics are
    /// captured atomically for consistency.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_bridge::Bridge;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    ///
    /// // Perform some operations
    /// bridge.connect(ServerId::new("test"), "test-cmd").await?;
    ///
    /// // Collect statistics
    /// let stats = bridge.collect_stats().await;
    /// println!("Tool calls: {}", stats.total_tool_calls);
    /// println!("Cache hits: {}", stats.cache_hits);
    /// println!("Active connections: {}", stats.active_connections);
    ///
    /// if let Some(hit_rate) = stats.cache_hit_rate() {
    ///     println!("Cache hit rate: {:.1}%", hit_rate * 100.0);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn collect_stats(&self) -> BridgeStats {
        let active_connections = self.connections.lock().await.len();

        BridgeStats::new(
            self.total_tool_calls.load(Ordering::Relaxed),
            self.cache_hits.load(Ordering::Relaxed),
            active_connections.try_into().unwrap_or(u32::MAX),
            self.total_connections.load(Ordering::Relaxed),
            self.connection_failures.load(Ordering::Relaxed),
        )
    }
}

/// Cache statistics.
///
/// Provides information about the current state of the result cache.
///
/// # Examples
///
/// ```
/// use mcp_bridge::CacheStats;
///
/// let stats = CacheStats {
///     size: 150,
///     capacity: 1000,
/// };
///
/// println!("Cache usage: {:.1}%", stats.usage_percent());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Current number of cached entries
    pub size: usize,
    /// Maximum cache capacity
    pub capacity: usize,
}

impl CacheStats {
    /// Returns cache usage as a percentage (0.0 to 100.0).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_bridge::CacheStats;
    ///
    /// let stats = CacheStats { size: 50, capacity: 100 };
    /// assert_eq!(stats.usage_percent(), 50.0);
    /// ```
    #[must_use]
    pub fn usage_percent(&self) -> f64 {
        if self.capacity == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            {
                (self.size as f64 / self.capacity as f64) * 100.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_new() {
        let bridge = Bridge::new(100);
        assert!(bridge.cache_enabled);
    }

    #[test]
    #[should_panic(expected = "Cache size must be greater than 0")]
    fn test_bridge_new_zero_cache() {
        let _bridge = Bridge::new(0);
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let bridge = Bridge::new(100);
        let stats = bridge.cache_stats().await;
        assert_eq!(stats.size, 0);
        assert_eq!(stats.capacity, 100);
    }

    #[tokio::test]
    async fn test_clear_cache() {
        let bridge = Bridge::new(100);
        bridge.clear_cache().await;
        let stats = bridge.cache_stats().await;
        assert_eq!(stats.size, 0);
    }

    #[test]
    fn test_disable_enable_cache() {
        let mut bridge = Bridge::new(100);
        assert!(bridge.cache_enabled);

        bridge.disable_cache();
        assert!(!bridge.cache_enabled);

        bridge.enable_cache();
        assert!(bridge.cache_enabled);
    }

    #[tokio::test]
    async fn test_connection_count() {
        let bridge = Bridge::new(100);
        assert_eq!(bridge.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_connection_call_count_not_found() {
        let bridge = Bridge::new(100);
        let server_id = ServerId::new("nonexistent");
        assert!(bridge.connection_call_count(&server_id).await.is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_cache_stats_usage_percent() {
        let stats = CacheStats {
            size: 50,
            capacity: 100,
        };
        assert_eq!(stats.usage_percent(), 50.0);

        let empty = CacheStats {
            size: 0,
            capacity: 100,
        };
        assert_eq!(empty.usage_percent(), 0.0);

        let full = CacheStats {
            size: 100,
            capacity: 100,
        };
        assert_eq!(full.usage_percent(), 100.0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_cache_stats_zero_capacity() {
        let stats = CacheStats {
            size: 0,
            capacity: 0,
        };
        assert_eq!(stats.usage_percent(), 0.0);
    }

    #[tokio::test]
    async fn test_disconnect() {
        let bridge = Bridge::new(100);
        let server_id = ServerId::new("test");

        // Disconnect non-existent connection (should not panic)
        bridge.disconnect(&server_id).await;
    }

    #[tokio::test]
    async fn test_collect_stats_empty() {
        let bridge = Bridge::new(100);
        let stats = bridge.collect_stats().await;

        assert_eq!(stats.total_tool_calls, 0);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.connection_failures, 0);
        assert_eq!(stats.cache_hit_rate(), None);
        assert_eq!(stats.connection_success_rate(), None);
    }

    #[tokio::test]
    async fn test_collect_stats_tracks_counters() {
        let bridge = Bridge::new(100);

        // Simulate some tool calls with cache tracking
        bridge.total_tool_calls.fetch_add(10, Ordering::Relaxed);
        bridge.cache_hits.fetch_add(7, Ordering::Relaxed);
        bridge.total_connections.fetch_add(5, Ordering::Relaxed);
        bridge.connection_failures.fetch_add(1, Ordering::Relaxed);

        let stats = bridge.collect_stats().await;

        assert_eq!(stats.total_tool_calls, 10);
        assert_eq!(stats.cache_hits, 7);
        assert_eq!(stats.total_connections, 5);
        assert_eq!(stats.connection_failures, 1);

        // Verify calculated rates
        assert_eq!(stats.cache_hit_rate(), Some(0.7));
        assert_eq!(stats.connection_success_rate(), Some(0.8));
    }

    #[tokio::test]
    async fn test_collect_stats_with_active_connections() {
        let bridge = Bridge::new(100);

        // Test that stats collection works even with empty connections
        let stats = bridge.collect_stats().await;
        assert_eq!(stats.active_connections, 0);

        // Note: We can't actually add real Connections here without spawning a process,
        // but the counter tracking logic is tested in other tests
    }

    #[tokio::test]
    async fn test_collect_stats_performance() {
        use std::time::Instant;

        let bridge = Bridge::new(1000);

        // Simulate realistic state
        bridge.total_tool_calls.fetch_add(1000, Ordering::Relaxed);
        bridge.cache_hits.fetch_add(850, Ordering::Relaxed);
        bridge.total_connections.fetch_add(50, Ordering::Relaxed);
        bridge.connection_failures.fetch_add(2, Ordering::Relaxed);

        // Measure collection time
        let start = Instant::now();
        let stats = bridge.collect_stats().await;
        let elapsed = start.elapsed();

        // Should complete in well under 1ms
        assert!(
            elapsed.as_micros() < 1000,
            "Stats collection took {elapsed:?}"
        );

        // Verify correctness
        assert_eq!(stats.total_tool_calls, 1000);
        assert_eq!(stats.cache_hits, 850);
    }

    #[tokio::test]
    async fn test_collect_stats_thread_safety() {
        use std::sync::Arc;
        use tokio::task;

        let bridge = Arc::new(Bridge::new(1000));

        // Spawn multiple tasks that increment counters concurrently
        let mut handles = vec![];

        for _ in 0..10 {
            let bridge_clone = Arc::clone(&bridge);
            let handle = task::spawn(async move {
                for _ in 0..100 {
                    bridge_clone
                        .total_tool_calls
                        .fetch_add(1, Ordering::Relaxed);
                    bridge_clone.cache_hits.fetch_add(1, Ordering::Relaxed);
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all increments were counted
        let stats = bridge.collect_stats().await;
        assert_eq!(stats.total_tool_calls, 1000); // 10 tasks * 100 increments
        assert_eq!(stats.cache_hits, 1000);
    }

    #[tokio::test]
    async fn test_collect_stats_after_operations() {
        let bridge = Bridge::new(100);

        // Initial state
        let stats = bridge.collect_stats().await;
        assert_eq!(stats.total_tool_calls, 0);

        // Can't test actual tool calls without spawning real MCP servers,
        // but we've verified the tracking logic above
    }
}
