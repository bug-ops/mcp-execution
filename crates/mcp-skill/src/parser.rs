//! Server metadata sidecar reader.
//!
//! Reads the structured `_meta.json` sidecar emitted by
//! `mcp-execution-codegen` alongside a server's generated TypeScript tool
//! files, and maps it into this crate's [`ParsedToolFile`] / [`ParsedParameter`]
//! types for skill generation.
//!
//! Prior to this module, tool metadata was recovered by re-parsing the
//! generated `.ts` files with regexes — a lossy, fragile round-trip that,
//! among other issues, could never recover parameter descriptions. The
//! sidecar is a structured, serde-derived contract shared with codegen via
//! `mcp_execution_core::metadata`, so no re-parsing of generated source is
//! needed at all.

use mcp_execution_core::metadata::{METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ServerMetadata};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use thiserror::Error;

/// Maximum number of tools accepted from a single sidecar (denial-of-service protection).
pub const MAX_TOOL_FILES: usize = 500;

/// Maximum sidecar file size to read in bytes (1MB).
pub const MAX_FILE_SIZE: u64 = 1024 * 1024;

// Regexes for SKILL.md frontmatter parsing
static FRONTMATTER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^---\s*\n([\s\S]*?)\n---").expect("valid regex"));
static NAME_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"name:\s*(.+)").expect("valid regex"));
static SKILL_DESC_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"description:\s*(.+)").expect("valid regex"));

/// Sanitize file path for error messages to prevent information disclosure.
///
/// Replaces the home directory with `~` to avoid leaking usernames and
/// full filesystem paths in error messages.
fn sanitize_path_for_error(path: &Path) -> String {
    dirs::home_dir().map_or_else(
        || path.display().to_string(),
        |home| {
            let path_str = path.display().to_string();
            path_str.replace(&home.display().to_string(), "~")
        },
    )
}

/// Errors that can occur while scanning a server directory for its `_meta.json` sidecar.
#[derive(Debug, Error)]
pub enum ScanError {
    /// I/O error reading the directory or sidecar file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Directory does not exist.
    #[error("directory does not exist: {path}")]
    DirectoryNotFound { path: String },

    /// The `_meta.json` sidecar is missing from the server directory.
    #[error("metadata sidecar not found: {path} (was the server directory regenerated?)")]
    MissingMetadata { path: String },

    /// The `_meta.json` sidecar could not be parsed as valid `ServerMetadata` JSON.
    #[error("failed to parse metadata sidecar {path}: {source}")]
    MetadataParse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    /// The sidecar's `schema_version` does not match the version this crate understands.
    #[error("unsupported metadata schema version: found {found}, expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },

    /// Too many tools in the sidecar (denial-of-service protection).
    #[error("too many tools: {count} exceeds limit of {limit}")]
    TooManyFiles { count: usize, limit: usize },

    /// Sidecar file too large to process.
    #[error("file too large: {path} ({size} bytes exceeds {limit} limit)")]
    FileTooLarge { path: String, size: u64, limit: u64 },

    /// A tool listed in the `_meta.json` sidecar has no corresponding `.ts`
    /// file on disk.
    ///
    /// This indicates the sidecar and the generated TypeScript files have
    /// drifted apart — e.g. the file was deleted manually, or a `generate`
    /// run was interrupted before writing it.
    #[error(
        "stale metadata: tool '{tool}' is listed in {sidecar_path} but its file '{expected_file}' \
         is missing (re-run 'generate' to regenerate this server)"
    )]
    StaleMetadata {
        tool: String,
        expected_file: String,
        sidecar_path: String,
    },
}

/// Parsed metadata from a server's generated tool set.
#[derive(Debug, Clone)]
pub struct ParsedToolFile {
    /// Original MCP tool name.
    pub name: String,

    /// TypeScript function name (`PascalCase` filename).
    pub typescript_name: String,

    /// Server identifier.
    pub server_id: String,

    /// Category for grouping.
    pub category: Option<String>,

    /// Keywords for discovery.
    pub keywords: Vec<String>,

    /// Tool description.
    pub description: Option<String>,

    /// Parsed parameters for the tool.
    pub parameters: Vec<ParsedParameter>,
}

/// A parsed parameter from a tool's metadata.
#[derive(Debug, Clone)]
pub struct ParsedParameter {
    /// Parameter name.
    pub name: String,

    /// TypeScript type (e.g., "string", "number", "boolean").
    pub typescript_type: String,

    /// Whether the parameter is required.
    pub required: bool,

    /// Parameter description.
    pub description: Option<String>,
}

