//! Integration tests for progressive loading code generation.
//!
//! Tests the full pipeline from `ServerInfo` to generated TypeScript files
//! for progressive loading pattern.

use mcp_execution_codegen::progressive::ProgressiveGenerator;
use mcp_execution_core::{ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::process::Command;

/// Creates a mock server info for testing.
fn create_test_server_info() -> ServerInfo {
    ServerInfo {
        id: ServerId::new("github"),
        name: "GitHub".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![
            ToolInfo {
                name: ToolName::new("create_issue"),
                description: "Creates a new issue".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string",
                            "description": "Repository name"
                        },
                        "title": {
                            "type": "string",
                            "description": "Issue title"
                        },
                        "body": {
                            "type": "string",
                            "description": "Issue body"
                        }
                    },
                    "required": ["repo", "title"]
                }),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("update_issue"),
                description: "Updates an existing issue".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string"
                        },
                        "issue_number": {
                            "type": "number"
                        },
                        "title": {
                            "type": "string"
                        }
                    },
                    "required": ["repo", "issue_number"]
                }),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("get_issue"),
                description: "Gets issue information".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string"
                        },
                        "issue_number": {
                            "type": "number"
                        }
                    },
                    "required": ["repo", "issue_number"]
                }),
                output_schema: None,
            },
        ],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

#[test]
fn test_progressive_generator_creates_correct_number_of_files() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    // Should generate:
    // - 3 tool files (createIssue.ts, updateIssue.ts, getIssue.ts)
    // - 1 index.ts
    // - 1 runtime bridge (_runtime/mcp-bridge.ts)
    // - 1 package.json
    // - 1 tsconfig.json
    // - 1 _meta.json
    assert_eq!(code.file_count(), 8);
}

#[test]
fn test_progressive_tool_files_exist() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
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
        .generate(&server_info)
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
        .generate(&server_info)
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
        .generate(&server_info)
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
        .generate(&server_info)
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
        .generate(&server_info)
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
        .generate(&server_info)
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
        id: ServerId::new("empty"),
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
        .generate(&server_info)
        .expect("Failed to generate code");

    // Should generate:
    // - 0 tool files
    // - 1 index.ts
    // - 1 runtime bridge
    // - 1 package.json
    // - 1 tsconfig.json
    // - 1 _meta.json
    assert_eq!(code.file_count(), 5);

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
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("send_test_message"),
            description: "Test tool".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let code = generator
        .generate(&server_info)
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
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("complex_tool"),
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
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let code = generator
        .generate(&server_info)
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

/// Name of the `tsc` executable to spawn directly via [`std::process::Command`].
///
/// `npm install -g typescript` installs `tsc` as a `.cmd`/`.ps1` shim script on Windows, not
/// a `.exe`. Windows' `CreateProcess` (which `Command` calls into directly, bypassing a
/// shell) only appends the `.exe` extension when none is given — it does not consult
/// `PATHEXT` the way `cmd.exe` does — so `Command::new("tsc")` silently fails to find it even
/// though `tsc` resolves fine when typed at a Windows shell prompt. Elsewhere, the real `tsc`
/// binary (or symlink to one) is on `PATH` unqualified.
const fn tsc_program() -> &'static str {
    if cfg!(windows) { "tsc.cmd" } else { "tsc" }
}

/// Ensures `tsc` (and, if `require_node`, `node`) are available on `PATH` before a test that
/// depends on the TypeScript/Node toolchain runs.
///
/// In CI (the `CI` env var is set — GitHub Actions sets it for every job) a missing
/// toolchain is a hard test failure rather than a silent skip: these tests exist
/// specifically to catch TypeScript-level regressions (e.g. #201's runtime-bridge
/// validation, #176's generated-wrapper type errors) that no Rust-only check can see, and a
/// CI runner missing Node/tsc would otherwise report green while running zero of that
/// coverage. Locally (no `CI` env var), a missing toolchain just skips the test with a
/// message, since not everyone running `cargo test` has Node installed.
///
/// Returns `true` if the required tools are present and the test should proceed.
fn require_ts_toolchain(test_name: &str, require_node: bool) -> bool {
    let tsc_missing = Command::new(tsc_program())
        .arg("--version")
        .output()
        .is_err();
    let node_missing = require_node && Command::new("node").arg("--version").output().is_err();

    if !tsc_missing && !node_missing {
        return true;
    }

    let missing = match (tsc_missing, node_missing) {
        (true, true) => "`tsc` and `node`",
        (true, false) => "`tsc`",
        (false, true) => "`node`",
        (false, false) => unreachable!("checked above"),
    };

    assert!(
        std::env::var_os("CI").is_none(),
        "{test_name}: {missing} not found on PATH in CI — this test exists to catch \
         TypeScript-level regressions and must not silently skip in CI. Install Node.js and \
         run `npm install -g typescript` in this CI job."
    );

    eprintln!("skipping {test_name}: {missing} not found on PATH");
    false
}

