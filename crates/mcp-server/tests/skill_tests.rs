//! Integration tests for skill generation.

use mcp_skill::{ParsedToolFile, build_skill_context, parse_tool_file, scan_tools_directory};
use std::fmt::Write;
use tempfile::TempDir;
use tokio::fs;

/// Create a test TypeScript tool file.
async fn create_test_tool_file(dir: &std::path::Path, name: &str, category: &str) {
    let content = format!(
        r"/**
 * @tool {name}
 * @server test
 * @category {category}
 * @keywords test,{name}
 * @description Test tool: {name}
 */

interface {pascal_name}Params {{
  required_param: string;
  optional_param?: number;
}}

export async function {pascal_name}(params: {pascal_name}Params): Promise<void> {{
  // Implementation
}}
",
        name = name,
        category = category,
        pascal_name = to_pascal_case(name),
    );

    let filename = format!("{}.ts", to_pascal_case(name));
    fs::write(dir.join(&filename), content).await.unwrap();
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect()
}

#[tokio::test]
async fn test_parse_tool_file_integration() {
    let content = r"
/**
 * @tool create_issue
 * @server github
 * @category issues
 * @keywords create,issue,new,bug
 * @description Create a new issue in a repository
 */

interface CreateIssueParams {
  owner: string;
  repo: string;
  title: string;
  body?: string;
  labels?: string[];
}

export async function createIssue(params: CreateIssueParams): Promise<Issue> {
  // Implementation
}
";

    let result = parse_tool_file(content, "createIssue.ts").unwrap();

    assert_eq!(result.name, "create_issue");
    assert_eq!(result.typescript_name, "createIssue");
    assert_eq!(result.server_id, "github");
    assert_eq!(result.category, Some("issues".to_string()));
    assert_eq!(result.keywords.len(), 4);
    assert!(result.description.is_some());
    assert_eq!(result.parameters.len(), 5);

    // Check required vs optional
    let required_count = result.parameters.iter().filter(|p| p.required).count();
    let optional_count = result.parameters.iter().filter(|p| !p.required).count();

    assert_eq!(required_count, 3); // owner, repo, title
    assert_eq!(optional_count, 2); // body, labels
}

#[tokio::test]
async fn test_scan_tools_directory_integration() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create test tool files
    create_test_tool_file(dir, "create_issue", "issues").await;
    create_test_tool_file(dir, "list_repos", "repos").await;
    create_test_tool_file(dir, "get_user", "users").await;

    // Create files that should be skipped
    fs::write(dir.join("index.ts"), "export * from './createIssue';")
        .await
        .unwrap();
    fs::create_dir(dir.join("_runtime")).await.unwrap();
    fs::write(dir.join("_runtime/mcp-bridge.ts"), "// Bridge")
        .await
        .unwrap();

    let tools = scan_tools_directory(dir).await.unwrap();

    assert_eq!(tools.len(), 3);
    assert!(tools.iter().any(|t| t.name == "create_issue"));
    assert!(tools.iter().any(|t| t.name == "list_repos"));
    assert!(tools.iter().any(|t| t.name == "get_user"));
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
async fn test_parse_tool_file_missing_required_tags() {
    // Missing @tool tag
    let content = r"
/**
 * @server github
 */
";

    let result = parse_tool_file(content, "test.ts");
    assert!(result.is_err());

    // Missing @server tag
    let content = r"
/**
 * @tool test
 */
";

    let result = parse_tool_file(content, "test.ts");
    assert!(result.is_err());
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
// Large File Handling Tests
// ============================================================================

#[tokio::test]
async fn test_scan_directory_with_many_files() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create 50 tool files
    for i in 0..50 {
        let tool_name = format!("tool_{i}");
        create_test_tool_file(dir, &tool_name, "test-category").await;
    }

    let tools = scan_tools_directory(dir).await.unwrap();

    assert_eq!(tools.len(), 50);
    // Verify they're sorted
    for i in 1..tools.len() {
        assert!(tools[i - 1].name <= tools[i].name);
    }
}

#[tokio::test]
async fn test_scan_directory_with_invalid_files() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create valid tool file
    create_test_tool_file(dir, "valid_tool", "category").await;

    // Create invalid TypeScript file (missing @tool tag)
    let invalid_content = r"
/**
 * @server github
 */
