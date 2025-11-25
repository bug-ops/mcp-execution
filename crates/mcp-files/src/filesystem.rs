//! Filesystem export for VFS.
//!
//! Provides high-performance export of VFS contents to real filesystem,
//! optimized for progressive loading pattern with batching and parallelization.
//!
//! # Performance Optimizations
//!
//! 1. **Directory Pre-creation**: Creates all directories first in single pass
//! 2. **Parallel Writes**: Uses rayon for parallel file writing (opt-in)
//! 3. **Atomic Operations**: Writes to temp file then renames
//! 4. **Minimal Allocations**: Reuses path buffers, caches canonicalized base
//!
//! # Examples
//!
//! ## Sequential export (safe default)
//!
//! ```
//! use mcp_files::FilesBuilder;
//! use std::path::PathBuf;
//! # use tempfile::TempDir;
//!
//! # let temp_dir = TempDir::new().unwrap();
//! # let output_dir = temp_dir.path();
//! let vfs = FilesBuilder::new()
//!     .add_file("/tools/create.ts", "export function create() {}")
//!     .add_file("/tools/update.ts", "export function update() {}")
//!     .build()
//!     .unwrap();
//!
//! // Export to filesystem
//! vfs.export_to_filesystem(output_dir).unwrap();
//!
//! assert!(output_dir.join("tools/create.ts").exists());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Parallel export (faster for many files)
//!
//! ```
//! use mcp_files::FilesBuilder;
//! # use tempfile::TempDir;
//!
//! # let temp_dir = TempDir::new().unwrap();
//! # let output_dir = temp_dir.path();
//! let vfs = FilesBuilder::new()
//!     .add_file("/tool1.ts", "export {}")
//!     .add_file("/tool2.ts", "export {}")
//!     .build()
//!     .unwrap();
//!
//! // Export with parallel writes (requires 'parallel' feature)
//! #[cfg(feature = "parallel")]
//! vfs.export_to_filesystem_parallel(output_dir).unwrap();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::types::{FilesError, Result};
use crate::vfs::FileSystem;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Options for filesystem export operations.
///
/// # Examples
///
/// ```
/// use mcp_files::ExportOptions;
///
/// let options = ExportOptions::default()
///     .with_atomic_writes(true)
///     .with_overwrite(true);
/// ```
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Use atomic writes (write to temp file, then rename)
    pub atomic: bool,
    /// Overwrite existing files
    pub overwrite: bool,
}

impl ExportOptions {
    /// Creates new export options with defaults.
    ///
    /// Defaults:
    /// - atomic: true (safer)
    /// - overwrite: true (common case)
    #[must_use]
    pub const fn new() -> Self {
        Self {
            atomic: true,
            overwrite: true,
        }
    }

    /// Sets whether to use atomic writes.
    #[must_use]
    pub const fn with_atomic_writes(mut self, atomic: bool) -> Self {
        self.atomic = atomic;
        self
    }

    /// Sets whether to overwrite existing files.
    #[must_use]
    pub const fn with_overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem {
    /// Exports VFS contents to real filesystem.
    ///
    /// This is a high-performance implementation optimized for the progressive
    /// loading pattern. It pre-creates all directories and writes files sequentially.
    ///
    /// # Performance
    ///
    /// Target: <50ms for 30 files (GitHub server typical case)
    ///
    /// Optimizations:
    /// - Single pass directory creation
    /// - Cached canonicalized base path
    /// - Minimal allocations
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Base path doesn't exist or isn't a directory
    /// - Permission denied
    /// - I/O error during write
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_files::FilesBuilder;
    /// # use tempfile::TempDir;
    ///
    /// # let temp = TempDir::new().unwrap();
    /// # let base = temp.path();
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/manifest.json", "{}")
    ///     .build()
    ///     .unwrap();
    ///
    /// vfs.export_to_filesystem(base).unwrap();
    /// assert!(base.join("manifest.json").exists());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn export_to_filesystem(&self, base_path: impl AsRef<Path>) -> Result<()> {
        self.export_to_filesystem_with_options(base_path, &ExportOptions::default())
    }

