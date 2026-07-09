//! In-memory filesystem and export functionality.
//!
//! Provides an in-memory filesystem for MCP tool definitions with
//! high-performance export to real filesystem.
//!
//! # Core Features
//!
//! - **In-memory storage**: Files stored in `HashMap` for O(1) lookup
//! - **Filesystem export**: Sequential and parallel export modes
//! - **Atomic writes**: Optional atomic file operations
//! - **Thread-safe**: All types are `Send + Sync`
//!
//! # Atomicity
//!
//! [`FileSystem::export_to_filesystem`] stages the entire export in a sibling
//! temporary directory and only publishes it by renaming that directory into
//! place once every file has been written successfully. A process interrupted
//! mid-export leaves the previous export (or nothing, on a first export)
//! untouched at the target path — never a partially written tree.
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
//! ## Basic usage
//!
//! ```
//! use mcp_execution_files::FileSystem;
//!
//! let mut fs = FileSystem::new();
//! fs.add_file("/mcp-tools/test.ts", "export const VERSION = '1.0';").unwrap();
//!
//! let content = fs.read_file("/mcp-tools/test.ts").unwrap();
//! assert_eq!(content, "export const VERSION = '1.0';");
//! ```
//!
//! ## Export to filesystem
//!
//! ```
//! use mcp_execution_files::FilesBuilder;
//! # use tempfile::TempDir;
//!
//! # let temp_dir = TempDir::new().unwrap();
//! # let output_dir = temp_dir.path();
//! let fs = FilesBuilder::new()
//!     .add_file("/tools/create.ts", "export function create() {}")
//!     .add_file("/tools/update.ts", "export function update() {}")
//!     .build()
//!     .unwrap();
//!
//! // Export to filesystem
//! fs.export_to_filesystem(output_dir).unwrap();
//!
//! assert!(output_dir.join("tools/create.ts").exists());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::types::{FileEntry, FilePath, FilesError, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

/// Minimum age (by mtime) a staging/stale sibling must have before
/// [`FileSystem::sweep_stale_artifacts`] will remove it.
///
/// A genuinely concurrent sibling export's staging and displaced
/// directories are kept younger than this: `staging` is freshly created via
/// `tempfile` for every export, and [`FileSystem::displace_existing_target`]
/// refreshes `target`'s mtime via [`FileSystem::touch_dir`] immediately
/// *before* renaming it aside to `displaced` — a rename preserves the
/// source's mtime, so `displaced` carries that fresh timestamp from the
/// instant it exists, rather than inheriting `target`'s (typically much
/// older) original mtime and being immediately sweep-eligible. All of the
/// work between
/// creating/refreshing one of these directories and either publishing or
/// rolling it back happens within a single
/// `export_to_filesystem_with_options` call, which completes in well under
/// this window even for large tool sets — assuming that call keeps adding
/// or removing immediate children of `staging` as it writes, since that is
/// what keeps its mtime fresh on the filesystems this crate supports.
/// Gating on age means the sweep can only ever reclaim directories that are
/// old enough to be leftovers from a process that was killed (e.g.
/// `SIGKILL`) before it could clean up after itself — never a live
/// sibling's in-flight artifacts. This is what closes the data-loss race
/// described in issue #169: previously the sweep matched purely on name, so
/// a concurrent export of the same target could delete another in-flight
/// export's staging directory, and — if the timing landed between
/// `swap_into_place`'s two renames — the displaced original too, defeating
/// the rollback and permanently losing the target.
const STALE_ARTIFACT_MIN_AGE: Duration = Duration::from_mins(5);

/// An in-memory virtual filesystem for MCP tool definitions.
///
/// `FileSystem` provides a read-only filesystem structure that stores generated
/// TypeScript files in memory. Files are organized in a hierarchical structure
/// like `/mcp-tools/servers/<server-id>/...`.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, making it safe to use across threads.
///
/// # Examples
///
/// ```
/// use mcp_execution_files::FileSystem;
///
/// let mut vfs = FileSystem::new();
/// vfs.add_file("/mcp-tools/manifest.json", "{}").unwrap();
///
/// assert!(vfs.exists("/mcp-tools/manifest.json"));
/// assert_eq!(vfs.file_count(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct FileSystem {
    files: HashMap<FilePath, FileEntry>,
}

