//! Acceptance test of the spill notice wording (#625): when the 8 MiB capture limit cut a
//! stream, the notice says the file holds only the first bytes instead of the full output.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`) and Node.js 22.18+.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, require_node};
use mcp_execution_cli::runtime::spill::MAX_SPILL_FILE_BYTES;

#[test]
fn run_notice_names_the_first_bytes_when_the_capture_limit_cut_the_stream() {
    if !require_node("run_notice_names_the_first_bytes_when_the_capture_limit_cut_the_stream") {
        return;
    }
    let sb = Sandbox::new();
    let total = 9 * 1024 * 1024;
    let script = sb.script(
        "big.ts",
        &format!("process.stdout.write('x'.repeat({total}));\n"),
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let notice = out
        .stdout
        .lines()
        .find(|line| line.contains("stdout cut at"))
        .unwrap_or_else(|| panic!("no spill notice for stdout in: {:.2000}", out.stdout));
    assert!(
        notice.contains(&format!("of {total} bytes")),
        "the notice must give the real total: {notice}"
    );
    assert!(
        notice.contains(&format!("first {MAX_SPILL_FILE_BYTES} bytes saved to")),
        "the notice must say the file holds only the first {MAX_SPILL_FILE_BYTES} bytes: {notice}"
    );
    assert!(
        !notice.contains("full output"),
        "a cut capture must not be called the full output: {notice}"
    );
    assert!(
        !notice.contains("capture limit reached"),
        "the trailing qualifier is replaced by the new wording: {notice}"
    );
}