/// `npm install -g typescript` installs a global `tsc`, but not `npm` itself as `npm.cmd` on
/// Windows in a way `Command::new("npm")` would find — mirrors `tsc_program`'s reasoning.
const fn npm_program() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

/// Installs `dir`'s `package.json` `devDependencies` (currently just `@types/node`, added for
/// #183) via `npm install`, so `tsc --noEmit` can resolve the Node builtin modules and ambient
/// globals the real runtime bridge references. Without this, `tsc` would fail with `TS2307`/
/// `TS2580`/`TS2503` regardless of how correct the generated `tsconfig.json` is.
///
/// Same skip-locally/hard-fail-in-CI policy as [`require_ts_toolchain`]: this exists to catch
/// exactly the kind of regression a missing `@types/node` produces (see #183's critique), so
/// silently skipping in CI would defeat the point. A local `npm install` failure (e.g. no
/// network) skips rather than hard-failing, since CI is presumed to have registry access but a
/// local sandbox may not.
///
/// Passes `--include=dev` explicitly: with `NODE_ENV=production` set in the environment, `npm
/// install` silently omits `devDependencies` (exit success, nothing installed) instead of
/// failing, which would otherwise let a missing `@types/node` regression slip through with no
/// signal — `--include=dev` overrides that regardless of `NODE_ENV`.
///
/// Returns `true` if the install succeeded and the caller should proceed to `tsc`.
fn install_declared_dev_dependencies(dir: &std::path::Path, test_name: &str) -> bool {
    if Command::new(npm_program())
        .arg("--version")
        .output()
        .is_err()
    {
        assert!(
            std::env::var_os("CI").is_none(),
            "{test_name}: npm not found on PATH in CI — cannot install the generated \
             package.json's devDependencies, so this test cannot catch a missing @types/node \
             regression."
        );
        eprintln!("skipping {test_name}: npm not found on PATH");
        return false;
    }

    let output = Command::new(npm_program())
        .args([
            "install",
            "--no-save",
            "--no-audit",
            "--no-fund",
            "--include=dev",
        ])
        .current_dir(dir)
        .output()
        .expect("Failed to run npm install");

    if !output.status.success() {
        assert!(
            std::env::var_os("CI").is_none(),
            "{test_name}: npm install failed in CI:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!(
            "skipping {test_name}: npm install failed (no network access?):\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return false;
    }

    true
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
        .generate(&server_info)
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

/// Ensures `node --experimental-strip-types` is usable before a test that depends on it runs.
/// Same skip-locally/hard-fail-in-CI policy as [`require_ts_toolchain`].
fn require_node_strip_types(test_name: &str) -> bool {
    let supported = Command::new("node")
        .args(["--experimental-strip-types", "--eval", ""])
        .output()
        .is_ok_and(|output| output.status.success());

    if supported {
        return true;
    }

    assert!(
        std::env::var_os("CI").is_none(),
        "{test_name}: `node --experimental-strip-types` not available in CI (needs Node \
         22.6+) — this test exists to catch ERR_MODULE_NOT_FOUND regressions that `tsc \
         --noEmit` cannot see."
    );

    eprintln!("skipping {test_name}: `node --experimental-strip-types` not available");
    false
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
        .generate(&server_info)
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

/// Compiles `bridge_ts` (the rendered runtime bridge) alongside a harness that calls
/// `callMCPTool(server_id, "noop", {})`, runs it under Node with `$HOME`/`%USERPROFILE%`
/// pointed at a temp directory containing `mcp_json` as `~/.claude/mcp.json`, and returns
/// `(exit_success, stdout, stderr)`.
///
/// Uses `tsc --noCheck` (type-erasure only, no type-checking) so this doesn't need
/// `@types/node` installed to resolve the bridge's Node builtin imports (`child_process`,
/// `fs/promises`, `os`, `path`, `stream`).
///
/// A hostile/unvalidated config that reaches `spawn()` can hang forever here (the bridge
/// waits on a JSON-RPC response from a subprocess that will never send one) — that's
/// exactly the failure mode these tests exist to catch, so the harness itself races a 5s
/// watchdog against `callMCPTool`'s promise, and this helper additionally bounds the whole
/// Node run at 15s so a regression fails the test suite instead of hanging it.
///
/// Returns `None` if `tsc`/`node` are not on `PATH` and the caller should skip locally; hard
/// fails (via `require_ts_toolchain`) instead of returning `None` when running in CI.
fn run_bridge_harness(
    test_name: &str,
    bridge_ts: &str,
    mcp_json: &serde_json::Value,
    server_id: &str,
) -> Option<(bool, String, String)> {
    run_bridge_harness_with_env(test_name, bridge_ts, mcp_json, server_id, &[])
}

/// Same as [`run_bridge_harness`], additionally setting `extra_env` on the spawned Node
/// harness process (e.g. `MCPBRIDGE_REQUEST_TIMEOUT_MS` for timeout-specific tests).
fn run_bridge_harness_with_env(
    test_name: &str,
    bridge_ts: &str,
    mcp_json: &serde_json::Value,
    server_id: &str,
    extra_env: &[(&str, &str)],
) -> Option<(bool, String, String)> {
    let harness_ts = format!(
        "import {{ callMCPTool }} from './mcp-bridge.js';\n\
         \n\
         const watchdog = setTimeout(() => {{\n\
         \x20\x20console.log('TIMEOUT: did not settle before spawning');\n\
         \x20\x20process.exit(2);\n\
         }}, 5000);\n\
         \n\
         callMCPTool('{server_id}', 'noop', {{}}).then(\n\
         \x20\x20() => {{\n\
         \x20\x20\x20\x20clearTimeout(watchdog);\n\
         \x20\x20\x20\x20console.log('UNEXPECTED_SUCCESS');\n\
         \x20\x20\x20\x20process.exit(1);\n\
         \x20\x20}},\n\
         \x20\x20(err: unknown) => {{\n\
         \x20\x20\x20\x20clearTimeout(watchdog);\n\
         \x20\x20\x20\x20console.log('REJECTED:', String(err));\n\
         \x20\x20\x20\x20process.exit(0);\n\
         \x20\x20}}\n\
         );\n"
    );

    compile_and_run_bridge_harness(test_name, bridge_ts, mcp_json, &harness_ts, extra_env)
}

/// Shared innards of [`run_bridge_harness_with_env`] and the concurrency regression test below:
/// compiles `bridge_ts` plus a caller-supplied `harness_ts` under tsc, then runs the result
/// under Node with `$HOME`/`%USERPROFILE%` pointed at a temp directory containing `mcp_json`
/// as `~/.claude/mcp.json`. Factored out of `run_bridge_harness_with_env` so a harness body
/// other than the single fixed `callMCPTool('...', 'noop', {})` call (e.g. two concurrent
/// calls) does not need to duplicate the tsc/Node plumbing.
///
/// Returns `None` if `tsc`/`node` are not on `PATH` and the caller should skip locally; hard
/// fails (via `require_ts_toolchain`) instead of returning `None` when running in CI.
fn compile_and_run_bridge_harness(
    test_name: &str,
    bridge_ts: &str,
    mcp_json: &serde_json::Value,
    harness_ts: &str,
    extra_env: &[(&str, &str)],
) -> Option<(bool, String, String)> {
    if !require_ts_toolchain(test_name, true) {
        return None;
    }

    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let home_dir = dir.path().join("home");
    let claude_dir = home_dir.join(".claude");
    std::fs::create_dir_all(&claude_dir).expect("Failed to create fake $HOME/.claude");
    std::fs::write(claude_dir.join("mcp.json"), mcp_json.to_string())
        .expect("Failed to write mcp.json");

    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("Failed to create src dir");
    // `--module NodeNext` picks CJS vs. ESM emit per-file based on the nearest
    // `package.json`'s "type" field, resolved from the *source* file's location — not the
    // `--outDir`. Without this, tsc silently emits CommonJS (`require`/`exports`), which
    // then fails at runtime once Node sees `dist/package.json`'s `"type": "module"`.
    std::fs::write(src_dir.join("package.json"), "{\"type\":\"module\"}\n")
        .expect("Failed to write src/package.json");
    std::fs::write(src_dir.join("mcp-bridge.ts"), bridge_ts)
        .expect("Failed to write mcp-bridge.ts");
    std::fs::write(src_dir.join("harness.ts"), harness_ts).expect("Failed to write harness.ts");

    let dist_dir = dir.path().join("dist");
    let tsc_output = Command::new(tsc_program())
        .args(["--noCheck", "--module", "NodeNext", "--moduleResolution"])
        .arg("NodeNext")
        .arg("--target")
        .arg("ES2022")
        .arg("--outDir")
        .arg(&dist_dir)
        .arg(src_dir.join("mcp-bridge.ts"))
        .arg(src_dir.join("harness.ts"))
        .output()
        .expect("Failed to run tsc");
    assert!(
        tsc_output.status.success(),
        "tsc failed to compile the rendered runtime bridge:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&tsc_output.stdout),
        String::from_utf8_lossy(&tsc_output.stderr)
    );
    std::fs::write(dist_dir.join("package.json"), "{\"type\":\"module\"}\n")
        .expect("Failed to write dist/package.json");

    let harness_path = dist_dir.join("harness.js");
    let owned_extra_env: Vec<(String, String)> = extra_env
        .iter()
        .map(|&(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut cmd = Command::new("node");
        cmd.arg(&harness_path)
            // Node's `os.homedir()` — which `loadServerConfig()` uses to find
            // `~/.claude/mcp.json` — reads `$HOME` on POSIX but `%USERPROFILE%` on Windows;
            // both must point at the fake home directory for the harness to isolate itself
            // from the real runner's home on every platform.
            .env("HOME", &home_dir)
            .env("USERPROFILE", &home_dir);
        for (key, value) in &owned_extra_env {
            cmd.env(key, value);
        }
        let result = cmd.output();
        let _ = tx.send(result);
    });

    let output = rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap_or_else(|_| {
            panic!(
                "node harness for {test_name} did not exit within 15s; the bridge may be \
                 hanging instead of settling every pending request"
            )
        })
        .expect("Failed to run node");

    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Behavioral regression guard for #201: a hostile `~/.claude/mcp.json` entry (a forbidden
/// `LD_PRELOAD` env var) must be rejected by the rendered `_runtime/mcp-bridge.ts` before it
/// ever spawns the configured subprocess. Unlike a string-grep over the rendered source (see
/// `test_generate_runtime_bridge_declares_forbidden_env_var_list` in
/// `progressive/generator.rs`, which can pass even against dead/unreachable validation code),
/// this actually compiles and executes the bridge under Node.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_forbidden_env_var_before_spawn() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    // Benign command/args (no shell metacharacters) so the metacharacter check doesn't fire
    // first and mask the env-var check under test.
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": ["--version"],
                "env": { "LD_PRELOAD": "/tmp/evil.so" }
            }
        }
    });

    // `require_ts_toolchain` (called inside `run_bridge_harness`) already prints the skip
    // reason locally and hard-fails in CI, so a missing toolchain here just means "skip".
    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_forbidden_env_var_before_spawn",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the hostile LD_PRELOAD config before spawning:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("LD_PRELOAD"),
        "rejection reason should name the forbidden env var: {stdout}"
    );
}

