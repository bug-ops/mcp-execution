//! Acceptance test of the 1.0 construction rule (#674): `ServerCapabilities` becomes
//! `#[non_exhaustive]`, so callers outside the crate build it through `Default`, whose natural
//! value advertises nothing.

use mcp_execution_introspector::ServerCapabilities;

#[test]
fn default_advertises_no_capability() {
    let caps = ServerCapabilities::default();
    assert!(!caps.supports_tools);
    assert!(!caps.supports_resources);
    assert!(!caps.supports_prompts);
}
