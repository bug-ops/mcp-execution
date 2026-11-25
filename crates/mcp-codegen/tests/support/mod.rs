//! Shared helpers for the `mcp-execution-codegen` integration test crates.
//!
//! Holds the `ServerInfo` fixture builders, the TypeScript/Node toolchain checks and the
//! runtime-bridge execution harness so every test crate reaches them through `mod support;`
//! instead of copying them.

#![allow(
    dead_code,
    reason = "shared by several test crates; each uses a subset"
)]

use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::process::Command;

/// Creates a mock `ServerConfig` for testing, passed alongside `ServerInfo` so
/// `generate`/`generate_with_categories` can stamp generation provenance.
pub fn test_config() -> ServerConfig {
    ServerConfig::builder()
        .command("test-command".to_string())
        .build()
        .unwrap()
}

/// Creates a mock server info for testing.
pub fn create_test_server_info() -> ServerInfo {
    ServerInfo::new(
        ServerId::new("github").unwrap(),
        "GitHub".to_string(),
        "1.0.0".to_string(),
        vec![
            ToolInfo::new(
                ToolName::new("create_issue").unwrap(),
                "Creates a new issue".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string",
                            "description": "Repository name"
                        },
                        "title": {
                            "type": "string",
                            "description": "Issue title"
                        },
                        "body": {
                            "type": "string",
                            "description": "Issue body"
                        }
                    },
                    "required": ["repo", "title"]
                }),
            ),
            ToolInfo::new(
                ToolName::new("update_issue").unwrap(),
                "Updates an existing issue".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string"
                        },
                        "issue_number": {
                            "type": "number"
                        },
                        "title": {
                            "type": "string"
                        }
                    },
                    "required": ["repo", "issue_number"]
                }),
            ),
            ToolInfo::new(
                ToolName::new("get_issue").unwrap(),
                "Gets issue information".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string"
                        },
                        "issue_number": {
                            "type": "number"
                        }
                    },
                    "required": ["repo", "issue_number"]
                }),
            ),
        ],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    )
}

/// Name of the `tsc` executable to spawn directly via [`std::process::Command`].
///
/// `npm install -g typescript` installs `tsc` as a `.cmd`/`.ps1` shim script on Windows, not
/// a `.exe`. Windows' `CreateProcess` (which `Command` calls into directly, bypassing a
/// shell) only appends the `.exe` extension when none is given — it does not consult
/// `PATHEXT` the way `cmd.exe` does — so `Command::new("tsc")` silently fails to find it even
/// though `tsc` resolves fine when typed at a Windows shell prompt. Elsewhere, the real `tsc`
/// binary (or symlink to one) is on `PATH` unqualified.
pub const fn tsc_program() -> &'static str {
    if cfg!(windows) { "tsc.cmd" } else { "tsc" }
}

/// Ensures `tsc` (and, if `require_node`, `node`) are available on `PATH` before a test that
/// depends on the TypeScript/Node toolchain runs.
///
/// In CI (the `CI` env var is set — GitHub Actions sets it for every job) a missing
/// toolchain is a hard test failure rather than a silent skip: these tests exist
/// specifically to catch TypeScript-level regressions (e.g. #201's runtime-bridge
/// validation, #176's generated-wrapper type errors) that no Rust-only check can see, and a
/// CI runner missing Node/tsc would otherwise report green while running zero of that
/// coverage. Locally (no `CI` env var), a missing toolchain just skips the test with a
/// message, since not everyone running `cargo test` has Node installed.
///
/// Returns `true` if the required tools are present and the test should proceed.
pub fn require_ts_toolchain(test_name: &str, require_node: bool) -> bool {
    let tsc_missing = Command::new(tsc_program())
        .arg("--version")
        .output()
        .is_err();
    let node_missing = require_node && Command::new("node").arg("--version").output().is_err();

    if !tsc_missing && !node_missing {
        return true;
    }

    let missing = match (tsc_missing, node_missing) {
        (true, true) => "`tsc` and `node`",
        (true, false) => "`tsc`",
        (false, true) => "`node`",
        (false, false) => unreachable!("checked above"),
    };

    assert!(
        std::env::var_os("CI").is_none(),
        "{test_name}: {missing} not found on PATH in CI — this test exists to catch \
         TypeScript-level regressions and must not silently skip in CI. Install Node.js and \
         run `npm install -g typescript` in this CI job."
    );

    eprintln!("skipping {test_name}: {missing} not found on PATH");
    false
}