/// Behavioral regression guard for #201's critic follow-up (S3): `{"transport":"http",...}`
/// is a valid `mcp.json` entry since #200 (all `ServerConfig` fields are `#[serde(default)]`
/// on the Rust side). Before this fix, the bridge's validator called `.trim()` on the
/// `undefined` `command`/`args` an http-transport entry has, throwing an opaque
/// `TypeError: Cannot read properties of undefined (reading 'trim')`. This runtime bridge
/// only ever spawns a subprocess (stdio transport); a non-stdio config must be rejected with
/// a clear, intentional error instead of that crash.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_with_clear_error_not_crash() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp"
            }
        }
    });

    // `require_ts_toolchain` (called inside `run_bridge_harness`) already prints the skip
    // reason locally and hard-fails in CI, so a missing toolchain here just means "skip".
    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_with_clear_error_not_crash",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the http-transport config cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the config: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties of undefined"),
        "must fail with a clear, intentional error, not the pre-fix opaque TypeError: {stdout}"
    );
    assert!(
        stdout.contains("http") || stdout.contains("transport"),
        "rejection reason should mention the unsupported transport: {stdout}"
    );
}

/// #221 item 2 — the rendered bridge must validate an http/sse config's URL scheme to the
/// same depth as `mcp_execution_core::validate_server_config` before rejecting the transport
/// as unsupported, so a bad scheme is reported precisely rather than masked by the generic
/// "unsupported transport" message.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_bad_url_scheme() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    for url in ["file:///etc/passwd", "ftp://host/path"] {
        let mcp_json = json!({
            "mcpServers": {
                "github": {
                    "transport": "http",
                    "url": url
                }
            }
        });

        let Some((success, stdout, stderr)) = run_bridge_harness(
            "test_runtime_bridge_rejects_http_transport_bad_url_scheme",
            &bridge.content,
            &mcp_json,
            "github",
        ) else {
            return;
        };

        assert!(
            success,
            "bridge did not reject url {url}:\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            stdout.contains("http:// or https://"),
            "rejection reason should name the required scheme for url {url}: {stdout}"
        );
    }
}

