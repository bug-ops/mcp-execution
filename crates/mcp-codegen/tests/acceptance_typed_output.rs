//! Acceptance tests for typed parameters and results: #565 (lossless types), #584 (raw keys),
//! #482 (result types and structured content) and #587 (observed result shapes), observed
//! through `generate`, `tsc` and a scriptable fake MCP server.
//!
//! The Node-based tests skip locally, and hard-fail in CI, when the toolchain is unavailable.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod acceptance_support;
mod support;

use acceptance_support::{
    REPLY_ECHO_ARGUMENTS, SERVER_ID, canonical_tempdir, fake_helper_js, file, generate,
    generate_with, run_node, server_info, tool, write_home, write_package,
};
use mcp_execution_codegen::package::BridgeContext;
use mcp_execution_codegen::{PackageGenerator, TemplateEngine};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use support::{
    compile_and_run_bridge_harness, install_declared_dev_dependencies, require_node_strip_types,
    require_ts_toolchain, tsc_program,
};

const OBSERVED_MARKER: &str = "observed in a past call";
const DEGRADED_DECLARED_RESULT: &str =
    "Record<string, unknown> | unknown[] | string | number | boolean | null | undefined";

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/shared_vectors.json")).expect("vectors parse")
}

fn rich_server() -> mcp_execution_introspector::ServerInfo {
    server_info(vec![
        tool(
            "list_items",
            json!({
                "type": "object",
                "properties": {
                    "state": {"type": "string", "enum": ["open", "closed", "all"]},
                    "per-page": {"type": "integer"},
                    "filter.name": {"type": "string"},
                    "1st": {"type": "string"},
                    "a-b": {"type": "string"},
                    "a.b": {"type": "number"},
                    "note": {"type": ["string", "null"]},
                    "mode": {"oneOf": [{"type": "string"}, {"type": "number"}]}
                },
                "required": ["state"]
            }),
            Some(json!({
                "type": "object",
                "properties": {"items": {"type": "array", "items": {"type": "string"}}},
                "required": ["items"]
            })),
        ),
        tool(
            "count_items",
            json!({"type": "object"}),
            Some(json!({"type": "number"})),
        ),
        tool(
            "list_ids",
            json!({"type": "object"}),
            Some(json!({"type": "array", "items": {"type": "string"}})),
        ),
    ])
}

fn result_declaration<'a>(tool_file: &'a str, typescript_name: &str) -> &'a str {
    let start = tool_file
        .find(&format!("export type {typescript_name}Result ="))
        .unwrap_or_else(|| panic!("no Result type in:\n{tool_file}"));
    let rest = &tool_file[start..];
    let end = rest
        .find("| undefined;")
        .unwrap_or_else(|| panic!("Result must end with `| undefined`:\n{rest}"));
    assert!(!rest[..end].contains("export function"), "{rest}");
    &rest[..end]
}

#[test]
fn generated_params_use_literal_unions_nullability_and_raw_quoted_keys() {
    let code = generate(&rich_server());
    let list_items = file(&code, "listItems.ts");

    for expected in [
        "state: 'open' | 'closed' | 'all'",
        "'per-page'?:",
        "'filter.name'?:",
        "'1st'?:",
        "'a-b'?: string",
        "'a.b'?: number",
        "note?: string | null",
        "mode?: string | number",
    ] {
        assert!(
            list_items.contains(expected),
            "missing `{expected}` in:\n{list_items}"
        );
    }
    for renamed in ["per_page", "filter_name", "a_b", "_2"] {
        assert!(
            !list_items.contains(renamed),
            "found sanitized `{renamed}` in:\n{list_items}"
        );
    }
}

