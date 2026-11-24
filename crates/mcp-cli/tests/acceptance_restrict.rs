//! Acceptance tests of `run --restrict` (#576): on Unix the script cannot write files, read files
//! outside its allow list or spawn processes, yet its tool calls still work through the helper
//! channel; on Windows the option is refused with exit 2.
//!
//! Requires the `test-fixtures` feature and Node.js 22.18+.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, require_node};

#[cfg(unix)]
#[test]
fn permission_args_allow_reading_only_the_script_servers_and_run_dir() {
    use std::ffi::OsString;
    use std::path::Path;

    use mcp_execution_cli::runtime::restrict::permission_args;

    let args = permission_args(
        Path::new("/work/script.ts"),
        Path::new("/home/u/.claude/servers"),
        Path::new("/state/runs/r1"),
    )
    .unwrap();

    let expected: Vec<OsString> = [
        "--permission",
        "--allow-fs-read=/work/script.ts",
        "--allow-fs-read=/home/u/.claude/servers",
        "--allow-fs-read=/state/runs/r1",
    ]
    .map(OsString::from)
    .to_vec();
    assert_eq!(
        args, expected,
        "one read grant per path, no comma-joined form, no write or child-process grant"
    );
}

#[cfg(unix)]
#[test]
fn restrict_denies_file_writes_foreign_reads_and_child_processes() {
    if !require_node("restrict_denies_file_writes_foreign_reads_and_child_processes") {
        return;
    }
    let sb = Sandbox::new();
    let victim = sb.root.join("written-by-script.txt");
    let foreign = sb.root.join("foreign-secret.txt");
    std::fs::write(&foreign, "do-not-read").unwrap();
    let script = sb.script(
        "restricted.ts",
        &format!(
            "import fs from 'node:fs';\n\
             import {{ execFileSync }} from 'node:child_process';\n\
             const attempt = (name: string, f: () => unknown) => {{\n\
               try {{ f(); console.log(name + '=allowed'); }} catch {{ console.log(name + '=denied'); }}\n\
             }};\n\
             attempt('WRITE', () => fs.writeFileSync({victim:?}, 'x'));\n\
             attempt('READ', () => fs.readFileSync({foreign:?}, 'utf8'));\n\
             attempt('SPAWN', () => execFileSync('echo', ['hi']));\n"
        ),
    );

    let out = sb.run_script(&script, &["--restrict"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    for name in ["WRITE", "READ", "SPAWN"] {
        assert!(
            out.stdout.contains(&format!("{name}=denied")),
            "{}",
            out.stdout
        );
    }
    assert!(!victim.exists(), "the denied write must not happen");
}

#[cfg(unix)]
#[test]
fn restrict_still_lets_the_script_call_tools_through_the_helper() {
    if !require_node("restrict_still_lets_the_script_call_tools_through_the_helper") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let script = sb.script(
        "restricted-call.ts",
        "import { echo } from 'mcp:fixture';\n\
         console.log('RESULT=' + JSON.stringify(await echo({ ok: 1 })));\n",
    );

    let out = sb.run_script(&script, &["--restrict"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains(r#"RESULT={"ok":1}"#), "{}", out.stdout);
    assert!(out.elapsed < std::time::Duration::from_secs(25));
}

#[cfg(windows)]
#[test]
fn restrict_is_refused_on_windows_with_exit_2_before_node_starts() {
    if !require_node("restrict_is_refused_on_windows_with_exit_2_before_node_starts") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("noop.ts", "console.log('should not run');\n");

    let out = sb.run_script(&script, &["--restrict"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(!out.stdout.contains("should not run"));
    assert!(
        !out.stdout.contains("run started"),
        "node must not have started"
    );
}