/// #221 item 2 — a hand-edited `mcp.json` with `"transport": "http"` and no `url` key is
/// valid JSON (every `ServerConfig` field is optional on the Rust side); the bridge must
/// reject it with a specific "url is required" message, not an opaque `TypeError`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_missing_url() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http"
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_missing_url",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject missing url cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties of undefined"),
        "must fail with a clear, intentional error, not an opaque TypeError: {stdout}"
    );
    assert!(
        stdout.contains("url is required"),
        "rejection reason should mention the missing url: {stdout}"
    );
}

/// #221 item 2 — header name/value safety for http/sse transports, mirroring
/// `mcp_execution_core::command`'s RFC 7230 `tchar` and control-character checks.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_unsafe_headers() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    // Space is not a control character but is still outside RFC 7230's `token` charset.
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "X Bad Header": "value" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_unsafe_headers",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the unsafe header name cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("header name"),
        "rejection reason should mention the header name: {stdout}"
    );
}

/// #221 item 2 — a header value containing a CR/LF must be rejected, and — like the Rust
/// source of truth — the value itself (which routinely carries secrets such as bearer
/// tokens) must never appear in the thrown error.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_control_char_in_header_value() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": "Bearer sekrit\r\nX-Injected: evil" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_control_char_in_header_value",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the unsafe header value cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Authorization"),
        "rejection reason should name the header: {stdout}"
    );
    assert!(
        !stdout.contains("sekrit") && !stdout.contains("X-Injected"),
        "the header VALUE must never appear in the error message: {stdout}"
    );
}

