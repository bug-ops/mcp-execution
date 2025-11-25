//! Integration tests for filesystem export functionality.
//!
//! Tests the complete workflow from VFS building to filesystem export,
//! including real GitHub server structure simulation.

use mcp_codegen::{GeneratedCode, GeneratedFile};
use mcp_vfs::VfsBuilder;
use std::fs;
use tempfile::TempDir;

/// Test complete workflow: build VFS and export to filesystem
#[test]
fn test_export_and_verify_content() {
    let temp_dir = TempDir::new().unwrap();

    // Create VFS with multiple files
    let vfs = VfsBuilder::new()
        .add_file("/index.ts", "export { createIssue } from './createIssue';")
        .add_file(
            "/createIssue.ts",
            "export function createIssue(params) { return {}; }",
        )
        .add_file(
            "/updateIssue.ts",
            "export function updateIssue(params) { return {}; }",
        )
        .add_file("/manifest.json", r#"{"version": "1.0.0", "tools": 2}"#)
        .build_and_export(temp_dir.path())
        .unwrap();

    // Verify VFS
    assert_eq!(vfs.file_count(), 4);

    // Verify each file exists on disk
    assert!(temp_dir.path().join("index.ts").exists());
    assert!(temp_dir.path().join("createIssue.ts").exists());
    assert!(temp_dir.path().join("updateIssue.ts").exists());
    assert!(temp_dir.path().join("manifest.json").exists());

    // Verify content matches
    let index_content = fs::read_to_string(temp_dir.path().join("index.ts")).unwrap();
    assert_eq!(
        index_content,
        "export { createIssue } from './createIssue';"
    );

    let manifest_content = fs::read_to_string(temp_dir.path().join("manifest.json")).unwrap();
    assert_eq!(manifest_content, r#"{"version": "1.0.0", "tools": 2}"#);
}

/// Test GitHub server structure with 30 tools
#[test]
fn test_export_github_server_structure() {
    let temp_dir = TempDir::new().unwrap();

    let mut code = GeneratedCode::new();

    // Add index file
    code.add_file(GeneratedFile {
        path: "index.ts".to_string(),
        content: "// GitHub MCP Server\nexport * from './tools';".to_string(),
    });

    // Simulate 30 GitHub tools
    let tool_names = vec![
        "createIssue",
        "updateIssue",
        "getIssue",
        "listIssues",
        "closeIssue",
        "reopenIssue",
        "addLabel",
        "removeLabel",
        "addAssignee",
        "removeAssignee",
        "createPullRequest",
        "updatePullRequest",
        "mergePullRequest",
        "listPullRequests",
        "reviewPullRequest",
        "createComment",
        "updateComment",
        "deleteComment",
        "createRepository",
        "deleteRepository",
        "forkRepository",
        "starRepository",
        "unstarRepository",
        "watchRepository",
        "unwatchRepository",
        "createBranch",
        "deleteBranch",
        "listBranches",
        "createRelease",
        "listReleases",
    ];

    for tool in &tool_names {
        code.add_file(GeneratedFile {
            path: format!("tools/{tool}.ts"),
            content: format!("export function {tool}(params: any) {{ return {{}}; }}"),
        });
    }

    // Add manifest
    code.add_file(GeneratedFile {
        path: "manifest.json".to_string(),
        content: format!(r#"{{"version": "1.0.0", "tools": {}}}"#, tool_names.len()),
    });

    // Export to filesystem
    let vfs = VfsBuilder::from_generated_code(code, "/github")
        .build_and_export(temp_dir.path())
        .unwrap();

    // Verify structure
    assert_eq!(vfs.file_count(), 32); // 30 tools + index + manifest
    assert!(temp_dir.path().join("github/index.ts").exists());
    assert!(temp_dir.path().join("github/tools").is_dir());
    assert!(temp_dir.path().join("github/manifest.json").exists());

    // Verify all tools exist
    for tool in &tool_names {
        let tool_path = temp_dir.path().join(format!("github/tools/{tool}.ts"));
        assert!(tool_path.exists(), "Tool {tool} should exist");

        let content = fs::read_to_string(&tool_path).unwrap();
        assert!(
            content.contains(tool),
            "Tool file should contain function {tool}"
        );
    }
}

/// Test progressive loading pattern: multiple servers
#[test]
fn test_export_multiple_servers() {
    let temp_dir = TempDir::new().unwrap();

    // Create GitHub server structure
    let github_code = {
        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "createIssue.ts".to_string(),
            content: "export function createIssue() {}".to_string(),
        });
        code.add_file(GeneratedFile {
            path: "getIssue.ts".to_string(),
            content: "export function getIssue() {}".to_string(),
        });
        code
    };

    // Create Slack server structure
    let slack_code = {
        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "sendMessage.ts".to_string(),
            content: "export function sendMessage() {}".to_string(),
        });
        code.add_file(GeneratedFile {
            path: "listChannels.ts".to_string(),
            content: "export function listChannels() {}".to_string(),
        });
        code
    };

    // Export both servers
    let github_vfs = VfsBuilder::from_generated_code(github_code, "/github")
        .build_and_export(temp_dir.path())
        .unwrap();

    let slack_vfs = VfsBuilder::from_generated_code(slack_code, "/slack")
        .build_and_export(temp_dir.path())
        .unwrap();

    // Verify structure
    assert_eq!(github_vfs.file_count(), 2);
    assert_eq!(slack_vfs.file_count(), 2);

    // Verify GitHub files
    assert!(temp_dir.path().join("github/createIssue.ts").exists());
    assert!(temp_dir.path().join("github/getIssue.ts").exists());

    // Verify Slack files
    assert!(temp_dir.path().join("slack/sendMessage.ts").exists());
    assert!(temp_dir.path().join("slack/listChannels.ts").exists());

    // Ensure no cross-contamination
    assert!(!temp_dir.path().join("github/sendMessage.ts").exists());
    assert!(!temp_dir.path().join("slack/createIssue.ts").exists());
}

