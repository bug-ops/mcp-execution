//! Types for code generation.
//!
//! Defines the data structures used during code generation from MCP
//! tool schemas to executable TypeScript or Rust code.
//!
//! # Examples
//!
//! ```
//! use mcp_codegen::{GeneratedCode, GeneratedFile};
//!
//! let file = GeneratedFile {
//!     path: "tools/sendMessage.ts".to_string(),
//!     content: "export function sendMessage() {}".to_string(),
//! };
//!
//! let code = GeneratedCode {
//!     files: vec![file],
//! };
//!
//! assert_eq!(code.files.len(), 1);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of code generation containing all generated files.
///
/// This is the main output type returned by the code generator.
/// Contains a list of files that should be written to disk.
///
/// # Examples
///
/// ```
/// use mcp_codegen::GeneratedCode;
///
/// let code = GeneratedCode {
///     files: vec![],
/// };
///
/// assert_eq!(code.file_count(), 0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedCode {
    /// List of generated files with paths and contents
    pub files: Vec<GeneratedFile>,
}

impl GeneratedCode {
    /// Creates a new empty generated code container.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::GeneratedCode;
    ///
    /// let code = GeneratedCode::new();
    /// assert_eq!(code.file_count(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Adds a generated file to the collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::{GeneratedCode, GeneratedFile};
    ///
    /// let mut code = GeneratedCode::new();
    /// code.add_file(GeneratedFile {
    ///     path: "index.ts".to_string(),
    ///     content: "export {}".to_string(),
    /// });
    ///
    /// assert_eq!(code.file_count(), 1);
    /// ```
    pub fn add_file(&mut self, file: GeneratedFile) {
        self.files.push(file);
    }

    /// Returns the number of generated files.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::GeneratedCode;
    ///
    /// let code = GeneratedCode::new();
    /// assert_eq!(code.file_count(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns an iterator over the generated files.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::{GeneratedCode, GeneratedFile};
    ///
    /// let mut code = GeneratedCode::new();
    /// code.add_file(GeneratedFile {
    ///     path: "test.ts".to_string(),
    ///     content: "content".to_string(),
    /// });
    ///
    /// for file in code.files() {
    ///     println!("Path: {}", file.path);
    /// }
    /// ```
    #[inline]
    pub fn files(&self) -> impl Iterator<Item = &GeneratedFile> {
        self.files.iter()
    }
}

impl Default for GeneratedCode {
    fn default() -> Self {
        Self::new()
    }
}

/// A single generated file with path and content.
///
/// Represents one file that will be written to the virtual filesystem
/// or actual filesystem during code generation.
///
/// # Examples
///
/// ```
/// use mcp_codegen::GeneratedFile;
///
/// let file = GeneratedFile {
///     path: "types.ts".to_string(),
///     content: "export type Params = {};".to_string(),
/// };
///
/// assert_eq!(file.path, "types.ts");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedFile {
    /// Relative path where the file should be written
    pub path: String,
    /// File content
    pub content: String,
}

impl GeneratedFile {
    /// Returns the file path.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::GeneratedFile;
    ///
    /// let file = GeneratedFile {
    ///     path: "test.ts".to_string(),
    ///     content: String::new(),
    /// };
    ///
    /// assert_eq!(file.path(), "test.ts");
    /// ```
    #[inline]
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the file content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::GeneratedFile;
    ///
    /// let file = GeneratedFile {
    ///     path: "test.ts".to_string(),
    ///     content: "export {}".to_string(),
    /// };
    ///
    /// assert_eq!(file.content(), "export {}");
    /// ```
    #[inline]
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// Template context for code generation.
///
/// Contains all the data needed to render a Handlebars template.
/// This is typically constructed from MCP server information.
///
/// # Examples
///
/// ```
/// use mcp_codegen::TemplateContext;
/// use std::collections::HashMap;
///
/// let context = TemplateContext {
///     server_name: "github".to_string(),
///     server_version: "1.0.0".to_string(),
///     tools: vec![],
///     metadata: HashMap::new(),
/// };
///
/// assert_eq!(context.server_name, "github");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateContext {
    /// Name of the MCP server
    pub server_name: String,
    /// Server version string
    pub server_version: String,
    /// List of tool definitions
    pub tools: Vec<ToolDefinition>,
    /// Additional metadata for template rendering
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Definition of a single MCP tool for code generation.
///
/// Contains all information needed to generate TypeScript or Rust
/// code for calling an MCP tool.
///
/// # Examples
///
/// ```
/// use mcp_codegen::ToolDefinition;
/// use serde_json::json;
///
/// let tool = ToolDefinition {
///     name: "send_message".to_string(),
///     description: "Sends a message".to_string(),
///     input_schema: json!({"type": "object"}),
///     typescript_name: "sendMessage".to_string(),
/// };
///
/// assert_eq!(tool.name, "send_message");
/// assert_eq!(tool.typescript_name, "sendMessage");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Original tool name (snake_case from MCP)
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: serde_json::Value,
    /// TypeScript-friendly name (camelCase)
    pub typescript_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generated_code_new() {
        let code = GeneratedCode::new();
        assert_eq!(code.file_count(), 0);
    }

    #[test]
    fn test_generated_code_default() {
        let code = GeneratedCode::default();
        assert_eq!(code.file_count(), 0);
    }

    #[test]
    fn test_add_file() {
        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "test.ts".to_string(),
            content: "content".to_string(),
        });
        assert_eq!(code.file_count(), 1);
    }

    #[test]
    fn test_tool_definition() {
        let tool = ToolDefinition {
            name: "test_tool".to_string(),
            description: "Test".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            typescript_name: "testTool".to_string(),
        };

        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.typescript_name, "testTool");
    }

    #[test]
    fn test_template_context() {
        let context = TemplateContext {
            server_name: "test-server".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![],
            metadata: HashMap::new(),
        };

        assert_eq!(context.server_name, "test-server");
        assert_eq!(context.tools.len(), 0);
    }
}