impl FileSystem {
    /// Creates a new empty virtual filesystem.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let vfs = FileSystem::new();
    /// assert_eq!(vfs.file_count(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Adds a file to the virtual filesystem.
    ///
    /// If a file already exists at the path, it will be replaced.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid (not absolute, contains '..', etc.).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// vfs.add_file("/mcp-tools/test.ts", "console.log('hello');").unwrap();
    ///
    /// assert!(vfs.exists("/mcp-tools/test.ts"));
    /// # Ok::<(), mcp_execution_files::FilesError>(())
    /// ```
    pub fn add_file(&mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Result<()> {
        let vfs_path = FilePath::new(path)?;
        let file = FileEntry::new(content);
        self.files.insert(vfs_path, file);
        Ok(())
    }

    /// Reads the content of a file.
    ///
    /// # Errors
    ///
    /// Returns `FilesError::FileNotFound` if the file does not exist.
    /// Returns `FilesError::InvalidPath` if the path is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// vfs.add_file("/test.ts", "export {}").unwrap();
    ///
    /// let content = vfs.read_file("/test.ts").unwrap();
    /// assert_eq!(content, "export {}");
    /// # Ok::<(), mcp_execution_files::FilesError>(())
    /// ```
    pub fn read_file(&self, path: impl AsRef<Path>) -> Result<&str> {
        let vfs_path = FilePath::new(path)?;
        self.files
            .get(&vfs_path)
            .map(FileEntry::content)
            .ok_or_else(|| FilesError::FileNotFound {
                path: vfs_path.as_str().to_string(),
            })
    }

    /// Checks if a file exists at the given path.
    ///
    /// Returns `false` if the path is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// vfs.add_file("/exists.ts", "").unwrap();
    ///
    /// assert!(vfs.exists("/exists.ts"));
    /// assert!(!vfs.exists("/missing.ts"));
    /// ```
    #[must_use]
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        FilePath::new(path)
            .ok()
            .and_then(|p| self.files.get(&p))
            .is_some()
    }

    /// Lists all files and directories in a directory.
    ///
    /// Returns an empty vector if the directory is empty or does not exist.
    ///
    /// # Errors
    ///
    /// Returns `FilesError::InvalidPath` if the path is invalid.
    /// Returns `FilesError::NotADirectory` if the path points to a file.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// vfs.add_file("/mcp-tools/servers/test1.ts", "").unwrap();
    /// vfs.add_file("/mcp-tools/servers/test2.ts", "").unwrap();
    ///
    /// let entries = vfs.list_dir("/mcp-tools/servers").unwrap();
    /// assert_eq!(entries.len(), 2);
    /// # Ok::<(), mcp_execution_files::FilesError>(())
    /// ```
    pub fn list_dir(&self, path: impl AsRef<Path>) -> Result<Vec<FilePath>> {
        let vfs_path = FilePath::new(path)?;
        let path_str = vfs_path.as_str();

        // Check if the path itself is a file
        if self.files.contains_key(&vfs_path) {
            return Err(FilesError::NotADirectory {
                path: path_str.to_string(),
            });
        }

        // Collect all direct children
        let mut children = Vec::new();
        let normalized_dir = if path_str.ends_with('/') {
            path_str.to_string()
        } else {
            format!("{path_str}/")
        };

        for file_path in self.files.keys() {
            let file_str = file_path.as_str();

            // Check if this file is under the directory
            if file_str.starts_with(&normalized_dir) {
                let relative = &file_str[normalized_dir.len()..];

                // Only include direct children (no subdirectories)
                if !relative.contains('/') && !relative.is_empty() {
                    children.push(file_path.clone());
                } else if let Some(idx) = relative.find('/') {
                    // This is a subdirectory, add the directory path
                    let subdir = format!("{}{}", normalized_dir, &relative[..idx]);
                    if let Ok(subdir_path) = FilePath::new(subdir)
                        && !children.contains(&subdir_path)
                    {
                        children.push(subdir_path);
                    }
                }
            }
        }

        children.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        Ok(children)
    }

    /// Returns the total number of files in the VFS.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// assert_eq!(vfs.file_count(), 0);
    ///
    /// vfs.add_file("/test1.ts", "").unwrap();
    /// vfs.add_file("/test2.ts", "").unwrap();
    /// assert_eq!(vfs.file_count(), 2);
    /// ```
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns all file paths in the VFS.
    ///
    /// The paths are returned in sorted order.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// vfs.add_file("/a.ts", "").unwrap();
    /// vfs.add_file("/b.ts", "").unwrap();
    ///
    /// let paths = vfs.all_paths();
    /// assert_eq!(paths.len(), 2);
    /// ```
    #[must_use]
    pub fn all_paths(&self) -> Vec<&FilePath> {
        let mut paths: Vec<_> = self.files.keys().collect();
        paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        paths
    }