impl From<mcp_execution_core::metadata::ParameterMetadata> for ParsedParameter {
    fn from(meta: mcp_execution_core::metadata::ParameterMetadata) -> Self {
        Self {
            name: meta.name,
            typescript_type: meta.typescript_type,
            required: meta.required,
            description: meta.description,
        }
    }
}

impl From<mcp_execution_core::metadata::ToolMetadata> for ParsedToolFile {
    fn from(meta: mcp_execution_core::metadata::ToolMetadata) -> Self {
        Self {
            name: meta.name,
            typescript_name: meta.typescript_name,
            server_id: String::new(),
            category: meta.category,
            keywords: meta.keywords,
            description: meta.description,
            parameters: meta.parameters.into_iter().map(Into::into).collect(),
        }
    }
}

/// Scan a server directory and read its `_meta.json` sidecar.
///
/// Reads the structured metadata sidecar written by `mcp-execution-codegen`
/// and maps each tool entry into a [`ParsedToolFile`]. Unlike the former
/// regex-based `.ts` scanner, tool metadata (name, category, keywords,
/// parameters) is never re-parsed from generated TypeScript source — the
/// sidecar remains the single source of truth for that. However, each
/// sidecar entry's `.ts` file is cross-checked for existence on disk to
/// detect drift between the sidecar and the generated files (see issues
/// #154, #155): a missing file is a hard error, while an unreferenced `.ts`
/// file on disk is logged via `tracing::warn!` and omitted from the result.
///
/// # Arguments
///
/// * `dir` - Path to server directory (e.g., `~/.claude/servers/github`)
///
/// # Returns
///
/// Vector of `ParsedToolFile`, one per tool in the sidecar, sorted by name.
///
/// # Errors
///
/// Returns `ScanError` if the directory doesn't exist, the sidecar is
/// missing or malformed, the sidecar's tool count exceeds
/// [`MAX_TOOL_FILES`], or a sidecar entry's `.ts` file is missing from disk
/// ([`ScanError::StaleMetadata`]).
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::scan_tools_directory;
/// use std::path::Path;
///
/// # async fn example() -> Result<(), mcp_execution_skill::ScanError> {
/// let tools = scan_tools_directory(Path::new("/home/user/.claude/servers/github")).await?;
/// println!("Found {} tools", tools.len());
/// # Ok(())
/// # }
/// ```
pub async fn scan_tools_directory(dir: &Path) -> Result<Vec<ParsedToolFile>, ScanError> {
    // Canonicalize the base directory to resolve symlinks and get absolute path
    let canonical_base =
        tokio::fs::canonicalize(dir)
            .await
            .map_err(|_| ScanError::DirectoryNotFound {
                path: sanitize_path_for_error(dir),
            })?;

    let meta_path = canonical_base.join(METADATA_FILE_NAME);

    // SECURITY: Canonicalize the sidecar path and validate it stays within the base
    // directory, preventing path traversal via a symlinked `_meta.json`.
    let canonical_meta = match tokio::fs::canonicalize(&meta_path).await {
        Ok(path) if path.starts_with(&canonical_base) => path,
        _ => {
            return Err(ScanError::MissingMetadata {
                path: sanitize_path_for_error(&meta_path),
            });
        }
    };

    let file_metadata = tokio::fs::metadata(&canonical_meta).await?;
    if file_metadata.len() > MAX_FILE_SIZE {
        return Err(ScanError::FileTooLarge {
            path: sanitize_path_for_error(&meta_path),
            size: file_metadata.len(),
            limit: MAX_FILE_SIZE,
        });
    }

    let content = tokio::fs::read_to_string(&canonical_meta).await?;

    let meta: ServerMetadata =
        serde_json::from_str(&content).map_err(|source| ScanError::MetadataParse {
            path: sanitize_path_for_error(&meta_path),
            source,
        })?;

    if meta.schema_version != METADATA_SCHEMA_VERSION {
        return Err(ScanError::UnsupportedSchema {
            found: meta.schema_version,
            expected: METADATA_SCHEMA_VERSION,
        });
    }

    if meta.tools.len() > MAX_TOOL_FILES {
        return Err(ScanError::TooManyFiles {
            count: meta.tools.len(),
            limit: MAX_TOOL_FILES,
        });
    }

    verify_tool_files_on_disk(&canonical_base, &meta.tools, &meta_path).await?;

    let server_id = meta.server_id.clone();
    let mut tools: Vec<ParsedToolFile> = meta
        .tools
        .into_iter()
        .map(|tool| {
            let mut parsed: ParsedToolFile = tool.into();
            parsed.server_id.clone_from(&server_id);
            parsed
        })
        .collect();

    // Sort by name for consistent ordering
    tools.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(tools)
}

