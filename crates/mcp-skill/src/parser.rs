//! TypeScript tool file parser.
//!
//! Extracts `JSDoc` metadata from generated TypeScript files:
//! - `@tool` - Original MCP tool name
//! - `@server` - Server identifier
//! - `@category` - Tool category
//! - `@keywords` - Comma-separated keywords
//! - `@description` - Tool description
//!
//! # `JSDoc` Format
//!
//! ```typescript
//! /**
//!  * @tool create_issue
//!  * @server github
//!  * @category issues
//!  * @keywords create,issue,new,bug,feature
//!  * @description Create a new issue in a repository
//!  */
//! ```

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use thiserror::Error;

/// Maximum number of tool files to scan (denial-of-service protection).
pub const MAX_TOOL_FILES: usize = 500;

/// Maximum file size to read in bytes (1MB).
pub const MAX_FILE_SIZE: u64 = 1024 * 1024;

// Pre-compiled regexes for performance (compiled once, reused)
static JSDOC_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/\*\*[\s\S]*?\*/").expect("valid regex"));
static TOOL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@tool\s+(\S+)").expect("valid regex"));
static SERVER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@server\s+(\S+)").expect("valid regex"));
static CATEGORY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@category\s+(\S+)").expect("valid regex"));
static KEYWORDS_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@keywords[ \t]+(.+)").expect("valid regex"));
static DESC_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@description[ \t]+(.+)").expect("valid regex"));
static INTERFACE_OPEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"interface\s+\w+Params\s*\{").expect("valid regex"));

/// Anchored property pattern, applied to a single already-isolated property
/// declaration (comments stripped, internal whitespace collapsed to single
/// spaces).
///
/// Matches TypeScript interface properties like:
/// - `name: string;`
/// - `count?: number;`
/// - `readonly id: string;`
/// - `filter: { foo: string; bar: number; }` (nested object type — the type
///   capture is unanchored to `;` since the caller has already isolated the
///   property boundary via [`split_top_level_properties`]).
static PROP_LINE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:readonly\s+)?([A-Za-z_$][A-Za-z0-9_$]*)(\?)?\s*:\s*(.+?)\s*[;,]?\s*$")
        .expect("valid regex")
});

/// Scanner state while walking raw TypeScript source one character at a
/// time, so structural characters (`{`, `}`, `;`) that appear inside a
/// comment or string literal are not mistaken for real syntax.
///
/// This is a targeted heuristic, not a full TypeScript lexer — constructs
/// like regex literals (`/a;b{2}/`) are not recognized and could still
/// confuse comment detection. Revisit if generated parameter types grow
/// more syntactically complex.
#[derive(Clone, Copy, PartialEq)]
enum ScanState {
    Code,
    LineComment,
    BlockComment,
    /// Inside a string/template literal; holds the opening quote character.
    StringLiteral(char),
}

/// Extract the body of a `...Params` interface via a single comment- and
/// string-literal-aware pass.
///
/// Brace depth is tracked from the opening `{` to its true match, but only
/// while scanning code — braces inside a `//` or `/* */` comment (e.g. an
/// MCP-supplied `JSDoc` `@description` mentioning `{`) or inside a string
/// literal (e.g. a `"curly: {"` enum value) are ignored rather than treated
/// as structural, so they can no longer corrupt the match or drop the whole
/// interface. Comments are stripped from the returned body; string-literal
/// contents are preserved verbatim.
fn extract_interface_body(content: &str) -> Option<String> {
    let open = INTERFACE_OPEN_REGEX.find(content)?;
    let mut chars = content[open.end()..].chars().peekable();

    let mut state = ScanState::Code;
    let mut depth = 1i32;
    let mut escaped = false;
    let mut body = String::new();

    while let Some(ch) = chars.next() {
        match state {
            ScanState::Code => match ch {
                '/' if chars.peek() == Some(&'/') => {
                    chars.next();
                    state = ScanState::LineComment;
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    state = ScanState::BlockComment;
                }
                '"' | '\'' | '`' => {
                    state = ScanState::StringLiteral(ch);
                    body.push(ch);
                }
                '{' => {
                    depth += 1;
                    body.push(ch);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(body);
                    }
                    body.push(ch);
                }
                _ => body.push(ch),
            },
            ScanState::LineComment => {
                if ch == '\n' {
                    state = ScanState::Code;
                    body.push(ch);
                }
            }
            ScanState::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    state = ScanState::Code;
                    body.push(' ');
                }
            }
            ScanState::StringLiteral(quote) => {
                body.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == quote {
                    state = ScanState::Code;
                }
            }
        }
    }

    None
}

