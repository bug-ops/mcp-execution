//! Acceptance tests for #587 on the CLI side: `generate --observed-shapes <file>` reads only the
//! last 4 MiB of the file and never fails on its size.
//!
//! Drives the real binary against a scriptable stdio MCP server run by Node, with `$HOME`
//! pointed at a temp directory. Skips locally, and hard-fails in CI, when Node is unavailable.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::path::Path;
use std::process::{Command, Output};

const OBSERVED_MARKER: &str = "observed in a past call";
const TAIL_BYTES: usize = 4 * 1024 * 1024;

const FAKE_SERVER_JS: &str = r"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin, terminal: false });
rl.on('line', (line) => {
  if (line.trim().length === 0) return;
  const m = JSON.parse(line);
  const reply = (result) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result }) + '\n');
  if (m.method === 'initialize') {
    reply({ protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1.0.0' } });
  } else if (m.method === 'tools/list') {
    reply({ tools: [{ name: 'noop', description: 'No-op', inputSchema: { type: 'object' } }] });
  }
});
";

fn node_available(test_name: &str) -> bool {
    if Command::new("node").arg("--version").output().is_ok() {
        return true;
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "{test_name}: node not found on PATH in CI; the fake MCP server needs it"
    );
    eprintln!("skipping {test_name}: node not found on PATH");
    false
}

fn shape_line(tool: &str, key: &str) -> String {
    format!(
        r#"{{"server":"fake","tool":"{tool}","shape":{{"type":"object","properties":{{"{key}":{{"type":"string"}}}}}}}}"#
    )
}

/// A file of about 6 MiB: `head` first, then filler lines for another tool, then `tail`.
fn observed_file(head: Option<&str>, tail: Option<&str>) -> String {
    let filler = shape_line("other", "filler");
    let mut content = String::new();
    if let Some(head) = head {
        content.push_str(head);
        content.push('\n');
    }
    while content.len() < 6 * 1024 * 1024 {
        content.push_str(&filler);
        content.push('\n');
    }
    if let Some(tail) = tail {
        content.push_str(tail);
        content.push('\n');
    }
    content
}

fn run_generate(root: &Path, observed: &Path) -> (Output, String) {
    let home = root.join("home");
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let script = root.join("fake-server.js");
    std::fs::write(&script, FAKE_SERVER_JS).unwrap();
    let mcp_json = serde_json::json!({
        "mcpServers": { "fake": { "command": "node", "args": [script.to_string_lossy()] } }
    });
    std::fs::write(home.join(".claude").join("mcp.json"), mcp_json.to_string()).unwrap();
    let out_dir = root.join("out");

    let output = Command::new(env!("CARGO_BIN_EXE_mcp-execution-cli"))
        .args([
            "generate",
            "--from-config",
            "fake",
            "--name",
            "fake",
            "--servers-dir",
        ])
        .arg(&out_dir)
        .arg("--observed-shapes")
        .arg(observed)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .expect("run mcp-execution-cli");

    let tool_file =
        std::fs::read_to_string(out_dir.join("fake").join("noop.ts")).unwrap_or_default();
    (output, tool_file)
}

fn root() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let canonical = dir.path().canonicalize().unwrap();
    // Node cannot resolve a Windows verbatim path (`\\?\C:\...`) given as a script.
    let path = canonical
        .to_str()
        .and_then(|s| s.strip_prefix(r"\\?\"))
        .map_or_else(|| canonical.clone(), std::path::PathBuf::from);
    (dir, path)
}

#[test]
fn generate_observed_shapes_reads_the_tail_of_a_file_over_4_mib_and_keeps_the_newest_shape() {
    let name =
        "generate_observed_shapes_reads_the_tail_of_a_file_over_4_mib_and_keeps_the_newest_shape";
    if !node_available(name) {
        return;
    }
    let (_guard, root) = root();
    let observed = root.join("observed.jsonl");
    let content = observed_file(
        Some(&shape_line("noop", "oldkey")),
        Some(&shape_line("noop", "newkey")),
    );
    assert!(
        content.len() > TAIL_BYTES,
        "fixture must exceed the tail window"
    );
    std::fs::write(&observed, content).unwrap();

    let (output, tool_file) = run_generate(&root, &observed);

    assert!(
        output.status.success(),
        "a large shapes file must not fail generation: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(tool_file.contains(OBSERVED_MARKER), "{tool_file}");
    assert!(tool_file.contains("newkey"), "{tool_file}");
    assert!(!tool_file.contains("oldkey"), "{tool_file}");
}

#[test]
fn generate_observed_shapes_ignores_lines_that_lie_before_the_last_4_mib() {
    let name = "generate_observed_shapes_ignores_lines_that_lie_before_the_last_4_mib";
    if !node_available(name) {
        return;
    }
    let (_guard, root) = root();
    let observed = root.join("observed.jsonl");
    let content = observed_file(Some(&shape_line("noop", "oldkey")), None);
    assert!(content.len() > TAIL_BYTES + shape_line("noop", "oldkey").len());
    std::fs::write(&observed, content).unwrap();

    let (output, tool_file) = run_generate(&root, &observed);

    assert!(
        output.status.success(),
        "generation must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!tool_file.is_empty(), "the tool file was generated");
    assert!(
        !tool_file.contains("oldkey") && !tool_file.contains(OBSERVED_MARKER),
        "{tool_file}"
    );
}
