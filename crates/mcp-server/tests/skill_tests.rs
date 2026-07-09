//! Integration tests for skill generation.

use mcp_execution_core::metadata::{
    METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ParameterMetadata, ServerMetadata, ToolMetadata,
};
use mcp_execution_skill::{ParsedToolFile, ScanError, build_skill_context, scan_tools_directory};
use tempfile::TempDir;
use tokio::fs;

/// Build a `ServerMetadata` sidecar with `count` simple test tools, each with one
/// required string parameter, and write it as `_meta.json` into `dir`.
async fn write_metadata_sidecar(dir: &std::path::Path, tool_names: &[&str]) {
    let meta = ServerMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        server_id: "test".to_string(),
        server_name: "Test Server".to_string(),
        server_version: "1.0.0".to_string(),
        tools: tool_names
            .iter()
            .map(|name| ToolMetadata {
                name: (*name).to_string(),
                typescript_name: to_camel_case(name),
                category: Some("test-category".to_string()),
                keywords: vec!["test".to_string(), (*name).to_string()],
                description: Some(format!("Test tool: {name}")),
                parameters: vec![
                    ParameterMetadata {
                        name: "required_param".to_string(),
                        typescript_type: "string".to_string(),
                        required: true,
                        description: Some("A required parameter".to_string()),
                    },
                    ParameterMetadata {
                        name: "optional_param".to_string(),
                        typescript_type: "number".to_string(),
                        required: false,
                        description: None,
                    },
                ],
            })
            .collect(),
    };

    let content = serde_json::to_string_pretty(&meta).unwrap();
    fs::write(dir.join(METADATA_FILE_NAME), content)
        .await
        .unwrap();
}

fn to_camel_case(s: &str) -> String {
    let mut parts = s.split('_');
    let Some(first) = parts.next() else {
        return String::new();
    };
    let mut result = first.to_string();
    for part in parts {
        let mut chars = part.chars();
        if let Some(c) = chars.next() {
            result.push(c.to_ascii_uppercase());
            result.push_str(chars.as_str());
        }
    }
    result
}

#[tokio::test]
async fn test_scan_tools_directory_integration() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    write_metadata_sidecar(dir, &["create_issue", "list_repos", "get_user"]).await;

    let tools = scan_tools_directory(dir).await.unwrap();

    assert_eq!(tools.len(), 3);
    assert!(tools.iter().any(|t| t.name == "create_issue"));
    assert!(tools.iter().any(|t| t.name == "list_repos"));
    assert!(tools.iter().any(|t| t.name == "get_user"));

    let create_issue = tools.iter().find(|t| t.name == "create_issue").unwrap();
    assert_eq!(create_issue.server_id, "test");
    assert_eq!(create_issue.category, Some("test-category".to_string()));
    assert_eq!(create_issue.parameters.len(), 2);

    let required_count = create_issue
        .parameters
        .iter()
        .filter(|p| p.required)
        .count();
    let optional_count = create_issue
        .parameters
        .iter()
        .filter(|p| !p.required)
        .count();
    assert_eq!(required_count, 1);
    assert_eq!(optional_count, 1);

    // Issue #141 regression: parameter descriptions must survive the round-trip.
    let required = create_issue
        .parameters
        .iter()
        .find(|p| p.name == "required_param")
        .unwrap();
    assert_eq!(
        required.description,
        Some("A required parameter".to_string())
    );
}

