//! Acceptance test for #568 / S3: the v3 `_meta.json` is no larger than the v2 sidecar master
//! produced for the same fixture, and stays under the shared scan cap.

mod acceptance_support;

use acceptance_support::{file, generate, server_info, tool};
use mcp_execution_core::metadata::MAX_METADATA_FILE_BYTES;
use serde_json::json;

/// `_meta.json` bytes master (v2, c873c77) wrote for [`typical_tools`].
const MASTER_V2_SIDECAR_BYTES: usize = 327_873;

fn typical_tools() -> Vec<mcp_execution_introspector::ToolInfo> {
    (0..500)
        .map(|i| {
            let mut t = tool(
                &format!("tool_{i:04}"),
                json!({
                    "type": "object",
                    "properties": {
                        "repo": {"type": "string", "description": "Repository"},
                        "page": {"type": "integer"},
                        "state": {"enum": ["open", "closed"]}
                    },
                    "required": ["repo"]
                }),
                None,
            );
            t.description = "Does the thing with the repository".to_string();
            t
        })
        .collect()
}

#[test]
fn v3_sidecar_for_500_typical_tools_is_no_larger_than_the_v2_baseline_and_under_the_cap() {
    let mut server = server_info(typical_tools());
    server.id = mcp_execution_core::ServerId::new("acme").unwrap();
    server.name = "Acme".to_string();

    let code = generate(&server);
    let size = file(&code, "_meta.json").len();

    assert!(
        size <= MASTER_V2_SIDECAR_BYTES,
        "{size} B against the v2 baseline {MASTER_V2_SIDECAR_BYTES} B"
    );
    assert!(
        u64::try_from(size).unwrap() <= MAX_METADATA_FILE_BYTES,
        "{size} B"
    );
}