    /// Exports VFS contents with custom options.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Base path does not exist
    /// - Base path cannot be canonicalized
    /// - I/O operations fail during directory creation or file writing
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_files::{FilesBuilder, ExportOptions};
    /// # use tempfile::TempDir;
    ///
    /// # let temp = TempDir::new().unwrap();
    /// # let base = temp.path();
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/test.ts", "export {}")
    ///     .build()
    ///     .unwrap();
    ///
    /// let options = ExportOptions::default().with_atomic_writes(false);
    /// vfs.export_to_filesystem_with_options(base, &options).unwrap();
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn export_to_filesystem_with_options(
        &self,
        base_path: impl AsRef<Path>,
        options: &ExportOptions,
    ) -> Result<()> {
        let base = base_path.as_ref();

        // Validate base path exists
        if !base.exists() {
            return Err(FilesError::FileNotFound {
                path: base.display().to_string(),
            });
        }

        // Canonicalize base path once (performance optimization)
        let canonical_base = base.canonicalize().map_err(|e| FilesError::InvalidPath {
            path: format!("Failed to canonicalize {}: {}", base.display(), e),
        })?;

        // Phase 1: Collect all unique directories
        let dirs = self.collect_directories(&canonical_base);

        // Phase 2: Create all directories in one pass
        Self::create_directories(&dirs)?;

        // Phase 3: Write all files
        self.write_files(&canonical_base, options)?;

        Ok(())
    }

    /// Exports VFS contents using parallel writes (requires 'parallel' feature).
    ///
    /// Faster for large numbers of files (>50), but may not preserve write order.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Base path doesn't exist or isn't a directory
    /// - Permission denied during directory creation or file write
    /// - I/O error during parallel write operations
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_files::FilesBuilder;
    /// # use tempfile::TempDir;
    ///
    /// # let temp = TempDir::new().unwrap();
    /// # let base = temp.path();
    /// let vfs = FilesBuilder::new()
    ///     .add_file("/tool1.ts", "export {}")
    ///     .add_file("/tool2.ts", "export {}")
    ///     .build()
    ///     .unwrap();
    ///
    /// #[cfg(feature = "parallel")]
    /// vfs.export_to_filesystem_parallel(base).unwrap();
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[cfg(feature = "parallel")]
    pub fn export_to_filesystem_parallel(&self, base_path: impl AsRef<Path>) -> Result<()> {
        use rayon::prelude::*;

        let base = base_path.as_ref();
        let canonical_base = base.canonicalize().map_err(|e| FilesError::InvalidPath {
            path: format!("Failed to canonicalize {}: {}", base.display(), e),
        })?;

        // Phase 1: Collect and create directories (must be sequential)
        let dirs = self.collect_directories(&canonical_base);
        Self::create_directories(&dirs)?;

        // Phase 2: Write files in parallel
        let files: Vec<_> = self.files().collect();
        let options = ExportOptions::default();

        files
            .par_iter()
            .try_for_each(|(vfs_path, file)| -> Result<()> {
                let disk_path = Self::vfs_to_disk_path(vfs_path.as_str(), &canonical_base);
                write_file_atomic(&disk_path, file.content(), &options)
            })?;

        Ok(())
    }

    /// Collects all unique directory paths needed for export.
    ///
    /// This is done in a single pass to minimize allocations.
    fn collect_directories(&self, base: &Path) -> HashSet<PathBuf> {
        let mut dirs = HashSet::new();

        for (vfs_path, _) in self.files() {
            let disk_path = Self::vfs_to_disk_path(vfs_path.as_str(), base);

            // Add all parent directories
            if let Some(parent) = disk_path.parent() {
                // Insert parent and all ancestors
                let mut current = parent;
                while current != base && dirs.insert(current.to_path_buf()) {
                    if let Some(p) = current.parent() {
                        current = p;
                    } else {
                        break;
                    }
                }
            }
        }

        dirs
    }

