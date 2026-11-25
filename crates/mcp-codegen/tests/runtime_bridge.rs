//! Integration tests that compile and execute the generated `_runtime/mcp-bridge.ts` under Node
//! against scripted helper channels.
//!
//! The bridge is a thin client of the helper protocol (#575): these tests pin its side of the
//! wire (framing, id matching, error mapping, limits, `ref`/`unref` accounting, channel loss and
//! the spawned-helper path). The Rust side of the same protocol is covered by the CLI crate's
//! helper tests.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod support;

use std::process::Command;

use mcp_execution_codegen::package::PackageGenerator;
use mcp_execution_core::helper_protocol::{
    CLI_PATH_ENV, HELPER_ENV, HELPER_TOKEN_ENV, MAX_HELPER_LINE_BYTES, RUN_MARKER_ENV,
};
use serde_json::Value;
use support::{BridgeHarness, create_test_server_info, run_bridge_harness_in, test_config};

fn rendered_bridge() -> String {
    let code = PackageGenerator::new()
        .expect("Failed to create generator")
        .generate(&create_test_server_info(), &test_config())
        .expect("Failed to generate code");
    code.files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found")
        .content
        .clone()
}

/// A preload registering a scripted helper channel. `body` defines `onLine(message)` for every
/// line the bridge writes and may call `send(object)` and `channel.destroy()`. `received` holds
/// the request lines (those with an `id`); `events` records `ref`/`unref` calls in order.
fn scripted_channel(body: &str) -> String {
    format!(
        r"
const {{ Duplex }} = require('node:stream');
const events = [];
const received = [];
globalThis.__events = events;
globalThis.__received = received;
let buffered = '';
const channel = new Duplex({{
  read() {{}},
  write(chunk, _encoding, callback) {{
    buffered += chunk.toString('utf8');
    const lines = buffered.split('\n');
    buffered = lines.pop();
    for (const line of lines) {{
      if (line.trim() === '') continue;
      const message = JSON.parse(line);
      if (message.id !== undefined) received.push(message);
      onLine(message);
    }}
    callback();
  }},
}});
channel.ref = () => events.push('ref');
channel.unref = () => events.push('unref');
const send = (object) => channel.push(JSON.stringify(object) + '\n');
send({{ v: 1 }});
globalThis[Symbol.for('mcp-execution.helper-channel')] = channel;
{body}
"
    )
}

const ECHO_BODY: &str = "function onLine(m) { if (m.id !== undefined) send({ id: m.id, value: { echoed: m.arguments } }); }";

/// Runs `harness_ts` against `preload`, returning the last stdout line parsed as JSON.
fn run(test_name: &str, preload: &str, harness_ts: &str) -> Option<Value> {
    let dir = tempfile::tempdir().expect("temp dir");
    let (success, stdout, stderr) = run_bridge_harness_in(
        dir.path(),
        &BridgeHarness {
            test_name,
            bridge_ts: &rendered_bridge(),
            harness_ts,
            preload_js: Some(preload),
            files: &[],
            env: &[],
        },
    )?;
    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    Some(parse_last_line(&stdout))
}

/// Runs `harness_ts` with no preloaded channel, so the bridge has to find its own helper.
fn run_without_channel(
    test_name: &str,
    harness_ts: &str,
    files: &[(&str, &str)],
    env: &[(&str, &str)],
) -> Option<Value> {
    let dir = tempfile::tempdir().expect("temp dir");
    let (success, stdout, stderr) = run_bridge_harness_in(
        dir.path(),
        &BridgeHarness {
            test_name,
            bridge_ts: &rendered_bridge(),
            harness_ts,
            preload_js: None,
            files,
            env,
        },
    )?;
    assert!(
        success,
        "node must exit by itself once no call is pending: stdout: {stdout}\nstderr: {stderr}"
    );
    Some(parse_last_line(&stdout))
}

fn parse_last_line(stdout: &str) -> Value {
    let line = stdout
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_else(|| panic!("the harness printed nothing"));
    serde_json::from_str(line).unwrap_or_else(|e| panic!("last line is not JSON ({e}): {line}"))
}

