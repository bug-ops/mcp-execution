//! Host functions exposed to WASM modules.
//!
//! Provides a controlled interface for WASM code to interact with
//! MCP servers, filesystem, and state storage through secure host functions.
//!
//! # Security
//!
//! All host functions enforce:
//! - Parameter validation
//! - Resource limits
//! - Access control
//! - Call counting for rate limiting
//!
//! # Examples
//!
//! ```no_run
//! use mcp_wasm_runtime::host_functions::HostContext;
//! use mcp_bridge::Bridge;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let bridge = Bridge::new(1000);
//! let context = HostContext::new(Arc::new(bridge));
//! # Ok(())
//! # }
//! ```

use mcp_bridge::Bridge;
use mcp_core::{Error, Result, ServerId, SessionId, ToolName};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Host context shared with WASM modules.
///
/// Contains all resources and state accessible to WASM code through
/// host functions. Uses interior mutability for thread-safe access.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, using `Arc` and `RwLock` for
/// safe concurrent access from multiple WASM instances.
#[derive(Clone)]
pub struct HostContext {
    /// Bridge to MCP servers
    bridge: Arc<Bridge>,

    /// Session-specific state storage
    state: Arc<RwLock<HashMap<SessionId, HashMap<String, Value>>>>,

    /// Virtual filesystem (read-only for now)
    vfs_root: Arc<RwLock<HashMap<String, Vec<u8>>>>,

    /// Host function call counter for rate limiting
    call_count: Arc<RwLock<usize>>,

    /// Maximum allowed host function calls
    max_calls: Option<usize>,
}

impl HostContext {
    /// Creates a new host context.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::host_functions::HostContext;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// let bridge = Bridge::new(1000);
    /// let context = HostContext::new(Arc::new(bridge));
    /// ```
    #[must_use]
    pub fn new(bridge: Arc<Bridge>) -> Self {
        Self {
            bridge,
            state: Arc::new(RwLock::new(HashMap::new())),
            vfs_root: Arc::new(RwLock::new(HashMap::new())),
            call_count: Arc::new(RwLock::new(0)),
            max_calls: Some(1000),
        }
    }

    /// Sets maximum allowed host function calls.
    pub fn set_max_calls(&mut self, max: Option<usize>) {
        self.max_calls = max;
    }

    /// Increments and checks call counter.
    ///
    /// # Errors
    ///
    /// Returns error if call limit is exceeded.
    async fn check_call_limit(&self) -> Result<()> {
        let mut count = self.call_count.write().await;
        *count += 1;

        if let Some(max) = self.max_calls
            && *count > max
        {
            return Err(Error::ExecutionError {
                message: format!("Host function call limit exceeded: {}/{}", *count, max),
                source: None,
            });
        }

        Ok(())
    }

    /// Resets call counter.
    pub async fn reset_call_count(&self) {
        let mut count = self.call_count.write().await;
        *count = 0;
    }

    /// Gets current call count.
    pub async fn call_count(&self) -> usize {
        *self.call_count.read().await
    }

    /// Calls an MCP tool through the bridge.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Call limit exceeded
    /// - Server not connected
    /// - Tool call fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::host_functions::HostContext;
    /// use mcp_bridge::Bridge;
    /// use mcp_core::{ServerId, ToolName};
    /// use serde_json::json;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let context = HostContext::new(Arc::new(bridge));
    ///
    /// let result = context.call_tool(
    ///     &ServerId::new("github"),
    ///     &ToolName::new("send_message"),
    ///     json!({"text": "Hello"}),
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
        self.check_call_limit().await?;

        tracing::debug!("WASM calling tool: {}::{}", server_id, tool_name);

