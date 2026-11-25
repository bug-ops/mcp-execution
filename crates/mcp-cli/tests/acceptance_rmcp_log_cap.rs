//! Acceptance test of the `rmcp` log-cap directive (#678): the core policy strings must be
//! valid `tracing-subscriber` directives, since the CLI filter parses them unconditionally.

use mcp_execution_core::cli::RmcpLogCap;
use tracing_subscriber::EnvFilter;

#[test]
fn every_cap_directive_parses_as_a_filter_directive() {
    for cap in [RmcpLogCap::Info, RmcpLogCap::Warn] {
        let directive = cap
            .directive()
            .parse()
            .expect("directive parses as a tracing directive");
        let filter = EnvFilter::new("debug").add_directive(directive);
        assert!(filter.to_string().contains(cap.directive()));
    }
}
