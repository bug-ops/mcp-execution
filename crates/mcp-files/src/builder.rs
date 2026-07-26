//! Builder pattern for constructing virtual filesystems.
//!
//! Provides a fluent API for building VFS instances from generated code
//! or by adding files programmatically.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_files::FilesBuilder;
//!
//! let vfs = FilesBuilder::new()
//!     .add_file("/mcp-tools/manifest.json", "{}")
//!     .add_file("/mcp-tools/types.ts", "export type Params = {};")
//!     .build()
//!     .unwrap();
//!
//! assert_eq!(vfs.file_count(), 2);
//! ```

use crate::filesystem::FileSystem;
use crate::types::{FilesError, Result};
use mcp_execution_codegen::GeneratedCode;
use std::collections::BTreeMap;
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
/// use mcp_execution_files::FilesBuilder;
///
/// let vfs = FilesBuilder::new()
///     .add_file("/test.ts", "console.log('test');")
///     .build()
///     .unwrap();
///
/// assert!(vfs.exists("/test.ts"));
/// # Ok::<(), mcp_execution_files::FilesError>(())
/// ```
///
/// ## Building from generated code
///
/// ```
/// use mcp_execution_files::FilesBuilder;
/// use mcp_execution_codegen::{GeneratedCode, GeneratedFile};
///
/// let mut code = GeneratedCode::new();
/// code.add_file(GeneratedFile {
///     path: "manifest.json".to_string(),
///     content: "{}".to_string(),
/// })
/// .unwrap();
///
/// let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/test")
///     .build()
///     .unwrap();
///
/// assert!(vfs.exists("/mcp-tools/servers/test/manifest.json"));
/// # Ok::<(), mcp_execution_files::FilesError>(())
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
    /// use mcp_execution_files::FilesBuilder;
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
    /// use mcp_execution_files::FilesBuilder;
    /// use mcp_execution_codegen::{GeneratedCode, GeneratedFile};
    ///
    /// let mut code = GeneratedCode::new();
    /// code.add_file(GeneratedFile {
    ///     path: "types.ts".to_string(),
    ///     content: "export type Params = {};".to_string(),
    /// })
    /// .unwrap();
    ///
    /// let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/test")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert!(vfs.exists("/mcp-tools/servers/test/types.ts"));
    /// # Ok::<(), mcp_execution_files::FilesError>(())
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
    /// use mcp_execution_files::FilesBuilder;
    ///
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/test.ts", "export const x = 1;")
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(vfs.read_file("/test.ts").unwrap(), "export const x = 1;");
    /// # Ok::<(), mcp_execution_files::FilesError>(())
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
    /// use mcp_execution_files::FilesBuilder;
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
    /// # Ok::<(), mcp_execution_files::FilesError>(())
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
    /// Unlike [`FileSystem::export_to_filesystem`], `base_path` here is not a
    /// directory this call owns exclusively — callers routinely export
    /// multiple independent batches (e.g. one per MCP server) into the same
    /// shared root, such as `~/.claude/servers/`, so a whole-`base_path`
    /// staging swap would delete every sibling batch already published
    /// there (see [`Self::from_generated_code`], whose files all share one
    /// top-level directory per call).
    ///
    /// Files are grouped by their top-level path component. A group with a
    /// real subdirectory (e.g. `/github/createIssue.ts` and
    /// `/github/getIssue.ts`, grouped under `github`) is published as a
    /// whole via [`FileSystem::export_to_filesystem`] — the same
    /// staging/atomic-rename mechanism `export_to_filesystem` gives its own
    /// target — so that entire subtree lands under `base_path` atomically,
    /// without disturbing sibling groups already there. Because that
    /// publish reuses `export_to_filesystem`'s directory swap, it also
    /// inherits its replace-not-merge semantics *for that one group*: a
    /// second `build_and_export` call for the same top-level group (e.g.
    /// re-exporting `/github/...` with a smaller tool set) deletes any file
    /// previously under `base_path/github` that is absent from the new
    /// batch, rather than only adding/updating what the new batch contains.
    /// Sibling groups and bare top-level files elsewhere under `base_path`
    /// are unaffected either way. A bare file with no subdirectory (e.g.
    /// `/manifest.json`) is written directly with its own atomic
    /// temp-file-then-rename, which is already a complete atomic unit on its
    /// own and is unconditionally additive/overwriting, never deleting
    /// unrelated files. This gives per-top-level-group atomicity, not
    /// whole-batch atomicity: if a batch spans multiple groups and one
    /// group's publish fails partway through processing the batch, groups
    /// already published (or bare files already written) are unaffected and
    /// remain in place, but the batch as a whole is not rolled back.
    ///
    /// As with [`FileSystem::export_to_filesystem`], concurrent calls that
    /// publish the *same* group into the *same* `base_path` from different
    /// processes are not locked against each other and can race on the
    /// final swap (a lost update); concurrent calls publishing different
    /// groups are safe. `base_path` is created (via `create_dir_all`) even
    /// for an empty VFS. The tilde (`~`) is expanded to the user's home
    /// directory before export.
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
    /// - The batch's file count or total byte size exceeds
    ///   [`crate::filesystem::MAX_EXPORT_FILES`] /
    ///   [`crate::filesystem::MAX_EXPORT_BYTES`]
    /// - `base_path` cannot be created
    /// - I/O operations fail (permissions, disk space, etc.)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_files::FilesBuilder;
    ///
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/github/createIssue.ts", "export function createIssue() {}")
    ///     .build_and_export("~/.claude/servers/")?;
    ///
    /// // Files are now at: ~/.claude/servers/github/createIssue.ts
    /// # Ok::<(), mcp_execution_files::FilesError>(())
    /// ```
    pub fn build_and_export(self, base_path: impl AsRef<Path>) -> Result<FileSystem> {
        // First, build the VFS to check for errors
        let vfs = self.build()?;

        // Bound the whole batch up front (not just each group below), so a
        // payload crafted as many small groups can't bypass the per-group
        // check each `export_to_filesystem` call below performs on its own.
        vfs.check_export_bounds()?;

        // Expand tilde in path
        let base = expand_tilde(base_path.as_ref())?;

        fs::create_dir_all(&base).map_err(|e| FilesError::IoError {
            path: base.display().to_string(),
            source: e,
        })?;

        // Split the batch into per-top-level-group sub-filesystems (relative
        // to their group root) plus a list of bare top-level files. A
        // `BTreeMap` gives deterministic (alphabetical) publish order, which
        // doesn't affect correctness but makes a partial-batch outcome
        // reproducible.
        let mut groups: BTreeMap<String, FileSystem> = BTreeMap::new();

        for path in vfs.all_paths() {
            let content = vfs.read_file(path)?;
            let relative = path.as_str().strip_prefix('/').unwrap_or(path.as_str());

            match relative.split_once('/') {
                Some((root, rest)) => {
                    groups
                        .entry(root.to_string())
                        .or_default()
                        .add_file(format!("/{rest}"), content)?;
                }
                None => write_file_atomic(&base, path.as_str(), content)?,
            }
        }

        for (root, group_vfs) in groups {
            group_vfs.export_to_filesystem(base.join(root))?;
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
    /// use mcp_execution_files::FilesBuilder;
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
    /// use mcp_execution_files::FilesBuilder;
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
    /// use mcp_execution_files::FilesBuilder;
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

/// Writes file content to disk atomically, creating parent directories
/// automatically.
///
/// Delegates the actual write to [`crate::filesystem::write_file_atomic`]
/// (temp file + `fsync` + rename) for durability parity with every other
/// export path in this crate, after resolving `vfs_path` against `base_path`
/// and validating it.
///
/// # Security
///
/// - Validates path to prevent directory traversal
/// - Creates parent directories with mode 0755
/// - Writes files with default permissions (typically 0644)
///
/// # Errors
///
/// Returns an error if `vfs_path` contains a `..` component, or if I/O
/// operations fail.
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

    crate::filesystem::write_file_atomic(&disk_path, content, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_codegen::GeneratedFile;
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

        assert!(matches!(result, Err(FilesError::PathNotAbsolute { .. })));
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
        })
        .unwrap();
        code.add_file(GeneratedFile {
            path: "types.ts".to_string(),
            content: "export {};".to_string(),
        })
        .unwrap();

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
        })
        .unwrap();

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
        })
        .unwrap();

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
    fn test_build_and_export_group_replaces_not_merges_on_re_export() {
        // Unlike `test_build_and_export_overwrites_existing` (a bare
        // top-level file, always additive/overwriting), a re-export of the
        // same top-level *group* goes through `FileSystem::export_to_filesystem`'s
        // directory swap and replaces the whole group: a file present in the
        // first export but absent from the second must be deleted, not just
        // left alone.
        let temp_dir = TempDir::new().unwrap();

        let vfs1 = FilesBuilder::new()
            .add_file("/github/createIssue.ts", "create v1")
            .add_file("/github/getIssue.ts", "get v1")
            .build_and_export(temp_dir.path())
            .unwrap();
        assert_eq!(vfs1.file_count(), 2);
        assert!(temp_dir.path().join("github/getIssue.ts").exists());

        let vfs2 = FilesBuilder::new()
            .add_file("/github/createIssue.ts", "create v2")
            .build_and_export(temp_dir.path())
            .unwrap();
        assert_eq!(vfs2.file_count(), 1);

        assert_eq!(
            fs::read_to_string(temp_dir.path().join("github/createIssue.ts")).unwrap(),
            "create v2"
        );
        assert!(
            !temp_dir.path().join("github/getIssue.ts").exists(),
            "a file present in the first export but absent from the second must be deleted, \
             not merged/left behind"
        );
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

        assert!(matches!(result, Err(FilesError::PathNotAbsolute { .. })));
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
    fn test_build_and_export_group_replaces_file_collision_without_losing_siblings() {
        // Regression test for a real bug found during review: a batch spanning
        // a bare top-level file plus a group whose name collides with a
        // pre-existing plain file at that path (e.g. `base/sub` already exists
        // as a file, but this batch wants to publish `/sub/nested.ts`). The
        // group publish goes through `FileSystem::export_to_filesystem`, which
        // displaces the existing file and replaces it with the new directory —
        // this must succeed as a whole, and must never leave bare sibling
        // files (already written directly, before the group publish runs)
        // missing or corrupted.
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("sub"), "not a directory").unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/first.ts", "first")
            .add_file("/second.ts", "second")
            .add_file("/sub/nested.ts", "nested")
            .build_and_export(temp_dir.path())
            .unwrap();

        assert_eq!(vfs.file_count(), 3);
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("first.ts")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("second.ts")).unwrap(),
            "second"
        );
        assert!(temp_dir.path().join("sub").is_dir());
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("sub/nested.ts")).unwrap(),
            "nested"
        );

        // Regression check: displacing a *file* (rather than a directory)
        // used to leave an unremovable `.sub.stale-*` artifact behind forever,
        // since `remove_dir_all` always fails on a plain file and the error
        // was silently discarded. Nothing but the three published entries
        // should remain in `base_path`.
        let mut children: Vec<String> = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        children.sort();
        assert_eq!(children, vec!["first.ts", "second.ts", "sub"]);
    }

    #[test]
    fn test_build_and_export_rejects_empty_top_level_component() {
        // Regression test for a real bug found during review: `"//x.ts"` used
        // to pass `FilePath` validation and split into an *empty* top-level
        // group name, so `base.join("")` resolved to `base` itself — the
        // group publish would then swap `base_path` wholesale via
        // `FileSystem::export_to_filesystem`, deleting every sibling batch
        // already published there. `FilePath::new` now rejects this at
        // `add_file` time, well before grouping.
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("existing-sibling.ts"), "keep").unwrap();

        let result = FilesBuilder::new()
            .add_file("//x.ts", "malicious")
            .build_and_export(temp_dir.path());

        assert!(matches!(
            result,
            Err(FilesError::InvalidPathComponent { .. })
        ));
        // Rejected during `build()`, before any group ever touches `base_path`.
        assert!(temp_dir.path().join("existing-sibling.ts").exists());
    }

    #[test]
    fn test_build_and_export_rejects_dot_component() {
        let temp_dir = TempDir::new().unwrap();

        let result = FilesBuilder::new()
            .add_file("/./x.ts", "content")
            .build_and_export(temp_dir.path());

        assert!(matches!(
            result,
            Err(FilesError::InvalidPathComponent { .. })
        ));
    }

    #[test]
    fn test_build_and_export_rejects_oversized_batch_before_touching_base() {
        let temp_dir = TempDir::new().unwrap();
        let mut builder = FilesBuilder::new();
        // Each file lives in its own group, so no single group's own
        // `export_to_filesystem` bound check would catch this — only the
        // whole-batch check in `build_and_export` itself can.
        for i in 0..=crate::filesystem::MAX_EXPORT_FILES {
            builder = builder.add_file(format!("/group{i}/tool.ts"), "export {}");
        }

        let target = temp_dir.path().join("out");
        let result = builder.build_and_export(&target);

        assert!(matches!(
            result,
            Err(FilesError::ResourceLimitExceeded { .. })
        ));
        // Rejected before any per-group publish ran, so the target directory
        // (which build_and_export would otherwise create unconditionally)
        // must never have been created.
        assert!(!target.exists());
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

        assert!(matches!(
            result,
            Err(FilesError::InvalidPathComponent { .. })
        ));
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
        })
        .unwrap();
        code.add_file(GeneratedFile {
            path: "tools/create.ts".to_string(),
            content: "export function create() {}".to_string(),
        })
        .unwrap();

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
