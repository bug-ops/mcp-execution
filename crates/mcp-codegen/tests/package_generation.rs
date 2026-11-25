//! Integration tests for progressive loading code generation.
//!
//! Tests the full pipeline from `ServerInfo` to generated TypeScript files
//! for progressive loading pattern. Tests that execute the generated runtime bridge live in
//! `runtime_bridge.rs`.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod support;

use mcp_execution_codegen::package::PackageGenerator;
use mcp_execution_core::{ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::collections::HashMap;
use std::process::Command;
use support::{
    create_test_server_info, install_declared_dev_dependencies, require_node_strip_types,
    require_ts_toolchain, test_config, tsc_program,
};

#[test]
fn test_package_generator_creates_correct_number_of_files() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    // Should generate:
    // - 3 tool files (createIssue.ts, updateIssue.ts, getIssue.ts)
    // - 1 index.ts
    // - 1 catalog.ts
    // - 1 runtime bridge (_runtime/mcp-bridge.ts)
    // - 1 single-call entry point (_runtime/cli.ts)
    // - 1 package.json
    // - 1 tsconfig.json
    // - 1 _meta.json
    assert_eq!(code.file_count(), 10);
}

/// Issue #458: the per-tool error wrapper in `emit_tool_files` (extract/render/track stages)
/// A non-object `input_schema` used to be the only cheap way to drive the `render tool
/// template` failure (#458) through the public entry point. The converter now degrades a
/// non-object root to generic parameters instead of failing, so no render or track failure is
/// reachable through `generate_with_categories` any more; the per-tool error wrapper keeps its
/// unit tests in `package/generator.rs`. This test pins the new behavior: generation
/// succeeds and the parameters are the generic record type.
#[test]
fn test_generate_with_categories_degrades_non_object_schema_to_generic_params() {
    let server_info = ServerInfo::new(
        ServerId::new("bulk-server").unwrap(),
        "Bulk Server".to_string(),
        "1.0.0".to_string(),
        vec![ToolInfo::new(
            ToolName::new("malformed_tool").unwrap(),
            String::new(),
            json!([1, 2, 3]),
        )],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );
    let generator = PackageGenerator::new().expect("Failed to create generator");

    let code = generator
        .generate_with_categories(&server_info, &test_config(), &HashMap::new())
        .expect("a non-object input schema degrades instead of failing");

    let tool_file = code
        .files
        .iter()
        .find(|f| f.path == "malformedTool.ts")
        .expect("malformedTool.ts not found");
    assert!(
        tool_file
            .content
            .contains("export type malformedToolParams = Record<string, unknown>;"),
        "{}",
        tool_file.content
    );
}

#[test]
fn test_package_tool_files_exist() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let file_paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();

    // Check tool files
    assert!(
        file_paths.contains(&"createIssue.ts"),
        "Missing createIssue.ts"
    );
    assert!(
        file_paths.contains(&"updateIssue.ts"),
        "Missing updateIssue.ts"
    );
    assert!(file_paths.contains(&"getIssue.ts"), "Missing getIssue.ts");

    // Check infrastructure files
    assert!(file_paths.contains(&"index.ts"), "Missing index.ts");
    assert!(
        file_paths.contains(&"_runtime/mcp-bridge.ts"),
        "Missing _runtime/mcp-bridge.ts"
    );
    assert!(
        file_paths.contains(&"tsconfig.json"),
        "Missing tsconfig.json"
    );
}

#[test]
fn test_package_tool_file_structure() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let create_issue_file = code
        .files
        .iter()
        .find(|f| f.path == "createIssue.ts")
        .expect("createIssue.ts not found");

    let content = &create_issue_file.content;

    // Should contain function export
    assert!(
        content.contains("export async function createIssue"),
        "Missing function export"
    );

    // Should contain parameter type alias (not an interface — interfaces are not
    // structurally assignable to Record<string, unknown>, which callMCPTool requires)
    assert!(
        content.contains("export type createIssueParams = {"),
        "Missing Params type alias"
    );

    // Should contain the widened Result union — an object-only shape would misdescribe what
    // callMCPTool can actually return (issue #182)
    assert!(
        content.contains(
            "export type createIssueResult = Record<string, unknown> | unknown[] | string | undefined;"
        ),
        "Missing widened Result type union"
    );

    // Should call callMCPTool
    assert!(content.contains("callMCPTool"), "Missing callMCPTool call");

    // Should cast callMCPTool's `unknown` return to the Result type — `unknown` is never
    // assignable to a concrete type without a cast, regardless of interface vs type alias
    assert!(
        content.contains(") as createIssueResult;"),
        "Missing cast of callMCPTool's return value to createIssueResult"
    );

    // Should include server_id and tool name
    assert!(
        content.contains("'github'"),
        "Missing server_id in callMCPTool"
    );
    assert!(
        content.contains("'create_issue'"),
        "Missing tool name in callMCPTool"
    );
}