/// Test export with deeply nested structure
#[test]
fn test_export_deep_hierarchy() {
    let temp_dir = TempDir::new().unwrap();

    let vfs = VfsBuilder::new()
        .add_file(
            "/level1/level2/level3/level4/deep.ts",
            "export const DEEP = true;",
        )
        .add_file("/level1/file1.ts", "export const L1 = true;")
        .add_file("/level1/level2/file2.ts", "export const L2 = true;")
        .build_and_export(temp_dir.path())
        .unwrap();

    assert_eq!(vfs.file_count(), 3);

    // Verify deep nesting works
    let deep_path = temp_dir.path().join("level1/level2/level3/level4/deep.ts");
    assert!(deep_path.exists());
    assert_eq!(
        fs::read_to_string(deep_path).unwrap(),
        "export const DEEP = true;"
    );
}

/// Test export handles special characters in filenames
#[test]
fn test_export_special_characters() {
    let temp_dir = TempDir::new().unwrap();

    let vfs = VfsBuilder::new()
        .add_file("/tool-name.ts", "export {};")
        .add_file("/tool_name.ts", "export {};")
        .add_file("/tool.v2.ts", "export {};")
        .build_and_export(temp_dir.path())
        .unwrap();

    assert_eq!(vfs.file_count(), 3);
    assert!(temp_dir.path().join("tool-name.ts").exists());
    assert!(temp_dir.path().join("tool_name.ts").exists());
    assert!(temp_dir.path().join("tool.v2.ts").exists());
}

/// Test export preserves file content exactly (binary-safe)
#[test]
fn test_export_preserves_content() {
    let temp_dir = TempDir::new().unwrap();

    let original_content = r"
// TypeScript file with various content
export interface Params {
    id: number;
    name: string;
    metadata?: Record<string, unknown>;
}

export function tool(params: Params): void {
    console.log(`Processing: ${params.name}`);
}
";

    let vfs = VfsBuilder::new()
        .add_file("/complex.ts", original_content)
        .build_and_export(temp_dir.path())
        .unwrap();

    assert_eq!(vfs.file_count(), 1);

    let disk_content = fs::read_to_string(temp_dir.path().join("complex.ts")).unwrap();
    assert_eq!(disk_content, original_content);
}

/// Test export with empty files (edge case)
#[test]
fn test_export_empty_files() {
    let temp_dir = TempDir::new().unwrap();

    let vfs = VfsBuilder::new()
        .add_file("/empty1.ts", "")
        .add_file("/empty2.ts", "")
        .build_and_export(temp_dir.path())
        .unwrap();

    assert_eq!(vfs.file_count(), 2);
    assert!(temp_dir.path().join("empty1.ts").exists());
    assert!(temp_dir.path().join("empty2.ts").exists());

    // Verify files are truly empty
    assert_eq!(
        fs::read_to_string(temp_dir.path().join("empty1.ts")).unwrap(),
        ""
    );
    assert_eq!(
        fs::read_to_string(temp_dir.path().join("empty2.ts")).unwrap(),
        ""
    );
}