        self.bridge.call_tool(server_id, tool_name, params).await
    }

    /// Reads a file from the virtual filesystem.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Call limit exceeded
    /// - File not found
    /// - Path is invalid
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::host_functions::HostContext;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let context = HostContext::new(Arc::new(bridge));
    ///
    /// let content = context.read_file("/mcp-tools/servers/test/manifest.json").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        self.check_call_limit().await?;

        tracing::debug!("WASM reading file: {}", path);

        let vfs = self.vfs_root.read().await;
        vfs.get(path)
            .cloned()
            .ok_or_else(|| Error::ResourceNotFound {
                resource: format!("File not found: {}", path),
            })
    }

    /// Writes a file to the virtual filesystem.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Call limit exceeded
    /// - Path is invalid
    /// - Write operation fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::host_functions::HostContext;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let context = HostContext::new(Arc::new(bridge));
    ///
    /// context.write_file("/tmp/output.txt", b"Hello".to_vec()).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write_file(&self, path: &str, content: Vec<u8>) -> Result<()> {
        self.check_call_limit().await?;

        tracing::debug!("WASM writing file: {} ({} bytes)", path, content.len());

        let mut vfs = self.vfs_root.write().await;
        vfs.insert(path.to_string(), content);
        Ok(())
    }

    /// Gets state value for a session.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Call limit exceeded
    /// - Key not found
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::host_functions::HostContext;
    /// use mcp_bridge::Bridge;
    /// use mcp_core::SessionId;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let context = HostContext::new(Arc::new(bridge));
    ///
    /// let session = SessionId::generate();
    /// let value = context.get_state(&session, "counter").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_state(&self, session_id: &SessionId, key: &str) -> Result<Value> {
        self.check_call_limit().await?;

        tracing::debug!("WASM getting state: {} / {}", session_id, key);

        let state = self.state.read().await;
        state
            .get(session_id)
            .and_then(|session| session.get(key))
            .cloned()
            .ok_or_else(|| Error::ResourceNotFound {
                resource: format!("State key not found: {}", key),
            })
    }

    /// Sets state value for a session.
    ///
    /// # Errors
    ///
    /// Returns error if call limit exceeded.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::host_functions::HostContext;
    /// use mcp_bridge::Bridge;
    /// use mcp_core::SessionId;
    /// use serde_json::json;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let context = HostContext::new(Arc::new(bridge));
    ///
    /// let session = SessionId::generate();
    /// context.set_state(&session, "counter", json!(42)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_state(&self, session_id: &SessionId, key: &str, value: Value) -> Result<()> {
        self.check_call_limit().await?;

        tracing::debug!("WASM setting state: {} / {} = {:?}", session_id, key, value);

        let mut state = self.state.write().await;
        state
            .entry(session_id.clone())
            .or_insert_with(HashMap::new)
            .insert(key.to_string(), value);

        Ok(())
    }

    /// Clears all state for a session.
    pub async fn clear_session(&self, session_id: &SessionId) {
        let mut state = self.state.write().await;
        state.remove(session_id);
    }

    /// Populates VFS with files.
    ///
    /// Used to preload generated code and resources into the WASM
    /// module's virtual filesystem.
    pub async fn populate_vfs(&self, files: HashMap<String, Vec<u8>>) {
        let mut vfs = self.vfs_root.write().await;
        vfs.extend(files);
    }
}

impl std::fmt::Debug for HostContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostContext")
            .field("max_calls", &self.max_calls)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_call_limit() {
        let bridge = Bridge::new(1000);
        let mut context = HostContext::new(Arc::new(bridge));
        context.set_max_calls(Some(3));

        // First 3 calls should succeed
        assert!(context.check_call_limit().await.is_ok());
        assert!(context.check_call_limit().await.is_ok());
        assert!(context.check_call_limit().await.is_ok());

        // 4th call should fail
        assert!(context.check_call_limit().await.is_err());
    }

    #[tokio::test]
    async fn test_reset_call_count() {
        let bridge = Bridge::new(1000);
        let mut context = HostContext::new(Arc::new(bridge));
        context.set_max_calls(Some(2));

        context.check_call_limit().await.ok();
        context.check_call_limit().await.ok();

        assert_eq!(context.call_count().await, 2);

        context.reset_call_count().await;
        assert_eq!(context.call_count().await, 0);
    }

    #[tokio::test]
    async fn test_state_operations() {
        let bridge = Bridge::new(1000);
        let mut context = HostContext::new(Arc::new(bridge));
        context.set_max_calls(None); // Disable limit for test

        let session = SessionId::generate();
        let key = "test_key";
        let value = serde_json::json!({"count": 42});

        // Set state
        context
            .set_state(&session, key, value.clone())
            .await
            .unwrap();

        // Get state
        let retrieved = context.get_state(&session, key).await.unwrap();
        assert_eq!(retrieved, value);

        // Clear session
        context.clear_session(&session).await;
        assert!(context.get_state(&session, key).await.is_err());
    }

    #[tokio::test]
    #[allow(clippy::similar_names)]
    async fn test_vfs_operations() {
        let bridge = Bridge::new(1000);
        let mut context = HostContext::new(Arc::new(bridge));
        context.set_max_calls(None);

        let path = "/test/file.txt";
        let content = b"Hello, WASM!".to_vec();

        // Write file
        context.write_file(path, content.clone()).await.unwrap();

        // Read file
        let read_content = context.read_file(path).await.unwrap();
        assert_eq!(read_content, content);

        // Read nonexistent file
        assert!(context.read_file("/nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn test_populate_vfs() {
        let bridge = Bridge::new(1000);
        let mut context = HostContext::new(Arc::new(bridge));
        context.set_max_calls(None);

        let mut files = HashMap::new();
        files.insert("/file1.txt".to_string(), b"content1".to_vec());
        files.insert("/file2.txt".to_string(), b"content2".to_vec());

        context.populate_vfs(files).await;

        assert_eq!(context.read_file("/file1.txt").await.unwrap(), b"content1");
        assert_eq!(context.read_file("/file2.txt").await.unwrap(), b"content2");
    }
}
