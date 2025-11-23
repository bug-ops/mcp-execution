---
applyTo: "crates/mcp-core/**/*.rs"
---

# Copilot Instructions: mcp-core

This crate is the **foundation** of the workspace. It defines core types, traits, and errors used by all other crates.

## Purpose

`mcp-core` provides:

- **Strong types** (ServerId, ToolName, SessionId, etc.)
- **Common traits** (shared interfaces)
- **Error types** (base errors for the workspace)
- **No external dependencies** (except basic ones)

## Error Handling - CRITICAL

**Only use `thiserror`**. This is a library crate.

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("invalid server ID: {0}")]
    InvalidServerId(String),

    #[error("invalid tool name: {0}")]
    InvalidToolName(String),

    #[error("tool '{tool}' not found in server '{server}'")]
    ToolNotFound { server: String, tool: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

**Never use `anyhow` in this crate**.

## Strong Types - CRITICAL

Following Microsoft Rust Guidelines, **use strong types instead of primitives**:

```rust
// ✅ GOOD: Strong type-safe ID
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerId(String);

impl ServerId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ❌ BAD: Using String directly
pub fn connect_server(server_id: String) -> Result<Connection> { ... }

// ✅ GOOD: Using strong type
pub fn connect_server(server_id: ServerId) -> Result<Connection> { ... }
```

### Example Strong Types

```rust
/// Unique identifier for an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerId(String);

/// Name of a tool in an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolName(String);

/// Unique session identifier for WASM execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

/// Cache key for tool results.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    hash: [u8; 32],  // Blake3 hash
}
```

## Trait Design

Define traits for common interfaces:

```rust
use async_trait::async_trait;

/// Trait for executing MCP tools.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a tool with given parameters.
    ///
    /// # Errors
    ///
    /// Returns error if tool execution fails or tool doesn't exist.
    async fn execute_tool(
        &self,
        tool_name: &ToolName,
        params: serde_json::Value,
    ) -> Result<serde_json::Value>;
}

/// Trait for server discovery.
#[async_trait]
pub trait ServerDiscovery: Send + Sync {
    /// Discover available servers.
    async fn discover_servers(&self) -> Result<Vec<ServerId>>;

    /// Get server information.
    async fn get_server_info(&self, server_id: &ServerId) -> Result<ServerInfo>;
}
```

## Send + Sync + Debug

**All public types MUST implement these traits**:

```rust
// ✅ GOOD: Implements required traits
#[derive(Debug, Clone)]
pub struct Config {
    max_connections: usize,
    timeout: Duration,
}

// Auto-implements Send + Sync because all fields are Send + Sync

// ✅ GOOD: Explicit implementation
pub struct Runtime {
    inner: Arc<RuntimeInner>,
}

// Send + Sync because Arc<T> is Send + Sync if T is Send + Sync

// ❌ BAD: Missing Debug
pub struct Tool {
    name: String,
    // ... no #[derive(Debug)]
}
```

## Stats and Metrics

Provide observability types:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

/// Statistics for the MCP bridge.
#[derive(Debug, Default)]
pub struct BridgeStats {
    pub total_calls: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub errors: AtomicU64,
}

impl BridgeStats {
    pub fn increment_calls(&self) {
        self.total_calls.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed) as f64;
        let total = self.total_calls.load(Ordering::Relaxed) as f64;

        if total == 0.0 {
            0.0
        } else {
            hits / total
        }
    }
}
```

## No Business Logic

`mcp-core` should **only contain types and traits**, not business logic:

```rust
// ✅ GOOD: Type definition
pub struct ToolName(String);

impl ToolName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ❌ BAD: Business logic in core
impl ToolName {
    pub async fn execute(&self, params: Value) -> Result<Value> {
        // This belongs in mcp-bridge, not mcp-core!
    }
}
```

## Serde Support

Provide serialization for wire formats:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
}
```

## CLI Types

Provide types for CLI interface:

```rust
/// Exit codes for CLI applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    GeneralError = 1,
    InvalidArguments = 2,
    ServerNotFound = 3,
    ToolNotFound = 4,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code as i32
    }
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Text,
    Table,
}
```

## Documentation

**Every public item needs comprehensive documentation**:

```rust
/// Unique identifier for an MCP server.
///
/// Server IDs are used throughout the system to reference specific
/// MCP server instances. They must be unique within a runtime.
///
/// # Examples
///
/// ```
/// use mcp_core::ServerId;
///
/// let id = ServerId::new("vkteams-bot");
/// assert_eq!(id.as_str(), "vkteams-bot");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerId(String);
```

## Testing

Test core types thoroughly:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_id_creation() {
        let id = ServerId::new("test-server");
        assert_eq!(id.as_str(), "test-server");
    }

    #[test]
    fn test_server_id_equality() {
        let id1 = ServerId::new("test");
        let id2 = ServerId::new("test");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_server_id_hash() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        map.insert(ServerId::new("test"), 42);

        assert_eq!(map.get(&ServerId::new("test")), Some(&42));
    }

    #[test]
    fn test_stats_increment() {
        let stats = BridgeStats::default();
        stats.increment_calls();
        stats.increment_calls();

        assert_eq!(stats.total_calls.load(Ordering::Relaxed), 2);
    }
}
```

## Summary

- **Foundation crate** - defines core types for entire workspace
- **Only `thiserror`** for errors (never `anyhow`)
- **Strong types** instead of primitives (ServerId, ToolName, etc.)
- **All public types** must be `Send + Sync + Debug`
- **No business logic** - only types, traits, and basic utilities
- **Comprehensive documentation** with examples
- **Well tested** - core types need thorough test coverage