/// Split a comment-free interface body into individual property
/// declarations on top-level `;` — semicolons that are nested inside
/// neither a property's own `{}` type (e.g. `filter: { a: string; b: number };`)
/// nor a string-literal type value (e.g. `pattern: "a;b";`).
fn split_top_level_properties(body: &str) -> Vec<&str> {
    let mut properties = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (offset, ch) in body.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '{' => depth += 1,
            '}' => depth -= 1,
            ';' if depth == 0 => {
                properties.push(body[start..offset].trim());
                start = offset + 1;
            }
            _ => {}
        }
    }

    let tail = body[start..].trim();
    if !tail.is_empty() {
        properties.push(tail);
    }

    properties
}

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

/// Errors that can occur during TypeScript file parsing.
#[derive(Debug, Error)]
pub enum ParseError {
    /// `JSDoc` block not found in file.
    #[error("JSDoc block not found in file")]
    MissingJsDoc,

    /// Required tag not found in `JSDoc`.
    #[error("required tag '@{tag}' not found")]
    MissingTag { tag: &'static str },

    /// Failed to parse file content.
    #[error("failed to parse file: {message}")]
    ParseFailed { message: String },
}

/// Errors that can occur during directory scanning.
#[derive(Debug, Error)]
pub enum ScanError {
    /// I/O error reading directory or files.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse a tool file.
    #[error("failed to parse {path}: {source}")]
    ParseFailed {
        path: String,
        #[source]
        source: ParseError,
    },

    /// Directory does not exist.
    #[error("directory does not exist: {path}")]
    DirectoryNotFound { path: String },

    /// Too many files in directory (denial-of-service protection).
    #[error("too many files: {count} exceeds limit of {limit}")]
    TooManyFiles { count: usize, limit: usize },

    /// File too large to process.
    #[error("file too large: {path} ({size} bytes exceeds {limit} limit)")]
    FileTooLarge { path: String, size: u64, limit: u64 },
}

/// Parsed metadata from a TypeScript tool file.
#[derive(Debug, Clone)]
pub struct ParsedToolFile {
    /// Original MCP tool name (from @tool tag).
    pub name: String,

    /// TypeScript function name (`PascalCase` filename).
    pub typescript_name: String,

    /// Server identifier (from @server tag).
    pub server_id: String,

    /// Category for grouping (from @category tag).
    pub category: Option<String>,

    /// Keywords for discovery (from @keywords tag).
    pub keywords: Vec<String>,

    /// Tool description (from @description tag).
    pub description: Option<String>,

    /// Parsed parameters from TypeScript interface.
    pub parameters: Vec<ParsedParameter>,
}

/// A parsed parameter from TypeScript interface.
#[derive(Debug, Clone)]
pub struct ParsedParameter {
    /// Parameter name.
    pub name: String,

    /// TypeScript type (e.g., "string", "number", "boolean").
    pub typescript_type: String,

    /// Whether the parameter is required.
    pub required: bool,

