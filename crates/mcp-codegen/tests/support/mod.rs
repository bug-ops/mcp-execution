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
    ServerInfo {
        id: ServerId::new("github").unwrap(),
        name: "GitHub".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![
            ToolInfo {
                name: ToolName::new("create_issue").unwrap(),
                description: "Creates a new issue".to_string(),
                input_schema: json!({
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
                output_schema: None,
                annotations: None,
            },
            ToolInfo {
                name: ToolName::new("update_issue").unwrap(),
                description: "Updates an existing issue".to_string(),
                input_schema: json!({
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
                output_schema: None,
                annotations: None,
            },
            ToolInfo {
                name: ToolName::new("get_issue").unwrap(),
                description: "Gets issue information".to_string(),
                input_schema: json!({
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
                output_schema: None,
                annotations: None,
            },
        ],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
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

/// Compiles `bridge_ts` (the rendered runtime bridge) alongside a harness that calls
/// `callMCPTool(server_id, "noop", {})`, runs it under Node with `$HOME`/`%USERPROFILE%`
/// pointed at a temp directory containing `mcp_json` as `~/.claude/mcp.json`, and returns
/// `(exit_success, stdout, stderr)`.
///
/// Uses `tsc --noCheck` (type-erasure only, no type-checking) so this doesn't need
/// `@types/node` installed to resolve the bridge's Node builtin imports (`child_process`,
/// `fs/promises`, `os`, `path`, `stream`).
///
/// A hostile/unvalidated config that reaches `spawn()` can hang forever here (the bridge
/// waits on a JSON-RPC response from a subprocess that will never send one) — that's
/// exactly the failure mode these tests exist to catch, so the harness itself races a 5s
/// watchdog against `callMCPTool`'s promise, and this helper additionally bounds the whole
/// Node run at 15s so a regression fails the test suite instead of hanging it.
///
/// Returns `None` if `tsc`/`node` are not on `PATH` and the caller should skip locally; hard
/// fails (via `require_ts_toolchain`) instead of returning `None` when running in CI.
pub fn run_bridge_harness(
    test_name: &str,
    bridge_ts: &str,
    mcp_json: &serde_json::Value,
    server_id: &str,
) -> Option<(bool, String, String)> {
    run_bridge_harness_with_env(test_name, bridge_ts, mcp_json, server_id, &[])
}

/// Same as [`run_bridge_harness`], additionally setting `extra_env` on the spawned Node
/// harness process (e.g. `MCPBRIDGE_REQUEST_TIMEOUT_MS` for timeout-specific tests).
pub fn run_bridge_harness_with_env(
    test_name: &str,
    bridge_ts: &str,
    mcp_json: &serde_json::Value,
    server_id: &str,
    extra_env: &[(&str, &str)],
) -> Option<(bool, String, String)> {
    let harness_ts = format!(
        "import {{ callMCPTool }} from './mcp-bridge.js';\n\
         \n\
         const watchdog = setTimeout(() => {{\n\
         \x20\x20console.log('TIMEOUT: did not settle before spawning');\n\
         \x20\x20process.exit(2);\n\
         }}, 5000);\n\
         \n\
         callMCPTool('{server_id}', 'noop', {{}}).then(\n\
         \x20\x20() => {{\n\
         \x20\x20\x20\x20clearTimeout(watchdog);\n\
         \x20\x20\x20\x20console.log('UNEXPECTED_SUCCESS');\n\
         \x20\x20\x20\x20process.exit(1);\n\
         \x20\x20}},\n\
         \x20\x20(err: unknown) => {{\n\
         \x20\x20\x20\x20clearTimeout(watchdog);\n\
         \x20\x20\x20\x20console.log('REJECTED:', String(err));\n\
         \x20\x20\x20\x20process.exit(0);\n\
         \x20\x20}}\n\
         );\n"
    );

    compile_and_run_bridge_harness(test_name, bridge_ts, mcp_json, &harness_ts, extra_env)
}

/// Shared innards of [`run_bridge_harness_with_env`] and the concurrency regression test below:
/// compiles `bridge_ts` plus a caller-supplied `harness_ts` under tsc, then runs the result
/// under Node with `$HOME`/`%USERPROFILE%` pointed at a temp directory containing `mcp_json`
/// as `~/.claude/mcp.json`. Factored out of `run_bridge_harness_with_env` so a harness body
/// other than the single fixed `callMCPTool('...', 'noop', {})` call (e.g. two concurrent
/// calls) does not need to duplicate the tsc/Node plumbing.
///
/// Returns `None` if `tsc`/`node` are not on `PATH` and the caller should skip locally; hard
/// fails (via `require_ts_toolchain`) instead of returning `None` when running in CI.
pub fn compile_and_run_bridge_harness(
    test_name: &str,
    bridge_ts: &str,
    mcp_json: &serde_json::Value,
    harness_ts: &str,
    extra_env: &[(&str, &str)],
) -> Option<(bool, String, String)> {
    if !require_ts_toolchain(test_name, true) {
        return None;
    }

    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).expect("Failed to create fake $HOME/.claude");
    std::fs::write(claude_dir.join("mcp.json"), mcp_json.to_string())
        .expect("Failed to write mcp.json");

    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("Failed to create src dir");
    // `--module NodeNext` picks CJS vs. ESM emit per-file based on the nearest
    // `package.json`'s "type" field, resolved from the *source* file's location — not the
    // `--outDir`. Without this, tsc silently emits CommonJS (`require`/`exports`), which
    // then fails at runtime once Node sees `dist/package.json`'s `"type": "module"`.
    std::fs::write(src_dir.join("package.json"), "{\"type\":\"module\"}\n")
        .expect("Failed to write src/package.json");
    std::fs::write(src_dir.join("mcp-bridge.ts"), bridge_ts)
        .expect("Failed to write mcp-bridge.ts");
    std::fs::write(src_dir.join("harness.ts"), harness_ts).expect("Failed to write harness.ts");

    let dist_dir = dir.path().join("dist");
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

    let harness_path = dist_dir.join("harness.js");
    let owned_extra_env: Vec<(String, String)> = extra_env
        .iter()
        .map(|&(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut cmd = Command::new("node");
        cmd.arg(&harness_path)
            // Node's `os.homedir()` — which `loadServerConfig()` uses to find
            // `~/.claude/mcp.json` — reads `$HOME` on POSIX but `%USERPROFILE%` on Windows;
            // both must point at the fake home directory for the harness to isolate itself
            // from the real runner's home on every platform.
            .env("HOME", &home_dir)
            .env("USERPROFILE", &home_dir);
        for (key, value) in &owned_extra_env {
            cmd.env(key, value);
        }
        let result = cmd.output();
        let _ = tx.send(result);
    });

    let output = rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap_or_else(|_| {
            panic!(
                "node harness for {test_name} did not exit within 15s; the bridge may be \
                 hanging instead of settling every pending request"
            )
        })
        .expect("Failed to run node");

    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Writes `content` to a standalone `.js` file inside a fresh temp directory and returns its
/// absolute path as a string, suitable for use as a stdio-transport `args` entry.
///
/// Used instead of `node -e '<code>'` for the #221 item 4 tests below: `(` and `)` are
/// themselves forbidden shell metacharacters (`FORBIDDEN_CHARS`), so virtually any real JS
/// snippet passed inline as an argument would trip `validateCommandString` before the
/// scenario under test — the process dying or hanging — ever gets a chance to run.
pub fn write_test_script(content: &str) -> String {
    let dir = tempfile::tempdir().expect("Failed to create temp dir for test script");
    let path = dir.path().join("script.js");
    std::fs::write(&path, content).expect("Failed to write test script");
    // Leak the tempdir so it outlives the spawned `node` process for the duration of the
    // test; these are single test-process-lifetime allocations, not a long-running leak.
    Box::leak(Box::new(dir));
    path.to_string_lossy().into_owned()
}

/// Builds a minimal fake MCP server script that answers `initialize` normally and replies to
/// the single `tools/call` request it receives with `result_json` verbatim as the `result`
/// field. Shared by the #255 regression tests below, which each need a different `result`
/// shape (empty `content`, `structuredContent`-only, etc.) but identical initialize/id
/// plumbing.
pub fn respond_once_fake_server_js(result_json: &str) -> String {
    format!(
        r"
const readline = require('readline');
const rl = readline.createInterface({{ input: process.stdin, terminal: false }});

rl.on('line', (line) => {{
  const trimmed = line.trim();
  if (trimmed.length === 0) return;
  const message = JSON.parse(trimmed);

  if (message.method === 'initialize') {{
    const response = {{
      jsonrpc: '2.0',
      id: message.id,
      result: {{
        protocolVersion: '2024-11-05',
        capabilities: {{}},
        serverInfo: {{ name: 'fake', version: '0.0.0' }}
      }}
    }};
    process.stdout.write(JSON.stringify(response) + '\n');
    return;
  }}

  if (message.method === 'tools/call') {{
    const response = {{
      jsonrpc: '2.0',
      id: message.id,
      result: {result_json}
    }};
    process.stdout.write(JSON.stringify(response) + '\n');
  }}
}});
"
    )
}

/// Harness that calls `callMCPTool('fake', 'noop', {})` once, printing `RESOLVED: <json>` or
/// `REJECTED: <message>` and exiting 0 in either case (the fake servers below never hang, so no
/// watchdog is needed) — the assertions inspect stdout rather than the exit code.
pub const SINGLE_CALL_HARNESS_TS: &str = r"
import { callMCPTool } from './mcp-bridge.js';

callMCPTool('fake', 'noop', {}).then(
  (result: unknown) => {
    console.log('RESOLVED:', JSON.stringify(result));
    process.exit(0);
  },
  (err: unknown) => {
    console.log('REJECTED:', String(err));
    process.exit(0);
  }
);
";