    /// Returns an iterator over all files in the VFS.
    ///
    /// Each item is a tuple of `(&FilePath, &FileEntry)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// vfs.add_file("/a.ts", "content a").unwrap();
    /// vfs.add_file("/b.ts", "content b").unwrap();
    ///
    /// let files: Vec<_> = vfs.files().collect();
    /// assert_eq!(files.len(), 2);
    /// ```
    pub fn files(&self) -> impl Iterator<Item = (&FilePath, &FileEntry)> {
        self.files.iter()
    }

    /// Removes all files from the VFS.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileSystem;
    ///
    /// let mut vfs = FileSystem::new();
    /// vfs.add_file("/test.ts", "").unwrap();
    /// assert_eq!(vfs.file_count(), 1);
    ///
    /// vfs.clear();
    /// assert_eq!(vfs.file_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.files.clear();
    }

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
    /// use mcp_execution_files::FilesBuilder;
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
    /// The export is staged in a temporary sibling directory next to
    /// `base_path` and published atomically: only once every file has been
    /// written to the staging directory is it renamed into place. If the
    /// process is interrupted at any point before publishing, `base_path` is
    /// left exactly as it was — either untouched or, if it did not exist yet,
    /// still absent. See the [module-level docs](self) for details.
    ///
    /// Concurrent exports of the *same* `base_path` from different processes
    /// are not locked against each other and can still race on the final
    /// swap — one export's result may be silently overwritten by another's
    /// (a lost update). The age-gated sweep of orphaned staging/displaced
    /// directories (`sweep_stale_artifacts`) guarantees neither export's
    /// staging/displaced directories can be deleted out from under it before
    /// its own rollback path runs, so this can no longer result in the
    /// target, its staging directory, and its displaced backup all being
    /// lost at once. Callers that need stronger-than-last-write-wins
    /// semantics for the same target should serialize writes themselves
    /// (e.g. a per-target lock) — see `mcp-execution-server`'s
    /// `introspector_for` for the pattern this project already uses for a
    /// similar problem. Concurrent exports of different targets sharing a
    /// parent directory are safe.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The parent directory of `base_path` does not exist
    /// - The staging directory cannot be created or canonicalized
    /// - I/O operations fail during directory creation, file writing, or the
    ///   final publish step
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::{FilesBuilder, ExportOptions};
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
        let target = base_path.as_ref();

        let parent = target.parent().ok_or_else(|| FilesError::InvalidPath {
            path: format!("Target path has no parent directory: {}", target.display()),
        })?;

        if !parent.exists() {
            return Err(FilesError::FileNotFound {
                path: parent.display().to_string(),
            });
        }

        // Best-effort cleanup of orphaned staging/displaced directories left
        // behind by a previous run that was killed (e.g. `SIGKILL`) before it
        // could clean up after itself — `TempDir::drop` never runs in that
        // case. This bounds the leak to at most one generation between
        // crashes rather than letting full tree copies accumulate forever.
        // Scoped to this `target`'s own name so it never touches a sibling
        // export's in-flight staging directory (e.g. two `generate` runs for
        // different servers publishing into the same `~/.claude/servers/`).
        Self::sweep_stale_artifacts(parent, target);

        // Stage the export in a sibling directory on the same filesystem so the
        // final publish step below is a single directory rename rather than a
        // sequence of individually-visible file writes. The prefix is scoped to
        // `target`'s own name for the same reason as the sweep above.
        let staging = tempfile::Builder::new()
            .prefix(&Self::staging_prefix(target))
            .tempdir_in(parent)
            .map_err(|e| FilesError::IoError {
                path: parent.display().to_string(),
                source: e,
            })?;

        let canonical_staging =
            staging
                .path()
                .canonicalize()
                .map_err(|e| FilesError::InvalidPath {
                    path: format!("Failed to canonicalize {}: {}", staging.path().display(), e),
                })?;

        // Phase 1: Collect all unique directories
        let dirs = self.collect_directories(&canonical_staging);

        // Phase 2: Create all directories in one pass
        Self::create_directories(&dirs)?;

        // Phase 3: Write all files into the staging directory. If this fails,
        // `staging` is dropped here and its `Drop` impl removes the partial
        // tree — `target` is never touched.
        self.write_files(&canonical_staging, options)?;

        // Every file landed successfully; publish by swapping the staged
        // directory into place.
        Self::publish_staged_export(staging, target)
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
    /// use mcp_execution_files::FilesBuilder;
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
    // TODO(critic): not directory-atomic — needs staging treatment before any
    // production caller is wired up (currently unused outside this crate's
    // own tests/benches, so the interrupted-export bug fixed in
    // `export_to_filesystem` does not apply here yet).
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
                write_file_atomic(&disk_path, file.content(), options.atomic)
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

    /// Writes all files into the staging directory ahead of publishing.
    fn write_files(&self, staging_base: &Path, options: &ExportOptions) -> Result<()> {
        for (vfs_path, file) in self.files() {
            let staging_disk_path = Self::vfs_to_disk_path(vfs_path.as_str(), staging_base);
            write_file_atomic(&staging_disk_path, file.content(), options.atomic)?;
        }
        Ok(())
    }

    /// Publishes a fully staged export by atomically swapping it into `target`.
    ///
    /// If the swap itself fails, the staging directory is removed so no
    /// orphaned staging directories (see [`Self::staging_prefix`]) accumulate
    /// next to `target`.
    fn publish_staged_export(staging: TempDir, target: &Path) -> Result<()> {
        // Disown the `TempDir` guard: ownership of cleanup now belongs to
        // `swap_into_place` (on failure) or to `target` itself (on success).
        let staging_path = staging.keep();

        if let Err(err) = Self::swap_into_place(&staging_path, target) {
            let _ = fs::remove_dir_all(&staging_path);
            return Err(err);
        }

        Ok(())
    }

    /// Atomically replaces `target` with the directory at `staging_path`.
    ///
    /// A directory rename cannot replace a non-empty destination on any
    /// platform this crate supports, so an existing `target` is first moved
    /// aside to a unique sibling path, the staged directory is renamed into
    /// `target`, and only then is the displaced directory removed. If this
    /// function *returns* an error, the second rename failed and the original
    /// directory has been moved back, so `target` is never left missing as
    /// observed by a caller of this function.
    ///
    /// This guarantee does not extend to a process that is killed (e.g.
    /// `SIGKILL`) between the two renames: in that narrow window `target` is
    /// transiently absent and the previous export sits at a `.stale-*`
    /// sibling until [`FileSystem::sweep_stale_artifacts`] reclaims it on a
    /// later export. That failure mode is louder (a missing directory) than
    /// the silent broken-import bug this fix replaces.
    fn swap_into_place(staging_path: &Path, target: &Path) -> Result<()> {
        if !target.exists() {
            return fs::rename(staging_path, target).map_err(|e| FilesError::IoError {
                path: target.display().to_string(),
                source: e,
            });
        }

        let parent = target.parent().ok_or_else(|| FilesError::InvalidPath {
            path: format!("Target path has no parent directory: {}", target.display()),
        })?;
        let displaced = Self::displace_existing_target(target, parent)?;

        if let Err(e) = fs::rename(staging_path, target) {
            // Roll back so `target` is never left missing.
            let _ = fs::rename(&displaced, target);
            return Err(FilesError::IoError {
                path: target.display().to_string(),
                source: e,
            });
        }

        let _ = fs::remove_dir_all(&displaced);
        Ok(())
    }

    /// Moves an existing `target` aside to a unique sibling path in `parent`
    /// so the staged directory can be renamed into `target`'s place,
    /// returning the displaced path.
    ///
    /// A directory rename does not reset the renamed directory's own mtime,
    /// so without an explicit refresh `displaced` would inherit `target`'s
    /// original mtime — almost always well past [`STALE_ARTIFACT_MIN_AGE`],
    /// since a target being regenerated has typically existed since a prior
    /// export. That would make it immediately eligible for a concurrent
    /// sibling's [`Self::sweep_stale_artifacts`] pass, defeating the age
    /// gate for exactly the directory [`Self::swap_into_place`]'s own
    /// rollback depends on (issue #169). [`Self::touch_dir`] refreshes
    /// `target`'s mtime *before* the rename rather than `displaced`'s
    /// *after* it, since a rename preserves the source's mtime — so
    /// `displaced` carries a fresh mtime from the instant it exists, with
    /// no window in which it is stale even momentarily.
    fn displace_existing_target(target: &Path, parent: &Path) -> Result<PathBuf> {
        let displaced = parent.join(Self::unique_sibling_name(target));

        Self::touch_dir(target);

        fs::rename(target, &displaced).map_err(|e| FilesError::IoError {
            path: target.display().to_string(),
            source: e,
        })?;

        Ok(displaced)
    }

    /// Best-effort refresh of `dir`'s modification time.
    ///
    /// Deliberately avoids opening `dir` as a `File` to call
    /// [`std::fs::File::set_times`], since opening a directory for write
    /// access is not portable across every platform this crate supports.
    /// Instead, creating and immediately removing a marker file inside
    /// `dir` achieves the same effect portably: any filesystem that tracks
    /// directory mtimes bumps them when an immediate child is added or
    /// removed. Best-effort like [`Self::sweep_stale_artifacts`]: failure
    /// (e.g. a read-only directory) is silently ignored rather than failing
    /// the export.
    fn touch_dir(dir: &Path) {
        let marker = dir.join(format!(".mtime-touch-{}", std::process::id()));
        if fs::File::create(&marker).is_ok() {
            let _ = fs::remove_file(&marker);
        }
    }

    /// Returns `target`'s file name for use as a namespacing stem in sibling
    /// artifact names, so that concurrent exports of *different* targets in
    /// the same parent directory (e.g. two `generate` runs publishing into
    /// the same `~/.claude/servers/`) never collide or interfere with one
    /// another's staging/displaced directories.
    fn target_stem(target: &Path) -> &str {
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("export")
    }

    /// Returns the `tempfile` prefix used for `target`'s staging directory.
    fn staging_prefix(target: &Path) -> String {
        format!(".{}.staging-", Self::target_stem(target))
    }

    /// Generates a unique sibling file name used to temporarily displace
    /// `target` during the atomic swap.
    fn unique_sibling_name(target: &Path) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let stem = Self::target_stem(target);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);

        PathBuf::from(format!(
            ".{stem}.stale-{}-{nanos}-{seq}",
            std::process::id()
        ))
    }

    /// Removes orphaned staging/displaced directories left next to `target`
    /// by a previous export of the *same* `target` that was killed before it
    /// could clean up after itself (`TempDir::drop` and the rollback in
    /// [`Self::swap_into_place`] both require the process to still be
    /// running). Scoped to `target`'s own name (see [`Self::target_stem`]) so
    /// it never touches a sibling export's in-flight artifacts.
    ///
    /// Best-effort: this is a hygiene pass, not part of the export's
    /// correctness, so any I/O error while scanning or removing an entry is
    /// silently ignored rather than failing the export.
    ///
    /// Only removes entries at least [`STALE_ARTIFACT_MIN_AGE`] old (by
    /// mtime) — see that constant's doc for why this age gate is what makes
    /// the sweep safe to run against a concurrently in-flight sibling
    /// export.
    fn sweep_stale_artifacts(parent: &Path, target: &Path) {
        Self::sweep_stale_artifacts_older_than(parent, target, STALE_ARTIFACT_MIN_AGE);
    }

    /// Inner implementation of [`Self::sweep_stale_artifacts`], parameterized
    /// over the age threshold so tests can exercise the sweep logic without
    /// backdating real file mtimes (pass [`Duration::ZERO`] to sweep
    /// unconditionally).
    fn sweep_stale_artifacts_older_than(parent: &Path, target: &Path, min_age: Duration) {
        let stem = Self::target_stem(target);
        let staging_prefix = Self::staging_prefix(target);
        let stale_prefix = format!(".{stem}.stale-");

        let Ok(entries) = fs::read_dir(parent) else {
            return;
        };

        for entry in entries.flatten() {
            let is_orphan = entry.file_name().to_str().is_some_and(|name| {
                name.starts_with(&staging_prefix) || name.starts_with(&stale_prefix)
            });
            if !is_orphan {
                continue;
            }

            // Too young to be a crash orphan — could be a live sibling
            // export's in-flight artifacts; leave it for a later sweep once
            // it ages past `min_age`. Metadata errors (entry vanished,
            // permission issue) are treated the same way — skip rather than
            // guess, since this is a best-effort hygiene pass, not part of
            // export correctness.
            let old_enough = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|modified| {
                    SystemTime::now()
                        .duration_since(modified)
                        .is_ok_and(|age| age >= min_age)
                });

            if old_enough {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    /// Converts VFS path to disk path.
    ///
    /// Strips leading '/' and joins with base path.
    ///
    /// # Panics
    ///
    /// Panics if path contains `..` (path traversal attempt).
    /// This is defense-in-depth since `FilePath::new()` also validates.
    fn vfs_to_disk_path(vfs_path: &str, base: &Path) -> PathBuf {
        // Strip leading '/' from VFS path
        let relative = vfs_path.strip_prefix('/').unwrap_or(vfs_path);

        // Defense-in-depth: reject path traversal attempts
        // Primary validation is in FilePath::new(), this is a safety net
        assert!(
            !relative.contains(".."),
            "SECURITY: Path traversal attempt detected in VFS path: {vfs_path}"
        );

        // Convert forward slashes to platform-specific separators
        let relative_path = if cfg!(target_os = "windows") {
            PathBuf::from(relative.replace('/', "\\"))
        } else {
            PathBuf::from(relative)
        };

        base.join(relative_path)
    }
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Options for filesystem export operations.
///
/// # Examples
///
/// ```
/// use mcp_execution_files::ExportOptions;
///
/// let options = ExportOptions::default().with_atomic_writes(true);
/// ```
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Use atomic writes (write to temp file, then rename)
    pub atomic: bool,
}

impl ExportOptions {
    /// Creates new export options with defaults.
    ///
    /// Defaults:
    /// - atomic: true (safer)
    #[must_use]
    pub const fn new() -> Self {
        Self { atomic: true }
    }

    /// Sets whether to use atomic writes.
    #[must_use]
    pub const fn with_atomic_writes(mut self, atomic: bool) -> Self {
        self.atomic = atomic;
        self
    }
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes file content to disk.
///
/// If `atomic` is `true`, writes to a temp file then renames it into place.
/// Otherwise, writes directly.
fn write_file_atomic(path: &Path, content: &str, atomic: bool) -> Result<()> {
    if atomic {
        // Atomic write: temp file + rename
        let temp_path = path.with_added_extension("tmp");

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

    // FileSystem core tests
    #[test]
    fn test_vfs_new() {
        let vfs = FileSystem::new();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_vfs_default() {
        let vfs = FileSystem::default();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_add_file() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/test.ts", "content").unwrap();
        assert_eq!(vfs.file_count(), 1);
    }

    #[test]
    fn test_add_file_invalid_path() {
        let mut vfs = FileSystem::new();
        let result = vfs.add_file("relative/path", "content");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_file() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/test.ts", "hello world").unwrap();

        let content = vfs.read_file("/test.ts").unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_read_file_not_found() {
        let vfs = FileSystem::new();
        let result = vfs.read_file("/missing.ts");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_not_found());
    }

    #[test]
    fn test_exists() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/exists.ts", "").unwrap();

        assert!(vfs.exists("/exists.ts"));
        assert!(!vfs.exists("/missing.ts"));
    }

    #[test]
    fn test_exists_invalid_path() {
        let vfs = FileSystem::new();
        assert!(!vfs.exists("relative/path"));
    }

    #[test]
    fn test_list_dir() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/mcp-tools/servers/test1.ts", "").unwrap();
        vfs.add_file("/mcp-tools/servers/test2.ts", "").unwrap();

        let entries = vfs.list_dir("/mcp-tools/servers").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_list_dir_empty() {
        let vfs = FileSystem::new();
        let entries = vfs.list_dir("/empty").unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_list_dir_not_a_directory() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/file.ts", "").unwrap();

        let result = vfs.list_dir("/file.ts");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_not_directory());
    }

    #[test]
    fn test_list_dir_subdirectories() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/mcp-tools/servers/test/file1.ts", "")
            .unwrap();
        vfs.add_file("/mcp-tools/servers/test/file2.ts", "")
            .unwrap();
        vfs.add_file("/mcp-tools/servers/other.ts", "").unwrap();

        let entries = vfs.list_dir("/mcp-tools/servers").unwrap();
        // Should include 'test' directory and 'other.ts' file
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_file_count() {
        let mut vfs = FileSystem::new();
        assert_eq!(vfs.file_count(), 0);

        vfs.add_file("/test1.ts", "").unwrap();
        assert_eq!(vfs.file_count(), 1);

        vfs.add_file("/test2.ts", "").unwrap();
        assert_eq!(vfs.file_count(), 2);
    }

    #[test]
    fn test_all_paths() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/b.ts", "").unwrap();
        vfs.add_file("/a.ts", "").unwrap();

        let paths = vfs.all_paths();
        assert_eq!(paths.len(), 2);
        // Should be sorted
        assert_eq!(paths[0].as_str(), "/a.ts");
        assert_eq!(paths[1].as_str(), "/b.ts");
    }

    #[test]
    fn test_clear() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/test1.ts", "").unwrap();
        vfs.add_file("/test2.ts", "").unwrap();
        assert_eq!(vfs.file_count(), 2);

        vfs.clear();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_replace_file() {
        let mut vfs = FileSystem::new();
        vfs.add_file("/test.ts", "original").unwrap();
        assert_eq!(vfs.read_file("/test.ts").unwrap(), "original");

        vfs.add_file("/test.ts", "updated").unwrap();
        assert_eq!(vfs.read_file("/test.ts").unwrap(), "updated");
        assert_eq!(vfs.file_count(), 1);
    }

    #[test]
    fn test_vfs_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<FileSystem>();
        assert_sync::<FileSystem>();
    }

    // Export tests
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
    fn test_export_failure_leaves_existing_target_untouched() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("out");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.ts"), "keep").unwrap();

        // "/conflict/child.ts" forces "/conflict" to be created as a directory
        // in the staging tree, while "/conflict" is also added as a file —
        // writing it always fails (renaming a file onto an existing
        // directory), regardless of `HashMap` iteration order.
        let vfs = FilesBuilder::new()
            .add_file("/conflict", "file content")
            .add_file("/conflict/child.ts", "child content")
            .build()
            .unwrap();

        let result = vfs.export_to_filesystem(&target);
        assert!(result.is_err());

        // The previous export must be completely untouched by the failure.
        assert!(target.join("keep.ts").exists());
        assert_eq!(fs::read_to_string(target.join("keep.ts")).unwrap(), "keep");
        assert!(!target.join("conflict").exists());

        // Nothing but `target` itself should remain next to it — no orphaned
        // staging directory left behind by the failed export.
        let siblings: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path() != target)
            .collect();
        assert!(siblings.is_empty(), "unexpected siblings: {siblings:?}");
    }

    #[test]
    fn test_swap_into_place_replaces_non_empty_target() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("out");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("old.ts"), "old").unwrap();

        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("new.ts"), "new").unwrap();

        FileSystem::swap_into_place(&staging, &target).unwrap();

        assert!(target.join("new.ts").exists());
        assert!(!target.join("old.ts").exists());
        assert_eq!(fs::read_to_string(target.join("new.ts")).unwrap(), "new");
    }

    #[test]
    fn test_swap_into_place_rolls_back_on_publish_failure() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("out");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.ts"), "keep").unwrap();

        // A staging path that does not exist forces the publish rename to fail.
        let missing_staging = temp.path().join("does-not-exist-staging");

        let result = FileSystem::swap_into_place(&missing_staging, &target);
        assert!(result.is_err());

        // `target` must be restored to its exact prior state, not left missing.
        assert!(target.join("keep.ts").exists());
        assert_eq!(fs::read_to_string(target.join("keep.ts")).unwrap(), "keep");
    }

    #[test]
    fn test_sweep_stale_artifacts_removes_orphans_only() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("out");

        let orphan_staging = temp.path().join(".out.staging-abc123");
        let orphan_stale = temp.path().join(".out.stale-999-1-1");
        let unrelated = temp.path().join("unrelated-dir");
        // A staging leftover belonging to a *different* target must survive:
        // sweeping for `out` must never touch a concurrent export of `other`.
        let other_target_staging = temp.path().join(".other.staging-abc123");
        fs::create_dir_all(&orphan_staging).unwrap();
        fs::create_dir_all(&orphan_stale).unwrap();
        fs::create_dir_all(&unrelated).unwrap();
        fs::create_dir_all(&other_target_staging).unwrap();

        // Bypass the production age gate (`Duration::ZERO`) to exercise the
        // name-matching logic in isolation, without backdating real mtimes.
        FileSystem::sweep_stale_artifacts_older_than(temp.path(), &target, Duration::ZERO);

        assert!(!orphan_staging.exists());
        assert!(!orphan_stale.exists());
        assert!(unrelated.exists());
        assert!(other_target_staging.exists());
    }

    #[test]
    fn test_export_does_not_sweep_recent_sibling_artifacts() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("out");
        fs::create_dir_all(&target).unwrap();

        // Simulate a concurrent sibling export's in-flight staging
        // directory, built the same way `export_to_filesystem_with_options`
        // builds it (fresh via `tempfile`).
        let sibling_staging = tempfile::Builder::new()
            .prefix(&FileSystem::staging_prefix(&target))
            .tempdir_in(temp.path())
            .unwrap();
        let sibling_staging_path = sibling_staging.path().to_path_buf();

        // Simulate a concurrent sibling export's displaced backup, built the
        // same way `swap_into_place` builds it: an existing (decoy) target
        // renamed aside via `displace_existing_target`. Regression test for
        // issue #169: previously `sweep_stale_artifacts` matched purely by
        // name, so a real export through this same public entry point would
        // delete this sibling's staging and displaced directories out from
        // under it. `decoy_target` is fresh at test time, so this does not
        // itself exercise the S1 mtime-inheritance defect (a fresh target's
        // displaced copy would read young even without `touch_dir`) — that
        // is covered by `displace_existing_target_refreshes_displaced_mtime`
        // below. This test instead proves the sweep spares young siblings
        // through the full public `export_to_filesystem` path, not just the
        // private sweep helper.
        let decoy_target = temp.path().join("decoy-out");
        fs::create_dir_all(&decoy_target).unwrap();
        let sibling_displaced =
            FileSystem::displace_existing_target(&decoy_target, temp.path()).unwrap();

        let vfs = FilesBuilder::new()
            .add_file("/tool.ts", "export {}")
            .build()
            .unwrap();
        vfs.export_to_filesystem(&target).unwrap();

        assert!(
            sibling_staging_path.exists(),
            "too young to be a crash orphan"
        );
        assert!(
            sibling_displaced.exists(),
            "too young to be a crash orphan (mtime just refreshed)"
        );
        assert!(target.join("tool.ts").exists());
    }

    #[test]
    fn displace_existing_target_refreshes_displaced_mtime() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("out");
        fs::create_dir_all(&target).unwrap();
        let target_created_at = fs::metadata(&target).unwrap().modified().unwrap();

        // Give the filesystem's mtime clock room to move past `target`'s
        // creation time before it gets renamed aside, so the comparison
        // below can distinguish "displaced kept target's original mtime"
        // (issue #169's S1 gap: a rename alone does not reset mtime) from
        // "displaced's mtime was refreshed by `touch_dir`" (the fix).
        std::thread::sleep(Duration::from_millis(1100));

        let displaced = FileSystem::displace_existing_target(&target, tmp.path()).unwrap();
        let displaced_modified = fs::metadata(&displaced).unwrap().modified().unwrap();

        assert!(
            displaced_modified > target_created_at,
            "displaced's mtime must be refreshed around the rename, not inherited from target"
        );
    }

    #[test]
    fn sweep_stale_artifacts_skips_recent_orphans() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("out");

        let staging = tempfile::Builder::new()
            .prefix(&FileSystem::staging_prefix(&target))
            .tempdir_in(tmp.path())
            .unwrap();
        let staging_path = staging.path().to_path_buf();

        // Real production threshold: a just-created dir must never be swept.
        FileSystem::sweep_stale_artifacts(tmp.path(), &target);

        assert!(staging_path.exists(), "a fresh sibling must not be swept");
    }

    #[test]
    fn sweep_stale_artifacts_older_than_removes_aged_orphans() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("out");

        let staging = tempfile::Builder::new()
            .prefix(&FileSystem::staging_prefix(&target))
            .tempdir_in(tmp.path())
            .unwrap();
        let staging_path = staging.keep(); // disown so the assert below is meaningful

        FileSystem::sweep_stale_artifacts_older_than(tmp.path(), &target, Duration::ZERO);

        assert!(
            !staging_path.exists(),
            "an orphan past the age threshold must be swept"
        );
    }

    #[test]
    fn test_export_to_nonexistent_target_directory() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("fresh-output");

        let vfs = FilesBuilder::new()
            .add_file("/tools/create.ts", "export function create() {}")
            .build()
            .unwrap();

        assert!(!target.exists());
        vfs.export_to_filesystem(&target).unwrap();

        assert!(target.join("tools/create.ts").exists());

        // Nothing but `target` itself should remain next to it — no orphaned
        // staging directory left behind.
        let siblings: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path() != target)
            .collect();
        assert!(siblings.is_empty(), "unexpected siblings: {siblings:?}");
    }

    #[test]
    fn test_publish_staged_export_cleans_up_staging_on_failure() {
        let temp = TempDir::new().unwrap();
        let staging = TempDir::new_in(temp.path()).unwrap();
        fs::write(staging.path().join("file.ts"), "content").unwrap();
        let staging_path = staging.path().to_path_buf();

        // Target's parent does not exist, so the publish rename fails.
        let target = temp.path().join("missing-parent").join("out");

        let result = FileSystem::publish_staged_export(staging, &target);
        assert!(result.is_err());

        // The now-disowned staging directory must be cleaned up, not leaked.
        assert!(!staging_path.exists());
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
    }

    #[test]
    fn test_export_options_builder() {
        let options = ExportOptions::new().with_atomic_writes(false);

        assert!(!options.atomic);
    }
}
