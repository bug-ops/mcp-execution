//! Acceptance test of the alt-runtime opt-in variable name (#605): CI sets it by this exact
//! spelling and the README cites it, so the test helpers must define the same one.

#![cfg(feature = "test-fixtures")]

mod common;

#[test]
fn require_alt_runtimes_env_has_the_documented_name() {
    assert_eq!(
        common::REQUIRE_ALT_RUNTIMES_ENV,
        "MCP_EXECUTION_REQUIRE_ALT_RUNTIMES"
    );
}