/// Harness calling `callMCPTool('fake', 'noop', { a: 1 })` once and printing the outcome as JSON:
/// `{ok, value}` or `{isMcpToolError, kind, message, serverMessage, truncated}`.
const ONE_CALL: &str = r"
import { callMCPTool, McpToolError } from './mcp-bridge.js';
try {
  const value = await callMCPTool('fake', 'noop', { a: 1 });
  console.log(JSON.stringify({ ok: true, value: value === undefined ? '<undefined>' : value }));
} catch (e: any) {
  console.log(JSON.stringify({
    isMcpToolError: e instanceof McpToolError,
    kind: e.kind,
    message: e.message,
    serverMessage: e.serverMessage,
    truncated: e.serverMessageTruncated,
  }));
}
";

/// Harness making two calls in a row and printing every rejection message.
const TWO_CALLS_MESSAGES: &str = r"
import { callMCPTool } from './mcp-bridge.js';
const messages: string[] = [];
for (let i = 0; i < 2; i++) {
  try { await callMCPTool('fake', 'noop', {}); } catch (e: any) { messages.push(e.message); }
}
console.log(JSON.stringify(messages));
";

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect()
}

#[test]
fn bridge_sends_the_documented_request_line_and_resolves_the_value() {
    let body =
        "function onLine(m) { if (m.id !== undefined) send({ id: m.id, value: { request: m } }); }";
    let Some(seen) = run(
        "bridge_sends_the_documented_request_line_and_resolves_the_value",
        &scripted_channel(body),
        ONE_CALL,
    ) else {
        return;
    };

    assert_eq!(seen["ok"], true, "{seen}");
    let request = &seen["value"]["request"];
    assert_eq!(request["server"], "fake");
    assert_eq!(request["tool"], "noop");
    assert_eq!(request["arguments"], serde_json::json!({"a": 1}));
    assert_eq!(
        request["result"], "content",
        "content is the default source"
    );
    assert!(request["id"].is_u64(), "{request}");
}

#[test]
fn bridge_resolves_an_empty_response_to_undefined() {
    let body = "function onLine(m) { if (m.id !== undefined) send({ id: m.id, empty: true }); }";
    let Some(seen) = run(
        "bridge_resolves_an_empty_response_to_undefined",
        &scripted_channel(body),
        ONE_CALL,
    ) else {
        return;
    };

    assert_eq!(seen["value"], "<undefined>", "{seen}");
}

#[test]
fn bridge_maps_every_error_kind_and_keeps_server_text_out_of_the_message() {
    let cases = [
        ("tool_error", "tool_error"),
        ("rpc_error", "rpc_error"),
        ("call_failed", "call_failed"),
        ("write_skipped", "write_skipped"),
        ("something_new", "call_failed"),
    ];
    for (wire_kind, expected) in cases {
        let body = format!(
            "function onLine(m) {{ if (m.id !== undefined) send({{ id: m.id, error: {{ kind: '{wire_kind}', message: 'fixed text', server_message: 'SERVERTEXT', server_message_truncated: true }} }}); }}"
        );
        let Some(seen) = run(
            "bridge_maps_every_error_kind_and_keeps_server_text_out_of_the_message",
            &scripted_channel(&body),
            ONE_CALL,
        ) else {
            return;
        };

        assert_eq!(seen["isMcpToolError"], true, "{wire_kind}: {seen}");
        assert_eq!(seen["kind"], expected, "{wire_kind}");
        assert_eq!(seen["message"], "fixed text");
        assert_eq!(seen["serverMessage"], "SERVERTEXT");
        assert_eq!(seen["truncated"], true);
    }
}

#[test]
fn bridge_treats_a_response_without_a_body_as_call_failed() {
    let body = "function onLine(m) { if (m.id !== undefined) send({ id: m.id }); }";
    let Some(seen) = run(
        "bridge_treats_a_response_without_a_body_as_call_failed",
        &scripted_channel(body),
        ONE_CALL,
    ) else {
        return;
    };

    assert_eq!(seen["kind"], "call_failed", "{seen}");
}

#[test]
fn bridge_matches_out_of_order_responses_by_id() {
    let body = r"
const waiting = [];
function onLine(m) {
  if (m.id === undefined) return;
  waiting.push(m);
  if (waiting.length === 2) {
    for (const r of [...waiting].reverse()) send({ id: r.id, value: r.arguments.n });
  }
}";
    let harness = r"
import { callMCPTool } from './mcp-bridge.js';
const [a, b] = await Promise.all([
  callMCPTool('fake', 'noop', { n: 1 }),
  callMCPTool('fake', 'noop', { n: 2 }),
]);
console.log(JSON.stringify({ a, b }));
";
    let Some(seen) = run(
        "bridge_matches_out_of_order_responses_by_id",
        &scripted_channel(body),
        harness,
    ) else {
        return;
    };

    assert_eq!(seen, serde_json::json!({"a": 1, "b": 2}));
}

