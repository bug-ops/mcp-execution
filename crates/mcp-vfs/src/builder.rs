//! Builder pattern for constructing virtual filesystems.
//!
//! Provides a fluent API for building VFS instances from generated code
//! or by adding files programmatically.
//!
//! # Examples
//!
//! ```
//! use mcp_vfs::VfsBuilder;
//!
//! let vfs = VfsBuilder::new()
//!     .add_file("/mcp-tools/manifest.json", "{}")
//!     .add_file("/mcp-tools/types.ts", "export type Params = {};")
//!     .build()
//!     .unwrap();
//!
//! assert_eq!(vfs.file_count(), 2);
//! ```

use crate::types::{Result, VfsError};
use crate::vfs::Vfs;
use mcp_codegen::GeneratedCode;
use std::path::Path;

/// Builder for constructing a virtual filesystem.
///
/// `VfsBuilder` provides a fluent API for creating VFS instances,
/// with support for adding files individually or bulk-loading from
/// generated code.
///
/// # Examples
///
/// ## Building from scratch
///
/// ```
/// use mcp_vfs::VfsBuilder;
///
/// let vfs = VfsBuilder::new()
///     .add_file("/test.ts", "console.log('test');")
///     .build()
///     .unwrap();
///
/// assert!(vfs.exists("/test.ts"));
/// # Ok::<(), mcp_vfs::VfsError>(())
/// ```
///
/// ## Building from generated code
///
/// ```
/// use mcp_vfs::VfsBuilder;
/// use mcp_codegen::{GeneratedCode, GeneratedFile};
///
/// let mut code = GeneratedCode::new();
/// code.add_file(GeneratedFile {
///     path: "manifest.json".to_string(),
///     content: "{}".to_string(),
/// });
///
/// let vfs = VfsBuilder::from_generated_code(code, "/mcp-tools/servers/test")
///     .build()
///     .unwrap();
///
/// assert!(vfs.exists("/mcp-tools/servers/test/manifest.json"));
/// # Ok::<(), mcp_vfs::VfsError>(())
/// ```
#[derive(Debug, Default)]
pub struct VfsBuilder {
    vfs: Vfs,
    errors: Vec<VfsError>,
}

