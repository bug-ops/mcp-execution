//! Helpers shared by the acceptance test crates that run a generated package under Node.
//!
//! Complements `support/mod.rs` (toolchain checks and the bridge harness) with a scriptable fake
//! MCP server and a runner for `node --experimental-strip-types <tool>.ts '<json>'`.

#![allow(
    dead_code,
    reason = "shared by several test crates; each uses a subset"
)]

use mcp_execution_codegen::{GeneratedCode, ProgressiveGenerator};
use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

/// Server id every acceptance package is generated for.
pub const SERVER_ID: &str = "fake";

/// A temporary directory addressed by its canonical path.
///
/// On macOS the system temp directory lives under the `/var` symlink, which would make every
/// direct `node <tool>.ts` run depend on #583's entry detection. Tests that are about something
/// else use this so a failure points at the behaviour under test.
#[derive(Debug)]
pub struct Scratch {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl Scratch {
    /// The canonical path of the directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Creates a [`Scratch`] directory.
pub fn canonical_tempdir() -> Scratch {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = without_verbatim_prefix(dir.path().canonicalize().expect("canonicalize temp dir"));
    Scratch { _dir: dir, path }
}

/// Drops the `\\?\` prefix Windows' `canonicalize` adds: node's `realpathSync` fails on such
/// paths with `EISDIR` for the bare drive. A no-op for every other path.
fn without_verbatim_prefix(path: std::path::PathBuf) -> std::path::PathBuf {
    match path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(plain) if !plain.starts_with(r"UNC\") => std::path::PathBuf::from(plain),
        _ => path,
    }
}

/// A tool with the given schemas and no annotations.
pub fn tool(name: &str, input: Value, output: Option<Value>) -> ToolInfo {
    ToolInfo {
        name: ToolName::new(name).unwrap(),
        description: format!("Tool {name}"),
        input_schema: input,
        output_schema: output,
        annotations: None,
    }
}

/// A server named [`SERVER_ID`] exposing `tools`.
pub fn server_info(tools: Vec<ToolInfo>) -> ServerInfo {
    ServerInfo {
        id: ServerId::new(SERVER_ID).unwrap(),
        name: "Fake".to_string(),
        version: "1.0.0".to_string(),
        tools,
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

/// Generates the package for `server` with the default generator.
pub fn generate(server: &ServerInfo) -> GeneratedCode {
    generate_with(&ProgressiveGenerator::new().expect("generator"), server)
}

/// Generates the package for `server` with `generator`.
pub fn generate_with(generator: &ProgressiveGenerator<'_>, server: &ServerInfo) -> GeneratedCode {
    let config = ServerConfig::builder()
        .command("test-command".to_string())
        .build()
        .unwrap();
    generator
        .generate(server, &config)
        .expect("generation succeeds")
}

/// The content of the generated file at `path`.
pub fn file<'a>(code: &'a GeneratedCode, path: &str) -> &'a str {
    &code
        .files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} not generated"))
        .content
}

/// Reply for a `tools/call` request: a JS arrow function from the parsed request `message` to
/// the JSON-RPC response members (`result` or `error`).
pub const REPLY_TEXT_JSON_OK: &str =
    r#"(m) => ({ result: { content: [{ type: 'text', text: '{"ok":true}' }] } })"#;

/// Reply that echoes the call's `arguments` back as the text content.
pub const REPLY_ECHO_ARGUMENTS: &str = r"(m) => ({ result: { content: [{ type: 'text', text: JSON.stringify(m.params.arguments) }] } })";

/// Builds a fake stdio MCP server that answers `initialize` and replies to every `tools/call`
/// with whatever the JS function `reply` returns.
pub fn fake_server_js(reply: &str) -> String {
    format!(
        r"
const readline = require('readline');
const rl = readline.createInterface({{ input: process.stdin, terminal: false }});
const reply = {reply};
rl.on('line', (line) => {{
  const trimmed = line.trim();
  if (trimmed.length === 0) return;
  const message = JSON.parse(trimmed);
  if (message.method === 'initialize') {{
    process.stdout.write(JSON.stringify({{
      jsonrpc: '2.0', id: message.id,
      result: {{ protocolVersion: '2024-11-05', capabilities: {{}}, serverInfo: {{ name: 'fake', version: '0.0.0' }} }}
    }}) + '\n');
    return;
  }}
  if (message.method === 'tools/call') {{
    process.stdout.write(JSON.stringify({{ jsonrpc: '2.0', id: message.id, ...reply(message) }}) + '\n');
  }}
}});
"
    )
}

/// Writes a fake `$HOME` whose `~/.claude/mcp.json` maps [`SERVER_ID`] to `server_js`, returning
/// the home directory path.
pub fn write_home(root: &Path, server_js: &str) -> std::path::PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(home.join(".claude")).expect("create fake home");
    let script = root.join("fake-server.js");
    std::fs::write(&script, server_js).expect("write fake server");
    let mcp_json = json!({
        "mcpServers": { SERVER_ID: { "command": "node", "args": [script.to_string_lossy()] } }
    });
    std::fs::write(home.join(".claude").join("mcp.json"), mcp_json.to_string())
        .expect("write mcp.json");
    home
}

/// Writes every generated file of `code` under `dir`, creating parent directories.
pub fn write_package(code: &GeneratedCode, dir: &Path) {
    for file in &code.files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.content).expect("write generated file");
    }
}

/// Outcome of one node run.
#[derive(Debug)]
pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// The single non-empty line of `text`, panicking when there is not exactly one.
    pub fn single_line(text: &str) -> &str {
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1, "expected exactly one line, got {text:?}");
        lines[0]
    }

    /// Parses the single stderr line as JSON.
    pub fn stderr_json(&self) -> Value {
        serde_json::from_str(Self::single_line(&self.stderr))
            .unwrap_or_else(|e| panic!("stderr line is not JSON ({e}): {:?}", self.stderr))
    }
}

/// Runs `node --experimental-strip-types <script> [argument]` with a 30 s watchdog.
pub fn run_node(
    script: &Path,
    cwd: &Path,
    home: &Path,
    argument: Option<&str>,
    extra_env: &[(&str, &str)],
) -> Run {
    let mut command = Command::new("node");
    command
        .arg("--experimental-strip-types")
        .arg("--no-warnings")
        .arg(script)
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home);
    if let Some(argument) = argument {
        command.arg(argument);
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(command.output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("node did not exit within 30s: the process is hanging instead of exiting")
        .expect("failed to run node");

    Run {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}