    /// Creates all directories in one pass.
    ///
    /// Uses `fs::create_dir_all` which is efficient for creating directory trees.
    fn create_directories(dirs: &HashSet<PathBuf>) -> Result<()> {
        for dir in dirs {
            fs::create_dir_all(dir).map_err(|e| FilesError::InvalidPath {
                path: format!("Failed to create directory {}: {}", dir.display(), e),
            })?;
        }
        Ok(())
    }

    /// Writes all files to disk.
    fn write_files(&self, base: &Path, options: &ExportOptions) -> Result<()> {
        for (vfs_path, file) in self.files() {
            let disk_path = Self::vfs_to_disk_path(vfs_path.as_str(), base);
            write_file_atomic(&disk_path, file.content(), options)?;
        }
        Ok(())
    }

    /// Converts VFS path to disk path.
    ///
    /// Strips leading '/' and joins with base path.
    fn vfs_to_disk_path(vfs_path: &str, base: &Path) -> PathBuf {
        // Strip leading '/' from VFS path
        let relative = vfs_path.strip_prefix('/').unwrap_or(vfs_path);

        // Convert forward slashes to platform-specific separators
        let relative_path = if cfg!(target_os = "windows") {
            PathBuf::from(relative.replace('/', "\\"))
        } else {
            PathBuf::from(relative)
        };

        base.join(relative_path)
    }
}