impl VfsBuilder {
    /// Creates a new empty VFS builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsBuilder;
    ///
    /// let builder = VfsBuilder::new();
    /// let vfs = builder.build().unwrap();
    /// assert_eq!(vfs.file_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            vfs: Vfs::new(),
            errors: Vec::new(),
        }
    }

    /// Creates a VFS builder from generated code.
    ///
    /// All files from the generated code will be placed under the specified
    /// base path. The base path should be an absolute VFS path like
    /// `/mcp-tools/servers/<server-id>`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsBuilder;
    /// use mcp_codegen::{GeneratedCode, GeneratedFile};
    ///
    /// let mut code = GeneratedCode::new();
    /// code.add_file(GeneratedFile {
    ///     path: "types.ts".to_string(),
    ///     content: "export type Params = {};".to_string(),
    /// });
    ///
    /// let vfs = VfsBuilder::from_generated_code(code, "/mcp-tools/servers/test")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert!(vfs.exists("/mcp-tools/servers/test/types.ts"));
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    #[must_use]
    pub fn from_generated_code(code: GeneratedCode, base_path: impl AsRef<Path>) -> Self {
        let mut builder = Self::new();
        let base = base_path.as_ref();

        for file in code.files {
            // Construct full path by joining base path with file path
            let full_path = base.join(&file.path);
            builder = builder.add_file(full_path, file.content);
        }

        builder
    }

    /// Adds a file to the VFS being built.
    ///
    /// If the path is invalid, the error will be collected and returned
    /// when `build()` is called.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsBuilder;
    ///
    /// let vfs = VfsBuilder::new()
    ///     .add_file("/test.ts", "export const x = 1;")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(vfs.read_file("/test.ts").unwrap(), "export const x = 1;");
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    #[must_use]
    pub fn add_file(mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Self {
        if let Err(e) = self.vfs.add_file(path, content) {
            self.errors.push(e);
        }
        self
    }

    /// Adds multiple files to the VFS being built.
    ///
    /// This is a convenience method for adding many files at once.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsBuilder;
    ///
    /// let files = vec![
    ///     ("/file1.ts", "content1"),
    ///     ("/file2.ts", "content2"),
    /// ];
    ///
    /// let vfs = VfsBuilder::new()
    ///     .add_files(files)
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(vfs.file_count(), 2);
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    #[must_use]
    pub fn add_files<P, C>(mut self, files: impl IntoIterator<Item = (P, C)>) -> Self
    where
        P: AsRef<Path>,
        C: Into<String>,
    {
        for (path, content) in files {
            if let Err(e) = self.vfs.add_file(path, content) {
                self.errors.push(e);
            }
        }
        self
    }

    /// Consumes the builder and returns the constructed VFS.
    ///
    /// # Errors
    ///
    /// Returns the first error encountered during file addition, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsBuilder;
    ///
    /// let vfs = VfsBuilder::new()
    ///     .add_file("/test.ts", "content")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(vfs.file_count(), 1);
    /// ```
    ///
    /// ```
    /// use mcp_vfs::VfsBuilder;
    ///
    /// let result = VfsBuilder::new()
    ///     .add_file("invalid/relative/path", "content")
    ///     .build();
    ///
    /// assert!(result.is_err());
    /// ```
    pub fn build(self) -> Result<Vfs> {
        if let Some(error) = self.errors.into_iter().next() {
            return Err(error);
        }
        Ok(self.vfs)
    }

    /// Returns the number of files currently in the builder.
    ///
    /// This can be used to check progress during construction.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsBuilder;
    ///
    /// let mut builder = VfsBuilder::new();
    /// assert_eq!(builder.file_count(), 0);
    ///
    /// builder = builder.add_file("/test.ts", "");
    /// assert_eq!(builder.file_count(), 1);
    /// ```
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.vfs.file_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_codegen::GeneratedFile;

    #[test]
    fn test_builder_new() {
        let builder = VfsBuilder::new();
        let vfs = builder.build().unwrap();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_builder_default() {
        let builder = VfsBuilder::default();
        let vfs = builder.build().unwrap();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_add_file() {
        let vfs = VfsBuilder::new()
            .add_file("/test.ts", "content")
            .build()
            .unwrap();

        assert_eq!(vfs.file_count(), 1);
        assert_eq!(vfs.read_file("/test.ts").unwrap(), "content");
    }

    #[test]
    fn test_add_file_invalid_path() {
        let result = VfsBuilder::new()
            .add_file("relative/path", "content")
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().is_invalid_path());
    }

    #[test]
    fn test_add_files() {
        let files = vec![("/file1.ts", "content1"), ("/file2.ts", "content2")];

        let vfs = VfsBuilder::new().add_files(files).build().unwrap();

        assert_eq!(vfs.file_count(), 2);
        assert_eq!(vfs.read_file("/file1.ts").unwrap(), "content1");
        assert_eq!(vfs.read_file("/file2.ts").unwrap(), "content2");
    }

    #[test]
    fn test_from_generated_code() {
        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "manifest.json".to_string(),
            content: "{}".to_string(),
        });
        code.add_file(GeneratedFile {
            path: "types.ts".to_string(),
            content: "export {};".to_string(),
        });

        let vfs = VfsBuilder::from_generated_code(code, "/mcp-tools/servers/test")
            .build()
            .unwrap();

        assert_eq!(vfs.file_count(), 2);
        assert!(vfs.exists("/mcp-tools/servers/test/manifest.json"));
        assert!(vfs.exists("/mcp-tools/servers/test/types.ts"));
    }

    #[test]
    fn test_from_generated_code_nested_paths() {
        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "tools/sendMessage.ts".to_string(),
            content: "export function sendMessage() {}".to_string(),
        });

        let vfs = VfsBuilder::from_generated_code(code, "/mcp-tools/servers/test")
            .build()
            .unwrap();

        assert!(vfs.exists("/mcp-tools/servers/test/tools/sendMessage.ts"));
    }

    #[test]
    fn test_file_count() {
        let mut builder = VfsBuilder::new();
        assert_eq!(builder.file_count(), 0);

        builder = builder.add_file("/test1.ts", "");
        assert_eq!(builder.file_count(), 1);

        builder = builder.add_file("/test2.ts", "");
        assert_eq!(builder.file_count(), 2);
    }

    #[test]
    fn test_chaining() {
        let vfs = VfsBuilder::new()
            .add_file("/file1.ts", "content1")
            .add_file("/file2.ts", "content2")
            .add_file("/file3.ts", "content3")
            .build()
            .unwrap();

        assert_eq!(vfs.file_count(), 3);
    }

    #[test]
    fn test_error_collection() {
        let result = VfsBuilder::new()
            .add_file("/valid.ts", "content")
            .add_file("invalid", "content") // Invalid path
            .add_file("/another-valid.ts", "content")
            .build();

        // Should fail due to invalid path
        assert!(result.is_err());
    }

    #[test]
    fn test_from_generated_code_with_additional_files() {
        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "generated.ts".to_string(),
            content: "// generated".to_string(),
        });

        let vfs = VfsBuilder::from_generated_code(code, "/mcp-tools/servers/test")
            .add_file("/mcp-tools/servers/test/manual.ts", "// manual")
            .build()
            .unwrap();

        assert_eq!(vfs.file_count(), 2);
        assert!(vfs.exists("/mcp-tools/servers/test/generated.ts"));
        assert!(vfs.exists("/mcp-tools/servers/test/manual.ts"));
    }
}