    /// Parameter description from `JSDoc`.
    pub description: Option<String>,
}

/// Parse `JSDoc` metadata from TypeScript file content.
///
/// # Arguments
///
/// * `content` - TypeScript file content as string
/// * `filename` - Filename for deriving TypeScript function name
///
/// # Returns
///
/// `ParsedToolFile` with extracted metadata.
///
/// # Errors
///
/// Returns `ParseError` if `JSDoc` block or required tags are missing.
///
/// # Panics
///
/// Panics if regex compilation fails (should never happen with hardcoded patterns).
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::parse_tool_file;
///
/// let content = r"
/// /**
///  * @tool create_issue
///  * @server github
///  * @category issues
///  * @keywords create,issue,new
///  * @description Create a new issue
///  */
/// ";
///
/// let result = parse_tool_file(content, "createIssue.ts");
/// assert!(result.is_ok());
/// ```
pub fn parse_tool_file(content: &str, filename: &str) -> Result<ParsedToolFile, ParseError> {
    // Extract JSDoc block (using pre-compiled regex)
    let jsdoc = JSDOC_REGEX
        .find(content)
        .map(|m| m.as_str())
        .ok_or(ParseError::MissingJsDoc)?;

    // Extract @tool tag (required)
    let name = TOOL_REGEX
        .captures(jsdoc)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or(ParseError::MissingTag { tag: "tool" })?;

    // Extract @server tag (required)
    let server_id = SERVER_REGEX
        .captures(jsdoc)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or(ParseError::MissingTag { tag: "server" })?;

    // Extract @category tag (optional)
    let category = CATEGORY_REGEX
        .captures(jsdoc)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    // Extract @keywords tag (optional)
    let keywords = KEYWORDS_REGEX
        .captures(jsdoc)
        .and_then(|c| c.get(1))
        .map(|m| {
            m.as_str()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // Extract @description tag (optional)
    let description = DESC_REGEX
        .captures(jsdoc)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string());

    // Derive TypeScript name from filename
    let typescript_name = filename.strip_suffix(".ts").unwrap_or(filename).to_string();

    // Parse parameters from TypeScript interface
    let parameters = parse_parameters(content);

    Ok(ParsedToolFile {
        name,
        typescript_name,
        server_id,
        category,
        keywords,
        description,
        parameters,
    })
}

/// Parse parameters from TypeScript interface definition.
///
/// Extracts parameter names, types, and optionality from:
/// ```typescript
/// interface CreateIssueParams {
///   owner: string;
///   repo: string;
///   title: string;
///   body?: string;  // optional
/// }
/// ```
///
/// The interface body is extracted via a comment- and string-literal-aware
/// balanced-brace scan (so nested object types like `filter: { a: string; b:
/// number }` don't truncate the body, and braces inside comments or string
/// literals don't corrupt it), then split into properties on top-level `;`.
/// Splitting on `;` rather than newlines means a property declaration whose
/// type wraps onto a second line (e.g. `mode:\n  "read" | "write";`) is
/// captured and parsed as a single unit; each property's internal whitespace
/// is then collapsed to single spaces before matching so a multi-line type
/// (including a nested object type with its own `;`-terminated members) is
/// captured in full.
fn parse_parameters(content: &str) -> Vec<ParsedParameter> {
    let mut parameters = Vec::new();

    let Some(body) = extract_interface_body(content) else {
        return parameters;
    };

    for property in split_top_level_properties(&body) {
        let collapsed = property.split_whitespace().collect::<Vec<_>>().join(" ");

        if let Some(cap) = PROP_LINE_REGEX.captures(&collapsed) {
            let name = cap
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let optional = cap.get(2).is_some();
            let typescript_type = cap
                .get(3)
                .map_or_else(|| "unknown".to_string(), |m| m.as_str().trim().to_string());

            parameters.push(ParsedParameter {
                name,
                typescript_type,
                required: !optional,
                description: None,
            });
        }
    }

    parameters
}

/// Scan directory and parse all tool files.
///
/// Reads all `.ts` files in the directory, excluding:
/// - `index.ts` (barrel export)
/// - Files in `_runtime/` subdirectory
/// - Files starting with `_`
///
/// # Arguments
///
/// * `dir` - Path to server directory (e.g., `~/.claude/servers/github`)
///
/// # Returns
///
/// Vector of `ParsedToolFile` for each successfully parsed file.
///
/// # Errors
///
/// Returns `ScanError` if directory doesn't exist or files can't be read.
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

    let mut tools = Vec::new();
    let mut file_count = 0usize;

    let mut entries = tokio::fs::read_dir(&canonical_base).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        // Skip directories (like _runtime)
        if path.is_dir() {
            continue;
        }

        // Get filename
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Skip non-TypeScript files
        if !std::path::Path::new(filename)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
        {
            continue;
        }

        // Skip index.ts and files starting with _
        if filename == "index.ts" || filename.starts_with('_') {
            continue;
        }

        // SECURITY: Canonicalize file path and validate it stays within base directory
        // This prevents path traversal via symlinks
        let Ok(canonical_file) = tokio::fs::canonicalize(&path).await else {
            tracing::warn!(
                "Skipping file with invalid path: {}",
                sanitize_path_for_error(&path)
            );
            continue;
        };

        // Prevent path traversal via symlinks
        if !canonical_file.starts_with(&canonical_base) {
            tracing::warn!(
                "Skipping file outside base directory: {} (symlink to {})",
                sanitize_path_for_error(&path),
                sanitize_path_for_error(&canonical_file)
            );
            continue;
        }

        // Check file count limit (DoS protection)
        file_count += 1;
        if file_count > MAX_TOOL_FILES {
            return Err(ScanError::TooManyFiles {
                count: file_count,
                limit: MAX_TOOL_FILES,
            });
        }

        // Check file size before reading (DoS protection)
        let metadata = tokio::fs::metadata(&canonical_file).await?;
        if metadata.len() > MAX_FILE_SIZE {
            return Err(ScanError::FileTooLarge {
                path: sanitize_path_for_error(&path),
                size: metadata.len(),
                limit: MAX_FILE_SIZE,
            });
        }

        // Read and parse file (use canonical path)
        let content = tokio::fs::read_to_string(&canonical_file).await?;

        match parse_tool_file(&content, filename) {
            Ok(tool) => tools.push(tool),
            Err(e) => {
                // Log warning but continue with other files
                tracing::warn!("Failed to parse {}: {}", sanitize_path_for_error(&path), e);
            }
        }
    }

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