#[test]
fn test_package_tool_file_has_proper_types() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let create_issue_file = code
        .files
        .iter()
        .find(|f| f.path == "createIssue.ts")
        .expect("createIssue.ts not found");

    let content = &create_issue_file.content;

    // Should have required fields without optional marker
    assert!(
        content.contains("repo: string;"),
        "Missing required repo field"
    );
    assert!(
        content.contains("title: string;"),
        "Missing required title field"
    );

    // Should have optional field with ? marker
    assert!(
        content.contains("body?: string;"),
        "Missing optional body field"
    );
}

#[test]
fn test_package_index_structure() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let index_file = code
        .files
        .iter()
        .find(|f| f.path == "index.ts")
        .expect("index.ts not found");

    let content = &index_file.content;

    // Should re-export all tools
    assert!(
        content.contains("export { createIssue"),
        "Missing createIssue export"
    );
    assert!(
        content.contains("export { updateIssue"),
        "Missing updateIssue export"
    );
    assert!(
        content.contains("export { getIssue"),
        "Missing getIssue export"
    );

    // Should export types
    assert!(
        content.contains("createIssueParams"),
        "Missing Params type export"
    );
    assert!(
        content.contains("createIssueResult"),
        "Missing Result type export"
    );

    // Should have tool count in documentation
    assert!(
        content.contains("3 tools"),
        "Missing tool count in documentation"
    );

    // Should re-export runtime bridge
    assert!(
        content.contains("export { callMCPTool }"),
        "Missing callMCPTool export"
    );
}

#[test]
fn test_package_index_doc_comment_not_prematurely_closed() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let index_file = code
        .files
        .iter()
        .find(|f| f.path == "index.ts")
        .expect("index.ts not found");

    let content = &index_file.content;

    let doc_start = content.find("/**").expect("Missing opening JSDoc block");
    let doc_end = content[doc_start..]
        .find("*/")
        .map(|i| doc_start + i)
        .expect("Missing JSDoc close");

    assert!(
        content[doc_start..doc_end].contains("@packageDocumentation"),
        "Top-level JSDoc block closed prematurely before @packageDocumentation \
         — likely a nested /* ... */ inside the doc comment (regression for #139)"
    );
}

/// Regression guard for #256: `index.ts`'s `export ... from './...'` specifiers must use `.ts`,
/// matching `tool.ts.hbs`'s import of the runtime bridge and the fact that generated files are
/// always written to disk as `.ts` (never compiled to `.js`). A `.js` specifier type-checks
/// under `tsc --noEmit` (which remaps it back to the sibling `.ts` file) but throws
/// `ERR_MODULE_NOT_FOUND` under Node's real ESM resolution — see
/// `test_generated_index_resolves_at_runtime_under_node_esm` for the full runtime
/// reproduction; this is the cheap, toolchain-free companion check.
#[test]
fn test_package_index_uses_ts_specifiers_not_js() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let index_file = code
        .files
        .iter()
        .find(|f| f.path == "index.ts")
        .expect("index.ts not found");

    let content = &index_file.content;

    assert!(
        content.contains("from './createIssue.ts';"),
        "tool re-export must use a .ts specifier: {content}"
    );
    assert!(
        content.contains("from './_runtime/mcp-bridge.ts';"),
        "runtime bridge re-export must use a .ts specifier: {content}"
    );
    assert!(
        !content.contains(".js';") && !content.contains(".js\";"),
        "index.ts must not import/export any sibling file with a .js specifier — the files on \
         disk are always .ts, and a .js specifier only resolves under tsc's type-checking, not \
         Node's real ESM resolution: {content}"
    );
}

#[test]
fn test_package_runtime_bridge_structure() {
    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let bridge_file = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let content = &bridge_file.content;

    // Should export callMCPTool function
    assert!(
        content.contains("export async function callMCPTool"),
        "Missing callMCPTool export"
    );

    // Should have proper function signature
    assert!(
        content.contains("serverId: string"),
        "Missing serverId parameter"
    );
    assert!(
        content.contains("toolName: string"),
        "Missing toolName parameter"
    );
    assert!(
        content.contains("params: Record<string, unknown>"),
        "Missing params parameter"
    );

    // Should have JSDoc documentation
    assert!(
        content.contains("@param serverId"),
        "Missing serverId JSDoc"
    );
    assert!(
        content.contains("@param toolName"),
        "Missing toolName JSDoc"
    );
    assert!(content.contains("@param params"), "Missing params JSDoc");
}

