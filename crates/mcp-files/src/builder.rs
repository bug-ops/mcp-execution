//! Builder pattern for constructing virtual filesystems.
//!
//! Provides a fluent API for building VFS instances from generated code
//! or by adding files programmatically.
//!
//! # Examples
//!
//! ```
//! use mcp_files::FilesBuilder;
//!
//! let vfs = FilesBuilder::new()
//!     .add_file("/mcp-tools/manifest.json", "{}")
//!     .add_file("/mcp-tools/types.ts", "export type Params = {};")
//!     .build()
//!     .unwrap();
//!
//! assert_eq!(vfs.file_count(), 2);
//! ```

use crate::types::{FilesError, Result};
use crate::vfs::FileSystem;
use mcp_codegen::GeneratedCode;
use std::fs;
use std::path::{Path, PathBuf};

/// Builder for constructing a virtual filesystem.
///
/// `FilesBuilder` provides a fluent API for creating VFS instances,
/// with support for adding files individually or bulk-loading from
/// generated code.
///
/// # Examples
///
/// ## Building from scratch
///
/// ```
/// use mcp_files::FilesBuilder;
///
/// let vfs = FilesBuilder::new()
///     .add_file("/test.ts", "console.log('test');")
///     .build()
///     .unwrap();
///
/// assert!(vfs.exists("/test.ts"));
/// # Ok::<(), mcp_files::FilesError>(())
/// ```
///
/// ## Building from generated code
///
/// ```
/// use mcp_files::FilesBuilder;
/// use mcp_codegen::{GeneratedCode, GeneratedFile};
///
/// let mut code = GeneratedCode::new();
/// code.add_file(GeneratedFile {
///     path: "manifest.json".to_string(),
///     content: "{}".to_string(),
/// });
///
/// let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/test")
///     .build()
///     .unwrap();
///
/// assert!(vfs.exists("/mcp-tools/servers/test/manifest.json"));
/// # Ok::<(), mcp_files::FilesError>(())
/// ```
#[derive(Debug, Default)]
pub struct FilesBuilder {
    vfs: FileSystem,
    errors: Vec<FilesError>,
}

