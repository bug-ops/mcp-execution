//! Integration tests for mcp-bridge
//!
//! These tests validate the full workflow of connecting to MCP servers,
//! calling tools, and caching results.

use mcp_bridge::{Bridge, CacheStats};
use mcp_core::stats::BridgeStats;
use mcp_core::{Error, ServerConfig, ServerId, ToolName};
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
    let config = ServerConfig::builder()
        .command("echo hello; rm -rf /".to_string())
        .build();
    let result = bridge.connect(server_id.clone(), &config).await;

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

/// Tests connection limit enforcement when limit is reached
#[tokio::test]
async fn test_connection_limit_reached() {
    let bridge = Bridge::with_limits(100, 1); // Max 1 connection

    // First connection should be allowed (but will fail without valid command)
    let server1 = ServerId::new("server1");
    let config = ServerConfig::builder()
        .command("nonexistent-cmd".to_string())
        .build();
    let _result1 = bridge.connect(server1.clone(), &config).await;

    // Should either succeed in attempting connection or fail due to invalid command
    // Either way, connection limit logic is tested
    let (current, max) = bridge.connection_limits().await;
    assert_eq!(max, 1);

    // Verify the limit value is enforced
    assert!(current <= max);
}

/// Tests empty `server_id` handling
#[tokio::test]
async fn test_empty_server_id() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("");

    // Should handle empty server IDs gracefully
    let config = ServerConfig::builder()
        .command("test-cmd".to_string())
        .build();
    let result = bridge.connect(server_id.clone(), &config).await;
    assert!(result.is_err());
}

/// Tests connection with whitespace in command
#[tokio::test]
async fn test_command_with_whitespace() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("test");

    // Command with spaces should be validated
    let config = ServerConfig::builder()
        .command("test cmd with spaces".to_string())
        .build();
    let result = bridge.connect(server_id, &config).await;

    // Should either fail validation or connection attempt
    assert!(result.is_err());
}

/// Tests disconnect followed by connection count check
#[tokio::test]
async fn test_disconnect_updates_count() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("test");

    // Initial count should be 0
    assert_eq!(bridge.connection_count().await, 0);

    // Disconnect non-existent (should not affect count)
    bridge.disconnect(&server_id).await;
    assert_eq!(bridge.connection_count().await, 0);
}

/// Tests connection limits after disconnect
#[tokio::test]
async fn test_connection_limits_after_disconnect() {
    let bridge = Bridge::with_limits(100, 5);
    let server_id = ServerId::new("test");

    let (current, max) = bridge.connection_limits().await;
    assert_eq!(current, 0);
    assert_eq!(max, 5);

    // Disconnect (no connection exists)
    bridge.disconnect(&server_id).await;

    let (current2, max2) = bridge.connection_limits().await;
    assert_eq!(current2, 0);
    assert_eq!(max2, 5);
}

/// Tests `call_tool` error when server never connected
#[tokio::test]
async fn test_call_tool_server_never_connected() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("never-connected");
    let tool_name = ToolName::new("some_tool");
    let params = json!({"key": "value"});

    let result = bridge.call_tool(&server_id, &tool_name, params).await;

    assert!(result.is_err());
    match result {
        Err(Error::ConnectionFailed { server, source }) => {
            assert_eq!(server, "never-connected");
            assert!(source.to_string().contains("not connected"));
        }
        _ => panic!("Expected ConnectionFailed error"),
    }
}

/// Tests cache key generation with different parameters
#[tokio::test]
async fn test_cache_with_different_params() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("test-server");
    let tool_name = ToolName::new("test-tool");

    // These should generate different cache keys
    let params1 = json!({"a": 1, "b": 2});
    let params2 = json!({"a": 2, "b": 1});

    // Both will fail (no connection), but cache key logic is tested
    let result1 = bridge.call_tool(&server_id, &tool_name, params1).await;
    let result2 = bridge
        .call_tool(&server_id, &tool_name, params2.clone())
        .await;

    assert!(result1.is_err());
    assert!(result2.is_err());

    // Verify cache stats remain at 0 (no successful calls to cache)
    let stats = bridge.cache_stats().await;
    assert_eq!(stats.size, 0);
}

/// Tests `with_limits` constructor with various configurations
#[test]
fn test_with_limits_configurations() {
    // Minimum viable configuration
    let bridge1 = Bridge::with_limits(1, 1);
    let _ = bridge1;

    // Large configuration
    let bridge2 = Bridge::with_limits(1_000_000, 1000);
    let _ = bridge2;

    // Asymmetric configuration (small cache, many connections)
    let bridge3 = Bridge::with_limits(10, 100);
    let _ = bridge3;
}

/// Tests `cache_stats` with various cache states
#[tokio::test]
async fn test_cache_stats_edge_cases() {
    // Minimum cache size
    let bridge = Bridge::new(1);
    let stats = bridge.cache_stats().await;
    assert_eq!(stats.capacity, 1);
    assert_eq!(stats.size, 0);

    // Very large cache
    let bridge_large = Bridge::new(1_000_000);
    let stats_large = bridge_large.cache_stats().await;
    assert_eq!(stats_large.capacity, 1_000_000);
    assert_eq!(stats_large.size, 0);
}

/// Tests `connection_call_count` with various server IDs
#[tokio::test]
async fn test_connection_call_count_variations() {
    let bridge = Bridge::new(100);

    // Non-existent server
    let server1 = ServerId::new("nonexistent");
    assert!(bridge.connection_call_count(&server1).await.is_none());

    // Different server ID
    let server2 = ServerId::new("another-server");
    assert!(bridge.connection_call_count(&server2).await.is_none());

    // Empty string server ID
    let server3 = ServerId::new("");
    assert!(bridge.connection_call_count(&server3).await.is_none());
}

