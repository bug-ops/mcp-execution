//! Acceptance test of the SIGCHLD reset (#689): a server started with SIGCHLD ignored must still
//! reap its introspection child, so discovery succeeds without a termination warning.
//!
//! Linux only: tokio reaps children through a pidfd there and never installs a SIGCHLD handler,
//! so the inherited ignore survives unless the server resets it. The server is started through
//! `perl`, which sets `SIG_IGN` that `exec` passes on; the MCP server it introspects is a `perl`
//! script too.
#![cfg(target_os = "linux")]

use serde_json::{Value, json};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"acceptance","version":"0.0.0"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// A one-tool MCP server over stdio that stays alive until it is killed.
const FAKE_SERVER: &str = r#"
$| = 1;
while (my $line = <STDIN>) {
    next unless $line =~ /"id"\s*:\s*("[^"]*"|\d+)/;
    my $id = $1;
    my $result;
    if ($line =~ /"method"\s*:\s*"initialize"/) {
        my ($version) = $line =~ /"protocolVersion"\s*:\s*"([^"]+)"/;
        $result = qq({"protocolVersion":"$version","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}});
    } elsif ($line =~ /"method"\s*:\s*"tools\/list"/) {
        $result = q({"tools":[{"name":"ping","description":"Pings","inputSchema":{"type":"object"}}]});
    } else {
        print qq({"jsonrpc":"2.0","id":$id,"error":{"code":-32601,"message":"no such method"}}\n);
        next;
    }
    print qq({"jsonrpc":"2.0","id":$id,"result":$result}\n);
}
sleep 60;
"#;

#[test]
fn introspect_server_reaps_its_child_when_started_with_sigchld_ignored() {
    let home = tempfile::tempdir().unwrap();
    let fake = home.path().join("fake_server.pl");
    std::fs::write(&fake, FAKE_SERVER).unwrap();

    let mut child = Command::new("perl")
        .args([
            "-e",
            "$SIG{CHLD} = 'IGNORE'; exec @ARGV or die \"exec: $!\"",
        ])
        .arg(env!("CARGO_BIN_EXE_mcp-execution"))
        .env("HOME", home.path())
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-execution through perl");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");
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
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "introspect_server",
            "arguments": {
                "server_id": "fake",
                "command": "perl",
                "args": [fake.display().to_string()],
            },
        },
    })
    .to_string();
    for line in [INITIALIZE, INITIALIZED, &call] {
        writeln!(stdin, "{line}").expect("write request");
    }
    stdin.flush().expect("flush");

    let response = rx.recv_timeout(Duration::from_secs(60));
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let response = response.expect("a response to introspect_server");
    let stderr = errors.join().unwrap();

    assert!(response.get("error").is_none(), "{response}\n{stderr}");
    assert_ne!(response["result"]["isError"], true, "{response}\n{stderr}");
    assert!(
        !stderr.contains("failed to terminate introspection child process"),
        "{stderr}"
    );
}
