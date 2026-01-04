//! Tests to verify that all public types are Send + Sync as required.

use mcp_execution_core::*;

const fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn test_domain_types_are_send_sync() {
    // All domain types must be Send + Sync
    assert_send_sync::<ServerId>();
    assert_send_sync::<ToolName>();
}

#[test]
fn test_config_types_are_send_sync() {
    // Configuration types must be Send + Sync
    assert_send_sync::<ServerConfig>();
    assert_send_sync::<TransportType>();
}

#[test]
fn test_error_is_send_sync() {
    // Error type must be Send + Sync
    assert_send_sync::<Error>();
}