#[test]
fn bridge_tells_the_user_to_regenerate_when_the_helper_speaks_another_version() {
    let preload = scripted_channel(ECHO_BODY).replace("send({ v: 1 });", "send({ v: 99 });");
    let Some(seen) = run(
        "bridge_tells_the_user_to_regenerate_when_the_helper_speaks_another_version",
        &preload,
        TWO_CALLS_MESSAGES,
    ) else {
        return;
    };

    let messages = strings(&seen);
    assert!(messages[0].contains("regenerate"), "{messages:?}");
    assert!(
        messages[1].contains("helper channel closed"),
        "{messages:?}"
    );
}

#[test]
fn bridge_rejects_pending_and_later_calls_when_the_channel_drops() {
    let body = "function onLine(m) { if (m.id !== undefined) channel.destroy(); }";
    let Some(seen) = run(
        "bridge_rejects_pending_and_later_calls_when_the_channel_drops",
        &scripted_channel(body),
        TWO_CALLS_MESSAGES,
    ) else {
        return;
    };

    let messages = strings(&seen);
    assert_eq!(messages.len(), 2, "{seen}");
    for message in messages {
        assert!(message.contains("helper channel closed"), "{message}");
    }
}

#[test]
fn bridge_destroys_the_channel_when_a_response_line_exceeds_the_bound() {
    let body = format!(
        "function onLine(m) {{ if (m.id !== undefined) channel.push('x'.repeat({})); }}",
        MAX_HELPER_LINE_BYTES + 1
    );
    let Some(seen) = run(
        "bridge_destroys_the_channel_when_a_response_line_exceeds_the_bound",
        &scripted_channel(&body),
        ONE_CALL,
    ) else {
        return;
    };

    assert_eq!(seen["kind"], "call_failed", "{seen}");
    let message = seen["message"].as_str().unwrap();
    assert!(message.contains("helper channel closed"), "{message}");
    assert!(
        message.contains(&MAX_HELPER_LINE_BYTES.to_string()),
        "{message}"
    );
}

#[test]
fn bridge_rejects_an_oversized_request_without_sending_it() {
    let harness = format!(
        r"
import {{ callMCPTool }} from './mcp-bridge.js';
let message = '';
try {{ await callMCPTool('fake', 'noop', {{ blob: 'x'.repeat({}) }}); }} catch (e: any) {{ message = e.message; }}
console.log(JSON.stringify({{ message, sent: (globalThis as any).__received.length }}));
",
        MAX_HELPER_LINE_BYTES + 1
    );
    let Some(seen) = run(
        "bridge_rejects_an_oversized_request_without_sending_it",
        &scripted_channel(ECHO_BODY),
        &harness,
    ) else {
        return;
    };

    assert_eq!(seen["sent"], 0, "{seen}");
    assert!(
        seen["message"]
            .as_str()
            .unwrap()
            .contains(&MAX_HELPER_LINE_BYTES.to_string()),
        "{seen}"
    );
}

#[test]
fn bridge_refs_the_channel_only_while_a_call_is_pending() {
    let harness = r"
import { callMCPTool } from './mcp-bridge.js';
await callMCPTool('fake', 'noop', {});
await callMCPTool('fake', 'noop', {});
console.log(JSON.stringify((globalThis as any).__events));
";
    let Some(seen) = run(
        "bridge_refs_the_channel_only_while_a_call_is_pending",
        &scripted_channel(ECHO_BODY),
        harness,
    ) else {
        return;
    };

    assert_eq!(
        seen,
        serde_json::json!(["unref", "ref", "unref", "ref", "unref"]),
        "an idle channel must never keep node alive"
    );
}

#[test]
fn bridge_never_spawns_a_helper_when_a_runner_channel_was_announced_but_not_registered() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir
        .path()
        .join("no-such-binary")
        .to_string_lossy()
        .into_owned();
    let Some(seen) = run_without_channel(
        "bridge_never_spawns_a_helper_when_a_runner_channel_was_announced_but_not_registered",
        ONE_CALL,
        &[],
        &[(HELPER_ENV, "fd:0"), (CLI_PATH_ENV, &missing)],
    ) else {
        return;
    };

    let message = seen["message"].as_str().unwrap();
    assert!(message.contains(HELPER_ENV), "{message}");
    assert!(
        !message.contains(CLI_PATH_ENV),
        "no spawn may be attempted: {message}"
    );
}

