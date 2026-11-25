//! Under `run --dry-run` a bridge in a worker thread or in a child node process has no helper
//! channel. It must refuse to call instead of starting a helper of its own, because that helper
//! would have neither the dry-run policy nor the journal and the write would really execute.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Out, Sandbox, require_node};

fn prepared(test: &str) -> Option<Sandbox> {
    if !require_node(test) {
        return None;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    Some(sb)
}

fn dry_run(sb: &Sandbox, body: &str) -> Out {
    let script = sb.script("isolation.ts", body);
    sb.run_script(&script, &["--dry-run"])
}

fn assert_refused_without_a_call(sb: &Sandbox, out: &Out, marker: &str) {
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains(&format!("{marker}=allowed")),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.to_lowercase().contains("refused"),
        "a clear refusal is expected: {}",
        out.stdout
    );
    assert!(sb.fixture_log().is_empty(), "no call may reach the server");
}

#[test]
fn a_worker_thread_cannot_call_a_write_tool_under_dry_run() {
    let Some(sb) = prepared("a_worker_thread_cannot_call_a_write_tool_under_dry_run") else {
        return;
    };
    let index = format!("file://{}/fixture/index.ts", sb.servers.display());
    let body = format!(
        "import {{ Worker }} from 'node:worker_threads';\n\
         const code = `const m = await import({index:?});\n\
           try {{ await m.createItem({{}}); console.log('WORKER=allowed'); }}\n\
           catch (e) {{ console.log('WORKER=refused ' + e.message); }}`;\n\
         await new Promise((resolve) => new Worker(code, {{ eval: true }}).on('exit', resolve));\n"
    );

    let out = dry_run(&sb, &body);

    assert_refused_without_a_call(&sb, &out, "WORKER");
}

#[test]
fn a_child_node_process_cannot_call_a_write_tool_under_dry_run() {
    let Some(sb) = prepared("a_child_node_process_cannot_call_a_write_tool_under_dry_run") else {
        return;
    };
    let tool = format!("{}/fixture/createItem.ts", sb.servers.display());
    let body = format!(
        "import {{ spawnSync }} from 'node:child_process';\n\
         const child = spawnSync(process.execPath, [{tool:?}, '{{}}'], {{ encoding: 'utf8' }});\n\
         const text = (child.stdout + child.stderr).toLowerCase();\n\
         console.log(child.status === 0 ? 'CHILD=allowed' : 'CHILD=' + (text.includes('refused') ? 'refused' : 'failed'));\n"
    );

    let out = dry_run(&sb, &body);

    assert_refused_without_a_call(&sb, &out, "CHILD");
}

#[test]
fn a_nested_run_is_refused_before_node_starts() {
    if !require_node("a_nested_run_is_refused_before_node_starts") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("nested.ts", "console.log('should not run');\n");
    let script_arg = script.display().to_string();
    let servers = sb.servers.display().to_string();

    let out = common::run_with_watchdog(
        sb.command().env("MCP_EXECUTION_RUN", "1").args([
            "run",
            &script_arg,
            "--servers-dir",
            &servers,
        ]),
        None,
        common::WATCHDOG,
    );

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(!out.stdout.contains("run started"), "node must not start");
    assert!(!out.stdout.contains("should not run"));
    assert!(
        out.stderr.contains("not supported inside"),
        "{}",
        out.stderr
    );
}