/// `npm install -g typescript` installs a global `tsc`, but not `npm` itself as `npm.cmd` on
/// Windows in a way `Command::new("npm")` would find — mirrors `tsc_program`'s reasoning.
pub const fn npm_program() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

/// Installs `dir`'s `package.json` `devDependencies` (currently just `@types/node`, added for
/// #183) via `npm install`, so `tsc --noEmit` can resolve the Node builtin modules and ambient
/// globals the real runtime bridge references. Without this, `tsc` would fail with `TS2307`/
/// `TS2580`/`TS2503` regardless of how correct the generated `tsconfig.json` is.
///
/// Same skip-locally/hard-fail-in-CI policy as [`require_ts_toolchain`]: this exists to catch
/// exactly the kind of regression a missing `@types/node` produces (see #183's critique), so
/// silently skipping in CI would defeat the point. A local `npm install` failure (e.g. no
/// network) skips rather than hard-failing, since CI is presumed to have registry access but a
/// local sandbox may not.
///
/// Passes `--include=dev` explicitly: with `NODE_ENV=production` set in the environment, `npm
/// install` silently omits `devDependencies` (exit success, nothing installed) instead of
/// failing, which would otherwise let a missing `@types/node` regression slip through with no
/// signal — `--include=dev` overrides that regardless of `NODE_ENV`.
///
/// Returns `true` if the install succeeded and the caller should proceed to `tsc`.
pub fn install_declared_dev_dependencies(dir: &std::path::Path, test_name: &str) -> bool {
    if Command::new(npm_program())
        .arg("--version")
        .output()
        .is_err()
    {
        assert!(
            std::env::var_os("CI").is_none(),
            "{test_name}: npm not found on PATH in CI — cannot install the generated \
             package.json's devDependencies, so this test cannot catch a missing @types/node \
             regression."
        );
        eprintln!("skipping {test_name}: npm not found on PATH");
        return false;
    }

    let output = Command::new(npm_program())
        .args([
            "install",
            "--no-save",
            "--no-audit",
            "--no-fund",
            "--include=dev",
        ])
        .current_dir(dir)
        .output()
        .expect("Failed to run npm install");

    if !output.status.success() {
        assert!(
            std::env::var_os("CI").is_none(),
            "{test_name}: npm install failed in CI:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!(
            "skipping {test_name}: npm install failed (no network access?):\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return false;
    }

    true
}

/// Ensures `node --experimental-strip-types` is usable before a test that depends on it runs.
/// Same skip-locally/hard-fail-in-CI policy as [`require_ts_toolchain`].
pub fn require_node_strip_types(test_name: &str) -> bool {
    let supported = Command::new("node")
        .args(["--experimental-strip-types", "--eval", ""])
        .output()
        .is_ok_and(|output| output.status.success());

    if supported {
        return true;
    }

    assert!(
        std::env::var_os("CI").is_none(),
        "{test_name}: `node --experimental-strip-types` not available in CI (needs Node \
         22.6+) — this test exists to catch ERR_MODULE_NOT_FOUND regressions that `tsc \
         --noEmit` cannot see."
    );

    eprintln!("skipping {test_name}: `node --experimental-strip-types` not available");
    false
}

/// One run of a harness script against the rendered runtime bridge.
#[derive(Debug, Clone, Copy)]
pub struct BridgeHarness<'a> {
    /// Name used in skip and failure messages.
    pub test_name: &'a str,
    /// The rendered `_runtime/mcp-bridge.ts`.
    pub bridge_ts: &'a str,
    /// TypeScript importing `./mcp-bridge.js`; compiled next to the bridge.
    pub harness_ts: &'a str,
    /// `CommonJS` preloaded with `node --require`, typically registering a helper channel.
    pub preload_js: Option<&'a str>,
    /// Extra files written into the working directory, which is also node's cwd.
    pub files: &'a [(&'a str, &'a str)],
    /// Extra environment variables for the node process.
    pub env: &'a [(&'a str, &'a str)],
}

