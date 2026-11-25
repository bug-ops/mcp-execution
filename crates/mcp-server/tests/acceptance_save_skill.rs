//! Acceptance tests of `save_skill` (#578, FR-043.12 and FR-043.14): the frontmatter `name` must
//! equal the directory it is written to, the `description` must fit the Agent Skills limit, a
//! violation is reported as `invalid_params` without echoing the content and before anything is
//! written, and a saved script next to `SKILL.md` is never touched.
//!
//! Drives the real binary over stdio with `$HOME` pointed at a temp directory, since the tool
//! handlers are private.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

const MARKER: &str = "BODY-SECRET-MARKER-5e7d";

/// Longest `description` the Agent Skills format allows.
const DESCRIPTION_LIMIT: usize = 1024;

/// Sends `initialize` and then one `tools/call` per entry (ids 2, 3, ...), returning the
/// responses in order.
fn save_skill_calls(home: &Path, arguments: &[Value]) -> Vec<Value> {
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
    let wanted = arguments.len();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && message["id"].as_u64().is_some_and(|id| id >= 2)
                && tx.send(message).is_err()
            {
                return;
            }
        }
    });

    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"acceptance","version":"0.0.0"}}}"#;
    writeln!(stdin, "{init}").unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    let mut responses = Vec::new();
    for (index, args) in arguments.iter().enumerate() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": index + 2,
            "method": "tools/call",
            "params": {"name": "save_skill", "arguments": args},
        });
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        responses.push(
            rx.recv_timeout(Duration::from_secs(30))
                .expect("a response to save_skill"),
        );
    }
    assert_eq!(responses.len(), wanted);
    let _ = child.kill();
    let _ = child.wait();
    responses
}

fn skill_content(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# Skill\n\n{MARKER}\n")
}

fn is_invalid_params(response: &Value) -> bool {
    response["error"]["code"] == -32602
}

fn skill_dir(home: &Path, server_id: &str) -> std::path::PathBuf {
    home.join(".claude").join("skills").join(server_id)
}

#[test]
fn save_skill_rejects_a_frontmatter_name_that_differs_from_the_directory() {
    let home = tempfile::tempdir().unwrap();
    let args = json!({
        "server_id": "acme",
        "content": skill_content("some-other-name", "Does acme things"),
    });

    let response = &save_skill_calls(home.path(), &[args])[0];

    assert!(is_invalid_params(response), "{response}");
    assert!(
        !response.to_string().contains(MARKER),
        "the content must not be echoed: {response}"
    );
    assert!(
        !skill_dir(home.path(), "acme").join("SKILL.md").exists(),
        "nothing may be written"
    );
}

#[test]
fn save_skill_rejects_a_description_over_the_agent_skills_limit() {
    let home = tempfile::tempdir().unwrap();
    let args = json!({
        "server_id": "acme",
        "content": skill_content("acme", &"d".repeat(DESCRIPTION_LIMIT + 1)),
    });

    let response = &save_skill_calls(home.path(), &[args])[0];

    assert!(is_invalid_params(response), "{response}");
    assert!(!response.to_string().contains(MARKER), "{response}");
    assert!(!skill_dir(home.path(), "acme").join("SKILL.md").exists());
}

#[test]
fn save_skill_leaves_saved_scripts_byte_identical_whether_it_rejects_or_writes() {
    let home = tempfile::tempdir().unwrap();
    let scripts = skill_dir(home.path(), "acme").join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let script = scripts.join("keep.ts");
    let script_bytes = b"/*! mcp-script\n{}\n*/\n// keep me\n";
    std::fs::write(&script, script_bytes).unwrap();
    let rejected = json!({
        "server_id": "acme",
        "content": skill_content("some-other-name", "Does acme things"),
    });
    let accepted = json!({
        "server_id": "acme",
        "content": skill_content("acme", "Does acme things"),
        "overwrite": true,
    });

    let responses = save_skill_calls(home.path(), &[rejected, accepted]);

    assert!(is_invalid_params(&responses[0]), "{}", responses[0]);
    assert!(responses[1]["error"].is_null(), "{}", responses[1]);
    assert!(skill_dir(home.path(), "acme").join("SKILL.md").is_file());
    assert_eq!(std::fs::read(&script).unwrap(), script_bytes);
}