fn node_executable() -> String {
    let output = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .expect("run node");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A stand-in for `mcp-execution-cli`: the bridge runs `<MCP_EXECUTION_CLI> helper`, and with
/// `MCP_EXECUTION_CLI` set to the node binary that is `node helper`, i.e. this file in the cwd.
const SPAWNED_HELPER_JS: &str = r"
const readline = require('node:readline');
const rl = readline.createInterface({ input: process.stdin, terminal: false });
const out = (object) => process.stdout.write(JSON.stringify(object) + '\n');
let greeted = false;
rl.on('line', (line) => {
  const message = JSON.parse(line);
  if (!greeted) {
    greeted = message.v === 1;
    out({ v: 1 });
    return;
  }
  out({ id: message.id, value: {
    script: require('node:path').basename(process.argv[1]),
    leakedHelperEnv: process.env.__HELPER_ENV__ !== undefined,
    leakedTokenEnv: process.env.__TOKEN_ENV__ !== undefined,
  } });
});
";

#[test]
fn bridge_spawns_the_helper_over_stdio_and_exits_when_idle() {
    let helper = SPAWNED_HELPER_JS
        .replace("__HELPER_ENV__", HELPER_ENV)
        .replace("__TOKEN_ENV__", HELPER_TOKEN_ENV);
    let node = node_executable();
    let Some(seen) = run_without_channel(
        "bridge_spawns_the_helper_over_stdio_and_exits_when_idle",
        ONE_CALL,
        &[("helper", &helper)],
        &[(CLI_PATH_ENV, &node), (HELPER_TOKEN_ENV, "secret")],
    ) else {
        return;
    };

    assert_eq!(seen["ok"], true, "{seen}");
    assert_eq!(
        seen["value"]["script"], "helper",
        "the bridge runs `<MCP_EXECUTION_CLI> helper`"
    );
    assert_eq!(seen["value"]["leakedTokenEnv"], false, "{seen}");
    assert_eq!(seen["value"]["leakedHelperEnv"], false, "{seen}");
}

#[test]
fn bridge_names_the_cli_variable_when_the_helper_cannot_start() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir
        .path()
        .join("no-such-binary")
        .to_string_lossy()
        .into_owned();
    let Some(seen) = run_without_channel(
        "bridge_names_the_cli_variable_when_the_helper_cannot_start",
        ONE_CALL,
        &[],
        &[(CLI_PATH_ENV, &missing)],
    ) else {
        return;
    };

    assert_eq!(seen["kind"], "call_failed", "{seen}");
    assert!(
        seen["message"].as_str().unwrap().contains(CLI_PATH_ENV),
        "{seen}"
    );
}