/// #221 item 2 — two header names differing only in case (e.g. `Authorization` and
/// `authorization`) must be rejected, mirroring the Rust source of truth's
/// case-insensitive-collision check.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_http_transport_duplicate_case_insensitive_headers() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "transport": "http",
                "url": "https://api.example.com/mcp",
                "headers": { "Authorization": "Bearer one", "authorization": "Bearer two" }
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_http_transport_duplicate_case_insensitive_headers",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the duplicate header cleanly:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("duplicate header"),
        "rejection reason should mention the duplicate header: {stdout}"
    );
}

/// Writes `content` to a standalone `.js` file inside a fresh temp directory and returns its
/// absolute path as a string, suitable for use as a stdio-transport `args` entry.
///
/// Used instead of `node -e '<code>'` for the #221 item 4 tests below: `(` and `)` are
/// themselves forbidden shell metacharacters (`FORBIDDEN_CHARS`), so virtually any real JS
/// snippet passed inline as an argument would trip `validateCommandString` before the
/// scenario under test — the process dying or hanging — ever gets a chance to run.
fn write_test_script(content: &str) -> String {
    let dir = tempfile::tempdir().expect("Failed to create temp dir for test script");
    let path = dir.path().join("script.js");
    std::fs::write(&path, content).expect("Failed to write test script");
    // Leak the tempdir so it outlives the spawned `node` process for the duration of the
    // test; these are single test-process-lifetime allocations, not a long-running leak.
    Box::leak(Box::new(dir));
    path.to_string_lossy().into_owned()
}

