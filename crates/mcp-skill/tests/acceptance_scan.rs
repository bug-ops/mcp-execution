//! Acceptance tests for #566, #568 and #584 on the skill side: the catalog is not a tool, the v3
//! sidecar and its size cap are enforced, and raw property keys are displayed safely.

use mcp_execution_core::metadata::{
    CATALOG_FILE_NAME, MAX_METADATA_FILE_BYTES, METADATA_FILE_NAME, METADATA_SCHEMA_VERSION,
    ResultTypeOrigin, ServerMetadata, ToolMetadata,
};
use mcp_execution_core::provenance::GenerationProvenance;
use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_skill::{
    ParsedParameter, ParsedToolFile, ScanError, build_skill_context, scan_tools_directory,
};
use std::path::Path;

fn sidecar(schema_version: u32, description: Option<String>) -> ServerMetadata {
    let config = ServerConfig::builder()
        .command("docker".to_string())
        .build()
        .unwrap();
    ServerMetadata {
        schema_version,
        server_id: ServerId::new("acme").unwrap(),
        server_name: "Acme".to_string(),
        server_version: "1.0.0".to_string(),
        tools: vec![ToolMetadata {
            name: ToolName::new("get_thing").unwrap(),
            typescript_name: "getThing".to_string(),
            category: None,
            keywords: vec![],
            description,
            parameters: vec![],
            annotations: None,
            result_type: None,
            result_type_origin: ResultTypeOrigin::Undeclared,
            manifest_size_bytes: 0,
        }],
        provenance: GenerationProvenance::capture(&config, &[]),
    }
}

fn write_package(dir: &Path, meta: &ServerMetadata) {
    std::fs::write(
        dir.join(METADATA_FILE_NAME),
        serde_json::to_string(meta).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("getThing.ts"), "export {};\n").unwrap();
    std::fs::write(dir.join("index.ts"), "export {};\n").unwrap();
    std::fs::write(dir.join(CATALOG_FILE_NAME), "export {};\n").unwrap();
}

#[tokio::test]
async fn scan_tools_directory_treats_catalog_ts_as_neither_tool_nor_drift() {
    let dir = tempfile::tempdir().unwrap();
    write_package(dir.path(), &sidecar(METADATA_SCHEMA_VERSION, None));

    let result = scan_tools_directory(dir.path())
        .await
        .expect("scan succeeds");

    assert_eq!(result.tools.len(), 1);
    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
}

#[tokio::test]
async fn scan_tools_directory_rejects_a_v2_sidecar_until_regenerated() {
    let dir = tempfile::tempdir().unwrap();
    write_package(dir.path(), &sidecar(2, None));

    let err = scan_tools_directory(dir.path())
        .await
        .expect_err("v2 must be rejected");

    assert!(matches!(
        err,
        ScanError::UnsupportedSchema {
            found: 2,
            expected: 3
        }
    ));
}

#[tokio::test]
async fn scan_tools_directory_rejects_a_sidecar_over_the_shared_cap() {
    let dir = tempfile::tempdir().unwrap();
    let padding = "x".repeat(usize::try_from(MAX_METADATA_FILE_BYTES).unwrap());
    write_package(dir.path(), &sidecar(METADATA_SCHEMA_VERSION, Some(padding)));

    let err = scan_tools_directory(dir.path())
        .await
        .expect_err("oversized sidecar");

    assert!(matches!(
        err,
        ScanError::FileTooLarge { limit, .. } if limit == MAX_METADATA_FILE_BYTES
    ));
}

fn param(name: &str, required: bool) -> ParsedParameter {
    ParsedParameter {
        name: name.to_string(),
        typescript_type: "string".to_string(),
        required,
        description: None,
    }
}

fn tool_with_params(params: Vec<ParsedParameter>) -> ParsedToolFile {
    ParsedToolFile {
        name: "search".to_string(),
        typescript_name: "search".to_string(),
        server_id: "acme".to_string(),
        category: None,
        keywords: vec![],
        description: Some("Searches".to_string()),
        parameters: params,
    }
}

#[test]
fn build_skill_context_displays_non_identifier_keys_as_json_literals() {
    let tools = vec![tool_with_params(vec![
        param("title", true),
        param("per-page", true),
        param("filter.name", false),
    ])];

    let context = build_skill_context("acme", &tools, None, None);
    let skill_tool = &context.categories[0].tools[0];

    assert_eq!(skill_tool.required_params, vec!["title", "\"per-page\""]);
    assert_eq!(skill_tool.optional_params, vec!["\"filter.name\""]);
    assert!(context.generation_prompt.contains("\"per-page\""));
}

#[test]
fn build_skill_context_example_omits_non_identifier_keys() {
    let tools = vec![tool_with_params(vec![
        param("title", true),
        param("it's", true),
        param("per-page", true),
    ])];

    let context = build_skill_context("acme", &tools, None, None);
    let example = &context.example_tools[0];

    assert!(example.cli_command.contains("title"));
    assert!(
        !example.cli_command.contains("it's"),
        "{}",
        example.cli_command
    );
    assert!(
        !example.cli_command.contains("per-page"),
        "{}",
        example.cli_command
    );
}

#[test]
fn build_skill_context_cannot_close_a_code_span_with_a_key_backtick() {
    let tools = vec![tool_with_params(vec![param("a`b", true)])];

    let context = build_skill_context("acme", &tools, None, None);
    let displayed = &context.categories[0].tools[0].required_params[0];

    assert!(!displayed.contains('`'), "{displayed}");
    assert!(displayed.contains("\\u0060"), "{displayed}");
}
