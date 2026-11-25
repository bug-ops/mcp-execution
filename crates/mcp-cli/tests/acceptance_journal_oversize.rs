//! Acceptance test of the journal status of a replaced helper response (#620, F1): a value
//! result too large for one helper line is replaced by `call_failed` for the script, so the
//! journal must record `failed` (not `ok`) and the size of the original response.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, require_node, spawn_http_fixture};
use serde_json::Value;

const OVERSIZE_BYTES: usize = 5 * 1024 * 1024;

#[test]
fn journal_records_failed_with_the_original_size_for_a_replaced_oversize_response() {
    if !require_node(
        "journal_records_failed_with_the_original_size_for_a_replaced_oversize_response",
    ) {
        return;
    }
    let sb = Sandbox::new();
    let (_server, url) = spawn_http_fixture(None);
    sb.add_http_server("fixture-http", &url);
    sb.generate_server("fixture-http");
    let script = sb.script(
        "oversize.ts",
        &format!(
            "import {{ large }} from 'mcp:fixture-http';\n\
             try {{ await large({{ bytes: {OVERSIZE_BYTES} }}); }}\n\
             catch (e) {{ console.log('KIND=' + e.kind); }}\n"
        ),
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("KIND=call_failed"), "{}", out.stdout);
    let ends: Vec<Value> = sb
        .state_files("journals")
        .iter()
        .flat_map(|file| {
            std::fs::read_to_string(file)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        })
        .filter(|entry| entry["event"] == "end")
        .collect();
    assert_eq!(ends.len(), 1, "one call, one end entry: {ends:?}");
    assert_eq!(
        ends[0]["status"], "failed",
        "the script saw call_failed, the journal must agree: {}",
        ends[0]
    );
    let bytes = ends[0]["result_bytes"].as_u64().unwrap();
    assert!(
        (OVERSIZE_BYTES as u64..OVERSIZE_BYTES as u64 + 64).contains(&bytes),
        "result_bytes must be the size of the original response (5 MiB plus a small envelope), got {bytes}"
    );
}
