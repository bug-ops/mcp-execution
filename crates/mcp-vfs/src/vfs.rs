//! Virtual filesystem implementation.
//!
//! Provides an in-memory, read-only virtual filesystem for MCP tool definitions.
//! Files are stored in a HashMap for O(1) lookup performance.
//!
//! # Examples
//!
//! ```
//! use mcp_vfs::{Vfs, VfsPath};
//!
//! let mut vfs = Vfs::new();
//! vfs.add_file("/mcp-tools/test.ts", "export const VERSION = '1.0';").unwrap();
//!
//! let content = vfs.read_file("/mcp-tools/test.ts").unwrap();
//! assert_eq!(content, "export const VERSION = '1.0';");
//! ```

use crate::types::{Result, VfsError, VfsFile, VfsPath};
use std::collections::HashMap;
use std::path::Path;

/// An in-memory virtual filesystem for MCP tool definitions.
///
/// `Vfs` provides a read-only filesystem structure that stores generated
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
/// use mcp_vfs::Vfs;
///
/// let mut vfs = Vfs::new();
/// vfs.add_file("/mcp-tools/manifest.json", "{}").unwrap();
///
/// assert!(vfs.exists("/mcp-tools/manifest.json"));
/// assert_eq!(vfs.file_count(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct Vfs {
    files: HashMap<VfsPath, VfsFile>,
}

impl Vfs {
    /// Creates a new empty virtual filesystem.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::Vfs;
    ///
    /// let vfs = Vfs::new();
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
    /// use mcp_vfs::Vfs;
    ///
    /// let mut vfs = Vfs::new();
    /// vfs.add_file("/mcp-tools/test.ts", "console.log('hello');").unwrap();
    ///
    /// assert!(vfs.exists("/mcp-tools/test.ts"));
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    pub fn add_file(&mut self, path: impl AsRef<Path>, content: impl Into<String>) -> Result<()> {
        let vfs_path = VfsPath::new(path)?;
        let file = VfsFile::new(content);
        self.files.insert(vfs_path, file);
        Ok(())
    }