/// Cross-checks sidecar tool entries against the `.ts` files actually
/// present in `dir`, guarding against drift between `_meta.json` and the
/// generated TypeScript output (see issues #154, #155).
///
/// Every sidecar entry must have a matching `{typescript_name}.ts` file, or
/// this returns [`ScanError::StaleMetadata`]. `.ts` files present on disk
/// but not referenced by the sidecar are not fatal — regenerating tool
/// files is a normal part of `generate` — but are logged via
/// `tracing::warn!` so the drift isn't silently dropped from `SKILL.md`.
///
/// # Errors
///
/// Returns `ScanError::Io` if the directory cannot be read, or
/// `ScanError::StaleMetadata` if a sidecar entry's `.ts` file is missing.
async fn verify_tool_files_on_disk(
    dir: &Path,
    tools: &[mcp_execution_core::metadata::ToolMetadata],
    meta_path: &Path,
) -> Result<(), ScanError> {
    // Generated aggregator file, not a per-tool file — never expected in the sidecar.
    const INDEX_FILE_NAME: &str = "index.ts";

    let mut expected_files: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(tools.len());

    for tool in tools {
        let file_name = format!("{}.ts", tool.typescript_name);
        if !dir.join(&file_name).is_file() {
            return Err(ScanError::StaleMetadata {
                tool: tool.name.clone(),
                expected_file: file_name,
                sidecar_path: sanitize_path_for_error(meta_path),
            });
        }
        expected_files.insert(file_name);
    }

    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("ts") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
            continue;
        };
        if file_name == INDEX_FILE_NAME || expected_files.contains(file_name) {
            continue;
        }
        tracing::warn!(
            file = %file_name,
            "found .ts tool file not referenced by _meta.json; it will be omitted from SKILL.md \
             (re-run 'generate' to refresh the sidecar)"
        );
    }

    Ok(())
}

