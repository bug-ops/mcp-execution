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
