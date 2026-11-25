//! Types for code generation.
//!
//! Defines the data structures used during code generation from MCP
//! tool schemas to executable TypeScript or Rust code.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_codegen::{GeneratedCode, GeneratedFile};
//!
//! let file = GeneratedFile::new(
//!     "tools/sendMessage.ts".to_string(),
//!     "export function sendMessage() {}".to_string(),
//! );
//!
//! let mut code = GeneratedCode::new();
//! code.files = vec![file];
//!
//! assert_eq!(code.files.len(), 1);
//! ```

use mcp_execution_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Result of code generation containing all generated files.
///
/// This is the main output type returned by the code generator.
/// Contains a list of files that should be written to disk.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::GeneratedCode;
///
/// let mut code = GeneratedCode::new();
/// code.files = vec![];
///
/// assert_eq!(code.file_count(), 0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
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
    /// use mcp_execution_codegen::GeneratedCode;
    ///
    /// let code = GeneratedCode::new();
    /// assert_eq!(code.file_count(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Adds a generated file to the collection.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DuplicateGeneratedFilePath`] if `file.path` is already present in
    /// this collection. Silently overwriting an existing entry would discard the file
    /// already added at that path with no signal to the caller (issue #312); every path
    /// added here is expected to already be unique by construction, so this only fires
    /// when that invariant has actually been violated upstream.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::assert_matches;
    /// use mcp_execution_codegen::{GeneratedCode, GeneratedFile};
    /// use mcp_execution_core::Error;
    ///
    /// let mut code = GeneratedCode::new();
    /// code.add_file(GeneratedFile::new("index.ts".to_string(), "export {}".to_string()))
    /// .unwrap();
    ///
    /// assert_eq!(code.file_count(), 1);
    ///
    /// let err = code
    ///     .add_file(GeneratedFile::new("index.ts".to_string(), "export const x = 1;".to_string()))
    ///     .unwrap_err();
    /// assert_matches!(err, Error::DuplicateGeneratedFilePath { .. });
    /// ```
    pub fn add_file(&mut self, file: GeneratedFile) -> Result<()> {
        if self.files.iter().any(|existing| existing.path == file.path) {
            return Err(Error::DuplicateGeneratedFilePath { path: file.path });
        }
        self.files.push(file);
        Ok(())
    }

    /// Returns the number of generated files.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::GeneratedCode;
    ///
    /// let code = GeneratedCode::new();
    /// assert_eq!(code.file_count(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns an iterator over the generated files.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::{GeneratedCode, GeneratedFile};
    ///
    /// let mut code = GeneratedCode::new();
    /// code.add_file(GeneratedFile::new("test.ts".to_string(), "content".to_string()));
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
/// use mcp_execution_codegen::GeneratedFile;
///
/// let file = GeneratedFile::new("types.ts".to_string(), "export type Params = {};".to_string());
///
/// assert_eq!(file.path, "types.ts");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GeneratedFile {
    /// Relative path where the file should be written
    pub path: String,
    /// File content
    pub content: String,
}

impl GeneratedFile {
    /// Creates a file with the given relative `path` and `content`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::GeneratedFile;
    ///
    /// let file = GeneratedFile::new("catalog.ts".to_string(), "export {};".to_string());
    /// assert_eq!(file.path(), "catalog.ts");
    /// ```
    #[must_use]
    pub const fn new(path: String, content: String) -> Self {
        Self { path, content }
    }

    /// Returns the file path.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::GeneratedFile;
    ///
    /// let file = GeneratedFile::new("test.ts".to_string(), String::new());
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
    /// use mcp_execution_codegen::GeneratedFile;
    ///
    /// let file = GeneratedFile::new("test.ts".to_string(), "export {}".to_string());
    ///
    /// assert_eq!(file.content(), "export {}");
    /// ```
    #[inline]
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

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
        })
        .unwrap();
        assert_eq!(code.file_count(), 1);
    }

    #[test]
    fn test_add_file_rejects_duplicate_path() {
        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "index.ts".to_string(),
            content: "first".to_string(),
        })
        .unwrap();

        let err = code
            .add_file(GeneratedFile {
                path: "index.ts".to_string(),
                content: "second".to_string(),
            })
            .unwrap_err();

        assert_matches!(err, Error::DuplicateGeneratedFilePath { .. });
        // The original file must be left untouched, not silently overwritten.
        assert_eq!(code.file_count(), 1);
        assert_eq!(code.files[0].content, "first");
    }
}
