//! Smoke tests of the `fixture-tools-mcp-server` binary the acceptance tests build on: its tool
//! list over stdio and Streamable HTTP, and the `MCP_FIXTURE_LOG` call log.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]

use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};

use mcp_execution_core::{ServerConfig, ServerId};
use mcp_execution_introspector::Introspector;

const FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_fixture-tools-mcp-server");

const EXPECTED_TOOLS: [&str; 9] = [
    "echo",
    "structured",
    "empty",
    "error",
    "large",
    "slow",
    "list_items",
    "create_item",
    "delete_item",
];

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tool_names(info: &mcp_execution_introspector::ServerInfo) -> Vec<&str> {
    info.tools.iter().map(|tool| tool.name.as_str()).collect()
}

#[tokio::test]
async fn stdio_fixture_serves_the_fixed_tool_set_and_logs_initialize() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("calls.log");
    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .env("MCP_FIXTURE_LOG".to_string(), log.display().to_string())
        .build()
        .unwrap();

    let info = Introspector::new()
        .discover_server(ServerId::new("fixture").unwrap(), &config)
        .await
        .unwrap();

    assert_eq!(tool_names(&info), EXPECTED_TOOLS);
    let structured = info.tools.iter().find(|t| t.name.as_str() == "structured");
    assert!(structured.unwrap().output_schema.is_some());
    let delete = info.tools.iter().find(|t| t.name.as_str() == "delete_item");
    assert!(delete.unwrap().annotations.is_some());
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "initialize\n");
}

#[tokio::test]
async fn http_fixture_prints_its_url_and_serves_the_same_tools() {
    let child = Command::new(FIXTURE_BIN)
        .args(["--http", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("fixture must start");
    let mut child = KillOnDrop(child);
    let stdout = child.0.stdout.take().expect("stdout is piped");
    let mut url = String::new();
    BufReader::new(stdout).read_line(&mut url).unwrap();
    let url = url.trim().to_string();
    assert!(url.starts_with("http://127.0.0.1:"), "unexpected url {url}");

    let config = ServerConfig::builder().http_transport(url).build().unwrap();
    let info = Introspector::new()
        .discover_server(ServerId::new("fixture-http").unwrap(), &config)
        .await
        .unwrap();

    assert_eq!(tool_names(&info), EXPECTED_TOOLS);
}