/// Compiles the bridge and harness of `run` under `tsc` (type-erasure only, so `@types/node` is
/// not needed) and runs the result under Node inside `work_dir`, returning
/// `(exit_success, stdout, stderr)`.
///
/// The run is bounded at 15 s so a bridge that hangs instead of settling every pending call
/// fails the suite instead of stalling it.
///
/// Returns `None` if `tsc`/`node` are not on `PATH` and the caller should skip locally; hard
/// fails (via `require_ts_toolchain`) instead of returning `None` when running in CI.
pub fn run_bridge_harness_in(
    work_dir: &std::path::Path,
    run: &BridgeHarness<'_>,
) -> Option<(bool, String, String)> {
    if !require_ts_toolchain(run.test_name, true) {
        return None;
    }

    let src_dir = work_dir.join("src");
    std::fs::create_dir_all(&src_dir).expect("Failed to create src dir");
    // `--module NodeNext` picks CJS vs. ESM emit per-file based on the nearest
    // `package.json`'s "type" field, resolved from the *source* file's location — not the
    // `--outDir`. Without this, tsc silently emits CommonJS, which then fails at runtime once
    // Node sees `dist/package.json`'s `"type": "module"`.
    std::fs::write(src_dir.join("package.json"), "{\"type\":\"module\"}\n")
        .expect("Failed to write src/package.json");
    std::fs::write(src_dir.join("mcp-bridge.ts"), run.bridge_ts)
        .expect("Failed to write mcp-bridge.ts");
    std::fs::write(src_dir.join("harness.ts"), run.harness_ts).expect("Failed to write harness.ts");

    let dist_dir = work_dir.join("dist");
    let tsc_output = Command::new(tsc_program())
        .args(["--noCheck", "--module", "NodeNext", "--moduleResolution"])
        .arg("NodeNext")
        .arg("--target")
        .arg("ES2022")
        .arg("--outDir")
        .arg(&dist_dir)
        .arg(src_dir.join("mcp-bridge.ts"))
        .arg(src_dir.join("harness.ts"))
        .output()
        .expect("Failed to run tsc");
    assert!(
        tsc_output.status.success(),
        "tsc failed to compile the rendered runtime bridge:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&tsc_output.stdout),
        String::from_utf8_lossy(&tsc_output.stderr)
    );
    std::fs::write(dist_dir.join("package.json"), "{\"type\":\"module\"}\n")
        .expect("Failed to write dist/package.json");

    for (name, content) in run.files {
        std::fs::write(work_dir.join(name), content).expect("Failed to write a harness file");
    }

    let mut command = Command::new("node");
    if let Some(preload) = run.preload_js {
        let path = work_dir.join("preload.cjs");
        std::fs::write(&path, preload).expect("Failed to write the preload");
        command.arg("--require").arg(path);
    }
    command
        .arg(dist_dir.join("harness.js"))
        .current_dir(work_dir);
    for (key, value) in run.env {
        command.env(key, value);
    }

    let test_name = run.test_name.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(command.output());
    });
    let output = rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap_or_else(|_| {
            panic!(
                "node harness for {test_name} did not exit within 15s; the bridge may be \
                 hanging instead of settling every pending call"
            )
        })
        .expect("Failed to run node");

    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// [`run_bridge_harness_in`] in a fresh temp directory, with `helper_js` preloaded as the
/// helper channel and no extra files.
pub fn compile_and_run_bridge_harness(
    test_name: &str,
    bridge_ts: &str,
    helper_js: &str,
    harness_ts: &str,
    env: &[(&str, &str)],
) -> Option<(bool, String, String)> {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    run_bridge_harness_in(
        dir.path(),
        &BridgeHarness {
            test_name,
            bridge_ts,
            harness_ts,
            preload_js: Some(helper_js),
            files: &[],
            env,
        },
    )
}

/// Harness that calls `callMCPTool('fake', 'noop', {})` once, printing `RESOLVED: <json>` or
/// `REJECTED: <message>` (followed by ` SERVER_MESSAGE: <text>` when the rejection is an
/// `McpToolError` carrying server text) and exiting 0 in either case; the assertions inspect
/// stdout rather than the exit code.
pub const SINGLE_CALL_HARNESS_TS: &str = r"
import { callMCPTool, McpToolError } from './mcp-bridge.js';

callMCPTool('fake', 'noop', {}).then(
  (result: unknown) => {
    console.log('RESOLVED:', JSON.stringify(result));
    process.exit(0);
  },
  (err: unknown) => {
    const server = err instanceof McpToolError && err.serverMessage !== undefined
      ? ' SERVER_MESSAGE: ' + err.serverMessage
      : '';
    console.log('REJECTED:', String(err) + server);
    process.exit(0);
  }
);
";