/// Writes file content to disk atomically.
///
/// If atomic mode is enabled, writes to temp file then renames.
/// Otherwise, writes directly.
fn write_file_atomic(path: &Path, content: &str, options: &ExportOptions) -> Result<()> {
    // Check if file exists and we shouldn't overwrite
    if !options.overwrite && path.exists() {
        return Ok(());
    }

    if options.atomic {
        // Atomic write: temp file + rename
        let temp_path = path.with_extension("tmp");

        // Write to temp file
        let mut file = fs::File::create(&temp_path).map_err(|e| FilesError::InvalidPath {
            path: format!("Failed to create temp file {}: {}", temp_path.display(), e),
        })?;

        file.write_all(content.as_bytes())
            .map_err(|e| FilesError::InvalidPath {
                path: format!("Failed to write to {}: {}", temp_path.display(), e),
            })?;

        file.sync_all().map_err(|e| FilesError::InvalidPath {
            path: format!("Failed to sync {}: {}", temp_path.display(), e),
        })?;

        // Rename to final location
        fs::rename(&temp_path, path).map_err(|e| FilesError::InvalidPath {
            path: format!(
                "Failed to rename {} to {}: {}",
                temp_path.display(),
                path.display(),
                e
            ),
        })?;
    } else {
        // Direct write (faster, but not atomic)
        fs::write(path, content).map_err(|e| FilesError::InvalidPath {
            path: format!("Failed to write {}: {}", path.display(), e),
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilesBuilder;
    use tempfile::TempDir;

    #[test]
    fn test_export_single_file() {
        let temp = TempDir::new().unwrap();
        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "export const VERSION = '1.0';")
            .build()
            .unwrap();

        vfs.export_to_filesystem(temp.path()).unwrap();

        let exported = temp.path().join("test.ts");
        assert!(exported.exists());
        assert_eq!(
            fs::read_to_string(exported).unwrap(),
            "export const VERSION = '1.0';"
        );
    }

    #[test]
    fn test_export_nested_files() {
        let temp = TempDir::new().unwrap();
        let vfs = FilesBuilder::new()
            .add_file("/tools/create.ts", "export function create() {}")
            .add_file("/tools/update.ts", "export function update() {}")
            .add_file("/manifest.json", "{}")
            .build()
            .unwrap();

        vfs.export_to_filesystem(temp.path()).unwrap();

        assert!(temp.path().join("tools/create.ts").exists());
        assert!(temp.path().join("tools/update.ts").exists());
        assert!(temp.path().join("manifest.json").exists());
    }

    #[test]
    fn test_export_overwrite() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.ts");

        // Write initial file
        fs::write(&path, "old content").unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "new content")
            .build()
            .unwrap();

        vfs.export_to_filesystem(temp.path()).unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "new content");
    }

    #[test]
    fn test_export_no_overwrite() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.ts");

        // Write initial file
        fs::write(&path, "old content").unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "new content")
            .build()
            .unwrap();

        let options = ExportOptions::default().with_overwrite(false);
        vfs.export_to_filesystem_with_options(temp.path(), &options)
            .unwrap();

        // Should not overwrite
        assert_eq!(fs::read_to_string(path).unwrap(), "old content");
    }

    #[test]
    fn test_export_atomic_writes() {
        let temp = TempDir::new().unwrap();
        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "atomic content")
            .build()
            .unwrap();

        let options = ExportOptions::default().with_atomic_writes(true);
        vfs.export_to_filesystem_with_options(temp.path(), &options)
            .unwrap();

        let path = temp.path().join("test.ts");
        assert!(path.exists());
        assert_eq!(fs::read_to_string(path).unwrap(), "atomic content");

        // Temp file should be cleaned up
        let temp_path = temp.path().join("test.tmp");
        assert!(!temp_path.exists());
    }

    #[test]
    fn test_export_non_atomic_writes() {
        let temp = TempDir::new().unwrap();
        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "direct content")
            .build()
            .unwrap();

        let options = ExportOptions::default().with_atomic_writes(false);
        vfs.export_to_filesystem_with_options(temp.path(), &options)
            .unwrap();

        let path = temp.path().join("test.ts");
        assert_eq!(fs::read_to_string(path).unwrap(), "direct content");
    }

    #[test]
    fn test_export_invalid_base_path() {
        let vfs = FilesBuilder::new()
            .add_file("/test.ts", "")
            .build()
            .unwrap();

        let result = vfs.export_to_filesystem("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
    }

    #[test]
    fn test_export_many_files() {
        let temp = TempDir::new().unwrap();
        let mut builder = FilesBuilder::new();

        // Add 30 files (GitHub server typical case)
        for i in 0..30 {
            builder = builder.add_file(
                format!("/tools/tool{i}.ts"),
                format!("export function tool{i}() {{}}"),
            );
        }

        let vfs = builder.build().unwrap();
        vfs.export_to_filesystem(temp.path()).unwrap();

        // Verify all files exist
        for i in 0..30 {
            assert!(temp.path().join(format!("tools/tool{i}.ts")).exists());
        }
    }

    #[test]
    fn test_export_deep_nesting() {
        let temp = TempDir::new().unwrap();
        let vfs = FilesBuilder::new()
            .add_file("/a/b/c/d/e/deep.ts", "export {}")
            .build()
            .unwrap();

        vfs.export_to_filesystem(temp.path()).unwrap();

        assert!(temp.path().join("a/b/c/d/e/deep.ts").exists());
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn test_export_parallel() {
        let temp = TempDir::new().unwrap();
        let mut builder = FilesBuilder::new();

        for i in 0..100 {
            builder = builder.add_file(format!("/file{i}.ts"), format!("export const N = {i};"));
        }

        let vfs = builder.build().unwrap();
        vfs.export_to_filesystem_parallel(temp.path()).unwrap();

        // Verify all files exist
        for i in 0..100 {
            let path = temp.path().join(format!("file{i}.ts"));
            assert!(path.exists());
        }
    }

    #[test]
    fn test_export_options_default() {
        let options = ExportOptions::default();
        assert!(options.atomic);
        assert!(options.overwrite);
    }

    #[test]
    fn test_export_options_builder() {
        let options = ExportOptions::new()
            .with_atomic_writes(false)
            .with_overwrite(false);

        assert!(!options.atomic);
        assert!(!options.overwrite);
    }
}