#[tokio::test]
async fn test_build_skill_context_integration() {
    let tools = vec![
        ParsedToolFile {
            name: "create_issue".to_string(),
            typescript_name: "createIssue".to_string(),
            server_id: "github".to_string(),
            category: Some("issues".to_string()),
            keywords: vec!["create".to_string(), "issue".to_string()],
            description: Some("Create a new issue".to_string()),
            parameters: vec![],
        },
        ParsedToolFile {
            name: "list_repos".to_string(),
            typescript_name: "listRepos".to_string(),
            server_id: "github".to_string(),
            category: Some("repos".to_string()),
            keywords: vec!["list".to_string(), "repos".to_string()],
            description: Some("List repositories".to_string()),
            parameters: vec![],
        },
    ];

    let use_case_hints = vec!["CI/CD".to_string()];
    let context = build_skill_context("github", &tools, Some(&use_case_hints));

    assert_eq!(context.server_id, "github");
    assert_eq!(context.skill_name, "github-progressive");
    assert_eq!(context.tool_count, 2);
    assert_eq!(context.categories.len(), 2);
    assert!(!context.generation_prompt.is_empty());
    assert!(context.generation_prompt.contains("CI/CD"));
}

#[tokio::test]
async fn test_scan_nonexistent_directory() {
    let result = scan_tools_directory(std::path::Path::new("/nonexistent/path")).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_scan_directory_missing_metadata() {
    // A directory that exists but was never generated with the sidecar (or was
    // generated by a pre-#141 version) must hard-error rather than silently
    // report zero tools.
    let temp_dir = TempDir::new().unwrap();

    let result = scan_tools_directory(temp_dir.path()).await;

    assert!(matches!(result, Err(ScanError::MissingMetadata { .. })));
}

#[tokio::test]
async fn test_skill_metadata_extraction() {
    let content = r"---
name: github-progressive
description: GitHub operations via MCP tools
---

# GitHub Progressive

Introduction paragraph.

## Quick Start

Steps here.

## Common Tasks

Tasks here.

## Troubleshooting

Troubleshooting here.
";

    // This tests the metadata extraction logic
    assert!(content.starts_with("---"));
    assert!(content.contains("name:"));
    assert!(content.contains("description:"));

    // Count sections
    let section_count = content.lines().filter(|l| l.starts_with("## ")).count();
    assert_eq!(section_count, 3);
}

// ============================================================================
// Large Metadata Handling Tests
// ============================================================================

#[tokio::test]
async fn test_scan_directory_with_many_tools() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    let tool_names: Vec<String> = (0..50).map(|i| format!("tool_{i}")).collect();
    let tool_name_refs: Vec<&str> = tool_names.iter().map(String::as_str).collect();
    write_metadata_sidecar(dir, &tool_name_refs).await;

    let tools = scan_tools_directory(dir).await.unwrap();

    assert_eq!(tools.len(), 50);
    // Verify they're sorted
    for i in 1..tools.len() {
        assert!(tools[i - 1].name <= tools[i].name);
    }
}

#[tokio::test]
async fn test_scan_directory_ignores_stray_files() {
    // Only `_meta.json` is read; stray `.ts` files, an `index.ts`, or a `_runtime`
    // directory left over in a server directory must not affect the scan.
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    write_metadata_sidecar(dir, &["valid_tool"]).await;

    fs::write(dir.join("index.ts"), "export * from './validTool';")
        .await
        .unwrap();
    fs::create_dir(dir.join("_runtime")).await.unwrap();
    fs::write(dir.join("_runtime/mcp-bridge.ts"), "// Bridge")
        .await
        .unwrap();
    fs::write(dir.join("readme.txt"), "Not a TypeScript file")
        .await
        .unwrap();

    let tools = scan_tools_directory(dir).await.unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "valid_tool");
}

#[tokio::test]
async fn test_build_skill_context_many_categories() {
    let tools: Vec<ParsedToolFile> = (0..20)
        .map(|i| ParsedToolFile {
            name: format!("tool_{i}"),
            typescript_name: format!("tool{i}"),
            server_id: "test".to_string(),
            category: Some(format!("category-{}", i % 5)), // 5 different categories
            keywords: vec![format!("keyword{i}")],
            description: Some(format!("Tool {i}")),
            parameters: vec![],
        })
        .collect();

    let context = build_skill_context("test", &tools, None);

    assert_eq!(context.tool_count, 20);
    assert_eq!(context.categories.len(), 5);

    // Verify each category has the right number of tools
    for category in &context.categories {
        assert_eq!(category.tools.len(), 4); // 20 tools / 5 categories = 4 per category
    }
}

