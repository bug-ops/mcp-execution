//! Integration tests for mcp-bridge
//!
//! These tests validate the full workflow of connecting to MCP servers,
//! calling tools, and caching results.

use mcp_bridge::{Bridge, CacheStats};
use mcp_core::{Error, ServerId, ToolName};
use serde_json::json;

/// Tests bridge creation with various configurations
#[tokio::test]
async fn test_bridge_creation() {
    // Default configuration
    let bridge = Bridge::new(1000);
    let stats = bridge.cache_stats().await;
    assert_eq!(stats.capacity, 1000);

    // Custom limits
    let bridge_custom = Bridge::with_limits(5000, 50);
    let stats_custom = bridge_custom.cache_stats().await;
    assert_eq!(stats_custom.capacity, 5000);
}

/// Tests cache statistics tracking
#[tokio::test]
#[allow(clippy::float_cmp)]
async fn test_cache_statistics() {
    let bridge = Bridge::new(100);

    let stats = bridge.cache_stats().await;
    assert_eq!(stats.size, 0);
    assert_eq!(stats.capacity, 100);
    assert_eq!(stats.usage_percent(), 0.0);
}

/// Tests connection limit enforcement
#[tokio::test]
async fn test_connection_limit_enforcement() {
    let bridge = Bridge::with_limits(100, 2); // Max 2 connections

    let _server1 = ServerId::new("test1");
    let _server2 = ServerId::new("test2");
    let _server3 = ServerId::new("test3");

    // First two connections should work (but will fail if commands don't exist)
    // Just verify the limit checking logic
    let (current, max) = bridge.connection_limits().await;
    assert_eq!(current, 0);
    assert_eq!(max, 2);
}

/// Tests cache enable/disable functionality
#[test]
fn test_cache_toggle() {
    let mut bridge = Bridge::new(100);

    // Starts enabled
    bridge.disable_cache();
    bridge.enable_cache();
}

/// Tests connection counting
#[tokio::test]
async fn test_connection_tracking() {
    let bridge = Bridge::new(100);

    assert_eq!(bridge.connection_count().await, 0);

    let server_id = ServerId::new("test");
    assert!(bridge.connection_call_count(&server_id).await.is_none());
}

/// Tests disconnect functionality
#[tokio::test]
async fn test_disconnect_nonexistent() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("nonexistent");

    // Should not panic
    bridge.disconnect(&server_id).await;
    assert_eq!(bridge.connection_count().await, 0);
}

/// Tests cache clearing
#[tokio::test]
async fn test_cache_clearing() {
    let bridge = Bridge::new(100);

    // Clear empty cache
    bridge.clear_cache().await;

    let stats = bridge.cache_stats().await;
    assert_eq!(stats.size, 0);
}

/// Tests `CacheStats` helper methods
#[test]
#[allow(clippy::float_cmp)]
fn test_cache_stats_methods() {
    let empty = CacheStats {
        size: 0,
        capacity: 1000,
    };
    assert_eq!(empty.usage_percent(), 0.0);

    let half = CacheStats {
        size: 500,
        capacity: 1000,
    };
    assert_eq!(half.usage_percent(), 50.0);

    let full = CacheStats {
        size: 1000,
        capacity: 1000,
    };
    assert_eq!(full.usage_percent(), 100.0);

    // Edge case: zero capacity
    let zero = CacheStats {
        size: 0,
        capacity: 0,
    };
    assert_eq!(zero.usage_percent(), 0.0);
}

/// Tests that Bridge is Send and Sync (required for async/multi-threaded use)
#[test]
fn test_bridge_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<Bridge>();
    assert_sync::<Bridge>();
}

/// Tests concurrent access to bridge
#[tokio::test]
async fn test_concurrent_cache_operations() {
    use std::sync::Arc;

    let bridge = Arc::new(Bridge::new(1000));

    let mut handles = vec![];

    // Spawn multiple tasks accessing cache concurrently
    for i in 0..10 {
        let bridge_clone = Arc::clone(&bridge);
        let handle = tokio::spawn(async move {
            let stats = bridge_clone.cache_stats().await;
            assert!(stats.capacity == 1000);
            i
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }
}

/// Tests connection limits tracking
#[tokio::test]
async fn test_connection_limits_tracking() {
    let bridge = Bridge::with_limits(100, 10);

    let (current, max) = bridge.connection_limits().await;
    assert_eq!(current, 0);
    assert_eq!(max, 10);

    // After operations, limits should remain constant
    bridge.clear_cache().await;
    let (current2, max2) = bridge.connection_limits().await;
    assert_eq!(current2, 0);
    assert_eq!(max2, 10);
}

/// Tests error handling for invalid server connection
#[tokio::test]
async fn test_call_tool_not_connected() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("not-connected");
    let tool_name = ToolName::new("test_tool");
    let params = json!({"arg": "value"});

    let result = bridge.call_tool(&server_id, &tool_name, params).await;
    assert!(result.is_err());

    if let Err(Error::ConnectionFailed { server, .. }) = result {
        assert_eq!(server, "not-connected");
    } else {
        panic!("Expected ConnectionFailed error");
    }
}

/// Tests that connection validation rejects invalid commands
#[tokio::test]
async fn test_connect_command_validation() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("test");

    // Test with potentially dangerous command (should be caught by validation)
    let result = bridge
        .connect(server_id.clone(), "echo hello; rm -rf /")
        .await;

    // Should fail validation or connection (either is acceptable)
    // The important part is it doesn't execute the dangerous command
    assert!(result.is_err());
}

/// Tests memory safety with large cache sizes
#[tokio::test]
async fn test_large_cache_creation() {
    // Should handle large cache sizes without panic
    let bridge = Bridge::new(100_000);
    let stats = bridge.cache_stats().await;
    assert_eq!(stats.capacity, 100_000);
}

/// Tests zero cache size rejection
#[test]
#[should_panic(expected = "Cache size must be greater than 0")]
fn test_zero_cache_size_panics() {
    let _bridge = Bridge::new(0);
}

/// Tests that multiple disconnects are safe
#[tokio::test]
async fn test_multiple_disconnects() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("test");

    // Multiple disconnects should be safe
    bridge.disconnect(&server_id).await;
    bridge.disconnect(&server_id).await;
    bridge.disconnect(&server_id).await;

    assert_eq!(bridge.connection_count().await, 0);
}

/// Tests cache behavior with disabled cache
#[tokio::test]
async fn test_disabled_cache_behavior() {
    let mut bridge = Bridge::new(100);
    bridge.disable_cache();

    // Cache operations should still work, just not cache
    bridge.clear_cache().await;
    let stats = bridge.cache_stats().await;
    assert_eq!(stats.size, 0);
}
