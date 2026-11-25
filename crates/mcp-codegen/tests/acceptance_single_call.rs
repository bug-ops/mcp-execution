//! Acceptance tests for the single-call path: #583 (entry detection), #570 (quiet output and
//! structured errors) and #567 (slim tool files that delegate to `_runtime/cli.ts`).
//!
//! The Node-based tests skip locally, and hard-fail in CI, when `node --experimental-strip-types`
//! is unavailable.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod acceptance_support;
mod support;

use acceptance_support::{
    REPLY_ECHO_ARGUMENTS, REPLY_TEXT_JSON_OK, Run, canonical_tempdir, file, generate, run_node,
    server_info, tool, write_home, write_package,
};
use mcp_execution_codegen::PackageGenerator;
use mcp_execution_codegen::package::ToolCategorization;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use support::{create_test_server_info, require_node_strip_types, test_config};

const TOOL_FILE: &str = "getThing.ts";

/// Average bytes of a tool file generated for `support::create_test_server_info()` on master
/// before the slimming (measured with the W0 tree, whose template equals master's).
const MASTER_AVG_TOOL_FILE_BYTES: usize = 2087;

fn thing_server() -> mcp_execution_introspector::ServerInfo {
    server_info(vec![tool(
        "get_thing",
        json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        None,
    )])
}

/// Generates the package into `pkg_dir` and a fake home answering with `reply`.
fn install(root: &Path, pkg_dir: &Path, reply: &str) -> PathBuf {
    std::fs::create_dir_all(pkg_dir).expect("create package dir");
    write_package(&generate(&thing_server()), pkg_dir);
    write_home(root, &acceptance_support::fake_helper_js(reply))
}

fn assert_tool_ran(run: &Run) {
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let printed: Value = serde_json::from_str(run.stdout.trim())
        .unwrap_or_else(|e| panic!("the tool printed no result ({e}): stdout {:?}", run.stdout));
    assert_eq!(printed, json!({"ok": true}));
}

#[test]
fn run_tool_from_a_directory_with_a_space_calls_the_tool() {
    if !require_node_strip_types("run_tool_from_a_directory_with_a_space_calls_the_tool") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("with space");
    let home = install(root.path(), &pkg, REPLY_TEXT_JSON_OK);

    assert_tool_ran(&run_node(
        &pkg.join(TOOL_FILE),
        root.path(),
        &home,
        Some("{}"),
        &[],
    ));
}

#[test]
fn run_tool_from_a_non_ascii_directory_calls_the_tool() {
    if !require_node_strip_types("run_tool_from_a_non_ascii_directory_calls_the_tool") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("h\u{e9}llo-\u{65e5}\u{672c}");
    let home = install(root.path(), &pkg, REPLY_TEXT_JSON_OK);

    assert_tool_ran(&run_node(
        &pkg.join(TOOL_FILE),
        root.path(),
        &home,
        Some("{}"),
        &[],
    ));
}

#[cfg(unix)]
#[test]
fn run_tool_through_a_symlinked_directory_calls_the_tool() {
    if !require_node_strip_types("run_tool_through_a_symlinked_directory_calls_the_tool") {
        return;
    }
    let root = canonical_tempdir();
    let real = root.path().join("real");
    let home = install(root.path(), &real, REPLY_TEXT_JSON_OK);
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("create symlink");

    assert_tool_ran(&run_node(
        &link.join(TOOL_FILE),
        root.path(),
        &home,
        Some("{}"),
        &[],
    ));
}

#[test]
fn run_tool_by_a_relative_path_calls_the_tool() {
    if !require_node_strip_types("run_tool_by_a_relative_path_calls_the_tool") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let home = install(root.path(), &pkg, REPLY_TEXT_JSON_OK);

    assert_tool_ran(&run_node(
        &Path::new("pkg").join(TOOL_FILE),
        root.path(),
        &home,
        Some("{}"),
        &[],
    ));
}

#[cfg(unix)]
#[test]
fn run_tool_under_tmp_which_may_be_a_symlink_calls_the_tool() {
    if !require_node_strip_types("run_tool_under_tmp_which_may_be_a_symlink_calls_the_tool") {
        return;
    }
    let root = tempfile::Builder::new()
        .prefix("mcp-acceptance-")
        .tempdir_in("/tmp")
        .expect("create a directory under /tmp");
    let pkg = root.path().join("pkg");
    let home = install(root.path(), &pkg, REPLY_TEXT_JSON_OK);

    assert_tool_ran(&run_node(
        &pkg.join(TOOL_FILE),
        root.path(),
        &home,
        Some("{}"),
        &[],
    ));
}