#[test]
fn test_package_generator_with_empty_server() {
    let generator = PackageGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo::new(
        ServerId::new("empty").unwrap(),
        "Empty Server".to_string(),
        "1.0.0".to_string(),
        vec![],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    // Should generate:
    // - 0 tool files
    // - 1 index.ts
    // - 1 catalog.ts
    // - 1 runtime bridge
    // - 1 single-call entry point (_runtime/cli.ts)
    // - 1 package.json
    // - 1 tsconfig.json
    // - 1 _meta.json
    assert_eq!(code.file_count(), 7);

    let file_paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();
    assert!(file_paths.contains(&"index.ts"));
    assert!(file_paths.contains(&"_runtime/mcp-bridge.ts"));
    assert!(file_paths.contains(&"package.json"));
    assert!(file_paths.contains(&"tsconfig.json"));
}

#[test]
fn test_package_tool_camel_case_conversion() {
    let generator = PackageGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        vec![ToolInfo::new(
            ToolName::new("send_test_message").unwrap(),
            "Test tool".to_string(),
            json!({
                "type": "object",
                "properties": {},
            }),
        )],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    // Should convert snake_case to camelCase for filename
    assert!(
        code.files
            .iter()
            .any(|f| f.path.as_str() == "sendTestMessage.ts")
    );

    let tool_file = code
        .files
        .iter()
        .find(|f| f.path == "sendTestMessage.ts")
        .expect("sendTestMessage.ts not found");

    // Should use camelCase in function name
    assert!(tool_file.content.contains("function sendTestMessage"));
}

#[test]
fn test_package_tool_with_complex_types() {
    let generator = PackageGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        vec![ToolInfo::new(
            ToolName::new("complex_tool").unwrap(),
            "Tool with complex types".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "config": {
                        "type": "object"
                    },
                    "count": {
                        "type": "number"
                    },
                    "enabled": {
                        "type": "boolean"
                    }
                },
                "required": ["items"]
            }),
        )],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let tool_file = code
        .files
        .iter()
        .find(|f| f.path == "complexTool.ts")
        .expect("complexTool.ts not found");

    let content = &tool_file.content;

    // Should handle array type
    assert!(content.contains("items: string[]"), "Missing array type");

    // Should handle an object without declared properties as an open object
    assert!(
        content.contains("config?: {\n    [key: string]: unknown;\n  };"),
        "Missing object type: {content}"
    );

    // Should handle number type
    assert!(content.contains("count?: number"), "Missing number type");

    // Should handle boolean type
    assert!(
        content.contains("enabled?: boolean"),
        "Missing boolean type"
    );
}

