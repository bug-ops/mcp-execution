//! Acceptance tests of the per-run helper channel (#575): the hello and its version check,
//! size bounds on both directions, connection-count rules of the Windows pipe, and the
//! script-side behaviour (one initialize for concurrent first calls, out-of-order responses
//! matched by id, a lost channel rejecting later calls).
//!
//! Raw-protocol tests drive an in-process `HelperServer` with a tiny Node client; the rest run
//! scripts through `run`. Requires the `test-fixtures` feature and Node.js 22.18+.

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use common::{Sandbox, require_node, run_with_watchdog};
use mcp_execution_cli::runtime::helper::{
    Carrier, DryRunMode, HelperConfig, HelperEndpoint, HelperHandle, HelperServer,
};
use mcp_execution_core::helper_protocol::{HELPER_HELLO_TIMEOUT, MAX_HELPER_LINE_BYTES};
use mcp_execution_introspector::CallTimeout;
use serde_json::{Value, json};

/// Minimal helper client. `H_HELLO` is the hello object (`"token": "from-env"` is replaced with
/// the endpoint's token); `H_MODE` is `silent` (never send the hello) or `exit_after_reply`
/// (leave as soon as the first line arrives); `H_OVERSIZE` sends one request line of that many padding bytes;
/// every received line and connection event is printed as one JSON line.
const CLIENT_JS: &str = r"
const net = require('node:net');
const hello = JSON.parse(process.env.H_HELLO);
if (hello.token === 'from-env') hello.token = process.env.MCP_EXECUTION_HELPER_TOKEN;
const oversize = Number(process.env.H_OVERSIZE || 0);
const mode = process.env.H_MODE || '';
const target = process.env.MCP_EXECUTION_HELPER;
const isPipe = typeof target === 'string' && target.startsWith('pipe:');
const emit = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const sock = isPipe ? net.connect(target.slice(5)) : new net.Socket({ fd: 0, readable: true, writable: true });
let buf = '';
sock.setEncoding('utf8');
sock.on('data', (d) => {
  buf += d;
  let i;
  while ((i = buf.indexOf('\n')) >= 0) {
    emit({ recv: buf.slice(0, i) });
    buf = buf.slice(i + 1);
    if (mode === 'exit_after_reply') setTimeout(() => process.exit(0), 20);
  }
});
sock.on('close', () => { emit({ event: 'closed' }); process.exit(0); });
sock.on('error', (e) => emit({ event: 'error', code: e.code }));
const go = () => {
  if (mode !== 'silent') sock.write(JSON.stringify(hello) + '\n');
  setTimeout(() => {
    if (oversize > 0) {
      sock.write(JSON.stringify({ id: 1, server: 'fixture', tool: 'echo',
        arguments: { pad: 'x'.repeat(oversize) }, result: 'content' }) + '\n');
    }
  }, 300);
};
if (isPipe) sock.on('connect', go); else go();
setTimeout(() => { emit({ event: 'timeout' }); process.exit(0); }, Number(process.env.H_LIMIT_MS || 8000));
";

fn bind() -> (HelperHandle, HelperEndpoint, tempfile::TempDir) {
    let servers = tempfile::tempdir().unwrap();
    let (handle, endpoint) = HelperServer::bind(
        Carrier::native(),
        HelperConfig {
            servers_dir: servers.path().to_path_buf(),
            call_timeout: CallTimeout::new(Duration::from_secs(30)).unwrap(),
            journal: None,
            dry_run: DryRunMode::Off,
        },
    )
    .expect("bind the helper");
    (handle, endpoint, servers)
}

/// Spawns the Node client against `endpoint` and returns what it printed, as JSON values.
fn run_client_mode(
    dir: &Path,
    env: &[(&'static str, String)],
    stdin: Stdio,
    hello: &Value,
    oversize: usize,
    limit_ms: u64,
    mode: &str,
) -> Vec<Value> {
    let script = dir.join("client.js");
    std::fs::write(&script, CLIENT_JS).unwrap();
    let mut cmd = std::process::Command::new("node");
    cmd.arg(&script)
        .env("H_HELLO", hello.to_string())
        .env("H_OVERSIZE", oversize.to_string())
        .env("H_LIMIT_MS", limit_ms.to_string())
        .env("H_MODE", mode);
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.stdin(stdin);
    let out = run_with_watchdog(&mut cmd, None, Duration::from_secs(60));
    out.stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("client line {l:?}: {e}")))
        .collect()
}

fn recv_lines(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|e| e["recv"].as_str())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("helper line {l:?}: {e}")))
        .collect()
}

fn closed(events: &[Value]) -> bool {
    events.iter().any(|e| e["event"] == "closed")
}

/// Connects with `hello` and returns the client's events.
async fn connect_with(hello: Value, oversize: usize) -> Vec<Value> {
    connect_with_mode(hello, oversize, "", 8000).await
}