#[test]
fn run_tool_prints_one_compact_line_and_importing_it_runs_nothing() {
    if !require_node_strip_types("run_tool_prints_one_compact_line_and_importing_it_runs_nothing") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let marker = root.path().join("called.marker");
    let reply = format!(
        "(m) => {{ require('fs').writeFileSync({marker:?}, 'x'); return {{ result: {{ content: [{{ type: 'text', text: '{{\"ok\":true}}' }}] }} }}; }}",
        marker = marker.to_string_lossy()
    );
    let home = install(root.path(), &pkg, &reply);
    std::fs::write(
        pkg.join("importer.ts"),
        "import './getThing.ts';\nconsole.log('LOADED');\n",
    )
    .unwrap();

    let imported = run_node(&pkg.join("importer.ts"), root.path(), &home, None, &[]);
    assert_eq!(
        imported.stdout, "LOADED\n",
        "importing must print nothing of its own"
    );
    assert_eq!(imported.code, Some(0));
    assert!(
        !marker.exists(),
        "importing a tool file must not call the tool"
    );

    let direct = run_node(&pkg.join(TOOL_FILE), root.path(), &home, Some("{}"), &[]);
    assert!(marker.exists(), "running the file directly calls the tool");
    assert_eq!(
        direct.stdout, "{\"ok\":true}\n",
        "success is one compact line"
    );
}

#[test]
fn run_tool_with_a_result_over_128_kib_through_a_pipe_arrives_complete() {
    if !require_node_strip_types(
        "run_tool_with_a_result_over_128_kib_through_a_pipe_arrives_complete",
    ) {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let reply = "(m) => ({ result: { content: [{ type: 'text', text: JSON.stringify({ blob: 'x'.repeat(300000) }) }] } })";
    let home = install(root.path(), &pkg, reply);

    let run = run_node(&pkg.join(TOOL_FILE), root.path(), &home, Some("{}"), &[]);

    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let printed: Value = serde_json::from_str(Run::single_line(&run.stdout))
        .expect("the full result arrives as one JSON line");
    assert_eq!(printed["blob"].as_str().map(str::len), Some(300_000));
}

#[test]
fn run_tool_without_an_argument_sends_empty_arguments() {
    if !require_node_strip_types("run_tool_without_an_argument_sends_empty_arguments") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let home = install(root.path(), &pkg, REPLY_ECHO_ARGUMENTS);

    let run = run_node(&pkg.join(TOOL_FILE), root.path(), &home, None, &[]);

    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "{}\n");
}