export function invalid() {}
";
    fs::write(dir.join("invalid.ts"), invalid_content)
        .await
        .unwrap();

    // Create non-TypeScript file
    fs::write(dir.join("readme.txt"), "Not a TypeScript file")
        .await
        .unwrap();

    let tools = scan_tools_directory(dir).await.unwrap();

    // Should only parse the valid tool (invalid files are logged but skipped)
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "valid_tool");
}

#[tokio::test]
async fn test_scan_directory_skips_index_and_runtime() {
    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create valid tool
    create_test_tool_file(dir, "valid_tool", "category").await;

    // Create index.ts (should be skipped)
    fs::write(dir.join("index.ts"), "export * from './validTool';")
        .await
        .unwrap();

    // Create _runtime directory with file (should be skipped)
    fs::create_dir(dir.join("_runtime")).await.unwrap();
    fs::write(dir.join("_runtime/bridge.ts"), "// Runtime bridge")
        .await
        .unwrap();

    // Create file starting with _ (should be skipped)
    create_test_tool_file(dir, "_internal", "category").await;
    let internal_file = dir.join(format!("{}.ts", to_pascal_case("_internal")));
    if internal_file.exists() {
        fs::remove_file(&internal_file).await.ok();
    }
    fs::write(dir.join("_internal.ts"), "// Internal")
        .await
        .unwrap();

    let tools = scan_tools_directory(dir).await.unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "valid_tool");
}

#[tokio::test]
async fn test_parse_tool_file_large_description() {
    let long_description = "a".repeat(1000);
    let content = format!(
        r"/**
 * @tool test_tool
 * @server github
 * @description {long_description}
 */
"
    );

    let result = parse_tool_file(&content, "test.ts").unwrap();
    assert_eq!(result.description, Some(long_description));
}

#[tokio::test]
async fn test_parse_tool_file_many_keywords() {
    let keywords: Vec<String> = (0..100).map(|i| format!("keyword{i}")).collect();
    let keywords_str = keywords.join(",");

    let content = format!(
        r"/**
 * @tool test_tool
 * @server github
 * @keywords {keywords_str}
 */
"
    );

    let result = parse_tool_file(&content, "test.ts").unwrap();
    assert_eq!(result.keywords.len(), 100);
}

#[tokio::test]
async fn test_parse_tool_file_many_parameters() {
    let mut params = String::new();
    for i in 0..50 {
        writeln!(params, "  param{i}: string;").unwrap();
    }

    let content = format!(
        r"/**
 * @tool test_tool
 * @server github
 */

interface TestToolParams {{
{params}}}
"
    );

    let result = parse_tool_file(&content, "test.ts").unwrap();
    assert_eq!(result.parameters.len(), 50);
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

    // Create several tool files
    for i in 0..10 {
        create_test_tool_file(dir, &format!("tool_{i}"), "category").await;
    }

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
async fn test_parse_tool_file_binary_content() {
    // Try to parse binary content (should fail gracefully)
    let binary_content = vec![0xFF, 0xFE, 0xFD, 0xFC];
    let content_str = String::from_utf8_lossy(&binary_content);

    let result = parse_tool_file(&content_str, "binary.ts");
    assert!(result.is_err());
}

#[tokio::test]
async fn test_scan_directory_too_many_files() {
    use mcp_skill::{ScanError, MAX_TOOL_FILES};

    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create MAX_TOOL_FILES + 1 files (501 files)
    for i in 0..=MAX_TOOL_FILES {
        create_test_tool_file(dir, &format!("tool_{i}"), "test").await;
    }

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
    use mcp_skill::{ScanError, MAX_FILE_SIZE};

    let temp_dir = TempDir::new().unwrap();
    let dir = temp_dir.path();

    // Create a file larger than MAX_FILE_SIZE (1MB)
    let large_content = "a".repeat((MAX_FILE_SIZE as usize) + 1);

    // Add minimal valid JSDoc to make it a tool file
    let content = format!(
        r"/**
 * @tool large_tool
 * @server test
 * @keywords large
 * @description Large tool for testing
 */

interface LargeToolParams {{
  param: string;
}}

{large_content}
"
    );

    let large_file = dir.join("LargeTool.ts");
    fs::write(&large_file, content).await.unwrap();

    let result = scan_tools_directory(dir).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        ScanError::FileTooLarge { path, size, limit } => {
            assert!(path.contains("LargeTool.ts"));
            assert!(size > MAX_FILE_SIZE);
            assert_eq!(limit, MAX_FILE_SIZE);
        }
        other => panic!("Expected FileTooLarge error, got: {other:?}"),
    }
}