/// Regression guard for #176/#182/#183: the *actual* exported package — every file `generate`
/// produces, unmodified — must pass `tsc --noEmit` out of the box. Earlier versions of this
/// test substituted a 9-line stub for `_runtime/mcp-bridge.ts` and a hand-written
/// `globals.d.ts` declaring `process`, which meant it passed regardless of whether the real
/// ~900-line bridge (which imports `child_process`/`fs/promises`/`os`/`path` and references the
/// `NodeJS` namespace) actually type-checked — exactly the gap that let #183 regress even after
/// a tsconfig-only fix (missing `@types/node`) landed. This writes every `code.files` entry
/// verbatim, installs the generated `package.json`'s devDependencies for real via `npm
/// install`, and only then runs `tsc --noEmit` against the generated `tsconfig.json`.
///
/// Skips locally (hard-fails in CI) when `tsc`/`node`/`npm` are not on `PATH`, or when `npm
/// install` fails (e.g. no network) — see `require_ts_toolchain` and
/// `install_declared_dev_dependencies`.
#[test]
fn test_generated_tool_passes_tsc_noemit() {
    let test_name = "test_generated_tool_passes_tsc_noemit";
    if !require_ts_toolchain(test_name, true) {
        return;
    }

    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let dir = tempfile::tempdir().expect("Failed to create temp dir");

    // Write every generated file verbatim — real bridge, real package.json, real
    // tsconfig.json, real tool/index files — rather than substituting stand-ins for any of
    // them, so this test proves the actual exported package type-checks, not a stand-in of it.
    for file in &code.files {
        let path = dir.path().join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create parent dir for file");
        }
        std::fs::write(&path, &file.content).expect("Failed to write generated file");
    }

    if !install_declared_dev_dependencies(dir.path(), test_name) {
        return;
    }

    let output = Command::new(tsc_program())
        .arg("--noEmit")
        .arg("-p")
        .arg(dir.path().join("tsconfig.json"))
        .output()
        .expect("Failed to run tsc");

    assert!(
        output.status.success(),
        "tsc --noEmit failed on the real generated package:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A typed `additionalProperties` next to declared properties must render a `Params` type a
/// caller can actually construct: TypeScript rejects properties that do not match a typed index
/// signature, so the index value type has to cover them.
#[test]
fn test_typed_additional_properties_params_are_constructible_under_tsc() {
    let test_name = "test_typed_additional_properties_params_are_constructible_under_tsc";
    if !require_ts_toolchain(test_name, true) {
        return;
    }
    let server_info = ServerInfo::new(
        ServerId::new("typed-extra").unwrap(),
        "Typed Extra".to_string(),
        "1.0.0".to_string(),
        vec![ToolInfo::new(
            ToolName::new("tag_item").unwrap(),
            "Tags an item".to_string(),
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}, "label": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": {"type": "number"}
            }),
        )],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );
    let code = PackageGenerator::new()
        .unwrap()
        .generate(&server_info, &test_config())
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for file in &code.files {
        let path = dir.path().join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, &file.content).unwrap();
    }
    std::fs::write(
        dir.path().join("usage.ts"),
        "import type { tagItemParams } from './tagItem.ts';\n\
         export const params: tagItemParams = { name: 'x', label: 'y', score: 3 };\n",
    )
    .unwrap();
    if !install_declared_dev_dependencies(dir.path(), test_name) {
        return;
    }

    let output = Command::new(tsc_program())
        .arg("--noEmit")
        .arg("-p")
        .arg(dir.path().join("tsconfig.json"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "tsc failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A narrowing keyword at the root of a declared output schema must not throw the converted
/// type away in favor of the generic union.
#[test]
fn test_root_narrowing_keyword_keeps_the_declared_result_type() {
    let server_info = ServerInfo::new(
        ServerId::new("narrow").unwrap(),
        "Narrow".to_string(),
        "1.0.0".to_string(),
        vec![{
            let mut value = ToolInfo::new(
                ToolName::new("list_ids").unwrap(),
                String::new(),
                json!({"type": "object"}),
            );
            value.output_schema =
                Some(json!({"type": "array", "items": {"type": "string"}, "uniqueItems": true}));
            value
        }],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );
    let code = PackageGenerator::new()
        .unwrap()
        .generate(&server_info, &test_config())
        .unwrap();
    let tool_file = code.files.iter().find(|f| f.path == "listIds.ts").unwrap();

    assert!(
        tool_file
            .content
            .contains("export type listIdsResult = string[] | undefined;"),
        "{}",
        tool_file.content
    );
}

/// Regression guard for #256: `index.ts`'s relative import specifiers must resolve under
/// Node's actual ESM module resolution, not merely under `tsc --noEmit`. `tsc --noEmit` remaps
/// a `.js` specifier back to a sibling `.ts` file for type-checking purposes only; Node's real
/// resolver does not, so a `.js` specifier pointing at a file that only exists as `.ts` throws
/// `ERR_MODULE_NOT_FOUND` the moment `index.ts` is loaded.
///
/// Writes the full generated output to disk exactly as `mcp-execution-files` would, then loads
/// `index.ts` directly under Node's native type-stripping — the same execution path the
/// original bug was reported against — instead of compiling first: compiling with
/// `--rewriteRelativeImportExtensions` (required to emit `allowImportingTsExtensions` sources)
/// normalizes every specifier to `.js` regardless of what the source said, which would hide
/// this exact bug.
///
/// Skips locally (hard-fails in CI) when `node --experimental-strip-types` is unavailable.
#[test]
fn test_generated_index_resolves_at_runtime_under_node_esm() {
    if !require_node_strip_types("test_generated_index_resolves_at_runtime_under_node_esm") {
        return;
    }

    let generator = PackageGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info, &test_config())
        .expect("Failed to generate code");

    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    for file in &code.files {
        let path = dir.path().join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .expect("Failed to create parent dir for generated file");
        }
        std::fs::write(&path, &file.content).expect("Failed to write generated file");
    }

    let output = Command::new("node")
        .arg("--experimental-strip-types")
        .arg("index.ts")
        .current_dir(dir.path())
        .output()
        .expect("Failed to run node");

    assert!(
        output.status.success(),
        "node failed to load the generated index.ts — likely a `.js` import specifier \
         pointing at a file that only exists as `.ts` on disk (regression for #256):\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