    #[test]
    fn test_parse_tool_file_complete() {
        let content = r"
/**
 * @tool create_issue
 * @server github
 * @category issues
 * @keywords create,issue,new,bug,feature
 * @description Create a new issue in a repository
 */

interface CreateIssueParams {
  owner: string;
  repo: string;
  title: string;
  body?: string;
  labels?: string[];
}
";

        let result = parse_tool_file(content, "createIssue.ts").unwrap();

        assert_eq!(result.name, "create_issue");
        assert_eq!(result.typescript_name, "createIssue");
        assert_eq!(result.server_id, "github");
        assert_eq!(result.category, Some("issues".to_string()));
        assert_eq!(
            result.keywords,
            vec!["create", "issue", "new", "bug", "feature"]
        );
        assert_eq!(
            result.description,
            Some("Create a new issue in a repository".to_string())
        );
        assert_eq!(result.parameters.len(), 5);

        // Check required params
        let owner = result
            .parameters
            .iter()
            .find(|p| p.name == "owner")
            .unwrap();
        assert!(owner.required);
        assert_eq!(owner.typescript_type, "string");

        // Check optional params
        let body = result.parameters.iter().find(|p| p.name == "body").unwrap();
        assert!(!body.required);
    }

    #[test]
    fn test_parse_tool_file_minimal() {
        let content = r"
/**
 * @tool get_user
 * @server github
 */
";

        let result = parse_tool_file(content, "getUser.ts").unwrap();

        assert_eq!(result.name, "get_user");
        assert_eq!(result.server_id, "github");
        assert!(result.category.is_none());
        assert!(result.keywords.is_empty());
        assert!(result.description.is_none());
    }

    #[test]
    fn test_parse_tool_file_missing_jsdoc() {
        let content = r"
// No JSDoc block
function test() {}
";

        let result = parse_tool_file(content, "test.ts");
        assert!(matches!(result, Err(ParseError::MissingJsDoc)));
    }

    #[test]
    fn test_parse_tool_file_missing_tool_tag() {
        let content = r"
/**
 * @server github
 */
";

        let result = parse_tool_file(content, "test.ts");
        assert!(matches!(
            result,
            Err(ParseError::MissingTag { tag: "tool" })
        ));
    }

    #[test]
    fn test_parse_parameters() {
        let content = r"
interface TestParams {
  required: string;
  optional?: number;
  array: string[];
  complex?: Record<string, unknown>;
}
";

        let params = parse_parameters(content);

        assert_eq!(params.len(), 4);

        let required = params.iter().find(|p| p.name == "required").unwrap();
        assert!(required.required);
        assert_eq!(required.typescript_type, "string");

        let optional = params.iter().find(|p| p.name == "optional").unwrap();
        assert!(!optional.required);
        assert_eq!(optional.typescript_type, "number");
    }

    #[test]
    fn test_parse_keywords_with_spaces() {
        let content = r"
/**
 * @tool test
 * @server test
 * @keywords  create , update,  delete
 */
";

        let result = parse_tool_file(content, "test.ts").unwrap();
        assert_eq!(result.keywords, vec!["create", "update", "delete"]);
    }

