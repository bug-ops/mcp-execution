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
    // - 1 _meta.json
    assert_eq!(code.file_count(), 7);
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

    // Should contain result interface
    assert!(
        content.contains("export interface createIssueResult"),
        "Missing Result interface"
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
    // - 1 _meta.json
    assert_eq!(code.file_count(), 4);

    let file_paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();
    assert!(file_paths.contains(&"index.ts"));
    assert!(file_paths.contains(&"_runtime/mcp-bridge.ts"));
    assert!(file_paths.contains(&"package.json"));
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

/// Regression guard for #176: generated tool wrappers must actually type-check under
/// `tsc --strict --noEmit`, not merely contain the right substrings. Type-checks the real
/// generated `createIssue.ts` against a minimal stub of `callMCPTool` (mirroring the
/// signature declared in `runtime-bridge.ts.hbs`) rather than the full runtime bridge, so
/// the test stays offline and doesn't depend on `@types/node` being installed.
///
/// Skips locally (hard-fails in CI — see `require_ts_toolchain`) when `tsc` is not on `PATH`.
#[test]
fn test_generated_tool_passes_tsc_noemit() {
    if !require_ts_toolchain("test_generated_tool_passes_tsc_noemit", false) {
        return;
    }

    // Guard against the stub below silently going stale if callMCPTool's signature changes.
    let bridge_template = include_str!("../templates/progressive/runtime-bridge.ts.hbs");
    assert!(
        bridge_template.contains("params: Record<string, unknown>")
            && bridge_template.contains("): Promise<unknown> {"),
        "callMCPTool signature in runtime-bridge.ts.hbs changed — update the stub in \
         test_generated_tool_passes_tsc_noemit to match"
    );

    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let tool_file = code
        .files
        .iter()
        .find(|f| f.path == "createIssue.ts")
        .expect("createIssue.ts not found");
    let package_json = code
        .files
        .iter()
        .find(|f| f.path == "package.json")
        .expect("package.json not found");

    let dir = tempfile::tempdir().expect("Failed to create temp dir");

    std::fs::write(dir.path().join("createIssue.ts"), &tool_file.content)
        .expect("Failed to write createIssue.ts");
    // Real package.json declares `"type": "module"`, which is what makes `import.meta.url`
    // in the generated CLI-mode block legal under NodeNext module resolution.
    std::fs::write(dir.path().join("package.json"), &package_json.content)
        .expect("Failed to write package.json");
    // Minimal ambient declaration for the subset of the Node `process` global the generated
    // CLI-mode block references, so the test doesn't depend on `@types/node` being installed.
    std::fs::write(
        dir.path().join("globals.d.ts"),
        "declare const process: {\n\
         \x20\x20argv: string[];\n\
         \x20\x20exit(code?: number): never;\n\
         };\n",
    )
    .expect("Failed to write globals.d.ts");

    let runtime_dir = dir.path().join("_runtime");
    std::fs::create_dir_all(&runtime_dir).expect("Failed to create _runtime dir");
    std::fs::write(
        runtime_dir.join("mcp-bridge.ts"),
        "export async function callMCPTool(\n\
         \x20\x20serverId: string,\n\
         \x20\x20toolName: string,\n\
         \x20\x20params: Record<string, unknown>\n\
         ): Promise<unknown> {\n\
         \x20\x20void serverId;\n\
         \x20\x20void toolName;\n\
         \x20\x20void params;\n\
         \x20\x20return undefined;\n\
         }\n",
    )
    .expect("Failed to write mcp-bridge.ts stub");

    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true,
    "allowImportingTsExtensions": true,
    "skipLibCheck": true
  },
  "include": ["**/*.ts"]
}
"#,
    )
    .expect("Failed to write tsconfig.json");

    let output = Command::new(tsc_program())
        .arg("--noEmit")
        .arg("-p")
        .arg(dir.path().join("tsconfig.json"))
        .output()
        .expect("Failed to run tsc");

    assert!(
        output.status.success(),
        "tsc --noEmit failed on generated createIssue.ts:\nstdout: {}\nstderr: {}",
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
    std::fs::write(
        src_dir.join("harness.ts"),
        format!(
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
        ),
    )
    .expect("Failed to write harness.ts");

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
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = Command::new("node")
            .arg(&harness_path)
            // Node's `os.homedir()` — which `loadServerConfig()` uses to find
            // `~/.claude/mcp.json` — reads `$HOME` on POSIX but `%USERPROFILE%` on Windows;
            // both must point at the fake home directory for the harness to isolate itself
            // from the real runner's home on every platform.
            .env("HOME", &home_dir)
            .env("USERPROFILE", &home_dir)
            .output();
        let _ = tx.send(result);
    });

    let output = rx
        .recv_timeout(std::time::Duration::from_secs(15))
        .unwrap_or_else(|_| {
            panic!(
                "node harness did not exit within 15s; the bridge may be hanging on an \
                 unvalidated spawn instead of rejecting the hostile config"
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