async fn connect_with_mode(
    hello: Value,
    oversize: usize,
    mode: &'static str,
    limit_ms: u64,
) -> Vec<Value> {
    let (handle, mut endpoint, servers) = bind();
    let env = endpoint.env();
    let stdin = endpoint.take_stdin().unwrap_or_else(Stdio::null);
    let dir = servers.path().to_path_buf();
    let events = tokio::task::spawn_blocking(move || {
        run_client_mode(&dir, &env, stdin, &hello, oversize, limit_ms, mode)
    })
    .await
    .unwrap();
    let _ = handle.finish().await;
    events
}

#[tokio::test]
async fn helper_answers_a_matching_hello_with_the_current_version() {
    if !require_node("helper_answers_a_matching_hello_with_the_current_version") {
        return;
    }

    let events = connect_with_mode(
        json!({"v": 1, "token": "from-env"}),
        0,
        "exit_after_reply",
        8000,
    )
    .await;

    assert_eq!(
        recv_lines(&events).first(),
        Some(&json!({"v": 1})),
        "{events:?}"
    );
}

/// Slow by design (about 5 s): the helper's hello timeout is a fixed constant with no seam.
#[tokio::test]
async fn helper_closes_a_channel_that_never_sends_a_hello_after_the_hello_timeout() {
    if !require_node("helper_closes_a_channel_that_never_sends_a_hello_after_the_hello_timeout") {
        return;
    }
    let started = std::time::Instant::now();

    let events = connect_with_mode(json!({}), 0, "silent", 20_000).await;

    assert!(closed(&events), "{events:?}");
    assert!(
        !events.iter().any(|e| e["event"] == "timeout"),
        "the helper must close the silent channel itself: {events:?}"
    );
    let waited = started.elapsed();
    assert!(
        waited + Duration::from_millis(500) >= HELPER_HELLO_TIMEOUT
            && waited < HELPER_HELLO_TIMEOUT + Duration::from_secs(8),
        "closed after {waited:?}, expected about {HELPER_HELLO_TIMEOUT:?}"
    );
}

#[tokio::test]
async fn helper_closes_the_channel_on_a_hello_version_mismatch() {
    if !require_node("helper_closes_the_channel_on_a_hello_version_mismatch") {
        return;
    }
    let started = std::time::Instant::now();

    let events = connect_with(json!({"v": 999, "token": "from-env"}), 0).await;

    assert!(closed(&events), "{events:?}");
    assert!(
        started.elapsed() < HELPER_HELLO_TIMEOUT + Duration::from_secs(3)
            && !events.iter().any(|e| e["event"] == "timeout"),
        "must close on the mismatch, not wait for the hello timeout: {events:?}"
    );
    assert!(
        recv_lines(&events).iter().all(|l| l["v"] != 999),
        "a mismatched hello must not be accepted: {events:?}"
    );
}

#[tokio::test]
async fn helper_answers_an_oversized_request_with_call_failed_and_closes_the_channel() {
    if !require_node("helper_answers_an_oversized_request_with_call_failed_and_closes_the_channel")
    {
        return;
    }

    let events = connect_with(
        json!({"v": 1, "token": "from-env"}),
        MAX_HELPER_LINE_BYTES + 1024,
    )
    .await;

    let lines = recv_lines(&events);
    assert!(
        lines.iter().any(|l| l["error"]["kind"] == "call_failed"),
        "no call_failed reply: {events:?}"
    );
    assert!(closed(&events), "{events:?}");
}

#[tokio::test]
async fn finish_without_any_call_reports_nothing_and_leaves_no_state() {
    let (handle, _endpoint, _servers) = bind();

    let report = handle.finish().await;

    assert_eq!(report.journal_summary, None);
    assert_eq!(report.dry_run_report, None);
}

