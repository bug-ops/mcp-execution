//! Integration tests for progressive loading code generation.
//!
//! Tests the full pipeline from `ServerInfo` to generated TypeScript files
//! for progressive loading pattern. Tests that execute the generated runtime bridge live in
//! `runtime_bridge.rs`.

mod support;

use mcp_execution_codegen::progressive::ProgressiveGenerator;
use mcp_execution_core::{Error, ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::collections::HashMap;
use std::process::Command;
use support::{
    create_test_server_info, install_declared_dev_dependencies, require_node_strip_types,
    require_ts_toolchain, test_config, tsc_program,
};

#[test]
fn test_progressive_generator_creates_correct_number_of_files() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
/// was previously only exercised by unit tests calling the private wrapper helper directly,
/// never by a real failure driven through the public `generate_with_categories` entry point.
///
/// Of the three stages, `extract property schema` can't fail through this entry point:
/// `extract_properties` always yields well-typed `name`/`type` strings regardless of input.
/// `render tool template` and `track generated tool file` are both reachable, but only from a
/// structurally invalid `ServerInfo` that `mcp-introspector` itself can never produce (it always
/// builds `input_schema` as a JSON object, and bounds schema size far below what `track` needs
/// to trip) — i.e. only from a direct library caller that hand-builds a `ToolInfo`, not from a
/// real MCP server round-trip. This test exercises `render`: `TemplateEngine` runs in Handlebars
/// strict mode, and `tool.ts.hbs` does `{{#if input_schema.description}}` — a JSON *array*
/// `input_schema` fails that path navigation (only objects/scalars survive it), at effectively
/// no cost, unlike forcing the `track` stage's byte-count check.
#[test]
fn test_generate_with_categories_wraps_non_object_schema_as_script_generation_error() {
    let server_info = ServerInfo {
        id: ServerId::new("bulk-server").unwrap(),
        name: "Bulk Server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("malformed_tool").unwrap(),
            description: String::new(),
            input_schema: json!([1, 2, 3]),
            output_schema: None,
            annotations: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");

    let result = generator.generate_with_categories(&server_info, &test_config(), &HashMap::new());

    match result {
        Err(Error::ScriptGenerationError {
            tool,
            message,
            source,
        }) => {
            assert_eq!(tool, "malformed_tool");
            assert_eq!(message, "failed to render tool template");
            assert!(
                source.is_some(),
                "source must be preserved for exit-code classification"
            );
        }
        other => panic!("expected ScriptGenerationError wrapping a render failure, got {other:?}"),
    }
}

#[test]
fn test_progressive_tool_files_exist() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
fn test_progressive_tool_file_structure() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
            "export type createIssueResult = Record<string, unknown> | unknown[] | string;"
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
fn test_progressive_tool_file_has_proper_types() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
fn test_progressive_index_structure() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
fn test_progressive_index_doc_comment_not_prematurely_closed() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
fn test_progressive_index_uses_ts_specifiers_not_js() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
fn test_progressive_runtime_bridge_structure() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
fn test_progressive_generator_with_empty_server() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo {
        id: ServerId::new("empty").unwrap(),
        name: "Empty Server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

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
fn test_progressive_tool_camel_case_conversion() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo {
        id: ServerId::new("test").unwrap(),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("send_test_message").unwrap(),
            description: "Test tool".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
            output_schema: None,
            annotations: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

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
fn test_progressive_tool_with_complex_types() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo {
        id: ServerId::new("test").unwrap(),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("complex_tool").unwrap(),
            description: "Tool with complex types".to_string(),
            input_schema: json!({
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
            output_schema: None,
            annotations: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

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

    // Should handle object type
    assert!(
        content.contains("config?: Record<string, unknown>"),
        "Missing object type"
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

    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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

    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
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