#[test]
fn generated_sidecar_parameters_hold_the_raw_schema_keys() {
    let code = generate(&rich_server());
    let meta: Value = serde_json::from_str(file(&code, "_meta.json")).expect("sidecar");

    let names: Vec<&str> = meta["tools"][0]["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();

    for raw in ["per-page", "filter.name", "1st", "a-b", "a.b"] {
        assert!(names.contains(&raw), "{raw} missing from {names:?}");
    }
}

#[test]
fn generated_result_types_end_in_undefined_for_object_array_and_primitive_schemas() {
    let code = generate(&rich_server());

    let object = result_declaration(file(&code, "listItems.ts"), "listItems");
    assert!(object.contains("items: string[]"), "{object}");
    assert!(
        result_declaration(file(&code, "countItems.ts"), "countItems").contains("= number"),
        "a primitive output schema becomes a primitive Result"
    );
    assert!(
        result_declaration(file(&code, "listIds.ts"), "listIds").contains("string[]"),
        "an array output schema becomes an array Result"
    );
}

#[test]
fn generated_unrepresentable_output_schema_widens_the_generic_union_for_a_declared_schema() {
    let server = server_info(vec![tool("odd", json!({"type": "object"}), Some(json!(7)))]);

    let odd = file(&generate(&server), "odd.ts").to_string();

    assert!(
        odd.contains(&format!("Result = {DEGRADED_DECLARED_RESULT};")),
        "{odd}"
    );
    assert!(
        odd.contains("'structured')"),
        "the result source stays structured for a declared schema: {odd}"
    );
}

#[test]
fn generated_package_with_rich_schemas_passes_tsc() {
    let test_name = "generated_package_with_rich_schemas_passes_tsc";
    let code = generate(&rich_server());
    assert!(
        file(&code, "listItems.ts").contains("'open' | 'closed' | 'all'"),
        "the rich types must actually be rendered before they are type-checked"
    );
    if !require_ts_toolchain(test_name, true) {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    write_package(&code, dir.path());
    if !install_declared_dev_dependencies(dir.path(), test_name) {
        return;
    }
    let output = Command::new(tsc_program())
        .arg("--noEmit")
        .arg("-p")
        .arg(dir.path().join("tsconfig.json"))
        .output()
        .expect("run tsc");

    assert!(
        output.status.success(),
        "tsc failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn install(
    root: &Path,
    server: &mcp_execution_introspector::ServerInfo,
    reply: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let pkg = root.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    write_package(&generate(server), &pkg);
    (pkg, write_home(root, &fake_helper_js(reply)))
}

#[test]
fn run_tool_sends_schema_keys_to_the_server_unchanged() {
    if !require_node_strip_types("run_tool_sends_schema_keys_to_the_server_unchanged") {
        return;
    }
    let code = generate(&rich_server());
    assert!(
        file(&code, "listItems.ts").contains("'per-page'?:"),
        "typed with the raw key"
    );
    let root = canonical_tempdir();
    let (pkg, home) = install(root.path(), &rich_server(), REPLY_ECHO_ARGUMENTS);

    let run = run_node(
        &pkg.join("listItems.ts"),
        root.path(),
        &home,
        Some(r#"{"state":"open","per-page":5,"filter.name":"x"}"#),
        &[],
    );

    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(run.stdout.trim()).unwrap(),
        json!({"state": "open", "per-page": 5, "filter.name": "x"})
    );
}

fn typed_and_plain_server() -> mcp_execution_introspector::ServerInfo {
    server_info(vec![
        tool(
            "typed_tool",
            json!({"type": "object"}),
            Some(json!({"type": "object", "properties": {"from": {"type": "string"}}})),
        ),
        tool("plain_tool", json!({"type": "object"}), None),
    ])
}

const REPLY_BOTH_CONTENT_AND_STRUCTURED: &str = "(m) => ({ result: { content: [{ type: 'text', text: '{\"from\":\"text\"}' }], structuredContent: { from: 'structured' } } })";

#[test]
fn run_tool_prefers_structured_content_only_for_a_declared_output_schema() {
    if !require_node_strip_types(
        "run_tool_prefers_structured_content_only_for_a_declared_output_schema",
    ) {
        return;
    }
    let root = canonical_tempdir();
    let (pkg, home) = install(
        root.path(),
        &typed_and_plain_server(),
        REPLY_BOTH_CONTENT_AND_STRUCTURED,
    );

    let typed = run_node(
        &pkg.join("typedTool.ts"),
        root.path(),
        &home,
        Some("{}"),
        &[],
    );
    let plain = run_node(
        &pkg.join("plainTool.ts"),
        root.path(),
        &home,
        Some("{}"),
        &[],
    );

    assert_eq!(
        typed.stdout.trim(),
        r#"{"from":"structured"}"#,
        "stderr: {}",
        typed.stderr
    );
    assert_eq!(
        plain.stdout.trim(),
        r#"{"from":"text"}"#,
        "stderr: {}",
        plain.stderr
    );
}

#[test]
fn run_tool_empty_success_resolves_to_nothing_and_exits_zero() {
    if !require_node_strip_types("run_tool_empty_success_resolves_to_nothing_and_exits_zero") {
        return;
    }
    let root = canonical_tempdir();
    let (pkg, home) = install(
        root.path(),
        &typed_and_plain_server(),
        "(m) => ({ result: { content: [] } })",
    );

    let run = run_node(
        &pkg.join("plainTool.ts"),
        root.path(),
        &home,
        Some("{}"),
        &[],
    );

    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert!(run.stdout.is_empty(), "stdout: {:?}", run.stdout);
    assert!(run.stderr.is_empty(), "stderr: {:?}", run.stderr);
}

fn bridge_ts_with(configure: impl FnOnce(&mut BridgeContext)) -> String {
    let mut context = BridgeContext::default();
    configure(&mut context);
    TemplateEngine::new()
        .expect("template engine")
        .render("package/runtime-bridge", &context)
        .expect("bridge renders")
}

fn default_bridge_ts() -> String {
    bridge_ts_with(|_| {})
}

fn last_line(stdout: &str) -> &str {
    stdout
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .unwrap_or_else(|| panic!("no output"))
}

const ERROR_HARNESS_TS: &str = r"
import { callMCPTool, McpToolError } from './mcp-bridge.js';
callMCPTool('fake', 'noop', {}).then(
  () => { console.log('UNEXPECTED_SUCCESS'); process.exit(1); },
  (e: any) => {
    console.log(JSON.stringify({
      isMcpToolError: e instanceof McpToolError,
      kind: e.kind,
      message: e.message,
      serverMessage: e.serverMessage,
      truncated: e.serverMessageTruncated,
    }));
    process.exit(0);
  }
);
";

fn run_error_harness(test_name: &str, bridge_ts: &str, reply: &str) -> Option<Value> {
    let (success, stdout, stderr) = compile_and_run_bridge_harness(
        test_name,
        bridge_ts,
        &fake_helper_js(reply),
        ERROR_HARNESS_TS,
        &[],
    )?;
    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    Some(serde_json::from_str(last_line(&stdout)).expect("harness prints JSON"))
}

#[test]
fn bridge_tool_error_is_an_mcp_tool_error_with_server_text_only_in_server_message() {
    let name = "bridge_tool_error_is_an_mcp_tool_error_with_server_text_only_in_server_message";
    let reply =
        "(m) => ({ result: { isError: true, content: [{ type: 'text', text: 'SERVERTEXT' }] } })";
    let Some(seen) = run_error_harness(name, &default_bridge_ts(), reply) else {
        return;
    };

    assert_eq!(seen["isMcpToolError"], true, "{seen}");
    assert_eq!(seen["kind"], "tool_error");
    assert_eq!(seen["serverMessage"], "SERVERTEXT");
    assert_eq!(seen["truncated"], false);
    assert!(
        !seen["message"].as_str().unwrap().contains("SERVERTEXT"),
        "{seen}"
    );
}

#[test]
fn bridge_rpc_error_is_an_mcp_tool_error_with_kind_rpc_error() {
    let name = "bridge_rpc_error_is_an_mcp_tool_error_with_kind_rpc_error";
    let reply = "(m) => ({ error: { code: -32000, message: 'RPCTEXT' } })";
    let Some(seen) = run_error_harness(name, &default_bridge_ts(), reply) else {
        return;
    };

    assert_eq!(seen["isMcpToolError"], true, "{seen}");
    assert_eq!(seen["kind"], "rpc_error");
    assert_eq!(seen["serverMessage"], "RPCTEXT");
    assert!(
        !seen["message"].as_str().unwrap().contains("RPCTEXT"),
        "{seen}"
    );
}

fn run_loop_harness(
    test_name: &str,
    bridge_ts: &str,
    reply: &str,
    count: usize,
    body: &str,
    env: &[(&str, &str)],
) -> Option<String> {
    let harness = format!(
        "import {{ callMCPTool }} from './mcp-bridge.js';\n\
         const out: unknown[] = [];\n\
         for (let i = 0; i < {count}; i++) {{\n  {body}\n}}\n\
         console.log(JSON.stringify(out));\n\
         process.exit(0);\n"
    );
    let (success, stdout, stderr) = compile_and_run_bridge_harness(
        test_name,
        bridge_ts,
        &fake_helper_js(reply),
        &harness,
        env,
    )?;
    assert!(success, "stdout: {stdout}\nstderr: {stderr}");
    Some(last_line(&stdout).to_string())
}

fn is_recordable_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && key.chars().count() <= 32
}

fn observed_line(tool_name: &str, shape: &Value) -> String {
    json!({"server": SERVER_ID, "tool": tool_name, "shape": shape}).to_string()
}

fn properties_shape(keys: &[&str]) -> Value {
    let properties: serde_json::Map<String, Value> = keys
        .iter()
        .map(|k| ((*k).to_string(), json!({"type": "string"})))
        .collect();
    json!({"type": "object", "properties": properties})
}

fn undeclared_tool_file(jsonl: &str) -> String {
    let server = server_info(vec![tool("noop", json!({"type": "object"}), None)]);
    let generator = PackageGenerator::new()
        .unwrap()
        .with_observed_shapes(jsonl);
    file(&generate_with(&generator, &server), "noop.ts").to_string()
}

#[test]
fn observe_shapes_records_keys_and_types_never_values_and_only_when_enabled() {
    if !require_node_strip_types(
        "observe_shapes_records_keys_and_types_never_values_and_only_when_enabled",
    ) {
        return;
    }
    let reply = "(m) => ({ result: { content: [{ type: 'text', text: JSON.stringify({ id: 123, name: 'secretvalue', tags: ['a'], nested: { ok: true } }) }] } })";
    let root = canonical_tempdir();
    let (pkg, home) = install(root.path(), &typed_and_plain_server(), reply);
    let observed = root.path().join("observed.jsonl");
    let observed_arg = observed.to_string_lossy().into_owned();

    let first = run_node(
        &pkg.join("plainTool.ts"),
        root.path(),
        &home,
        Some("{}"),
        &[("MCPBRIDGE_OBSERVE_SHAPES", &observed_arg)],
    );
    assert_eq!(first.code, Some(0), "stderr: {}", first.stderr);
    let recorded = std::fs::read_to_string(&observed).expect("a line was appended");
    let line: Value =
        serde_json::from_str(acceptance_support::Run::single_line(&recorded)).unwrap();

    assert_eq!(line["server"], SERVER_ID);
    assert_eq!(line["tool"], "plain_tool");
    assert_eq!(line["shape"]["type"], "object");
    let mut keys: Vec<&str> = line["shape"]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["id", "name", "nested", "tags"]);
    assert!(
        !recorded.contains("secretvalue") && !recorded.contains("123"),
        "{recorded}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&observed).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let second = run_node(
        &pkg.join("plainTool.ts"),
        root.path(),
        &home,
        Some("{}"),
        &[],
    );
    assert_eq!(second.code, Some(0), "stderr: {}", second.stderr);
    assert_eq!(
        std::fs::read_to_string(&observed).unwrap(),
        recorded,
        "nothing is recorded when the variable is unset"
    );
}

#[test]
fn observed_shape_hint_renders_only_for_tools_without_a_declared_output_schema() {
    let server = typed_and_plain_server();
    let jsonl = [
        observed_line("plain_tool", &properties_shape(&["id", "name"])),
        observed_line("typed_tool", &properties_shape(&["leaked"])),
    ]
    .join("\n");
    let generator = PackageGenerator::new()
        .unwrap()
        .with_observed_shapes(&jsonl);

    let code = generate_with(&generator, &server);
    let plain = file(&code, "plainTool.ts");
    let typed = file(&code, "typedTool.ts");

    assert!(plain.contains(OBSERVED_MARKER), "{plain}");
    assert!(plain.contains("id") && plain.contains("name"));
    assert!(plain.contains("Record<string, unknown> | unknown[] | string | undefined"));
    assert!(
        !typed.contains(OBSERVED_MARKER) && !typed.contains("leaked"),
        "{typed}"
    );
}

#[test]
fn observed_shapes_skip_hostile_deep_oversized_foreign_and_keyword_violating_lines() {
    let control = observed_line("noop", &properties_shape(&["fine"]));
    assert!(
        undeclared_tool_file(&control).contains(OBSERVED_MARKER),
        "a well-formed line must produce the hint"
    );

    let mut deep = json!({"type": "string"});
    for _ in 0..8 {
        deep = json!({"type": "object", "properties": {"a": deep}});
    }
    let many: Vec<String> = (0..17).map(|i| format!("k{i:02}")).collect();
    let many_refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let oversized_keys: Vec<String> = (0..16)
        .map(|i| format!("{i:02}{}", "k".repeat(30)))
        .collect();
    let oversized_refs: Vec<&str> = oversized_keys.iter().map(String::as_str).collect();
    let bad_lines = [
        (
            "hostile key",
            observed_line("noop", &properties_shape(&["x*/y"])),
        ),
        (
            "email key",
            observed_line("noop", &properties_shape(&["a@b.com"])),
        ),
        (
            "17 keys",
            observed_line("noop", &properties_shape(&many_refs)),
        ),
        ("too deep", observed_line("noop", &deep)),
        (
            "oversized",
            observed_line("noop", &properties_shape(&oversized_refs)) + &" ".repeat(9000),
        ),
        (
            "foreign server",
            json!({"server": "someone-else", "tool": "noop", "shape": properties_shape(&["fine"])})
                .to_string(),
        ),
        (
            "keyword outside the allowed set",
            observed_line(
                "noop",
                &json!({"type": "object", "oneOf": [{"type": "string"}]}),
            ),
        ),
        ("not json", "{not json".to_string()),
    ];

    for (reason, line) in &bad_lines {
        let tool_file = undeclared_tool_file(line);
        assert!(
            !tool_file.contains(OBSERVED_MARKER),
            "{reason}: line must be skipped:\n{tool_file}"
        );
        assert!(
            !tool_file.contains("x*/y") && !tool_file.contains("a@b.com"),
            "{reason}"
        );
    }

    let unaffected = undeclared_tool_file(&bad_lines[5].1);
    assert_eq!(
        unaffected,
        undeclared_tool_file(""),
        "a foreign line changes nothing"
    );
}

#[test]
fn observed_shapes_last_valid_line_wins_per_tool() {
    let jsonl = [
        observed_line("noop", &properties_shape(&["firstkey"])),
        observed_line("noop", &properties_shape(&["secondkey"])),
        "{not json".to_string(),
    ]
    .join("\n");

    let tool_file = undeclared_tool_file(&jsonl);

    assert!(tool_file.contains("secondkey"), "{tool_file}");
    assert!(!tool_file.contains("firstkey"), "{tool_file}");
}

#[test]
fn observed_shape_hint_is_a_comment_that_cannot_close_early() {
    let shape =
        json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]});

    let tool_file = undeclared_tool_file(&observed_line("noop", &shape));

    let hint_start = tool_file.find(OBSERVED_MARKER).expect("hint rendered");
    let comment_end = tool_file[hint_start..]
        .find("*/")
        .expect("the comment closes");
    assert!(
        !tool_file[hint_start..hint_start + comment_end].contains("export "),
        "{tool_file}"
    );
}

