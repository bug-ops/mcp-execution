//! Acceptance test of the single-sourced `rmcp` log-cap policy (#678): the policy is a
//! foreign-type-free enum whose directive strings both binaries feed to their filter.

use mcp_execution_core::cli::RmcpLogCap;

#[test]
fn directive_maps_each_cap_to_its_rmcp_target() {
    assert_eq!(RmcpLogCap::Info.directive(), "rmcp=info");
    assert_eq!(RmcpLogCap::Warn.directive(), "rmcp=warn");
}