/// The helper command resolution is a pure function of platform, variable, `PATH` and a file
/// predicate, so the Windows rules (never a bare name: the current directory is searched before
/// `PATH` there, CWE-427; relative `PATH` entries skipped) are checked on every platform.
#[test]
fn bridge_resolves_the_helper_command_from_absolute_path_entries_only() {
    let harness = r"
import { resolveHelperCommand } from './mcp-bridge.js';
const files = new Set([
  'C:\\bin\\mcp-execution-cli.exe',
  '.\\mcp-execution-cli.exe',
  'tools\\mcp-execution-cli.exe',
  'mcp-execution-cli.exe',
  '/opt/bin/mcp-execution-cli',
  'rel/mcp-execution-cli',
]);
const isFile = (p: string) => files.has(p);
const outcome = (platform: string, configured: string | undefined, path: string | undefined) => {
  try { return { ok: resolveHelperCommand(platform, configured, path, isFile) }; }
  catch (e: any) { return { error: e.message, kind: e.kind }; }
};
console.log(JSON.stringify({
  windows_resolves_from_path: outcome('win32', undefined, 'C:\\other;C:\\bin'),
  windows_skips_relative_and_empty_entries: outcome('win32', undefined, '.;;tools;C:\\bin'),
  windows_only_relative_entries: outcome('win32', undefined, '.;tools;'),
  windows_no_path: outcome('win32', undefined, undefined),
  windows_configured_wins: outcome('win32', 'C:\\custom\\cli.exe', 'C:\\bin'),
  windows_configured_relative: outcome('win32', 'tools\\planted.exe', 'C:\\bin'),
  windows_configured_bare: outcome('win32', 'mcp-execution-cli.exe', 'C:\\bin'),
  posix_resolves_from_path: outcome('linux', undefined, 'rel:/usr/bin:/opt/bin'),
  posix_only_relative_entries: outcome('linux', undefined, 'rel:.'),
  posix_configured_relative: outcome('linux', 'planted/mcp-execution-cli', '/opt/bin'),
}));
";
    let Some(seen) = run_without_channel(
        "bridge_resolves_the_helper_command_from_absolute_path_entries_only",
        harness,
        &[],
        &[],
    ) else {
        return;
    };

    assert_eq!(
        seen["windows_resolves_from_path"]["ok"], "C:\\bin\\mcp-execution-cli.exe",
        "{seen}"
    );
    assert_eq!(
        seen["windows_skips_relative_and_empty_entries"]["ok"], "C:\\bin\\mcp-execution-cli.exe",
        "{seen}"
    );
    assert_eq!(
        seen["windows_configured_wins"]["ok"], "C:\\custom\\cli.exe",
        "{seen}"
    );
    assert_eq!(
        seen["posix_resolves_from_path"]["ok"], "/opt/bin/mcp-execution-cli",
        "{seen}"
    );
    for not_found in [
        "windows_only_relative_entries",
        "windows_no_path",
        "posix_only_relative_entries",
    ] {
        assert_eq!(
            seen[not_found]["error"], "mcp-execution-cli not found on PATH; set MCP_EXECUTION_CLI",
            "{not_found}"
        );
        assert_eq!(seen[not_found]["kind"], "call_failed");
    }
    for unusable in [
        "windows_configured_relative",
        "windows_configured_bare",
        "posix_configured_relative",
    ] {
        let message = seen[unusable]["error"].as_str().unwrap();
        assert!(message.contains(CLI_PATH_ENV), "{unusable}: {message}");
        assert!(!message.contains("planted"), "{unusable}: {message}");
    }
}

/// A planted binary in the current directory is never run: `.` on `PATH` is a relative entry.
#[test]
fn bridge_does_not_run_a_binary_planted_in_the_current_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    let node = node_executable();
    let node_dir = std::path::Path::new(&node).parent().unwrap().to_path_buf();
    let path = std::env::join_paths([std::path::PathBuf::from("."), node_dir]).unwrap();
    let marker_file = dir.path().join("planted-ran.marker");
    let planted = format!(
        "require('node:fs').writeFileSync({:?}, 'x');",
        marker_file.to_string_lossy()
    );
    let name = if cfg!(windows) {
        "mcp-execution-cli.exe"
    } else {
        "mcp-execution-cli"
    };
    let Some((success, stdout, stderr)) = run_bridge_harness_in(
        dir.path(),
        &BridgeHarness {
            test_name: "bridge_does_not_run_a_binary_planted_in_the_current_directory",
            bridge_ts: &rendered_bridge(),
            harness_ts: ONE_CALL,
            preload_js: None,
            files: &[(name, &planted)],
            env: &[("PATH", &path.to_string_lossy())],
        },
    ) else {
        return;
    };

    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    let seen = parse_last_line(&stdout);
    assert_eq!(
        seen["message"], "mcp-execution-cli not found on PATH; set MCP_EXECUTION_CLI",
        "{seen}"
    );
    assert!(!marker_file.exists(), "the planted binary must not run");
}

#[cfg(unix)]
#[test]
fn bridge_finds_the_helper_in_an_absolute_path_entry() {
    use std::os::unix::fs::PermissionsExt as _;
    let bin = tempfile::tempdir().expect("bin dir");
    let node = node_executable();
    let script = bin.path().join("helper.js");
    let helper = SPAWNED_HELPER_JS
        .replace("__HELPER_ENV__", HELPER_ENV)
        .replace("__TOKEN_ENV__", HELPER_TOKEN_ENV);
    std::fs::write(&script, helper).unwrap();
    let launcher = bin.path().join("mcp-execution-cli");
    std::fs::write(
        &launcher,
        format!("#!/bin/sh\nexec '{node}' '{}'\n", script.display()),
    )
    .unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
    let node_dir = std::path::Path::new(&node).parent().unwrap().to_path_buf();
    let path = std::env::join_paths([bin.path().to_path_buf(), node_dir]).unwrap();

    let Some(seen) = run_without_channel(
        "bridge_finds_the_helper_in_an_absolute_path_entry",
        ONE_CALL,
        &[],
        &[("PATH", &path.to_string_lossy())],
    ) else {
        return;
    };

    assert_eq!(seen["ok"], true, "{seen}");
}

