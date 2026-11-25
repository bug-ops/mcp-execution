//! Acceptance test of the run wrap-up bound against hanging MCP sessions (#636): after a signal,
//! closing sessions of servers that never answer the close must not keep the runner longer than
//! `WRAP_UP_BOUND` plus the kill, and the dropped call summary is announced as a notice.
//!
//! The HTTP fixture stalls every session `DELETE`, so each session close runs into the 2 s close
//! bound of the introspector. Sessions are closed one after another, so six of them need at
//! least 12 s without the bound and the test can tell the bound from a lucky fast close.
//!
//! Unix only: the runner is stopped with a signal.

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::io::Read as _;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{SERVER, Sandbox, require_node, spawn_http_fixture_with_env};
use rustix::process::{Pid, Signal, kill_process};
use serde_json::{Value, json};

const HANGING_SESSIONS: usize = 6;
/// Mirrors the private `WRAP_UP_BOUND` of `commands::run`.
const WRAP_UP_BOUND: Duration = Duration::from_secs(4);
/// Time allowed on top of the bound for the kill and the exit.
const KILL_SLACK: Duration = Duration::from_secs(2);
const MARKER_WITHIN: Duration = Duration::from_secs(60);
const WATCHDOG: Duration = Duration::from_secs(60);
const SUMMARY_DROPPED: &str = "call summary dropped: closing the MCP sessions was cut short";

struct KillRunnerOnDrop(Child);

impl Drop for KillRunnerOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + MARKER_WITHIN;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "the script never finished its calls"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_signalled_run_with_hanging_sessions_exits_within_the_wrap_up_bound() {
    if !require_node("a_signalled_run_with_hanging_sessions_exits_within_the_wrap_up_bound") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let (_server, url) = spawn_http_fixture_with_env(None, &[("MCP_FIXTURE_HANG_ON_CLOSE", "1")]);
    let ids: Vec<String> = (1..=HANGING_SESSIONS)
        .map(|n| format!("hang-{n}"))
        .collect();
    let mut servers = serde_json::Map::new();
    servers.insert(SERVER.to_string(), sb.fixture_entry());
    for id in &ids {
        servers.insert(id.clone(), json!({ "type": "http", "url": url }));
    }
    sb.write_mcp_json(&Value::Object(servers));

    let marker = sb.root.join("calls.done");
    let bridge = sb
        .servers
        .join(SERVER)
        .join("_runtime")
        .join("mcp-bridge.ts");
    let script = sb.script(
        "hang.ts",
        &format!(
            "import fs from 'node:fs';\n\
             import {{ callMCPTool }} from {bridge:?};\n\
             for (const id of {ids}) {{ await callMCPTool(id, 'echo', {{}}, 'content'); }}\n\
             fs.writeFileSync({marker:?}, 'done');\n",
            bridge = bridge.display().to_string(),
            ids = json!(ids),
            marker = marker.display().to_string(),
        ),
    );
    let mut cmd = sb.command();
    cmd.args(["--format", "json", "run", &script.display().to_string()])
        .args(["--servers-dir", &sb.servers.display().to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut runner = KillRunnerOnDrop(cmd.spawn().unwrap());
    let mut stdout = runner.0.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    wait_for_file(&marker);

    kill_process(Pid::from_child(&runner.0), Signal::INT).unwrap();
    let signalled = Instant::now();
    let code = loop {
        if let Some(status) = runner.0.try_wait().unwrap() {
            break status.code();
        }
        assert!(
            signalled.elapsed() < WATCHDOG,
            "the runner is still alive {WATCHDOG:?} after SIGINT"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let elapsed = signalled.elapsed();

    let stdout = reader.join().unwrap();
    assert_eq!(code, Some(130), "stdout: {stdout}");
    assert!(
        elapsed < WRAP_UP_BOUND + KILL_SLACK,
        "the runner took {elapsed:?} to exit after SIGINT; the bound is {WRAP_UP_BOUND:?}"
    );
    let envelope: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not one JSON document ({e}): {stdout:.300}"));
    let notices = envelope["notices"].as_array().unwrap();
    assert!(
        notices.iter().any(|notice| notice == SUMMARY_DROPPED),
        "missing the dropped-summary notice: {notices:?}"
    );
}