    /// Reads the content of a file.
    ///
    /// # Errors
    ///
    /// Returns `VfsError::FileNotFound` if the file does not exist.
    /// Returns `VfsError::InvalidPath` if the path is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::Vfs;
    ///
    /// let mut vfs = Vfs::new();
    /// vfs.add_file("/test.ts", "export {}").unwrap();
    ///
    /// let content = vfs.read_file("/test.ts").unwrap();
    /// assert_eq!(content, "export {}");
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    pub fn read_file(&self, path: impl AsRef<Path>) -> Result<&str> {
        let vfs_path = VfsPath::new(path)?;
        self.files
            .get(&vfs_path)
            .map(|f| f.content())
            .ok_or_else(|| VfsError::FileNotFound {
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
    /// use mcp_vfs::Vfs;
    ///
    /// let mut vfs = Vfs::new();
    /// vfs.add_file("/exists.ts", "").unwrap();
    ///
    /// assert!(vfs.exists("/exists.ts"));
    /// assert!(!vfs.exists("/missing.ts"));
    /// ```
    #[must_use]
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        VfsPath::new(path)
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
    /// Returns `VfsError::InvalidPath` if the path is invalid.
    /// Returns `VfsError::NotADirectory` if the path points to a file.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::Vfs;
    ///
    /// let mut vfs = Vfs::new();
    /// vfs.add_file("/mcp-tools/servers/test1.ts", "").unwrap();
    /// vfs.add_file("/mcp-tools/servers/test2.ts", "").unwrap();
    ///
    /// let entries = vfs.list_dir("/mcp-tools/servers").unwrap();
    /// assert_eq!(entries.len(), 2);
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    pub fn list_dir(&self, path: impl AsRef<Path>) -> Result<Vec<VfsPath>> {
        let vfs_path = VfsPath::new(path)?;
        let path_str = vfs_path.as_str();

        // Check if the path itself is a file
        if self.files.contains_key(&vfs_path) {
            return Err(VfsError::NotADirectory {
                path: path_str.to_string(),
            });
        }

        // Collect all direct children
        let mut children = Vec::new();
        let normalized_dir = if path_str.ends_with('/') {
            path_str.to_string()
        } else {
            format!("{}/", path_str)
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
                    if let Ok(subdir_path) = VfsPath::new(subdir) {
                        if !children.contains(&subdir_path) {
                            children.push(subdir_path);
                        }
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
    /// use mcp_vfs::Vfs;
    ///
    /// let mut vfs = Vfs::new();
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
    /// use mcp_vfs::Vfs;
    ///
    /// let mut vfs = Vfs::new();
    /// vfs.add_file("/a.ts", "").unwrap();
    /// vfs.add_file("/b.ts", "").unwrap();
    ///
    /// let paths = vfs.all_paths();
    /// assert_eq!(paths.len(), 2);
    /// ```
    #[must_use]
    pub fn all_paths(&self) -> Vec<&VfsPath> {
        let mut paths: Vec<_> = self.files.keys().collect();
        paths.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        paths
    }

    /// Removes all files from the VFS.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::Vfs;
    ///
    /// let mut vfs = Vfs::new();
    /// vfs.add_file("/test.ts", "").unwrap();
    /// assert_eq!(vfs.file_count(), 1);
    ///
    /// vfs.clear();
    /// assert_eq!(vfs.file_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.files.clear();
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfs_new() {
        let vfs = Vfs::new();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_vfs_default() {
        let vfs = Vfs::default();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_add_file() {
        let mut vfs = Vfs::new();
        vfs.add_file("/test.ts", "content").unwrap();
        assert_eq!(vfs.file_count(), 1);
    }

    #[test]
    fn test_add_file_invalid_path() {
        let mut vfs = Vfs::new();
        let result = vfs.add_file("relative/path", "content");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_file() {
        let mut vfs = Vfs::new();
        vfs.add_file("/test.ts", "hello world").unwrap();

        let content = vfs.read_file("/test.ts").unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_read_file_not_found() {
        let vfs = Vfs::new();
        let result = vfs.read_file("/missing.ts");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_not_found());
    }

    #[test]
    fn test_exists() {
        let mut vfs = Vfs::new();
        vfs.add_file("/exists.ts", "").unwrap();

        assert!(vfs.exists("/exists.ts"));
        assert!(!vfs.exists("/missing.ts"));
    }

    #[test]
    fn test_exists_invalid_path() {
        let vfs = Vfs::new();
        assert!(!vfs.exists("relative/path"));
    }

    #[test]
    fn test_list_dir() {
        let mut vfs = Vfs::new();
        vfs.add_file("/mcp-tools/servers/test1.ts", "").unwrap();
        vfs.add_file("/mcp-tools/servers/test2.ts", "").unwrap();

        let entries = vfs.list_dir("/mcp-tools/servers").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_list_dir_empty() {
        let vfs = Vfs::new();
        let entries = vfs.list_dir("/empty").unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_list_dir_not_a_directory() {
        let mut vfs = Vfs::new();
        vfs.add_file("/file.ts", "").unwrap();

        let result = vfs.list_dir("/file.ts");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_not_directory());
    }

    #[test]
    fn test_list_dir_subdirectories() {
        let mut vfs = Vfs::new();
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
        let mut vfs = Vfs::new();
        assert_eq!(vfs.file_count(), 0);

        vfs.add_file("/test1.ts", "").unwrap();
        assert_eq!(vfs.file_count(), 1);

        vfs.add_file("/test2.ts", "").unwrap();
        assert_eq!(vfs.file_count(), 2);
    }

    #[test]
    fn test_all_paths() {
        let mut vfs = Vfs::new();
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
        let mut vfs = Vfs::new();
        vfs.add_file("/test1.ts", "").unwrap();
        vfs.add_file("/test2.ts", "").unwrap();
        assert_eq!(vfs.file_count(), 2);

        vfs.clear();
        assert_eq!(vfs.file_count(), 0);
    }

    #[test]
    fn test_replace_file() {
        let mut vfs = Vfs::new();
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

        assert_send::<Vfs>();
        assert_sync::<Vfs>();
    }
}
