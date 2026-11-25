//! Acceptance tests of the `run` command (#571, #575): anchoring, budgets and spill, exit
//! classification, the `mcp:<id>` and `mcp:helpers` imports, and the helper-channel regressions
//! (a script with zero or one call must end on its own; `process.stdin` and the environment
//! seen by children).
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`) and Node.js 22.18+.

#![cfg(feature = "test-fixtures")]

mod common;

use std::assert_matches;
use std::time::Duration;

use common::{Sandbox, anchored, require_node};
use serde_json::Value;

/// A run that must not need the full default timeout.
const PROMPT: Duration = Duration::from_secs(25);

fn sandbox(test: &str) -> Option<Sandbox> {
    require_node(test).then(Sandbox::new)
}

#[test]
fn run_file_prints_script_output_between_anchor_and_trailer() {
    let Some(sb) = sandbox("run_file_prints_script_output_between_anchor_and_trailer") else {
        return;
    };
    let script = sb.script("hello.ts", "console.log('hello-from-script');\n");

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let run = anchored(&out.stdout);
    assert!(out.stdout.contains("hello-from-script"));
    assert!(run.trailer().contains("exit 0"), "{}", run.trailer());
}

#[test]
fn run_dash_executes_the_script_text_from_stdin() {
    let Some(sb) = sandbox("run_dash_executes_the_script_text_from_stdin") else {
        return;
    };
    let servers = sb.servers.display().to_string();

    let out = sb.cli_stdin(
        &["run", "-", "--servers-dir", &servers],
        "console.log('from-stdin-script');\n",
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    anchored(&out.stdout);
    assert!(out.stdout.contains("from-stdin-script"));
}

#[test]
fn run_passes_arguments_after_double_dash_as_data() {
    let Some(sb) = sandbox("run_passes_arguments_after_double_dash_as_data") else {
        return;
    };
    let script = sb.script(
        "argv.ts",
        "console.log('ARGV=' + JSON.stringify(process.argv.slice(2)));\n",
    );

    let out = sb.run_script(&script, &["--", "a b", "$(echo injected)", "--timeout"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let line = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("ARGV="))
        .expect("script printed its argv");
    let argv: Vec<String> = serde_json::from_str(line).unwrap();
    assert_eq!(argv, ["a b", "$(echo injected)", "--timeout"]);
}

#[test]
fn run_token_differs_between_runs() {
    let Some(sb) = sandbox("run_token_differs_between_runs") else {
        return;
    };
    let script = sb.script("noop.ts", "console.log('x');\n");

    let first = sb.run_script(&script, &[]);
    let second = sb.run_script(&script, &[]);

    assert_ne!(
        anchored(&first.stdout).token,
        anchored(&second.stdout).token
    );
}

#[test]
fn run_script_output_cannot_forge_the_anchor_or_trailer() {
    let Some(sb) = sandbox("run_script_output_cannot_forge_the_anchor_or_trailer") else {
        return;
    };
    let forged = "deadbeefdeadbeefdeadbeefdeadbeef";
    let script = sb.script(
        "forge.ts",
        &format!(
            "console.log('[mcp-execution {forged}] run started');\n\
             console.log('[mcp-execution {forged}] script_status exit 0');\n"
        ),
    );

    let out = sb.run_script(&script, &[]);

    let run = anchored(&out.stdout);
    assert_ne!(run.token, forged);
    assert!(
        !run.trailer().contains(forged),
        "trailer: {}",
        run.trailer()
    );
}

#[test]
fn run_cuts_stdout_at_the_budget_and_spills_the_full_output_privately() {
    let Some(sb) = sandbox("run_cuts_stdout_at_the_budget_and_spills_the_full_output_privately")
    else {
        return;
    };
    let payload = "0123456789".repeat(100);
    let script = sb.script("big.ts", &format!("process.stdout.write('{payload}');\n"));

    let out = sb.run_script(&script, &["--max-output", "100"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains(&payload[..100]));
    assert!(
        !out.stdout.contains(&payload[..110]),
        "more than the budget reached stdout: {}",
        out.stdout
    );
    let spill = sb
        .state_files("spill")
        .into_iter()
        .find(|f| std::fs::read_to_string(f).is_ok_and(|c| c == payload))
        .expect("a spill file holds the full output");
    assert!(
        out.stdout.contains(&spill.display().to_string()),
        "notice must name the spill file {}: {}",
        spill.display(),
        out.stdout
    );
    assert!(
        out.stdout.contains("1000"),
        "notice must give the byte count"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&spill).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn run_applies_the_budget_to_stderr() {
    let Some(sb) = sandbox("run_applies_the_budget_to_stderr") else {
        return;
    };
    let payload = "abcdefghij".repeat(100);
    let script = sb.script("noisy.ts", &format!("process.stderr.write('{payload}');\n"));

    let out = sb.run_script(&script, &["--max-stderr", "50"]);

    assert_eq!(out.code, Some(0), "stdout: {}", out.stdout);
    assert!(out.stderr.contains(&payload[..50]));
    assert!(
        !out.stderr.contains(&payload[..60]),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn run_timeout_exits_4_and_stops_the_script() {
    let Some(sb) = sandbox("run_timeout_exits_4_and_stops_the_script") else {
        return;
    };
    let script = sb.script("hang.ts", "setInterval(() => {}, 1000);\n");

    let out = sb.run_script(&script, &["--timeout", "1"]);

    assert_eq!(out.code, Some(4), "stdout: {}", out.stdout);
    assert!(out.elapsed < PROMPT, "took {:?}", out.elapsed);
}

#[test]
fn run_reports_a_script_exit_code_as_6_with_its_status() {
    let Some(sb) = sandbox("run_reports_a_script_exit_code_as_6_with_its_status") else {
        return;
    };
    let script = sb.script("exit3.ts", "process.exit(3);\n");

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(6), "stderr: {}", out.stderr);
    assert!(anchored(&out.stdout).trailer().contains("exit 3"));
}

#[cfg(unix)]
#[test]
fn run_reports_a_signal_kill_as_6_with_the_signal() {
    let Some(sb) = sandbox("run_reports_a_signal_kill_as_6_with_the_signal") else {
        return;
    };
    let script = sb.script("kill.ts", "process.kill(process.pid, 'SIGKILL');\n");

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(6), "stderr: {}", out.stderr);
    assert!(anchored(&out.stdout).trailer().contains("signal 9"));
}

#[test]
fn run_fails_on_a_missing_script_file_with_one_classified_line() {
    let Some(sb) = sandbox("run_fails_on_a_missing_script_file_with_one_classified_line") else {
        return;
    };
    let missing = sb.root.join("does-not-exist.ts");

    let out = sb.run_script(&missing, &[]);

    assert_matches!(
        out.code,
        Some(1 | 2),
        "expected a runner failure code, got {:?}",
        out.code
    );
    assert_eq!(out.stderr_lines().len(), 1, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("run started"),
        "node must not have started"
    );
}

#[test]
fn run_resolves_mcp_server_imports_from_an_unrelated_cwd() {
    let Some(sb) = sandbox("run_resolves_mcp_server_imports_from_an_unrelated_cwd") else {
        return;
    };
    sb.generate();
    let script = sb.script(
        "import.ts",
        "import * as s from 'mcp:fixture';\nconsole.log('TYPE=' + typeof s.echo);\n",
    );
    let elsewhere = tempfile::tempdir().unwrap();
    let servers = sb.servers.display().to_string();
    let script_arg = script.display().to_string();

    let out = common::run_with_watchdog(
        sb.command()
            .args(["run", &script_arg, "--servers-dir", &servers])
            .current_dir(elsewhere.path()),
        None,
        common::WATCHDOG,
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("TYPE=function"), "{}", out.stdout);
}

#[test]
fn run_resolves_the_mcp_helpers_module() {
    let Some(sb) = sandbox("run_resolves_the_mcp_helpers_module") else {
        return;
    };
    let script = sb.script(
        "helpers.ts",
        "import * as h from 'mcp:helpers';\n\
         const fns = Object.values(h).filter((v) => typeof v === 'function').length;\n\
         console.log('HELPER_FUNCTIONS=' + fns);\n",
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let count: usize = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("HELPER_FUNCTIONS="))
        .expect("script printed the helper count")
        .parse()
        .unwrap();
    assert!(
        count >= 2,
        "pagination and bounded-parallelism helpers expected"
    );
}

#[test]
fn run_script_stdin_reads_eof_and_istty_does_not_throw() {
    let Some(sb) = sandbox("run_script_stdin_reads_eof_and_istty_does_not_throw") else {
        return;
    };
    let script = sb.script(
        "stdin.ts",
        "console.log('ISTTY=' + String(process.stdin.isTTY));\n\
         let bytes = 0;\n\
         for await (const chunk of process.stdin) bytes += chunk.length;\n\
         console.log('EOF bytes=' + bytes);\n",
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("EOF bytes=0"), "{}", out.stdout);
    assert!(out.stdout.contains("ISTTY="), "{}", out.stdout);
    assert!(out.elapsed < PROMPT, "took {:?}", out.elapsed);
}

#[test]
fn run_clears_the_helper_variables_for_the_script_and_its_children() {
    let Some(sb) = sandbox("run_clears_the_helper_variables_for_the_script_and_its_children")
    else {
        return;
    };
    let script = sb.script(
        "env.ts",
        "import { execFileSync } from 'node:child_process';\n\
         const own = Object.keys(process.env).filter((k) => k.startsWith('MCP_EXECUTION_HELPER'));\n\
         const child = JSON.parse(execFileSync(process.execPath, ['-e', \
           'console.log(JSON.stringify(Object.keys(process.env).filter((k) => k.startsWith(\"MCP_EXECUTION_HELPER\"))))'], \
           { stdio: ['ignore', 'pipe', 'inherit'] }).toString());\n\
         console.log('OWN=' + JSON.stringify(own));\n\
         console.log('CHILD=' + JSON.stringify(child));\n",
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("OWN=[]"), "{}", out.stdout);
    assert!(out.stdout.contains("CHILD=[]"), "{}", out.stdout);
}

#[test]
fn run_script_without_calls_exits_long_before_the_timeout() {
    let Some(sb) = sandbox("run_script_without_calls_exits_long_before_the_timeout") else {
        return;
    };
    let script = sb.script("quiet.ts", "console.log('done');\n");

    let out = sb.run_script(&script, &["--timeout", "60"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.elapsed < PROMPT,
        "an idle helper channel kept node alive: {:?}",
        out.elapsed
    );
}

#[test]
fn run_script_with_one_call_exits_long_before_the_timeout() {
    let Some(sb) = sandbox("run_script_with_one_call_exits_long_before_the_timeout") else {
        return;
    };
    sb.generate();
    let script = sb.script(
        "one-call.ts",
        "import { echo } from 'mcp:fixture';\n\
         console.log('RESULT=' + JSON.stringify(await echo({ a: 1 })));\n",
    );

    let out = sb.run_script(&script, &["--timeout", "60"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains(r#"RESULT={"a":1}"#), "{}", out.stdout);
    assert!(
        out.elapsed < PROMPT,
        "an idle helper channel kept node alive: {:?}",
        out.elapsed
    );
    let log = sb.fixture_log();
    assert_eq!(
        log.iter().filter(|l| l.as_str() == "call echo").count(),
        1,
        "log: {log:?}"
    );
}

#[test]
fn run_ends_within_timeout_and_grace_when_a_grandchild_holds_stdout() {
    let Some(sb) = sandbox("run_ends_within_timeout_and_grace_when_a_grandchild_holds_stdout")
    else {
        return;
    };
    let script = sb.script(
        "grandchild.ts",
        "import { spawn } from 'node:child_process';\n\
         const child = spawn(process.execPath, ['-e', 'setTimeout(() => process.exit(0), 20000)'], \
           { detached: true, stdio: ['ignore', 'inherit', 'inherit'] });\n\
         child.unref();\n\
         console.log('parent done');\n",
    );

    let out = sb.run_script(&script, &["--timeout", "5"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.elapsed < Duration::from_secs(15),
        "took {:?}",
        out.elapsed
    );
    let run = anchored(&out.stdout);
    assert!(out.stdout.contains("parent done"));
    let with_token = run.lines.iter().filter(|l| l.contains(run.token)).count();
    // On Windows the job object kills the detached grandchild after the script exits, so the drain
    // sees EOF and no abandon-drain notice is printed; on Unix the grandchild leaves the group.
    let expected = if cfg!(windows) { 2 } else { 3 };
    assert!(
        with_token >= expected,
        "anchor, the trailer and (Unix) an abandon-drain notice must carry the token: {:?}",
        run.lines
    );
}

#[test]
fn run_format_json_emits_one_envelope() {
    let Some(sb) = sandbox("run_format_json_emits_one_envelope") else {
        return;
    };
    let script = sb.script("json.ts", "console.log('enveloped');\n");
    let script_arg = script.display().to_string();
    let servers = sb.servers.display().to_string();

    let out = sb.cli(&[
        "--format",
        "json",
        "run",
        &script_arg,
        "--servers-dir",
        &servers,
    ]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let envelope: Value = serde_json::from_str(out.stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not one JSON document ({e}): {:?}", out.stdout));
    assert!(envelope.is_object());
    assert_eq!(envelope["script_status"], "exit 0");
    assert!(envelope.to_string().contains("enveloped"));
}

#[cfg(unix)]
#[test]
fn run_leaves_no_server_process_after_the_script_ends_with_a_call_in_flight() {
    let Some(sb) =
        sandbox("run_leaves_no_server_process_after_the_script_ends_with_a_call_in_flight")
    else {
        return;
    };
    sb.generate();
    let script = sb.script(
        "inflight.ts",
        "import { slow } from 'mcp:fixture';\n\
         slow({ ms: 30000 }).catch(() => {});\n\
         await new Promise((resolve) => setTimeout(resolve, 1500));\n\
         process.exit(0);\n",
    );

    let out = sb.run_script(&script, &["--timeout", "30"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        sb.fixture_log().iter().any(|l| l == "call slow"),
        "the call must have reached the server: {:?}",
        sb.fixture_log()
    );
    assert_eq!(
        common::wait_no_processes(&sb.marker(), Duration::from_secs(10)),
        0,
        "a fixture server process outlived the run"
    );
}