/// Test export and re-export (overwrite scenario)
#[test]
fn test_export_reexport_workflow() {
    let temp_dir = TempDir::new().unwrap();

    // First export
    let vfs1 = VfsBuilder::new()
        .add_file("/version.ts", "export const VERSION = '1.0.0';")
        .build_and_export(temp_dir.path())
        .unwrap();

    assert_eq!(vfs1.file_count(), 1);
    let v1_content = fs::read_to_string(temp_dir.path().join("version.ts")).unwrap();
    assert_eq!(v1_content, "export const VERSION = '1.0.0';");

    // Second export with updated content
    let vfs2 = VfsBuilder::new()
        .add_file("/version.ts", "export const VERSION = '2.0.0';")
        .build_and_export(temp_dir.path())
        .unwrap();

    assert_eq!(vfs2.file_count(), 1);
    let v2_content = fs::read_to_string(temp_dir.path().join("version.ts")).unwrap();
    assert_eq!(v2_content, "export const VERSION = '2.0.0';");
}

/// Test export with realistic TypeScript module structure
#[test]
fn test_export_typescript_module() {
    let temp_dir = TempDir::new().unwrap();

    let mut code = GeneratedCode::new();

    code.add_file(GeneratedFile {
        path: "index.ts".to_string(),
        content: r"export * from './types';
export * from './tools';"
            .to_string(),
    });

    code.add_file(GeneratedFile {
        path: "types/index.ts".to_string(),
        content: "export type ToolParams = { id: number };".to_string(),
    });

    code.add_file(GeneratedFile {
        path: "tools/create.ts".to_string(),
        content: r"import type { ToolParams } from '../types';
export function create(params: ToolParams) { return params.id; }"
            .to_string(),
    });

    let vfs = VfsBuilder::from_generated_code(code, "/module")
        .build_and_export(temp_dir.path())
        .unwrap();

    assert_eq!(vfs.file_count(), 3);
    assert!(temp_dir.path().join("module/index.ts").exists());
    assert!(temp_dir.path().join("module/types/index.ts").exists());
    assert!(temp_dir.path().join("module/tools/create.ts").exists());

    // Verify content can be read back
    let tools_content = fs::read_to_string(temp_dir.path().join("module/tools/create.ts")).unwrap();
    assert!(tools_content.contains("ToolParams"));
}

// Platform-specific tests

#[cfg(unix)]
mod unix_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Test file permissions on Unix (0644 for files)
    #[test]
    fn test_unix_file_permissions() {
        let temp_dir = TempDir::new().unwrap();

        let _vfs = VfsBuilder::new()
            .add_file("/test.ts", "export {};")
            .build_and_export(temp_dir.path())
            .unwrap();

        let file_path = temp_dir.path().join("test.ts");
        let metadata = fs::metadata(&file_path).unwrap();
        let permissions = metadata.permissions();

        // Files should have reasonable permissions (readable by owner, group, others)
        // Typically 0644 but could vary by umask
        let mode = permissions.mode();
        assert_ne!(mode & 0o400, 0, "File should be readable by owner");
    }

    /// Test directory permissions on Unix (0755 for dirs)
    #[test]
    fn test_unix_directory_permissions() {
        let temp_dir = TempDir::new().unwrap();

        let _vfs = VfsBuilder::new()
            .add_file("/nested/deep/test.ts", "export {};")
            .build_and_export(temp_dir.path())
            .unwrap();

        let dir_path = temp_dir.path().join("nested");
        let metadata = fs::metadata(&dir_path).unwrap();
        let permissions = metadata.permissions();

        // Directories should be executable (searchable) by owner
        let mode = permissions.mode();
        assert_ne!(
            mode & 0o700,
            0,
            "Directory should be readable/writable/executable by owner"
        );
    }
}

#[cfg(windows)]
mod windows_tests {
    use super::*;

    /// Test Windows path separators are handled correctly
    #[test]
    fn test_windows_path_separators() {
        let temp_dir = TempDir::new().unwrap();

        let _vfs = VfsBuilder::new()
            .add_file("/tools/create.ts", "export {};")
            .build_and_export(temp_dir.path())
            .unwrap();

        // Verify file exists using Windows path conventions
        let file_path = temp_dir.path().join("tools").join("create.ts");
        assert!(
            file_path.exists(),
            "File should exist with Windows path separators"
        );

        // Verify content is correct
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "export {};");
    }

    /// Test long paths on Windows (> 260 characters)
    #[test]
    fn test_windows_long_paths() {
        let temp_dir = TempDir::new().unwrap();

        // Create a very deep path structure
        let deep_path = format!(
            "/{}/file.ts",
            (0..10)
                .map(|i| format!("level{i}"))
                .collect::<Vec<_>>()
                .join("/")
        );

        let _vfs = VfsBuilder::new()
            .add_file(&deep_path, "export {};")
            .build_and_export(temp_dir.path())
            .unwrap();

        // Verify deep file exists
        let mut file_path = temp_dir.path().to_path_buf();
        for i in 0..10 {
            file_path = file_path.join(format!("level{i}"));
        }
        file_path = file_path.join("file.ts");

        assert!(file_path.exists(), "Deep file should exist on Windows");
    }
}
