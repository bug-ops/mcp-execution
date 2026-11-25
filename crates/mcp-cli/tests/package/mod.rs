//! Offline writer of a generated server package, shared by tests that need `mcp:<server>` imports
//! without contacting any server.
//!
//! Kept apart from `common`, whose fixture-binary environment exists only with `test-fixtures`,
//! so test files built without that feature can use it too.

#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::path::Path;

use mcp_execution_codegen::PackageGenerator;
use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

/// Writes the generated package of the server `server_id` (shown as `display_name`) with one
/// empty-object-input tool per entry of `tools` below `servers/<server_id>`.
pub fn write_package(servers: &Path, server_id: &str, display_name: &str, tools: &[&str]) {
    let info = ServerInfo::new(
        ServerId::new(server_id).expect("valid id"),
        display_name.to_string(),
        "1.0.0".to_string(),
        tools
            .iter()
            .map(|name| {
                ToolInfo::new(
                    ToolName::new(*name).expect("valid tool name"),
                    format!("The {name} tool"),
                    json!({"type": "object"}),
                )
            })
            .collect(),
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );
    let config = ServerConfig::builder()
        .command("unused".to_string())
        .build()
        .expect("valid config");
    let code = PackageGenerator::new()
        .expect("generator")
        .generate(&info, &config)
        .expect("generate");
    for file in &code.files {
        let path = servers.join(server_id).join(&file.path);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(path, &file.content).expect("write generated file");
    }
}
