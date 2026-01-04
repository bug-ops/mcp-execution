---
applyTo: "crates/mcp-bridge/**/*.rs"
---
# Copilot Instructions: mcp-bridge

This crate implements the **MCP Bridge** that proxies WASM calls to real MCP servers using the **official rmcp SDK**. It provides connection pooling, caching, and thread-safe async operations.

## Error Handling

**Use `thiserror`** for all errors:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("failed to connect to MCP server '{server}'")]
    ConnectionFailed {
        server: String,
        #[source]
        source: std::io::Error,
    },

    #[error("tool '{tool}' not found in server '{server}'")]
    ToolNotFound { server: String, tool: String },

    #[error("timeout after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("rmcp error: {0}")]
    RmcpError(#[from] rmcp::Error),

    #[error("invalid tool response: {0}")]
    InvalidResponse(String),
}

pub type Result<T> = std::result::Result<T, BridgeError>;
```

## rmcp SDK Integration - CRITICAL

**ALWAYS use the official rmcp SDK for MCP communication**:

### Connecting to Servers

```rust
use rmcp::transport::{TokioChildProcess, ConfigureCommandExt};
use rmcp::client::Client;
use std::process::Command;

// ✅ GOOD: Use rmcp's TokioChildProcess
async fn connect_server(command: &str) -> Result<Client<TokioChildProcess>> {
    let mut cmd = Command::new(command);
    cmd.stdin(std::process::Stdio::piped())
       .stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::inherit());

    let transport = TokioChildProcess::new(cmd)
        .await
        .map_err(|e| BridgeError::ConnectionFailed {
            server: command.to_string(),
            source: e.into(),
        })?;

    let client = Client::new(transport);
    Ok(client)
}
```

### Calling Tools

```rust
use rmcp::ServiceExt;
use serde_json::Value;

// ✅ GOOD: Use ServiceExt::call_tool
async fn call_tool(
    client: &Client<TokioChildProcess>,
    tool_name: &str,
    params: Value,
) -> Result<Value> {
    let result = client
        .call_tool(tool_name, params)
        .await
        .map_err(BridgeError::from)?;

    // Extract content from MCP response
    let content = result
        .content
        .first()
        .ok_or_else(|| BridgeError::InvalidResponse("empty content".into()))?;

    match content {
        rmcp::Content::Text { text } => {
            serde_json::from_str(text)
                .map_err(|e| BridgeError::InvalidResponse(e.to_string()))
        }
        _ => Err(BridgeError::InvalidResponse("expected text content".into())),
    }
}
```

### Getting Server Info

```rust
use rmcp::ServiceExt;

// ✅ GOOD: Use ServiceExt::get_server_info
async fn get_server_capabilities(
    client: &Client<TokioChildProcess>,
) -> Result<rmcp::ServerInfo> {
    let info = client
        .get_server_info()
        .await
        .map_err(BridgeError::from)?;

    Ok(info)
}
```

## Bridge Architecture

The bridge manages multiple MCP server connections with caching:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use lru::LruCache;
use mcp_execution_core::{ServerId, ToolName, CacheKey};

pub struct Bridge {
    // Connection pool - one client per server
    connections: Arc<RwLock<HashMap<ServerId, Client<TokioChildProcess>>>>,

    // LRU cache for tool results
    cache: Arc<RwLock<LruCache<CacheKey, Value>>>,

    // Statistics
    stats: Arc<RwLock<BridgeStats>>,
}

impl Bridge {
    pub fn new(cache_capacity: usize) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            cache: Arc::new(RwLock::new(LruCache::new(cache_capacity.try_into().unwrap()))),
            stats: Arc::new(RwLock::new(BridgeStats::default())),
        }
    }

    pub async fn connect(&self, server_id: ServerId, command: &str) -> Result<()> {
        let client = connect_server(command).await?;

        let mut connections = self.connections.write().await;
        connections.insert(server_id, client);

        Ok(())
    }

    pub async fn call_tool(
        &self,
        server_id: &ServerId,
        tool_name: &ToolName,
        params: Value,
    ) -> Result<Value> {
        // Check cache first
        let cache_key = CacheKey::new(server_id, tool_name, &params);

        {
            let mut cache = self.cache.write().await;
            if let Some(cached) = cache.get(&cache_key) {
                self.update_stats(|s| s.cache_hits += 1).await;
                return Ok(cached.clone());
            }
        }

        // Get connection
        let connections = self.connections.read().await;
        let client = connections
            .get(server_id)
            .ok_or_else(|| BridgeError::ToolNotFound {
                server: server_id.to_string(),
                tool: tool_name.to_string(),
            })?;

        // Call tool with timeout
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            call_tool(client, tool_name.as_ref(), params.clone())
        )
        .await
        .map_err(|_| BridgeError::Timeout {
            timeout: Duration::from_secs(30),
        })??;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.put(cache_key, result.clone());
        }

        self.update_stats(|s| {
            s.cache_misses += 1;
            s.total_calls += 1;
        }).await;

        Ok(result)
    }

    async fn update_stats<F>(&self, f: F)
    where
        F: FnOnce(&mut BridgeStats),
    {
        let mut stats = self.stats.write().await;
        f(&mut *stats);
    }
}
```