#[cfg(windows)]
#[tokio::test]
async fn pipe_rejects_a_hello_with_a_wrong_token() {
    if !require_node("pipe_rejects_a_hello_with_a_wrong_token") {
        return;
    }

    let events = connect_with(json!({"v": 1, "token": "not-the-token"}), 0).await;

    assert!(closed(&events), "{events:?}");
    assert!(
        recv_lines(&events).iter().all(|l| l["v"] != 1),
        "a wrong token must not be answered: {events:?}"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn pipe_refuses_a_second_connection() {
    if !require_node("pipe_refuses_a_second_connection") {
        return;
    }
    let (handle, endpoint, servers) = bind();
    let env = endpoint.env();
    let dir = servers.path().to_path_buf();
    let hello = json!({"v": 1, "token": "from-env"});
    let (first_env, first_dir, first_hello) = (env.clone(), dir.clone(), hello.clone());
    let first = tokio::task::spawn_blocking(move || {
        run_client_mode(
            &first_dir,
            &first_env,
            Stdio::null(),
            &first_hello,
            0,
            6000,
            "",
        )
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let second = tokio::task::spawn_blocking(move || {
        run_client_mode(&dir, &env, Stdio::null(), &hello, 0, 3000, "")
    })
    .await
    .unwrap();

    assert!(
        recv_lines(&second).iter().all(|l| l["v"] != 1),
        "a second connection must not complete the hello: {second:?}"
    );
    let first = first.await.unwrap();
    assert_eq!(
        recv_lines(&first).first(),
        Some(&json!({"v": 1})),
        "{first:?}"
    );
    let _ = handle.finish().await;
}

#[test]
fn two_concurrent_first_calls_open_one_connection() {
    if !require_node("two_concurrent_first_calls_open_one_connection") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let script = sb.script(
        "concurrent.ts",
        "import { echo } from 'mcp:fixture';\n\
         const [a, b] = await Promise.all([echo({ n: 1 }), echo({ n: 2 })]);\n\
         console.log('RESULTS=' + JSON.stringify([a, b]));\n",
    );
    let before = sb.fixture_log().len();

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#"RESULTS=[{"n":1},{"n":2}]"#),
        "{}",
        out.stdout
    );
    let log = sb.fixture_log();
    let during: Vec<&String> = log.iter().skip(before).collect();
    assert_eq!(
        during.iter().filter(|l| l.as_str() == "initialize").count(),
        1,
        "log: {log:?}"
    );
    assert_eq!(
        during.iter().filter(|l| l.as_str() == "call echo").count(),
        2
    );
}

#[test]
fn responses_arriving_out_of_order_are_matched_by_id() {
    if !require_node("responses_arriving_out_of_order_are_matched_by_id") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let script = sb.script(
        "order.ts",
        "import { slow, echo } from 'mcp:fixture';\n\
         const order: string[] = [];\n\
         const slowCall = slow({ ms: 1500 }).then((v) => { order.push('slow'); return v; });\n\
         const fastCall = echo({ tag: 'fast' }).then((v) => { order.push('echo'); return v; });\n\
         const [s, f] = await Promise.all([slowCall, fastCall]);\n\
         console.log('ORDER=' + order.join(','));\n\
         console.log('SLOW=' + JSON.stringify(s));\n\
         console.log('FAST=' + JSON.stringify(f));\n",
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("ORDER=echo,slow"), "{}", out.stdout);
    assert!(out.stdout.contains(r#"SLOW="done""#), "{}", out.stdout);
    assert!(
        out.stdout.contains(r#"FAST={"tag":"fast"}"#),
        "{}",
        out.stdout
    );
}

#[test]
fn a_result_over_the_helper_line_bound_becomes_call_failed() {
    if !require_node("a_result_over_the_helper_line_bound_becomes_call_failed") {
        return;
    }
    let sb = Sandbox::new();
    let (_server, url) = common::spawn_http_fixture(None);
    sb.add_http_server("fixture-http", &url);
    sb.generate_server("fixture-http");
    let script = sb.script(
        "oversize.ts",
        "import { large } from 'mcp:fixture-http';\n\
         try {\n\
           await large({ bytes: 5 * 1024 * 1024 });\n\
           console.log('UNEXPECTED success');\n\
         } catch (e) {\n\
           console.log('KIND=' + e.kind);\n\
           console.log('MESSAGE=' + e.message);\n\
         }\n",
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("KIND=call_failed"), "{}", out.stdout);
    assert!(out.stdout.contains("exceeds"), "{}", out.stdout);
}

#[test]
fn a_lost_channel_rejects_in_flight_and_later_calls() {
    if !require_node("a_lost_channel_rejects_in_flight_and_later_calls") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let script = sb.script(
        "lost.ts",
        "import { slow, echo } from 'mcp:fixture';\n\
         const channel = (globalThis as any)[Symbol.for('mcp-execution.helper-channel')];\n\
         const inFlight = slow({ ms: 30000 }).then(() => 'resolved', (e) => 'rejected:' + e.message);\n\
         await new Promise((resolve) => setTimeout(resolve, 800));\n\
         channel.destroy();\n\
         console.log('INFLIGHT=' + await inFlight);\n\
         try { await echo({}); console.log('LATER=resolved'); }\n\
         catch (e) { console.log('LATER=rejected:' + e.message); }\n",
    );

    let out = sb.run_script(&script, &["--timeout", "30"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("INFLIGHT=rejected:"), "{}", out.stdout);
    assert!(out.stdout.contains("LATER=rejected:"), "{}", out.stdout);
    assert!(
        out.stdout.matches("helper channel closed").count() >= 2,
        "{}",
        out.stdout
    );
    assert!(
        out.elapsed < Duration::from_secs(25),
        "took {:?}",
        out.elapsed
    );
}