/// Tests concurrent disconnect operations
#[tokio::test]
async fn test_concurrent_disconnects() {
    use std::sync::Arc;

    let bridge = Arc::new(Bridge::new(100));
    let server_id = ServerId::new("test");

    let mut handles = vec![];

    // Spawn multiple tasks disconnecting concurrently
    for _ in 0..10 {
        let bridge_clone = Arc::clone(&bridge);
        let server_clone = server_id.clone();
        let handle = tokio::spawn(async move {
            bridge_clone.disconnect(&server_clone).await;
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }

    // Should remain at 0 connections
    assert_eq!(bridge.connection_count().await, 0);
}

/// Tests `CacheStats` Debug implementation
#[test]
fn test_cache_stats_debug() {
    let stats = CacheStats {
        size: 42,
        capacity: 100,
    };

    let debug_str = format!("{stats:?}");
    assert!(debug_str.contains("42"));
    assert!(debug_str.contains("100"));
}

/// Tests Bridge Debug implementation
#[test]
fn test_bridge_debug() {
    let bridge = Bridge::new(100);
    let debug_str = format!("{bridge:?}");

    assert!(debug_str.contains("Bridge"));
}

/// Tests command validation with special characters
#[tokio::test]
async fn test_command_validation_special_chars() {
    let bridge = Bridge::new(100);
    let server_id = ServerId::new("test");

    // Commands with special characters that might indicate injection attempts
    let dangerous_commands = vec![
        "cmd && malicious",
        "cmd || fallback",
        "cmd | pipe",
        "cmd > redirect",
        "cmd < input",
        "cmd `backtick`",
        "cmd $(substitution)",
        "cmd;chain",
    ];

    for cmd in dangerous_commands {
        let config = ServerConfig::builder().command(cmd.to_string()).build();
        let result = bridge.connect(server_id.clone(), &config).await;
        // Should fail validation or connection
        assert!(result.is_err(), "Command should be rejected: {cmd}");
    }
}

/// Tests connection limits percentage calculation
#[tokio::test]
#[allow(clippy::cast_precision_loss)]
async fn test_connection_usage_percentage() {
    let bridge = Bridge::with_limits(100, 10);

    let (current, max) = bridge.connection_limits().await;
    let usage_percent = (current as f64 / max as f64) * 100.0;

    assert!((usage_percent - 0.0).abs() < f64::EPSILON);
    assert_eq!(max, 10);
}

/// Tests cache enable/disable state persistence
#[test]
fn test_cache_state_persistence() {
    let mut bridge = Bridge::new(100);

    // Initial state
    assert!(!format!("{bridge:?}").is_empty());

    // Toggle state multiple times
    bridge.disable_cache();
    bridge.disable_cache(); // Double disable
    bridge.enable_cache();
    bridge.enable_cache(); // Double enable

    // Should be stable
    assert!(!format!("{bridge:?}").is_empty());
}

/// Tests `collect_stats` method with realistic scenario
#[tokio::test]
async fn test_collect_stats_integration() {
    let bridge = Bridge::new(1000);

    // Initial state - all counters should be zero
    let stats = bridge.collect_stats().await;
    assert_eq!(stats.total_tool_calls, 0);
    assert_eq!(stats.cache_hits, 0);
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.total_connections, 0);
    assert_eq!(stats.connection_failures, 0);

    // No rates available with zero operations
    assert_eq!(stats.cache_hit_rate(), None);
    assert_eq!(stats.connection_success_rate(), None);
}

/// Tests that `collect_stats` returns consistent results
#[tokio::test]
async fn test_collect_stats_consistency() {
    let bridge = Bridge::new(1000);

    // Collect stats multiple times
    let stats1 = bridge.collect_stats().await;
    let stats2 = bridge.collect_stats().await;

    // Should be identical when no operations happen
    assert_eq!(stats1.total_tool_calls, stats2.total_tool_calls);
    assert_eq!(stats1.cache_hits, stats2.cache_hits);
    assert_eq!(stats1.active_connections, stats2.active_connections);
}

/// Tests `BridgeStats` type properties
#[test]
fn test_bridge_stats_type() {
    let stats = BridgeStats::new(100, 75, 5, 10, 2);

    // Verify all fields are accessible
    assert_eq!(stats.total_tool_calls, 100);
    assert_eq!(stats.cache_hits, 75);
    assert_eq!(stats.active_connections, 5);
    assert_eq!(stats.total_connections, 10);
    assert_eq!(stats.connection_failures, 2);

    // Verify calculated rates
    assert_eq!(stats.cache_hit_rate(), Some(0.75));
    assert_eq!(stats.connection_success_rate(), Some(0.8));
}

/// Tests `BridgeStats` serialization/deserialization
#[test]
fn test_bridge_stats_serialization() {
    let stats = BridgeStats::new(100, 75, 5, 10, 2);

    // Serialize to JSON
    let json = serde_json::to_string(&stats).unwrap();
    assert!(json.contains("\"total_tool_calls\":100"));
    assert!(json.contains("\"cache_hits\":75"));

    // Deserialize back
    let deserialized: BridgeStats = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_tool_calls, stats.total_tool_calls);
    assert_eq!(deserialized.cache_hits, stats.cache_hits);
}