## Caching Strategy

Use **Blake3** for fast, cryptographic cache keys:

```rust
use blake3::Hasher;
use serde::Serialize;

impl CacheKey {
    pub fn new<T: Serialize>(
        server_id: &ServerId,
        tool_name: &ToolName,
        params: &T,
    ) -> Self {
        let mut hasher = Hasher::new();

        hasher.update(server_id.as_bytes());
        hasher.update(tool_name.as_bytes());

        // Canonicalize JSON for consistent hashing
        let params_json = serde_json::to_string(params)
            .expect("params must be serializable");
        hasher.update(params_json.as_bytes());

        let hash = hasher.finalize();
        Self { hash: *hash.as_bytes() }
    }
}
```

### When to Cache

```rust
// ✅ CACHE: Idempotent operations
// - get_user_info
// - list_chats
// - get_message

// ❌ DO NOT CACHE: Side effects
// - send_message
// - delete_message
// - update_status

impl Bridge {
    fn should_cache(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "get_user_info" | "list_chats" | "get_message" | "get_file"
        )
    }

    pub async fn call_tool(
        &self,
        server_id: &ServerId,
        tool_name: &ToolName,
        params: Value,
    ) -> Result<Value> {
        // Only check cache for idempotent operations
        if Self::should_cache(tool_name.as_ref()) {
            let cache_key = CacheKey::new(server_id, tool_name, &params);

            if let Some(cached) = self.cache.read().await.peek(&cache_key) {
                return Ok(cached.clone());
            }
        }

        // ... rest of implementation
    }
}
```

## Thread Safety

The bridge is designed for concurrent access:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

// ✅ GOOD: Bridge is Send + Sync
impl Bridge {
    pub fn clone_bridge(&self) -> Self {
        Self {
            connections: Arc::clone(&self.connections),
            cache: Arc::clone(&self.cache),
            stats: Arc::clone(&self.stats),
        }
    }
}

// Multiple tasks can share the same bridge
let bridge = Arc::new(Bridge::new(1000));

let handle1 = tokio::spawn({
    let bridge = Arc::clone(&bridge);
    async move {
        bridge.call_tool(&server_id, &tool_name, params).await
    }
});

let handle2 = tokio::spawn({
    let bridge = Arc::clone(&bridge);
    async move {
        bridge.call_tool(&server_id2, &tool_name2, params2).await
    }
});
```

## Error Recovery

Handle rmcp errors gracefully:

```rust
impl Bridge {
    pub async fn call_tool_with_retry(
        &self,
        server_id: &ServerId,
        tool_name: &ToolName,
        params: Value,
        max_retries: usize,
    ) -> Result<Value> {
        let mut attempts = 0;

        loop {
            match self.call_tool(server_id, tool_name, params.clone()).await {
                Ok(result) => return Ok(result),
                Err(e) if attempts < max_retries && e.is_retryable() => {
                    attempts += 1;
                    tracing::warn!(
                        "Tool call failed (attempt {}/{}): {}",
                        attempts,
                        max_retries,
                        e
                    );

                    // Exponential backoff
                    let delay = Duration::from_millis(100 * 2_u64.pow(attempts as u32));
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl BridgeError {
    fn is_retryable(&self) -> bool {
        matches!(self, BridgeError::Timeout { .. })
    }
}
```

## Testing

Mock rmcp clients for testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;

    mock! {
        McpClient {}

        #[async_trait::async_trait]
        impl ServiceExt for McpClient {
            async fn call_tool(
                &self,
                name: &str,
                params: Value,
            ) -> Result<rmcp::CallToolResult>;

            async fn get_server_info(&self) -> Result<rmcp::ServerInfo>;
        }
    }

    #[tokio::test]
    async fn test_cache_hit() {
        let bridge = Bridge::new(100);

        // First call - cache miss
        let result1 = bridge.call_tool(&server_id, &tool_name, params.clone()).await.unwrap();

        // Second call - cache hit
        let result2 = bridge.call_tool(&server_id, &tool_name, params).await.unwrap();

        assert_eq!(result1, result2);

        let stats = bridge.stats.read().await;
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);
    }
}
```

## Summary

- **Always use rmcp SDK** - Never implement custom MCP protocol
- **Use `thiserror`** for all error types
- **Thread-safe** with `Arc<RwLock<T>>`
- **Cache idempotent operations** with Blake3 + LRU
- **Handle rmcp errors** appropriately
- **Timeout all network operations**
- **Track statistics** for monitoring