/// #221 item 4 — a subprocess that exits before ever writing a JSON-RPC response must be
/// rejected with a clear error instead of hanging forever. Reproduces the issue's own
/// repro steps almost verbatim (`node -e "process.exit(1)"`, via a script file — see
/// `write_test_script`).
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_when_child_exits_before_responding() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script("process.exit(1);\n");
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": [script_path]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness(
        "test_runtime_bridge_rejects_when_child_exits_before_responding",
        &bridge.content,
        &mcp_json,
        "github",
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not reject the dead server cleanly (may have hung until the harness \
         watchdog fired):\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the call: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("exited before responding"),
        "rejection reason should explain the process died before replying: {stdout}"
    );
}

/// #221 item 4 — a subprocess that spawns successfully but never replies at all must still
/// fail with a clear timeout error rather than hang forever. Uses
/// `MCPBRIDGE_REQUEST_TIMEOUT_MS` (well under the harness's 5s watchdog) so this test doesn't
/// need to wait out the bridge's real 30s default.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_times_out_when_server_never_replies() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    // Stays alive indefinitely without ever writing to stdout.
    let script_path = write_test_script("setInterval(function keepAlive() {}, 1000);\n");
    let mcp_json = json!({
        "mcpServers": {
            "github": {
                "command": "node",
                "args": [script_path]
            }
        }
    });

    let Some((success, stdout, stderr)) = run_bridge_harness_with_env(
        "test_runtime_bridge_times_out_when_server_never_replies",
        &bridge.content,
        &mcp_json,
        "github",
        &[("MCPBRIDGE_REQUEST_TIMEOUT_MS", "200")],
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not time out cleanly (may have hung until the harness watchdog \
         fired):\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "expected the bridge to reject the call: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        stdout.contains("Timed out"),
        "rejection reason should explain the request timed out: {stdout}"
    );
}

/// Regression guard for #232: two concurrent `callMCPTool` calls on the same connection must
/// each resolve with the response matching THEIR OWN JSON-RPC request id, even when the
/// server replies out of order (responds to the second request before the first). Before the
/// fix, each call's response was consumed by whichever per-call listener happened to fire
/// first — not matched by id — so an out-of-order reply silently handed one caller the other's
/// result.
///
/// The fake MCP server below deliberately buffers both `tools/call` requests and replies to
/// the second-received one first, echoing back each call's own argument so a mismatch is
/// directly observable in the result value rather than merely "no crash occurred".
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_dispatches_concurrent_out_of_order_responses_by_request_id() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let fake_server_js = r"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin, terminal: false });
const pendingToolCalls = [];

rl.on('line', (line) => {
  const trimmed = line.trim();
  if (trimmed.length === 0) return;
  const message = JSON.parse(trimmed);

  if (message.method === 'initialize') {
    const response = {
      jsonrpc: '2.0',
      id: message.id,
      result: {
        protocolVersion: '2024-11-05',
        capabilities: {},
        serverInfo: { name: 'fake', version: '0.0.0' }
      }
    };
    process.stdout.write(JSON.stringify(response) + '\n');
    return;
  }

  if (message.method === 'tools/call') {
    pendingToolCalls.push(message);
    if (pendingToolCalls.length < 2) return;

    // Deliberately reply in the REVERSE of arrival order: the second request received gets
    // its response written first. This is the out-of-order scenario from issue #232 — a
    // correct dispatcher must still resolve each caller with the response matching ITS OWN
    // request id, not whichever response happens to arrive first.
    const [receivedFirst, receivedSecond] = pendingToolCalls;
    for (const call of [receivedSecond, receivedFirst]) {
      const response = {
        jsonrpc: '2.0',
        id: call.id,
        result: {
          content: [{ type: 'text', text: `${call.params.arguments.value}-response` }]
        }
      };
      process.stdout.write(JSON.stringify(response) + '\n');
    }
  }
});
";
    let script_path = write_test_script(fake_server_js);

    let mcp_json = json!({
        "mcpServers": {
            "fake": {
                "command": "node",
                "args": [script_path]
            }
        }
    });

    let harness_ts = r"