#[tokio::test]
async fn test_build_skill_context_empty_tools() {
    let tools: Vec<ParsedToolFile> = vec![];

    let context = build_skill_context("test", &tools, None);

    assert_eq!(context.tool_count, 0);
    assert_eq!(context.categories.len(), 0);
    assert_eq!(context.example_tools.len(), 0);
    assert!(context.generation_prompt.contains('0')); // Total tools: 0
}

#[tokio::test]
async fn test_scan_directory_concurrent_access() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    let tool_names: Vec<String> = (0..10).map(|i| format!("tool_{i}")).collect();
    let tool_name_refs: Vec<&str> = tool_names.iter().map(String::as_str).collect();
    write_metadata_sidecar(dir, &tool_name_refs).await;

    // Scan the same directory concurrently
    let dir_path = dir.to_path_buf();
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let dir = dir_path.clone();
            tokio::spawn(async move { scan_tools_directory(&dir).await })
        })
        .collect();

    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 10);
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_scan_directory_permission_denied() {
    // This test is platform-specific and might not work on all systems
    // We'll skip it on Windows
    if cfg!(windows) {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let restricted_dir = temp_dir.path().join("restricted");
    fs::create_dir(&restricted_dir).await.unwrap();

    // Remove read permissions (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(&restricted_dir)
            .await
            .unwrap()
            .permissions();
        perms.set_mode(0o000);
        tokio::fs::set_permissions(&restricted_dir, perms)
            .await
            .unwrap();

        let result = scan_tools_directory(&restricted_dir).await;
        assert!(result.is_err());

        // Restore permissions for cleanup
        let mut perms = tokio::fs::metadata(&restricted_dir)
            .await
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        tokio::fs::set_permissions(&restricted_dir, perms)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_scan_directory_too_many_tools() {
    use mcp_execution_skill::MAX_TOOL_FILES;

    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    let tool_names: Vec<String> = (0..=MAX_TOOL_FILES).map(|i| format!("tool_{i}")).collect();
    let tool_name_refs: Vec<&str> = tool_names.iter().map(String::as_str).collect();
    write_metadata_sidecar(dir, &tool_name_refs).await;

    let result = scan_tools_directory(dir).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        ScanError::TooManyFiles { count, limit } => {
            assert_eq!(count, MAX_TOOL_FILES + 1);
            assert_eq!(limit, MAX_TOOL_FILES);
        }
        other => panic!("Expected TooManyFiles error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_scan_directory_file_too_large() {
    use mcp_execution_skill::MAX_FILE_SIZE;

    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create a sidecar larger than MAX_FILE_SIZE (1MB) by padding one tool's description.
    #[allow(clippy::cast_possible_truncation)]
    let large_description = "a".repeat((MAX_FILE_SIZE as usize) + 1);

    let mut meta = ServerMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        server_id: "test".to_string(),
        server_name: "Test Server".to_string(),
        server_version: "1.0.0".to_string(),
        tools: vec![ToolMetadata {
            name: "large_tool".to_string(),
            typescript_name: "largeTool".to_string(),
            category: None,
            keywords: vec!["large".to_string()],
            description: Some(String::new()),
            parameters: vec![],
        }],
    };
    meta.tools[0].description = Some(large_description);

    let content = serde_json::to_string_pretty(&meta).unwrap();
    fs::write(dir.join(METADATA_FILE_NAME), &content)
        .await
        .unwrap();

    let result = scan_tools_directory(dir).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        ScanError::FileTooLarge { path, size, limit } => {
            assert!(path.contains(mcp_execution_core::metadata::METADATA_FILE_NAME));
            assert!(size > MAX_FILE_SIZE);
            assert_eq!(limit, MAX_FILE_SIZE);
        }
        other => panic!("Expected FileTooLarge error, got: {other:?}"),
    }
}
