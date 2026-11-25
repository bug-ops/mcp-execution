//! Acceptance tests of Windows reserved device names as server ids on the CLI (#696): an explicit
//! one is invalid input, a derived one gets `-server` appended.

#![cfg(feature = "test-fixtures")]

mod common;

use common::Sandbox;
use serde_json::json;

#[test]
fn generate_rejects_a_reserved_name_override_as_invalid_input() {
    let sb = Sandbox::new();

    let out = sb.cli(&["generate", "--from-config", "fixture", "--name", "con"]);

    assert_eq!(out.code, Some(2), "{} {}", out.stdout, out.stderr);
    assert!(
        out.stderr.contains("reserved device name"),
        "{}",
        out.stderr
    );
    assert!(!sb.servers.join("con").exists());
}

#[test]
fn generate_from_config_suggests_a_suffixed_name_for_a_reserved_key() {
    let sb = Sandbox::new();
    sb.write_mcp_json(&json!({ "con": sb.fixture_entry() }));

    let out = sb.cli(&["generate", "--from-config", "con"]);

    assert_ne!(out.code, Some(0), "{}", out.stdout);
    assert!(out.stderr.contains("--name con-server"), "{}", out.stderr);
    assert!(!sb.servers.join("con").exists());
}

/// `con` is a command found on `PATH` that starts the fixture server, so the id is derived from
/// the command name.
#[cfg(unix)]
#[test]
fn generate_derives_a_suffixed_id_from_a_command_named_like_a_device() {
    use common::{WATCHDOG, run_with_watchdog};
    use std::os::unix::fs::PermissionsExt as _;

    let sb = Sandbox::new();
    let bin = sb.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let con = bin.join("con");
    std::fs::write(&con, format!("#!/bin/sh\nexec '{}'\n", common::FIXTURE_BIN)).unwrap();
    std::fs::set_permissions(&con, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut cmd = sb.command();
    cmd.env("PATH", path)
        .args(["generate", "con", "--servers-dir"])
        .arg(&sb.servers);

    let out = run_with_watchdog(&mut cmd, None, WATCHDOG);

    assert_eq!(out.code, Some(0), "{} {}", out.stdout, out.stderr);
    assert!(sb.servers.join("con-server").join("index.ts").is_file());
    assert!(!sb.servers.join("con").exists());
}

/// An MCP server that records its own start in `argv[2]` and lists no tools.
const EMPTY_SERVER: &str = r"const fs = require('node:fs');
const readline = require('node:readline');
fs.writeFileSync(process.argv[2], 'started');
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.id === undefined) return;
  const reply = (body) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: message.id, ...body }) + '\n');
  if (message.method === 'initialize') {
    reply({ result: { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'empty', version: '1' } } });
  } else {
    reply({ result: { tools: [] } });
  }
});
";

#[test]
fn generate_rejects_an_invalid_config_key_before_the_server_is_started() {
    for key in ["nul", "My_Server"] {
        let sb = Sandbox::new();
        sb.write_mcp_json(&json!({ key: sb.fixture_entry() }));

        let out = sb.cli(&["generate", "--from-config", key]);

        assert_eq!(out.code, Some(1), "{key}: {} {}", out.stdout, out.stderr);
        assert!(
            out.stderr.contains("is not a valid directory name"),
            "{key}: {}",
            out.stderr
        );
        assert_eq!(sb.fixture_log(), [] as [String; 0], "{key}: server started");
    }
}

#[test]
fn generate_rejects_an_invalid_config_key_of_a_server_without_tools() {
    if !common::require_node("generate_rejects_an_invalid_config_key_of_a_server_without_tools") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("empty.cjs", EMPTY_SERVER);
    let started = sb.root.join("started");
    sb.write_mcp_json(&json!({ "My_Server": {
        "command": "node",
        "args": [script.display().to_string(), started.display().to_string()]
    }}));

    let out = sb.cli(&["generate", "--from-config", "My_Server"]);

    assert_eq!(out.code, Some(1), "{} {}", out.stdout, out.stderr);
    assert!(
        out.stderr.contains("is not a valid directory name"),
        "{}",
        out.stderr
    );
    assert!(!started.exists(), "the server was started");
}

#[test]
fn generate_rejects_an_unusable_servers_dir_before_the_server_is_started() {
    let sb = Sandbox::new();

    let out = sb.cli(&[
        "generate",
        "--from-config",
        common::SERVER,
        "--servers-dir",
        "~someone/servers",
    ]);

    assert_ne!(out.code, Some(0), "{} {}", out.stdout, out.stderr);
    assert_eq!(sb.fixture_log(), [] as [String; 0], "server started");
}