import { callMCPTool } from './mcp-bridge.js';

const watchdog = setTimeout(() => {
  console.log('TIMEOUT: did not settle both calls');
  process.exit(2);
}, 5000);

const first = callMCPTool('fake', 'echo', { value: 'first' });
const second = callMCPTool('fake', 'echo', { value: 'second' });

Promise.all([first, second]).then(
  ([firstResult, secondResult]) => {
    clearTimeout(watchdog);
    console.log('FIRST:', JSON.stringify(firstResult));
    console.log('SECOND:', JSON.stringify(secondResult));
    if (firstResult === 'first-response' && secondResult === 'second-response') {
      console.log('MATCH');
      process.exit(0);
    } else {
      console.log('MISMATCH');
      process.exit(1);
    }
  },
  (err: unknown) => {
    clearTimeout(watchdog);
    console.log('REJECTED:', String(err));
    process.exit(3);
  }
);
";

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_dispatches_concurrent_out_of_order_responses_by_request_id",
        &bridge.content,
        &mcp_json,
        harness_ts,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "bridge did not dispatch out-of-order concurrent responses correctly:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("MATCH"),
        "each caller must resolve with the response matching its own request id, not the \
         other caller's response: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Builds a minimal fake MCP server script that answers `initialize` normally and replies to
/// the single `tools/call` request it receives with `result_json` verbatim as the `result`
/// field. Shared by the #255 regression tests below, which each need a different `result`
/// shape (empty `content`, `structuredContent`-only, etc.) but identical initialize/id
/// plumbing.
fn respond_once_fake_server_js(result_json: &str) -> String {
    format!(
        r"
const readline = require('readline');
const rl = readline.createInterface({{ input: process.stdin, terminal: false }});

rl.on('line', (line) => {{
  const trimmed = line.trim();
  if (trimmed.length === 0) return;
  const message = JSON.parse(trimmed);

  if (message.method === 'initialize') {{
    const response = {{
      jsonrpc: '2.0',
      id: message.id,
      result: {{
        protocolVersion: '2024-11-05',
        capabilities: {{}},
        serverInfo: {{ name: 'fake', version: '0.0.0' }}
      }}
    }};
    process.stdout.write(JSON.stringify(response) + '\n');
    return;
  }}

  if (message.method === 'tools/call') {{
    const response = {{
      jsonrpc: '2.0',
      id: message.id,
      result: {result_json}
    }};
    process.stdout.write(JSON.stringify(response) + '\n');
  }}
}});
"
    )
}

/// Harness that calls `callMCPTool('fake', 'noop', {})` once, printing `RESOLVED: <json>` or
/// `REJECTED: <message>` and exiting 0 in either case (the fake servers below never hang, so no
/// watchdog is needed) — the assertions inspect stdout rather than the exit code.
const SINGLE_CALL_HARNESS_TS: &str = r"
import { callMCPTool } from './mcp-bridge.js';

callMCPTool('fake', 'noop', {}).then(
  (result: unknown) => {
    console.log('RESOLVED:', JSON.stringify(result));
    process.exit(0);
  },
  (err: unknown) => {
    console.log('REJECTED:', String(err));
    process.exit(0);
  }
);
";

/// Regression guard for #255: a tool-error response (`isError: true`) with an empty `content`
/// array must surface as a clear rejection, not crash with an unguarded
/// `Cannot read properties of undefined (reading 'text')` `TypeError`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_surfaces_tool_error_with_empty_content() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "isError": true, "content": [] }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_surfaces_tool_error_with_empty_content",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:") && stdout.contains("Tool returned error"),
        "an empty content array on an isError response must reject with a clear \
         'Tool returned error' message, not crash on an unguarded content[0] dereference: \
         stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties"),
        "must not crash with an unguarded property-access TypeError: stdout: {stdout}"
    );
}