/// Extract skill metadata from SKILL.md content.
///
/// Parses YAML frontmatter to extract name and description, and counts
/// sections (H2 headers) and words.
///
/// # Arguments
///
/// * `content` - SKILL.md content with YAML frontmatter
///
/// # Returns
///
/// `SkillMetadata` with extracted information.
///
/// # Errors
///
/// Returns error if YAML frontmatter is missing or required fields not found.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::extract_skill_metadata;
///
/// let content = r"---
/// name: github-progressive
/// description: GitHub MCP server operations
/// ---
///
/// # GitHub Progressive
///
/// ## Quick Start
///
/// Content here.
/// ";
///
/// let metadata = extract_skill_metadata(content).unwrap();
/// assert_eq!(metadata.name, "github-progressive");
/// assert_eq!(metadata.description, "GitHub MCP server operations");
/// ```
pub fn extract_skill_metadata(content: &str) -> Result<crate::types::SkillMetadata, String> {
    use crate::types::SkillMetadata;

    // Extract YAML frontmatter (using pre-compiled regex)
    let frontmatter = FRONTMATTER_REGEX
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or("YAML frontmatter not found")?;

    // Extract name (using pre-compiled regex)
    let name = NAME_REGEX
        .captures(frontmatter)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .ok_or("'name' field not found in frontmatter")?;

    // Extract description (using pre-compiled regex)
    let description = SKILL_DESC_REGEX
        .captures(frontmatter)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .ok_or("'description' field not found in frontmatter")?;

    // Count sections (H2 headers)
    let section_count = content.lines().filter(|l| l.starts_with("## ")).count();

    // Count words (approximate)
    let word_count = content.split_whitespace().count();

    Ok(SkillMetadata {
        name,
        description,
        section_count,
        word_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::metadata::{ParameterMetadata, ToolMetadata};
    use tempfile::TempDir;

    fn sample_metadata(tool_count: usize) -> ServerMetadata {
        ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: "github".to_string(),
            server_name: "GitHub".to_string(),
            server_version: "1.0.0".to_string(),
            tools: (0..tool_count)
                .map(|i| ToolMetadata {
                    name: format!("tool_{i}"),
                    typescript_name: format!("tool{i}"),
                    category: Some("test".to_string()),
                    keywords: vec!["test".to_string()],
                    description: Some(format!("Tool {i}")),
                    parameters: vec![ParameterMetadata {
                        name: "param".to_string(),
                        typescript_type: "string".to_string(),
                        required: true,
                        description: Some("A parameter".to_string()),
                    }],
                })
                .collect(),
        }
    }

    /// Writes `_meta.json` plus a matching stub `.ts` file for each tool, since
    /// `scan_tools_directory` cross-checks the sidecar against files on disk.
    async fn write_metadata(dir: &Path, meta: &ServerMetadata) {
        let content = serde_json::to_string_pretty(meta).unwrap();
        tokio::fs::write(dir.join(METADATA_FILE_NAME), content)
            .await
            .unwrap();

        for tool in &meta.tools {
            tokio::fs::write(
                dir.join(format!("{}.ts", tool.typescript_name)),
                "export {}",
            )
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_round_trip_preserves_parameter_descriptions() {
        // Issue #141 regression: the old regex-based parser hard-coded parameter
        // descriptions to `None`. The sidecar-backed scanner must preserve them.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(2);
        write_metadata(temp_dir.path(), &meta).await;

        let tools = scan_tools_directory(temp_dir.path()).await.unwrap();

        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "tool_0");
        assert_eq!(tools[0].server_id, "github");
        assert_eq!(tools[0].parameters.len(), 1);
        assert_eq!(
            tools[0].parameters[0].description,
            Some("A parameter".to_string()),
            "parameter descriptions must survive the sidecar round-trip"
        );
    }

    #[tokio::test]
    async fn test_scan_tools_directory_sorts_by_name() {
        let temp_dir = TempDir::new().unwrap();
        let mut meta = sample_metadata(0);
        meta.tools = vec![
            ToolMetadata {
                name: "zebra".to_string(),
                typescript_name: "zebra".to_string(),
                category: None,
                keywords: vec![],
                description: None,
                parameters: vec![],
            },
            ToolMetadata {
                name: "alpha".to_string(),
                typescript_name: "alpha".to_string(),
                category: None,
                keywords: vec![],
                description: None,
                parameters: vec![],
            },
        ];
        write_metadata(temp_dir.path(), &meta).await;

        let tools = scan_tools_directory(temp_dir.path()).await.unwrap();

        assert_eq!(tools[0].name, "alpha");
        assert_eq!(tools[1].name, "zebra");
    }

    #[tokio::test]
    async fn test_scan_tools_directory_stale_metadata_missing_ts_file() {
        // Issue #154/#155 regression: a sidecar entry whose `.ts` file was
        // deleted (or never written, e.g. an interrupted `generate`) must be
        // reported instead of silently vanishing from `SKILL.md`.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(1);
        // Write only the sidecar, not the tool's `.ts` file.
        let content = serde_json::to_string_pretty(&meta).unwrap();
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), content)
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::StaleMetadata {
                tool,
                expected_file,
                ..
            }) => {
                assert_eq!(tool, "tool_0");
                assert_eq!(expected_file, "tool0.ts");
            }
            other => panic!("expected StaleMetadata, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_stale_metadata_reports_first_missing_in_sidecar_order() {
        // With multiple tools in the sidecar, only some of which have a missing
        // `.ts` file, the check short-circuits on the first missing entry in
        // sidecar order rather than scanning every tool up front.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(3);
        let content = serde_json::to_string_pretty(&meta).unwrap();
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), content)
            .await
            .unwrap();

        // Only write the `.ts` file for the middle tool; `tool_0` and `tool_2`
        // are both missing, but `tool_0` is first in sidecar order.
        tokio::fs::write(temp_dir.path().join("tool1.ts"), "export {}")
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::StaleMetadata {
                tool,
                expected_file,
                ..
            }) => {
                assert_eq!(tool, "tool_0");
                assert_eq!(expected_file, "tool0.ts");
            }
            other => panic!("expected StaleMetadata for tool_0, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_extra_ts_file_excluded_from_result() {
        // Issue #154/#155 regression: a `.ts` file on disk that the sidecar
        // does not reference (e.g. left over from a renamed/removed tool) must
        // not be fatal and must not leak into the scan result — it is logged
        // via `tracing::warn!` instead.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(1);
        write_metadata(temp_dir.path(), &meta).await;

        tokio::fs::write(temp_dir.path().join("orphan.ts"), "export {}")
            .await
            .unwrap();

        let tools = scan_tools_directory(temp_dir.path()).await.unwrap();

        assert_eq!(
            tools.len(),
            1,
            "the orphaned .ts file must not be reported as a tool"
        );
        assert_eq!(tools[0].name, "tool_0");
    }

    #[tokio::test]
    async fn test_scan_tools_directory_index_ts_not_treated_as_extra() {
        // `index.ts` is the generated aggregator file and is never listed in
        // the sidecar; its presence alone must not affect the scan result.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(1);
        write_metadata(temp_dir.path(), &meta).await;

        tokio::fs::write(temp_dir.path().join("index.ts"), "export * from './tool0';")
            .await
            .unwrap();

        let tools = scan_tools_directory(temp_dir.path()).await.unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "tool_0");
    }

    #[test]
    fn test_stale_metadata_error_message_tells_user_to_regenerate() {
        let err = ScanError::StaleMetadata {
            tool: "create_issue".to_string(),
            expected_file: "createIssue.ts".to_string(),
            sidecar_path: "~/.claude/servers/github/_meta.json".to_string(),
        };

        let message = err.to_string();
        assert!(
            message.contains("create_issue"),
            "message must name the affected tool"
        );
        assert!(
            message.contains("createIssue.ts"),
            "message must name the missing file"
        );
        assert!(
            message.contains("re-run 'generate'"),
            "message must tell the user how to fix it: {message}"
        );
    }

    #[tokio::test]
    async fn test_scan_tools_directory_missing_metadata() {
        let temp_dir = TempDir::new().unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        assert!(matches!(result, Err(ScanError::MissingMetadata { .. })));
    }

    #[tokio::test]
    async fn test_scan_tools_directory_corrupt_json() {
        let temp_dir = TempDir::new().unwrap();
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), "{not valid json")
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        assert!(matches!(result, Err(ScanError::MetadataParse { .. })));
    }

    #[tokio::test]
    async fn test_scan_tools_directory_unsupported_schema() {
        let temp_dir = TempDir::new().unwrap();
        let mut meta = sample_metadata(1);
        meta.schema_version = METADATA_SCHEMA_VERSION + 1;
        write_metadata(temp_dir.path(), &meta).await;

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::UnsupportedSchema { found, expected }) => {
                assert_eq!(found, METADATA_SCHEMA_VERSION + 1);
                assert_eq!(expected, METADATA_SCHEMA_VERSION);
            }
            other => panic!("expected UnsupportedSchema, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_too_many_tools() {
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(MAX_TOOL_FILES + 1);
        write_metadata(temp_dir.path(), &meta).await;

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::TooManyFiles { count, limit }) => {
                assert_eq!(count, MAX_TOOL_FILES + 1);
                assert_eq!(limit, MAX_TOOL_FILES);
            }
            other => panic!("expected TooManyFiles, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_file_too_large() {
        let temp_dir = TempDir::new().unwrap();
        let mut meta = sample_metadata(1);
        // MAX_FILE_SIZE (1MB) always fits in usize; the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation)]
        let padding = "a".repeat((MAX_FILE_SIZE as usize) + 1);
        meta.tools[0].description = Some(padding);
        write_metadata(temp_dir.path(), &meta).await;

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::FileTooLarge { size, limit, .. }) => {
                assert!(size > MAX_FILE_SIZE);
                assert_eq!(limit, MAX_FILE_SIZE);
            }
            other => panic!("expected FileTooLarge, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_nonexistent() {
        let result = scan_tools_directory(Path::new("/nonexistent/path/for/testing")).await;

        assert!(matches!(result, Err(ScanError::DirectoryNotFound { .. })));
    }

    // ========================================================================
    // extract_skill_metadata Tests
    // ========================================================================

    #[test]
    fn test_extract_skill_metadata_valid() {
        let content = r"---
name: github-progressive
description: GitHub MCP server operations
---

# GitHub Progressive

## Quick Start

Content here.

## Common Tasks

More content.
";

        let result = extract_skill_metadata(content);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.name, "github-progressive");
        assert_eq!(metadata.description, "GitHub MCP server operations");
        assert_eq!(metadata.section_count, 2);
        assert!(metadata.word_count > 0);
    }

    #[test]
    fn test_extract_skill_metadata_no_frontmatter() {
        let content = "# Test\n\nNo frontmatter";

        let result = extract_skill_metadata(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("YAML frontmatter not found"));
    }

    #[test]
    fn test_extract_skill_metadata_missing_name() {
        let content = "---\ndescription: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("'name' field not found"));
    }

    #[test]
    fn test_extract_skill_metadata_missing_description() {
        let content = "---\nname: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("'description' field not found")
        );
    }

    #[test]
    fn test_extract_skill_metadata_with_extra_fields() {
        let content = r"---
name: test-skill
description: Test description
version: 1.0.0
author: Test Author
---

# Test
";

        let result = extract_skill_metadata(content);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test-skill");
        assert_eq!(metadata.description, "Test description");
    }

    #[test]
    fn test_extract_skill_metadata_multiline_description() {
        let content = r"---
name: test
description: This is a long description that contains multiple words
---

# Test
";

        let result = extract_skill_metadata(content);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert!(metadata.description.contains("multiple words"));
    }
}