fn key_rule_cases(vectors: &Value) -> Vec<(String, Vec<String>, bool)> {
    vectors["observed_key_rule"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_string(),
                c["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| k.as_str().unwrap().to_string())
                    .collect(),
                c["recorded"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[test]
fn observed_reader_applies_the_key_rule_of_the_shared_vectors() {
    for (label, keys, recorded) in key_rule_cases(&vectors()) {
        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let tool_file = undeclared_tool_file(&observed_line("noop", &properties_shape(&key_refs)));

        assert_eq!(
            tool_file.contains(OBSERVED_MARKER),
            recorded,
            "reader key rule disagrees with the vectors for {label}"
        );
        if recorded {
            assert!(
                key_refs.iter().all(|k| tool_file.contains(k)),
                "{label}: {tool_file}"
            );
        }
    }
}

#[test]
fn observed_recorder_applies_the_key_rule_of_the_shared_vectors_and_the_reader_accepts_its_lines() {
    let name = "observed_recorder_applies_the_key_rule_of_the_shared_vectors_and_the_reader_accepts_its_lines";
    if !require_node_strip_types(name) {
        return;
    }
    let cases = key_rule_cases(&vectors());
    let key_sets: Vec<&Vec<String>> = cases.iter().map(|(_, keys, _)| keys).collect();
    let reply = format!(
        "(m) => {{ const K = {}; const o = {{}}; for (const k of K[m.params.arguments.i]) o[k] = 'v'; return {{ result: {{ content: [{{ type: 'text', text: JSON.stringify(o) }}] }} }}; }}",
        serde_json::to_string(&key_sets).unwrap()
    );
    let root = canonical_tempdir();
    let observed = root.path().join("observed.jsonl");
    let observed_arg = observed.to_string_lossy().into_owned();

    let Some(_) = run_loop_harness(
        name,
        &default_bridge_ts(),
        &reply,
        cases.len(),
        "await callMCPTool('fake', 'noop', { i });",
        &[("MCPBRIDGE_OBSERVE_SHAPES", &observed_arg)],
    ) else {
        return;
    };

    let recorded = std::fs::read_to_string(&observed).expect("recorder wrote lines");
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(lines.len(), cases.len(), "one line per call: {recorded}");
    for ((label, keys, expect_keys), line) in cases.iter().zip(&lines) {
        let shape: Value = serde_json::from_str::<Value>(line).unwrap()["shape"].clone();

        if *expect_keys {
            let mut got: Vec<&str> = shape["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{label}: {line}"))
                .keys()
                .map(String::as_str)
                .collect();
            let mut want: Vec<&str> = keys.iter().map(String::as_str).collect();
            got.sort_unstable();
            want.sort_unstable();
            assert_eq!(got, want, "recorder keys for {label}");
        } else {
            assert!(
                shape.get("additionalProperties").is_some(),
                "{label}: {line}"
            );
            assert!(
                shape["properties"]
                    .as_object()
                    .is_none_or(serde_json::Map::is_empty),
                "{label}: {line}"
            );
            for key in keys
                .iter()
                .filter(|k| !k.is_empty() && !is_recordable_key(k))
            {
                assert!(
                    !line.contains(key.as_str()),
                    "{label}: key leaked into {line}"
                );
            }
        }

        assert!(
            undeclared_tool_file(line).contains(OBSERVED_MARKER),
            "the reader must accept the recorder's own output for {label}"
        );
    }
}

#[test]
fn recorder_key_bounds_are_rendered_from_the_bridge_context() {
    let name = "recorder_key_bounds_are_rendered_from_the_bridge_context";
    if !require_node_strip_types(name) {
        return;
    }
    let bridge = bridge_ts_with(|c| {
        c.observed_shape_max_keys = 2;
        c.observed_shape_max_key_len = 4;
    });
    let reply = "(m) => { const O = [{ ab: 1, cd: 2 }, { ab: 1, cd: 2, ef: 3 }, { abcde: 1 }]; return { result: { content: [{ type: 'text', text: JSON.stringify(O[m.params.arguments.i]) }] } }; }";
    let root = canonical_tempdir();
    let observed = root.path().join("observed.jsonl");
    let observed_arg = observed.to_string_lossy().into_owned();

    let Some(_) = run_loop_harness(
        name,
        &bridge,
        reply,
        3,
        "await callMCPTool('fake', 'noop', { i });",
        &[("MCPBRIDGE_OBSERVE_SHAPES", &observed_arg)],
    ) else {
        return;
    };

    let recorded = std::fs::read_to_string(&observed).expect("recorder wrote lines");
    let shapes: Vec<Value> = recorded
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["shape"].clone())
        .collect();
    assert!(
        shapes[0]["properties"].is_object(),
        "2 keys fit the lowered bound: {shapes:?}"
    );
    assert!(
        shapes[1].get("additionalProperties").is_some(),
        "3 keys exceed it: {shapes:?}"
    );
    assert!(
        shapes[2].get("additionalProperties").is_some(),
        "a 5-char key exceeds 4: {shapes:?}"
    );
}