#[test]
fn bridge_refuses_to_spawn_a_helper_when_the_run_marker_is_set_without_a_channel() {
    let dir = tempfile::tempdir().expect("temp dir");
    let node = node_executable();
    let marker_file = dir.path().join("helper-started.marker");
    let helper = format!(
        "require('node:fs').writeFileSync({:?}, 'x');",
        marker_file.to_string_lossy()
    );
    let Some(seen) = run_without_channel(
        "bridge_refuses_to_spawn_a_helper_when_the_run_marker_is_set_without_a_channel",
        TWO_CALLS_MESSAGES,
        &[("helper", &helper)],
        &[(CLI_PATH_ENV, &node), (RUN_MARKER_ENV, "1")],
    ) else {
        return;
    };

    let messages = strings(&seen);
    assert_eq!(messages.len(), 2, "every call is refused: {seen}");
    for message in messages {
        assert!(
            message.contains("refused")
                && message.contains(
                    "from a child process or worker thread are not supported under `run`"
                ),
            "{message}"
        );
    }
    assert!(!marker_file.exists(), "no helper process may be started");
}

#[test]
fn bridge_uses_a_registered_channel_even_when_the_run_marker_is_set() {
    let dir = tempfile::tempdir().expect("temp dir");
    let Some((success, stdout, stderr)) = run_bridge_harness_in(
        dir.path(),
        &BridgeHarness {
            test_name: "bridge_uses_a_registered_channel_even_when_the_run_marker_is_set",
            bridge_ts: &rendered_bridge(),
            harness_ts: ONE_CALL,
            preload_js: Some(&scripted_channel(ECHO_BODY)),
            files: &[],
            env: &[(RUN_MARKER_ENV, "1")],
        },
    ) else {
        return;
    };

    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    assert_eq!(parse_last_line(&stdout)["ok"], true, "{stdout}");
}

#[test]
fn bridge_refuses_a_relative_cli_path_on_every_call_and_never_falls_back_to_path() {
    let Some(seen) = run_without_channel(
        "bridge_refuses_a_relative_cli_path_on_every_call_and_never_falls_back_to_path",
        TWO_CALLS_MESSAGES,
        &[],
        &[(CLI_PATH_ENV, "relative-secret-name")],
    ) else {
        return;
    };

    let messages = strings(&seen);
    assert_eq!(messages.len(), 2, "{seen}");
    for message in messages {
        assert!(message.contains(CLI_PATH_ENV), "{message}");
        assert!(message.contains("unusable"), "{message}");
        assert!(
            !message.contains("relative-secret-name"),
            "the value must not be repeated: {message}"
        );
    }
}

#[test]
fn bridge_treats_a_response_with_id_zero_as_fatal_for_the_channel() {
    let body = "function onLine(m) { if (m.id !== undefined) send({ id: 0, error: { kind: 'call_failed', message: 'bad line' } }); }";
    let Some(seen) = run(
        "bridge_treats_a_response_with_id_zero_as_fatal_for_the_channel",
        &scripted_channel(body),
        TWO_CALLS_MESSAGES,
    ) else {
        return;
    };

    let messages = strings(&seen);
    assert_eq!(messages.len(), 2, "{seen}");
    for message in messages {
        assert!(message.contains("helper channel closed"), "{message}");
    }
}

#[test]
fn bridge_rejects_pending_calls_when_the_spawned_helper_dies() {
    let dying = r"
process.stdout.write(JSON.stringify({ v: 1 }) + '\n');
process.stdin.once('data', () => process.exit(3));
";
    let node = node_executable();
    let Some(seen) = run_without_channel(
        "bridge_rejects_pending_calls_when_the_spawned_helper_dies",
        ONE_CALL,
        &[("helper", dying)],
        &[(CLI_PATH_ENV, &node)],
    ) else {
        return;
    };

    assert!(
        seen["message"]
            .as_str()
            .unwrap()
            .contains("helper channel closed"),
        "{seen}"
    );
}