impl FilesBuilder {
    /// Creates a new empty VFS builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_files::FilesBuilder;
    ///
    /// let builder = FilesBuilder::new();
    /// let vfs = builder.build().unwrap();
    /// assert_eq!(vfs.file_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            vfs: FileSystem::new(),
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
    /// use mcp_files::FilesBuilder;
    /// use mcp_codegen::{GeneratedCode, GeneratedFile};
    ///
    /// let mut code = GeneratedCode::new();
    /// code.add_file(GeneratedFile {
    ///     path: "types.ts".to_string(),
    ///     content: "export type Params = {};".to_string(),
    /// });
    ///
    /// let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/test")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert!(vfs.exists("/mcp-tools/servers/test/types.ts"));
    /// # Ok::<(), mcp_files::FilesError>(())
    /// ```
    #[must_use]
    pub fn from_generated_code(code: GeneratedCode, base_path: impl AsRef<Path>) -> Self {
        let mut builder = Self::new();
        let base = base_path.as_ref().to_string_lossy();

        // Ensure base path ends with a trailing slash for proper joining
        let base_normalized = if base.ends_with('/') {
            base.into_owned()
        } else {
            format!("{base}/")
        };

        for file in code.files {
            // Use string concatenation to maintain Unix-style paths on all platforms
            // This ensures VFS paths are always forward-slash separated, even on Windows
            let full_path = format!("{}{}", base_normalized, file.path);
            builder = builder.add_file(full_path.as_str(), file.content);
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
    /// use mcp_files::FilesBuilder;
    ///
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/test.ts", "export const x = 1;")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(vfs.read_file("/test.ts").unwrap(), "export const x = 1;");
    /// # Ok::<(), mcp_files::FilesError>(())
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
    /// use mcp_files::FilesBuilder;
    ///
    /// let files = vec![
    ///     ("/file1.ts", "content1"),
    ///     ("/file2.ts", "content2"),
    /// ];
    ///
    /// let vfs = FilesBuilder::new()
    ///     .add_files(files)
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(vfs.file_count(), 2);
    /// # Ok::<(), mcp_files::FilesError>(())
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

    /// Builds the VFS and exports all files to the real filesystem.
    ///
    /// Files are written to disk at the specified base path with atomic
    /// operations (write to temp file, then rename). Parent directories
    /// are created automatically. The tilde (`~`) is expanded to the
    /// user's home directory.
    ///
    /// # Arguments
    ///
    /// * `base_path` - Root directory for export (e.g., `~/.claude/servers/`)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Any file path is invalid
    /// - Home directory cannot be determined (when using `~`)
    /// - I/O operations fail (permissions, disk space, etc.)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_files::FilesBuilder;
    ///
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/github/createIssue.ts", "export function createIssue() {}")
    ///     .build_and_export("~/.claude/servers/")?;
    ///
    /// // Files are now at: ~/.claude/servers/github/createIssue.ts
    /// # Ok::<(), mcp_files::FilesError>(())
    /// ```
    pub fn build_and_export(self, base_path: impl AsRef<Path>) -> Result<FileSystem> {
        // First, build the VFS to check for errors
        let vfs = self.build()?;

        // Expand tilde in path
        let base = expand_tilde(base_path.as_ref())?;

        // Export all files to disk
        for path in vfs.all_paths() {
            let content = vfs.read_file(path)?;
            write_file_atomic(&base, path.as_str(), content)?;
        }

        Ok(vfs)
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
    /// use mcp_files::FilesBuilder;
    ///
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/test.ts", "content")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(vfs.file_count(), 1);
    /// ```
    ///
    /// ```
    /// use mcp_files::FilesBuilder;
    ///
    /// let result = FilesBuilder::new()
    ///     .add_file("invalid/relative/path", "content")
    ///     .build();
    ///
    /// assert!(result.is_err());
    /// ```
    pub fn build(self) -> Result<FileSystem> {
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
    /// use mcp_files::FilesBuilder;
    ///
    /// let mut builder = FilesBuilder::new();
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

/// Expands tilde (~) in path to user's home directory.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
fn expand_tilde(path: &Path) -> Result<PathBuf> {
    let path_str = path.to_str().ok_or_else(|| FilesError::InvalidPath {
        path: path.display().to_string(),
    })?;

    if path_str.starts_with("~/") || path_str == "~" {
        let home = dirs::home_dir().ok_or_else(|| FilesError::IoError {
            path: path_str.to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Cannot determine home directory",
            ),
        })?;

        if path_str == "~" {
            Ok(home)
        } else {
            Ok(home.join(&path_str[2..]))
        }
    } else {
        Ok(path.to_path_buf())
    }
}

/// Writes file content to disk atomically using temp file + rename.
///
/// Creates parent directories automatically. Uses atomic write pattern:
/// 1. Write to temporary file
/// 2. Rename temp file to final path
///
/// This ensures no partial files are visible if write fails.
///
/// # Security
///
/// - Validates path to prevent directory traversal
/// - Creates parent directories with mode 0755
/// - Writes files with default permissions (typically 0644)
///
/// # Errors
///
/// Returns an error if I/O operations fail.
fn write_file_atomic(base_path: &Path, vfs_path: &str, content: &str) -> Result<()> {
    // Remove leading slash and validate
    let relative_path = vfs_path.strip_prefix('/').unwrap_or(vfs_path);

    // Security: Check for directory traversal
    if relative_path.contains("..") {
        return Err(FilesError::InvalidPathComponent {
            path: vfs_path.to_string(),
        });
    }

    // Construct full disk path
    let disk_path = base_path.join(relative_path);

    // Create parent directories
    if let Some(parent) = disk_path.parent() {
        fs::create_dir_all(parent).map_err(|e| FilesError::IoError {
            path: parent.display().to_string(),
            source: e,
        })?;
    }

    // Atomic write: write to temp file, then rename
    let temp_path = disk_path.with_extension("tmp");

    fs::write(&temp_path, content).map_err(|e| FilesError::IoError {
        path: temp_path.display().to_string(),
        source: e,
    })?;

    fs::rename(&temp_path, &disk_path).map_err(|e| FilesError::IoError {
        path: disk_path.display().to_string(),
        source: e,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_codegen::GeneratedFile;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_builder_new() {
        let builder = FilesBuilder::new();
        let vfs = builder.build().unwrap();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_builder_default() {
        let builder = FilesBuilder::default();
        let vfs = builder.build().unwrap();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_add_file() {
        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "content")
            .build()
            .unwrap();

        assert_eq!(vfs.file_count(), 1);
        assert_eq!(vfs.read_file("/test.ts").unwrap(), "content");
    }

    #[test]
    fn test_add_file_invalid_path() {
        let result = FilesBuilder::new()
            .add_file("relative/path", "content")
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().is_invalid_path());
    }

    #[test]
    fn test_add_files() {
        let files = vec![("/file1.ts", "content1"), ("/file2.ts", "content2")];

        let vfs = FilesBuilder::new().add_files(files).build().unwrap();

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

        let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/test")
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

        let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/test")
            .build()
            .unwrap();

        assert!(vfs.exists("/mcp-tools/servers/test/tools/sendMessage.ts"));
    }

    #[test]
    fn test_file_count() {
        let mut builder = FilesBuilder::new();
        assert_eq!(builder.file_count(), 0);

        builder = builder.add_file("/test1.ts", "");
        assert_eq!(builder.file_count(), 1);

        builder = builder.add_file("/test2.ts", "");
        assert_eq!(builder.file_count(), 2);
    }

    #[test]
    fn test_chaining() {
        let vfs = FilesBuilder::new()
            .add_file("/file1.ts", "content1")
            .add_file("/file2.ts", "content2")
            .add_file("/file3.ts", "content3")
            .build()
            .unwrap();

        assert_eq!(vfs.file_count(), 3);
    }

    #[test]
    fn test_error_collection() {
        let result = FilesBuilder::new()
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

        let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/test")
            .add_file("/mcp-tools/servers/test/manual.ts", "// manual")
            .build()
            .unwrap();

        assert_eq!(vfs.file_count(), 2);
        assert!(vfs.exists("/mcp-tools/servers/test/generated.ts"));
        assert!(vfs.exists("/mcp-tools/servers/test/manual.ts"));
    }

    // Tests for build_and_export

    #[test]
    fn test_build_and_export_creates_files() {
        let temp_dir = TempDir::new().unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "export const VERSION = '1.0';")
            .build_and_export(temp_dir.path())
            .unwrap();

        // Verify file was created on disk
        let file_path = temp_dir.path().join("test.ts");
        assert!(file_path.exists(), "File should exist on disk");

        // Verify content matches
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "export const VERSION = '1.0';");

        // Verify VFS was also returned correctly
        assert_eq!(vfs.file_count(), 1);
        assert_eq!(
            vfs.read_file("/test.ts").unwrap(),
            "export const VERSION = '1.0';"
        );
    }

    #[test]
    fn test_build_and_export_preserves_structure() {
        let temp_dir = TempDir::new().unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/index.ts", "export {};")
            .add_file("/tools/create.ts", "export function create() {}")
            .add_file("/tools/update.ts", "export function update() {}")
            .add_file("/types/models.ts", "export type Model = {};")
            .build_and_export(temp_dir.path())
            .unwrap();

        // Verify directory hierarchy
        assert!(temp_dir.path().join("index.ts").exists());
        assert!(temp_dir.path().join("tools").is_dir());
        assert!(temp_dir.path().join("tools/create.ts").exists());
        assert!(temp_dir.path().join("tools/update.ts").exists());
        assert!(temp_dir.path().join("types").is_dir());
        assert!(temp_dir.path().join("types/models.ts").exists());

        // Verify VFS
        assert_eq!(vfs.file_count(), 4);
    }

    #[test]
    fn test_build_and_export_creates_parent_dirs() {
        let temp_dir = TempDir::new().unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/deeply/nested/path/to/file.ts", "content")
            .build_and_export(temp_dir.path())
            .unwrap();

        let file_path = temp_dir.path().join("deeply/nested/path/to/file.ts");
        assert!(file_path.exists());
        assert_eq!(fs::read_to_string(file_path).unwrap(), "content");
        assert_eq!(vfs.file_count(), 1);
    }

    #[test]
    fn test_build_and_export_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();

        // First export
        let vfs1 = FilesBuilder::new()
            .add_file("/test.ts", "original content")
            .build_and_export(temp_dir.path())
            .unwrap();

        assert_eq!(vfs1.file_count(), 1);
        let file_path = temp_dir.path().join("test.ts");
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "original content");

        // Second export with updated content
        let vfs2 = FilesBuilder::new()
            .add_file("/test.ts", "updated content")
            .build_and_export(temp_dir.path())
            .unwrap();

        assert_eq!(vfs2.file_count(), 1);
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "updated content");
    }

    #[test]
    fn test_build_and_export_returns_vfs() {
        let temp_dir = TempDir::new().unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/file1.ts", "content1")
            .add_file("/file2.ts", "content2")
            .build_and_export(temp_dir.path())
            .unwrap();

        // VFS should be fully functional
        assert_eq!(vfs.file_count(), 2);
        assert!(vfs.exists("/file1.ts"));
        assert!(vfs.exists("/file2.ts"));
        assert_eq!(vfs.read_file("/file1.ts").unwrap(), "content1");
        assert_eq!(vfs.read_file("/file2.ts").unwrap(), "content2");
    }

    #[test]
    fn test_build_and_export_with_invalid_path_in_vfs() {
        let temp_dir = TempDir::new().unwrap();

        let result = FilesBuilder::new()
            .add_file("/valid.ts", "content")
            .add_file("invalid/relative", "content")
            .build_and_export(temp_dir.path());

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.is_invalid_path());
    }