/// Regression guard for #255: a successful response carrying only `structuredContent` (no
/// `content`, per spec 2025-06-18+) must resolve with that `structuredContent` value as a
/// distinct, well-typed case, not crash or silently return `undefined`.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_returns_structured_content_when_content_empty() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "content": [], "structuredContent": { "answer": 42 } }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_returns_structured_content_when_content_empty",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(r#"RESOLVED: {"answer":42}"#),
        "a structuredContent-only response must resolve with structuredContent, not crash or \
         resolve with something else: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Regression guard for #255: a successful response with an empty `content` array and no
/// `structuredContent` at all must surface as a clear rejection, not crash on an unguarded
/// `content[0]` dereference.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_empty_content_without_structured_content() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(r#"{ "content": [] }"#));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_rejects_empty_content_without_structured_content",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "an empty content array with no structuredContent must reject with a clear error, not \
         crash or silently resolve: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties"),
        "must not crash with an unguarded property-access TypeError: stdout: {stdout}"
    );
}

/// Regression guard for #262: a successful response whose `content` array contains a `null`
/// first element must surface as a clear rejection, not crash on an unguarded
/// `firstContent.type` dereference.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_rejects_null_first_content_element() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(r#"{ "content": [null] }"#));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_rejects_null_first_content_element",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:"),
        "a null first content element must reject with a clear error, not crash or silently \
         resolve: stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Cannot read properties"),
        "must not crash with an unguarded property-access TypeError: stdout: {stdout}"
    );
}

/// Regression guard for #262: a successful response whose `content` array contains a `null`
/// first element but also carries a populated `structuredContent` must resolve with the
/// `structuredContent` value instead of discarding it and throwing — a malformed `content[0]`
/// should not shadow otherwise-usable structured data.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_falls_back_to_structured_content_on_null_first_content_element() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "content": [null], "structuredContent": { "answer": 42 } }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_falls_back_to_structured_content_on_null_first_content_element",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(r#"RESOLVED: {"answer":42}"#),
        "a null first content element with populated structuredContent must resolve with \
         structuredContent, not discard it and reject: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Regression guard for #262: a successful response carrying a literal `structuredContent: null`
/// alongside an empty `content` array must be treated the same as an absent `structuredContent`
/// (i.e. reject with the standard empty-content error), not returned as if `null` were real data.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_treats_null_structured_content_as_absent() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "content": [], "structuredContent": null }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_treats_null_structured_content_as_absent",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:") && stdout.contains("no content and no structuredContent"),
        "a literal structuredContent: null must be treated as absent and reject with the \
         standard empty-content error, not resolve with null: stdout: {stdout}, stderr: {stderr}"
    );
}

/// Regression guard for #262: an `isError: true` response with an empty `content` array but a
/// populated `structuredContent` must surface that structured detail in the thrown error message
/// instead of discarding it behind a generic 'Unknown error'.
///
/// Skips (does not fail) when `tsc` or `node` is not on `PATH`.
#[test]
fn test_runtime_bridge_surfaces_structured_content_on_tool_error() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");
    let bridge = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let script_path = write_test_script(&respond_once_fake_server_js(
        r#"{ "isError": true, "content": [], "structuredContent": { "reason": "boom" } }"#,
    ));
    let mcp_json = json!({
        "mcpServers": { "fake": { "command": "node", "args": [script_path] } }
    });

    let Some((success, stdout, stderr)) = compile_and_run_bridge_harness(
        "test_runtime_bridge_surfaces_structured_content_on_tool_error",
        &bridge.content,
        &mcp_json,
        SINGLE_CALL_HARNESS_TS,
        &[],
    ) else {
        return;
    };

    assert!(
        success,
        "harness process itself must exit 0 regardless of resolve/reject: stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("REJECTED:")
            && stdout.contains("Tool returned error")
            && stdout.contains(r#"{"reason":"boom"}"#),
        "an isError response with populated structuredContent must surface it in the error \
         message instead of falling back to 'Unknown error': stdout: {stdout}, stderr: {stderr}"
    );
    assert!(
        !stdout.contains("Unknown error"),
        "structuredContent detail must take priority over the generic fallback: stdout: {stdout}"
    );
}
