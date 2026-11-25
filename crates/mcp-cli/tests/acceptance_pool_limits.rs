//! Acceptance tests of the helper's concurrency bounds (#575): at most 16 calls in flight, at
//! most 16 pooled servers, and a server that failed to start can be retried once it exists.
//!
//! Scripts import `callMCPTool` straight from the generated bridge so they can address server
//! ids that have no generated package.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{FIXTURE_BIN, SERVER, Sandbox, file_url, require_node};
use mcp_execution_cli::runtime::pool::{MAX_INFLIGHT_CALLS, MAX_POOLED_SERVERS};
use serde_json::json;

fn prepared(test: &str) -> Option<Sandbox> {
    if !require_node(test) {
        return None;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    Some(sb)
}

fn bridge_import(sb: &Sandbox) -> String {
    let bridge = sb
        .servers
        .join(SERVER)
        .join("_runtime")
        .join("mcp-bridge.ts");
    format!("import {{ callMCPTool }} from '{}';\n", file_url(&bridge))
}

#[test]
fn no_more_than_the_in_flight_limit_of_calls_run_at_once() {
    let Some(sb) = prepared("no_more_than_the_in_flight_limit_of_calls_run_at_once") else {
        return;
    };
    let calls = MAX_INFLIGHT_CALLS * 2 + 8;
    let rounds = calls.div_ceil(MAX_INFLIGHT_CALLS);
    let script = sb.script(
        "inflight.ts",
        &format!(
            "import {{ slow }} from 'mcp:fixture';\n\
             const started = Date.now();\n\
             await Promise.all(Array.from({{ length: {calls} }}, () => slow({{ ms: 500 }})));\n\
             console.log('ELAPSED_MS=' + (Date.now() - started));\n"
        ),
    );

    let out = sb.run_script(&script, &["--timeout", "60"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let elapsed: u64 = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("ELAPSED_MS="))
        .expect("the script printed its elapsed time")
        .parse()
        .unwrap();
    let floor = u64::try_from(rounds).unwrap() * 500 - 100;
    assert!(
        elapsed >= floor,
        "{calls} half-second calls took {elapsed} ms; with at most {MAX_INFLIGHT_CALLS} in flight they need {rounds} rounds ({floor} ms at least)"
    );
    assert!(elapsed < 20_000, "calls must overlap, took {elapsed} ms");
}

#[test]
fn the_pool_serves_sixteen_servers_and_refuses_the_next_one() {
    let Some(sb) = prepared("the_pool_serves_sixteen_servers_and_refuses_the_next_one") else {
        return;
    };
    let ids: Vec<String> = (1..=MAX_POOLED_SERVERS + 1)
        .map(|n| format!("pool-{n:02}"))
        .collect();
    let mut servers = serde_json::Map::new();
    servers.insert(SERVER.to_string(), sb.fixture_entry());
    for id in &ids {
        servers.insert(id.clone(), sb.fixture_entry());
    }
    sb.write_mcp_json(&serde_json::Value::Object(servers));
    let script = sb.script(
        "pool.ts",
        &format!(
            "{}const ids = {};\n\
             const outcomes: string[] = [];\n\
             for (const id of ids) {{\n\
               try {{ await callMCPTool(id, 'echo', {{}}, 'content'); outcomes.push('ok'); }}\n\
               catch (e: any) {{ outcomes.push(String(e.kind)); }}\n\
             }}\n\
             console.log('OUTCOMES=' + outcomes.join(','));\n",
            bridge_import(&sb),
            json!(ids)
        ),
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let mut expected = vec!["ok"; MAX_POOLED_SERVERS];
    expected.push("call_failed");
    assert!(
        out.stdout
            .contains(&format!("OUTCOMES={}", expected.join(","))),
        "{}",
        out.stdout
    );
}

#[test]
fn a_server_that_failed_to_start_is_retried_on_the_next_call() {
    let Some(sb) = prepared("a_server_that_failed_to_start_is_retried_on_the_next_call") else {
        return;
    };
    let late = sb
        .root
        .join(format!("late-server{}", std::env::consts::EXE_SUFFIX));
    sb.write_mcp_json(&json!({
        SERVER: sb.fixture_entry(),
        "late": { "command": late.display().to_string() },
    }));
    let script = sb.script(
        "retry.ts",
        &format!(
            "import fs from 'node:fs';\n{}\
             const attempt = async () => {{\n\
               try {{ await callMCPTool('late', 'echo', {{}}, 'content'); return 'ok'; }}\n\
               catch (e: any) {{ return String(e.kind); }}\n\
             }};\n\
             console.log('FIRST=' + await attempt());\n\
             fs.copyFileSync({FIXTURE_BIN:?}, {late:?});\n\
             fs.chmodSync({late:?}, 0o755);\n\
             console.log('SECOND=' + await attempt());\n",
            bridge_import(&sb)
        ),
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("FIRST=call_failed"), "{}", out.stdout);
    assert!(
        out.stdout.contains("SECOND=ok"),
        "a failed start must not be cached: {}",
        out.stdout
    );
}
