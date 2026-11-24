//! Acceptance test for #566 on the server side: `list_generated_servers` counts tool files and
//! must not count `catalog.ts` (nor `index.ts`) as a tool.
//!
//! Drives the real binary over stdio with `$HOME` pointed at a temp directory, since the tool
//! handlers are private.

use serde_json::Value;
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"acceptance","version":"0.0.0"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const LIST: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_generated_servers","arguments":{}}}"#;

fn run_list(home: &std::path::Path) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-execution"))
        .env("HOME", home)
        .env("USERPROFILE", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp-execution");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && message["id"] == 2
            {
                let _ = tx.send(message);
                return;
            }
        }
    });

    for line in [INITIALIZE, INITIALIZED, LIST] {
        writeln!(stdin, "{line}").expect("write request");
    }
    stdin.flush().expect("flush");

    let response = rx.recv_timeout(Duration::from_secs(30));
    let _ = child.kill();
    let _ = child.wait();
    response.expect("a response to list_generated_servers")
}

#[test]
#[cfg_attr(
    windows,
    ignore = "dirs::home_dir() ignores HOME and USERPROFILE on Windows (known-folder API), so this end-to-end test cannot redirect the home directory there; tracked in #604"
)]
fn list_generated_servers_counts_tool_files_but_not_catalog_or_index() {
    let home = tempfile::tempdir().unwrap();
    let server_dir = home.path().join(".claude").join("servers").join("acme");
    std::fs::create_dir_all(server_dir.join("_runtime")).unwrap();
    for file in ["getThing.ts", "putThing.ts", "index.ts", "catalog.ts"] {
        std::fs::write(server_dir.join(file), "export {};\n").unwrap();
    }

    let response = run_list(home.path());
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let listing: Value = serde_json::from_str(text).expect("listing JSON");

    assert_eq!(
        listing["servers"][0]["tool_count"], 2,
        "catalog.ts and index.ts must not be counted as tools: {listing}"
    );
}
