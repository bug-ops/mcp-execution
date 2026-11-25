//! `drift` finds the generated directory the way `generate` places it: `--name` and
//! `--servers-dir` carry the same meaning in both commands (FR-045.10).

#![cfg(feature = "test-fixtures")]

mod common;

use common::Sandbox;

#[test]
fn drift_with_name_reads_the_directory_generate_name_wrote() {
    let sb = Sandbox::new();
    let servers = sb.servers.display().to_string();
    let out = sb.cli(&[
        "generate",
        "--from-config",
        "fixture",
        "--name",
        "renamed",
        "--servers-dir",
        &servers,
    ]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);

    let out = sb.cli(&["drift", "--from-config", "fixture", "--name", "renamed"]);

    assert_eq!(out.code, Some(0), "{} {}", out.stdout, out.stderr);
    assert_ne!(sb.cli(&["drift", "--from-config", "fixture"]).code, Some(0));
}

#[test]
fn drift_with_servers_dir_reads_that_base_directory() {
    let sb = Sandbox::new();
    let custom = sb.root.join("custom-servers");
    let custom_arg = custom.display().to_string();
    let out = sb.cli(&[
        "generate",
        "--from-config",
        "fixture",
        "--servers-dir",
        &custom_arg,
    ]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);

    let out = sb.cli(&[
        "drift",
        "--from-config",
        "fixture",
        "--servers-dir",
        &custom_arg,
    ]);

    assert_eq!(out.code, Some(0), "{} {}", out.stdout, out.stderr);
}

#[test]
fn drift_rejects_an_invalid_name_without_connecting() {
    let sb = Sandbox::new();

    let out = sb.cli(&["drift", "--from-config", "fixture", "--name", "../x"]);

    assert_ne!(out.code, Some(0), "{}", out.stderr);
    assert_ne!(out.code, Some(5), "{}", out.stderr);
}
