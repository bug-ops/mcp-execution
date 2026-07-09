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
/// regex-based `.ts` scanner, this does not enumerate or read any generated
/// TypeScript source — the sidecar is the single source of truth.
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
/// missing or malformed, or the sidecar's tool count exceeds
/// [`MAX_TOOL_FILES`].
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

    async fn write_metadata(dir: &Path, meta: &ServerMetadata) {
        let content = serde_json::to_string_pretty(meta).unwrap();
        tokio::fs::write(dir.join(METADATA_FILE_NAME), content)
            .await
            .unwrap();
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