    // ========================================================================
    // Edge Cases
    // ========================================================================

    #[test]
    fn test_parse_tool_file_missing_server_tag() {
        let content = r"
/**
 * @tool test_tool
 */
";

        let result = parse_tool_file(content, "test.ts");
        assert!(matches!(
            result,
            Err(ParseError::MissingTag { tag: "server" })
        ));
    }

    #[test]
    fn test_parse_tool_file_malformed_jsdoc() {
        let content = r"
/**
 * @tool
 * @server github
 */
";

        // @tool with no value - regex requires @tool\s+(\S+) which would capture
        // the `*` from the next line as the tool name. Parser is lenient.
        let result = parse_tool_file(content, "test.ts");
        // Parsing succeeds but tool_name may be unexpected (e.g., "*")
        // Validation of proper tool names happens at a higher level
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_tool_file_multiline_description() {
        let content = r"
/**
 * @tool test
 * @server github
 * @description This is a very long description that spans
 */
";

        let result = parse_tool_file(content, "test.ts").unwrap();
        assert!(result.description.is_some());
        assert!(
            result
                .description
                .unwrap()
                .contains("This is a very long description")
        );
    }

    #[test]
    fn test_parse_tool_file_empty_keywords() {
        let content = r"
/**
 * @tool test
 * @server github
 * @keywords
 */
";

        // When @keywords has no value, the regex doesn't match, so keywords will be default (empty vec)
        let result = parse_tool_file(content, "test.ts").unwrap();
        // This is acceptable - parsing should succeed with empty keywords
        assert!(result.keywords.is_empty());
    }

    #[test]
    fn test_parse_tool_file_single_keyword() {
        let content = r"
/**
 * @tool test
 * @server github
 * @keywords single
 */
";

        let result = parse_tool_file(content, "test.ts").unwrap();
        assert_eq!(result.keywords, vec!["single"]);
    }

    #[test]
    fn test_parse_tool_file_with_hyphens_in_names() {
        let content = r"
/**
 * @tool create-pull-request
 * @server git-hub
 * @category pull-requests
 */
";

        let result = parse_tool_file(content, "test.ts").unwrap();
        assert_eq!(result.name, "create-pull-request");
        assert_eq!(result.server_id, "git-hub");
        assert_eq!(result.category, Some("pull-requests".to_string()));
    }

    #[test]
    fn test_parse_parameters_no_interface() {
        let content = r"
export async function test(): Promise<void> {
  // No interface
}
";

        let params = parse_parameters(content);
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_parse_parameters_empty_interface() {
        let content = r"
interface TestParams {
}
";

        let params = parse_parameters(content);
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_parse_parameters_complex_types() {
        let content = r"
interface TestParams {
  callback?: (arg: string) => void;
  union: string | number;
  generic: Array<string>;
  nested: { foo: string };
}
";

        let params = parse_parameters(content);
        // Complex types like nested objects may not parse correctly with simple regex
        // We should get at least 3 params (callback, union, generic)
        assert!(params.len() >= 3);

        if let Some(callback) = params.iter().find(|p| p.name == "callback") {
            assert!(!callback.required);
        }

        if let Some(union) = params.iter().find(|p| p.name == "union") {
            assert!(union.required);
        }
    }

    #[test]
    fn test_parse_parameters_nested_type_does_not_truncate_body() {
        // Issue #124 regression: a property with an inline nested object type
        // containing `}` must not truncate the rest of the interface body.
        let content = r"
interface TestParams {
  filter: { foo: string };
  limit: number;
}
";

        let params = parse_parameters(content);
        assert_eq!(
            params.len(),
            2,
            "expected filter and limit, got: {params:?}"
        );

        let filter = params.iter().find(|p| p.name == "filter").unwrap();
        assert!(filter.required);
        assert_eq!(filter.typescript_type, "{ foo: string }");

        let limit = params.iter().find(|p| p.name == "limit").unwrap();
        assert!(limit.required);
        assert_eq!(limit.typescript_type, "number");
    }

    #[test]
    fn test_parse_parameters_multiline_property_declaration() {
        // Issue #125 regression: a property whose type wraps onto a second
        // line must still be parsed, with its full type captured.
        let content = "
interface TestParams {
  mode:\n    \"read\" | \"write\";
  name: string;
}
";

        let params = parse_parameters(content);
        assert_eq!(params.len(), 2, "expected mode and name, got: {params:?}");

        let mode = params.iter().find(|p| p.name == "mode").unwrap();
        assert!(mode.required);
        assert_eq!(mode.typescript_type, "\"read\" | \"write\"");

        let name = params.iter().find(|p| p.name == "name").unwrap();
        assert!(name.required);
    }

    #[test]
    fn test_parse_parameters_multi_member_nested_object_type() {
        // S1 regression: a multi-line nested object type with its own
        // `;`-terminated members (the idiomatic codegen shape for an
        // object-typed parameter) must still be captured in full, not
        // dropped because its type contains internal `;`.
        let content = r"
interface TestParams {
  filter: {
    foo: string;
    bar: number;
  };
  limit: number;
}
";

        let params = parse_parameters(content);
        assert_eq!(
            params.len(),
            2,
            "expected filter and limit, got: {params:?}"
        );

        let filter = params.iter().find(|p| p.name == "filter").unwrap();
        assert!(filter.required);
        assert_eq!(filter.typescript_type, "{ foo: string; bar: number; }");

        let limit = params.iter().find(|p| p.name == "limit").unwrap();
        assert!(limit.required);
        assert_eq!(limit.typescript_type, "number");
    }

    #[test]
    fn test_parse_parameters_unbalanced_brace_in_line_comment() {
        // S2 regression: a `}` inside a `//` comment (e.g. from an
        // MCP-supplied JSDoc description) must not be treated as closing the
        // interface — it must not drop every parameter.
        let content = r"
interface TestParams {
  // returns just a closing brace }
  name: string;
}
";

        let params = parse_parameters(content);
        assert_eq!(
            params.len(),
            1,
            "brace inside a // comment must not corrupt body extraction, got: {params:?}"
        );
        assert_eq!(params[0].name, "name");
    }

    #[test]
    fn test_parse_parameters_unbalanced_brace_in_block_comment() {
        // S2 regression: an unmatched `{` inside a `/* */` comment must not
        // corrupt brace-depth tracking for the rest of the interface.
        let content = r"
interface TestParams {
  /* opening brace: { */
  name: string;
}
";

        let params = parse_parameters(content);
        assert_eq!(
            params.len(),
            1,
            "brace inside a block comment must not corrupt body extraction, got: {params:?}"
        );
        assert_eq!(params[0].name, "name");
    }

    #[test]
    fn test_parse_parameters_string_literal_with_semicolon_and_brace() {
        // S3 regression: `;` and `{` inside string-literal type values (e.g.
        // enum-derived unions) must not be treated as structural — they must
        // not corrupt splitting or brace-depth tracking.
        let content = r#"
interface TestParams {
  pattern: "a;b";
  note: "curly open: {";
  limit: number;
}
"#;

        let params = parse_parameters(content);
        assert_eq!(
            params.len(),
            3,
            "expected pattern, note, limit — got: {params:?}"
        );

        let pattern = params.iter().find(|p| p.name == "pattern").unwrap();
        assert_eq!(pattern.typescript_type, "\"a;b\"");

        let note = params.iter().find(|p| p.name == "note").unwrap();
        assert_eq!(note.typescript_type, "\"curly open: {\"");

        let limit = params.iter().find(|p| p.name == "limit").unwrap();
        assert_eq!(limit.typescript_type, "number");
    }

    #[test]
    fn test_parse_parameters_with_comments() {
        let content = r"
interface TestParams {
  // This is a comment
  param1: string;
  /* Another comment */
  param2: number;
}
";

        let params = parse_parameters(content);
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_parse_tool_file_special_chars_in_description() {
        // Need r#""# because content contains embedded double quotes
        let content = r#"
/**
 * @tool test
 * @server github
 * @description Create & update <items> with "quotes" and 'apostrophes'
 */
"#;

        let result = parse_tool_file(content, "test.ts").unwrap();
        assert!(result.description.is_some());
        let description = result.description.unwrap();
        assert!(description.contains('&'));
        assert!(description.contains('"'));
    }

    #[test]
    fn test_parse_tool_file_numeric_category() {
        let content = r"
/**
 * @tool test
 * @server github
 * @category v2-api
 */
";

        let result = parse_tool_file(content, "test.ts").unwrap();
        assert_eq!(result.category, Some("v2-api".to_string()));
    }

    #[test]
    fn test_parse_tool_file_unicode_in_description() {
        let content = r"
/**
 * @tool test
 * @server github
 * @description Create issue with emoji 🚀 and unicode ™
 */
";

        let result = parse_tool_file(content, "test.ts").unwrap();
        assert!(result.description.is_some());
        let description = result.description.unwrap();
        assert!(description.contains("🚀"));
    }

    #[test]
    fn test_parse_tool_file_duplicate_tags() {
        let content = r"
/**
 * @tool first_tool
 * @tool second_tool
 * @server github
 */
";

        // Should use the first match
        let result = parse_tool_file(content, "test.ts").unwrap();
        assert_eq!(result.name, "first_tool");
    }

    #[test]
    fn test_parse_parameters_readonly_modifier() {
        let content = r"
interface TestParams {
  readonly id: string;
  readonly count?: number;
}
";

        let params = parse_parameters(content);
        // PROP_LINE_REGEX handles `readonly` — both fields must be extracted.
        assert_eq!(params.len(), 2);

        let id = params.iter().find(|p| p.name == "id").unwrap();
        assert!(id.required);
        assert_eq!(id.typescript_type, "string");

        let count = params.iter().find(|p| p.name == "count").unwrap();
        assert!(!count.required);
        assert_eq!(count.typescript_type, "number");
    }

    #[test]
    fn test_parse_parameters_inline_block_comment_stripped() {
        // S1 regression: inline `/* */` block comments must not drop the parameter.
        let content = r"
interface TestParams {
  name: string; /* required */
  count?: number; /* default: 5 */
}
";

        let params = parse_parameters(content);
        assert_eq!(params.len(), 2);
        assert!(params.iter().any(|p| p.name == "name" && p.required));
        assert!(params.iter().any(|p| p.name == "count" && !p.required));
    }

    #[test]
    fn test_parse_parameters_url_type_not_stripped() {
        // S2 regression: `//` inside a string-literal type must not truncate the line.
        let content = r"
interface TestParams {
  url: string; // endpoint URL
  mode: string;
}
";

        let params = parse_parameters(content);
        assert_eq!(params.len(), 2);
        assert!(
            params.iter().any(|p| p.name == "url"),
            "url must not be dropped"
        );
        assert!(params.iter().any(|p| p.name == "mode"));
    }

    #[test]
    fn test_parse_parameters_jsdoc_body_comments_not_extracted() {
        // Regression: comment lines inside JSDoc must NOT produce parameters.
        let content = r"
interface FooParams {
  /**
   * @default 4
   * Tags to include: foo, bar
   */
  count?: number;
  name: string;
}
";

        let params = parse_parameters(content);
        assert_eq!(
            params.len(),
            2,
            "expected exactly count and name, got: {params:?}"
        );

        let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"count"), "missing 'count'");
        assert!(names.contains(&"name"), "missing 'name'");
        assert!(!names.contains(&"default"), "false positive: 'default'");
        assert!(!names.contains(&"include"), "false positive: 'include'");
    }

    #[test]
    fn test_parse_parameters_inline_comment_stripped() {
        // C1: trailing `// ...` inline comment must be stripped before matching.
        let content = r"
interface TestParams {
  name: string; // parameter name
  count?: number; // how many
}
";

        let params = parse_parameters(content);
        assert_eq!(params.len(), 2);

        let name = params.iter().find(|p| p.name == "name").unwrap();
        assert!(name.required);
        assert_eq!(name.typescript_type, "string");

        let count = params.iter().find(|p| p.name == "count").unwrap();
        assert!(!count.required);
    }

    #[test]
    fn test_parse_tool_file_filename_without_extension() {
        let content = r"
/**
 * @tool test
 * @server github
 */
";

        let result = parse_tool_file(content, "testFile").unwrap();
        assert_eq!(result.typescript_name, "testFile");
    }

    #[test]
    fn test_parse_keywords_trailing_commas() {
        let content = r"
/**
 * @tool test
 * @server test
 * @keywords create,update,delete,
 */
";

        let result = parse_tool_file(content, "test.ts").unwrap();
        // Empty strings from trailing commas should be filtered out
        assert_eq!(result.keywords, vec!["create", "update", "delete"]);
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