    #[test]
    fn test_build_and_export_multiple_files() {
        let temp_dir = TempDir::new().unwrap();

        let files = vec![
            ("/index.ts", "export {};"),
            ("/tool1.ts", "export function tool1() {}"),
            ("/tool2.ts", "export function tool2() {}"),
            ("/manifest.json", r#"{"version": "1.0.0"}"#),
        ];

        let vfs = FilesBuilder::new()
            .add_files(files)
            .build_and_export(temp_dir.path())
            .unwrap();

        assert_eq!(vfs.file_count(), 4);
        assert!(temp_dir.path().join("index.ts").exists());
        assert!(temp_dir.path().join("tool1.ts").exists());
        assert!(temp_dir.path().join("tool2.ts").exists());
        assert!(temp_dir.path().join("manifest.json").exists());
    }

    #[test]
    fn test_build_and_export_empty_vfs() {
        let temp_dir = TempDir::new().unwrap();

        let vfs = FilesBuilder::new()
            .build_and_export(temp_dir.path())
            .unwrap();

        assert_eq!(vfs.file_count(), 0);
        // Directory should be created even if empty
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn test_expand_tilde_expands_home() {
        let path = Path::new("~/test/path");
        let expanded = expand_tilde(path).unwrap();

        // Should not contain tilde anymore
        assert!(!expanded.to_string_lossy().contains('~'));

        // Should be absolute
        assert!(expanded.is_absolute());
    }

    #[test]
    fn test_expand_tilde_preserves_absolute() {
        let path = Path::new("/absolute/path");
        let expanded = expand_tilde(path).unwrap();

        assert_eq!(expanded, Path::new("/absolute/path"));
    }

    #[test]
    fn test_expand_tilde_just_tilde() {
        let path = Path::new("~");
        let expanded = expand_tilde(path).unwrap();

        // Should expand to home directory
        assert!(expanded.is_absolute());
        assert!(!expanded.to_string_lossy().contains('~'));
    }

    #[test]
    fn test_write_file_atomic_directory_traversal() {
        let temp_dir = TempDir::new().unwrap();

        let result = write_file_atomic(temp_dir.path(), "/../etc/passwd", "malicious");

        assert!(result.is_err());
        assert!(result.unwrap_err().is_invalid_path());
    }

    #[test]
    fn test_write_file_atomic_creates_parents() {
        let temp_dir = TempDir::new().unwrap();

        write_file_atomic(
            temp_dir.path(),
            "/deep/nested/structure/file.txt",
            "content",
        )
        .unwrap();

        let file_path = temp_dir.path().join("deep/nested/structure/file.txt");
        assert!(file_path.exists());
        assert_eq!(fs::read_to_string(file_path).unwrap(), "content");
    }

    #[test]
    fn test_build_and_export_from_generated_code() {
        let temp_dir = TempDir::new().unwrap();

        let mut code = GeneratedCode::new();
        code.add_file(GeneratedFile {
            path: "index.ts".to_string(),
            content: "export {};".to_string(),
        });
        code.add_file(GeneratedFile {
            path: "tools/create.ts".to_string(),
            content: "export function create() {}".to_string(),
        });

        let vfs = FilesBuilder::from_generated_code(code, "/github")
            .build_and_export(temp_dir.path())
            .unwrap();

        assert_eq!(vfs.file_count(), 2);
        assert!(temp_dir.path().join("github/index.ts").exists());
        assert!(temp_dir.path().join("github/tools/create.ts").exists());
    }

    #[test]
    fn test_build_and_export_unicode_content() {
        let temp_dir = TempDir::new().unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/unicode.ts", "export const emoji = '🚀';")
            .build_and_export(temp_dir.path())
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("unicode.ts")).unwrap();
        assert_eq!(content, "export const emoji = '🚀';");
        assert_eq!(vfs.file_count(), 1);
    }

    #[test]
    fn test_build_and_export_large_content() {
        let temp_dir = TempDir::new().unwrap();

        // Create a large file (100KB)
        let large_content = "x".repeat(100_000);

        let vfs = FilesBuilder::new()
            .add_file("/large.ts", &large_content)
            .build_and_export(temp_dir.path())
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("large.ts")).unwrap();
        assert_eq!(content.len(), 100_000);
        assert_eq!(vfs.file_count(), 1);
    }
}