#[test]
fn run_tool_tool_error_prints_one_structured_stderr_line_without_stack() {
    if !require_node_strip_types(
        "run_tool_tool_error_prints_one_structured_stderr_line_without_stack",
    ) {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let reply = "(m) => ({ result: { isError: true, content: [{ type: 'text', text: 'boom' }] } })";
    let home = install(root.path(), &pkg, reply);

    let run = run_node(&pkg.join(TOOL_FILE), root.path(), &home, Some("{}"), &[]);

    assert_eq!(run.code, Some(1));
    assert!(run.stdout.is_empty(), "stdout: {:?}", run.stdout);
    let line = run.stderr_json();
    assert_eq!(line["kind"], "tool_error");
    assert_eq!(line["server_message"], "boom");
    assert_eq!(line["server_message_truncated"], false);
    assert!(line.get("stack").is_none(), "{line}");
    assert!(
        !line["error"]
            .as_str()
            .expect("error is a string")
            .contains("boom"),
        "server text lives only in server_message: {line}"
    );
}

#[test]
fn run_tool_long_server_message_is_truncated_at_500_code_points_and_marked() {
    if !require_node_strip_types(
        "run_tool_long_server_message_is_truncated_at_500_code_points_and_marked",
    ) {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let reply = "(m) => ({ result: { isError: true, content: [{ type: 'text', text: 'a'.repeat(499) + '\\u{1F600}\\u{1F600}' + 'tail' }] } })";
    let home = install(root.path(), &pkg, reply);

    let run = run_node(&pkg.join(TOOL_FILE), root.path(), &home, Some("{}"), &[]);

    let line = run.stderr_json();
    let message = line["server_message"].as_str().expect("server_message");
    assert_eq!(message.chars().count(), 500, "{message:?}");
    assert_eq!(line["server_message_truncated"], true);
}

#[test]
fn run_tool_rpc_error_reports_kind_rpc_error_with_the_server_message() {
    if !require_node_strip_types(
        "run_tool_rpc_error_reports_kind_rpc_error_with_the_server_message",
    ) {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let home = install(
        root.path(),
        &pkg,
        "(m) => ({ error: { code: -32000, message: 'rpc boom' } })",
    );

    let run = run_node(&pkg.join(TOOL_FILE), root.path(), &home, Some("{}"), &[]);

    assert_eq!(run.code, Some(1));
    let line = run.stderr_json();
    assert_eq!(line["kind"], "rpc_error");
    assert_eq!(line["server_message"], "rpc boom");
}

#[test]
fn run_tool_invalid_arguments_are_reported_without_echoing_them() {
    if !require_node_strip_types("run_tool_invalid_arguments_are_reported_without_echoing_them") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let home = install(root.path(), &pkg, REPLY_TEXT_JSON_OK);

    for argument in ["{\"SECRETARG", "[\"SECRETARG\"]", "\"SECRETARG\"", "7"] {
        let run = run_node(
            &pkg.join(TOOL_FILE),
            root.path(),
            &home,
            Some(argument),
            &[],
        );

        assert_eq!(run.code, Some(1), "argument {argument}");
        assert!(
            !run.stderr.contains("SECRETARG"),
            "echoed {argument}: {}",
            run.stderr
        );
        let line = run.stderr_json();
        assert_eq!(line["kind"], "invalid_arguments", "argument {argument}");
        assert!(line.get("stack").is_none());
    }
}

#[test]
fn run_tool_debug_switch_adds_the_stack_to_the_last_stderr_line() {
    if !require_node_strip_types("run_tool_debug_switch_adds_the_stack_to_the_last_stderr_line") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let reply = "(m) => ({ result: { isError: true, content: [{ type: 'text', text: 'boom' }] } })";
    let home = install(root.path(), &pkg, reply);

    let run = run_node(
        &pkg.join(TOOL_FILE),
        root.path(),
        &home,
        Some("{}"),
        &[("MCPBRIDGE_DEBUG", "1")],
    );

    assert_eq!(run.code, Some(1));
    let last = run
        .stderr
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .expect("stderr line");
    let line: Value = serde_json::from_str(last).expect("the last line is the error JSON");
    assert!(line["stack"].is_string(), "{line}");
}

#[test]
fn run_if_main_reports_a_non_error_throw_as_call_failed() {
    if !require_node_strip_types("run_if_main_reports_a_non_error_throw_as_call_failed") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    let home = install(root.path(), &pkg, REPLY_TEXT_JSON_OK);
    std::fs::write(
        pkg.join("thrower.ts"),
        "import { runIfMain } from './_runtime/cli.ts';\n\
         runIfMain(import.meta.url, async () => { throw 'plain string'; });\n",
    )
    .unwrap();

    let run = run_node(&pkg.join("thrower.ts"), root.path(), &home, Some("{}"), &[]);

    assert_eq!(run.code, Some(1));
    assert_eq!(run.stderr_json()["kind"], "call_failed");
}

#[test]
fn tool_file_has_no_inline_cli_block_and_one_header() {
    let categorizations = HashMap::from([(
        "create_issue".to_string(),
        ToolCategorization::new(
            "issues".to_string(),
            vec!["create".into(), "issue".into(), "new".into(), "bug".into()],
            "Create a new issue".to_string(),
        ),
    )]);
    let code = PackageGenerator::new()
        .unwrap()
        .generate_with_categories(&create_test_server_info(), &test_config(), &categorizations)
        .unwrap();

    let tool_file = file(&code, "createIssue.ts");

    for forbidden in [
        "process.argv",
        "JSON.parse",
        "process.exit",
        "Narrow before accessing",
    ] {
        assert!(
            !tool_file.contains(forbidden),
            "{forbidden} found in:\n{tool_file}"
        );
    }
    assert_eq!(tool_file.matches("@tool ").count(), 1, "{tool_file}");
    assert_eq!(tool_file.matches("@keywords").count(), 1, "{tool_file}");
    assert!(
        tool_file.contains("runIfMain(import.meta.url, createIssue);"),
        "{tool_file}"
    );
}

#[test]
fn cli_module_is_identical_across_servers_and_static() {
    let first = generate(&server_info(vec![tool(
        "alpha",
        json!({"type": "object"}),
        None,
    )]));
    let mut other = server_info(vec![tool("beta", json!({"type": "object"}), None)]);
    other.name = "Another".to_string();
    let second = generate(&other);

    let cli = file(&first, "_runtime/cli.ts");

    assert_eq!(cli, file(&second, "_runtime/cli.ts"));
    assert!(cli.contains("export function runIfMain"));
    assert!(
        cli.contains("realpathSync"),
        "entry detection compares resolved paths"
    );
    assert!(
        !cli.contains("alpha") && !cli.contains("Fake"),
        "no server data in the module"
    );
}

#[test]
fn average_tool_file_is_at_most_half_of_the_master_baseline() {
    let code = PackageGenerator::new()
        .unwrap()
        .generate(&create_test_server_info(), &test_config())
        .unwrap();
    let tool_files: Vec<usize> = ["createIssue.ts", "updateIssue.ts", "getIssue.ts"]
        .iter()
        .map(|p| file(&code, p).len())
        .collect();
    let average = tool_files.iter().sum::<usize>() / tool_files.len();

    assert!(
        average * 2 <= MASTER_AVG_TOOL_FILE_BYTES,
        "average tool file is {average} B against the master baseline {MASTER_AVG_TOOL_FILE_BYTES} B"
    );
}
