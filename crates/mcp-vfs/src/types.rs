//! Core types for the virtual filesystem.
//!
//! This module defines strong types for VFS paths, files, and errors,
//! following Microsoft Rust Guidelines for type safety and error handling.
//!
//! # Examples
//!
//! ```
//! use mcp_vfs::{VfsPath, VfsFile};
//!
//! let path = VfsPath::new("/mcp-tools/servers/vkteams-bot/manifest.json").unwrap();
//! let file = VfsFile::new("{}");
//!
//! assert_eq!(path.as_str(), "/mcp-tools/servers/vkteams-bot/manifest.json");
//! assert_eq!(file.content(), "{}");
//! ```

use std::fmt;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during VFS operations.
///
/// All error variants include contextual information and implement
/// `is_xxx()` methods for easy error classification.
///
/// # Examples
///
/// ```
/// use mcp_vfs::VfsError;
///
/// let error = VfsError::FileNotFound {
///     path: "/missing.txt".to_string(),
/// };
///
/// assert!(error.is_not_found());
/// ```
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum VfsError {
    /// File or directory not found at the specified path
    #[error("File not found: {path}")]
    FileNotFound {
        /// The path that was not found
        path: String,
    },

    /// Path exists but is not a directory
    #[error("Not a directory: {path}")]
    NotADirectory {
        /// The path that is not a directory
        path: String,
    },

    /// Path is invalid or malformed
    #[error("Invalid path: {path}")]
    InvalidPath {
        /// The invalid path
        path: String,
    },

    /// Path is not absolute (must start with '/')
    #[error("Path must be absolute: {path}")]
    PathNotAbsolute {
        /// The relative path
        path: String,
    },

    /// Path contains invalid components (e.g., '..')
    #[error("Path contains invalid components: {path}")]
    InvalidPathComponent {
        /// The path with invalid components
        path: String,
    },
}

impl VfsError {
    /// Returns `true` if this is a file not found error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsError;
    ///
    /// let error = VfsError::FileNotFound {
    ///     path: "/test.txt".to_string(),
    /// };
    ///
    /// assert!(error.is_not_found());
    /// ```
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(self, Self::FileNotFound { .. })
    }

    /// Returns `true` if this is a not-a-directory error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsError;
    ///
    /// let error = VfsError::NotADirectory {
    ///     path: "/file.txt".to_string(),
    /// };
    ///
    /// assert!(error.is_not_directory());
    /// ```
    #[must_use]
    pub const fn is_not_directory(&self) -> bool {
        matches!(self, Self::NotADirectory { .. })
    }

    /// Returns `true` if this is an invalid path error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsError;
    ///
    /// let error = VfsError::InvalidPath {
    ///     path: "".to_string(),
    /// };
    ///
    /// assert!(error.is_invalid_path());
    /// ```
    #[must_use]
    pub const fn is_invalid_path(&self) -> bool {
        matches!(
            self,
            Self::InvalidPath { .. }
                | Self::PathNotAbsolute { .. }
                | Self::InvalidPathComponent { .. }
        )
    }
}

/// A validated virtual filesystem path.
///
/// `VfsPath` ensures paths use Unix-style conventions on all platforms:
/// - Must start with '/' (absolute paths only)
/// - Free of parent directory references ('..')
/// - Use forward slashes as separators
///
/// This is intentional: VFS paths are platform-independent and always use
/// Unix conventions, even on Windows. This enables consistent path handling
/// across development machines and CI environments.
///
/// # Examples
///
/// ```
/// use mcp_vfs::VfsPath;
///
/// let path = VfsPath::new("/mcp-tools/servers/test/file.ts").unwrap();
/// assert_eq!(path.as_str(), "/mcp-tools/servers/test/file.ts");
/// ```
///
/// ```
/// use mcp_vfs::VfsPath;
///
/// // Invalid paths are rejected
/// assert!(VfsPath::new("relative/path").is_err());
/// assert!(VfsPath::new("/parent/../escape").is_err());
/// ```
///
/// On Windows, Unix-style paths like "/mcp-tools/servers/test" are accepted
/// (not Windows paths like "C:\mcp-tools\servers\test").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VfsPath(String);

impl VfsPath {
    /// Creates a new `VfsPath` from a path-like type.
    ///
    /// The path must be absolute (start with '/') and must not contain parent
    /// directory references ('..').
    ///
    /// `VfsPath` uses Unix-style path conventions on all platforms, ensuring
    /// consistent behavior on Linux, macOS, and Windows. Paths are validated
    /// using string-based checks rather than platform-specific `Path::is_absolute()`,
    /// which enables cross-platform compatibility.
    ///
    /// # Errors
    ///
    /// Returns `VfsError::PathNotAbsolute` if the path does not start with '/'.
    /// Returns `VfsError::InvalidPathComponent` if the path contains '..'.
    /// Returns `VfsError::InvalidPath` if the path is empty or not UTF-8 valid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsPath;
    ///
    /// let path = VfsPath::new("/mcp-tools/test.ts")?;
    /// assert_eq!(path.as_str(), "/mcp-tools/test.ts");
    ///
    /// // Works on all platforms (Unix-style paths)
    /// let path = VfsPath::new("/mcp-tools/servers/test/manifest.json")?;
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Convert to string for platform-independent validation
        let path_str = path.to_str().ok_or_else(|| VfsError::InvalidPath {
            path: path.display().to_string(),
        })?;

        // Normalize path separators to Unix-style (forward slashes) on all platforms
        // This ensures VFS paths are consistent regardless of the host OS
        let normalized_str = if cfg!(target_os = "windows") {
            // Replace Windows backslashes with forward slashes
            path_str.replace(std::path::MAIN_SEPARATOR, "/")
        } else {
            path_str.to_string()
        };

        // Check if empty
        if normalized_str.is_empty() {
            return Err(VfsError::InvalidPath {
                path: String::new(),
            });
        }

        // Check if absolute using Unix-style path rules (starts with '/')
        // VFS uses Unix-style paths on all platforms
        if !normalized_str.starts_with('/') {
            return Err(VfsError::PathNotAbsolute {
                path: normalized_str,
            });
        }

        // Check for '..' components in the path string
        if normalized_str.contains("..") {
            return Err(VfsError::InvalidPathComponent {
                path: normalized_str,
            });
        }

        // Store as String with normalized Unix-style separators
        Ok(Self(normalized_str))
    }

    /// Returns the path as a `Path` reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsPath;
    ///
    /// let vfs_path = VfsPath::new("/test.ts")?;
    /// let path = vfs_path.as_path();
    /// assert_eq!(path.to_str(), Some("/test.ts"));
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    #[must_use]
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    /// Returns the path as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsPath;
    ///
    /// let path = VfsPath::new("/mcp-tools/file.ts")?;
    /// assert_eq!(path.as_str(), "/mcp-tools/file.ts");
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the parent directory of this path.
    ///
    /// Returns `None` if this is the root path.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsPath;
    ///
    /// let path = VfsPath::new("/mcp-tools/servers/test.ts")?;
    /// let parent = path.parent().unwrap();
    /// assert_eq!(parent.as_str(), "/mcp-tools/servers");
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        // Find the last '/' separator
        self.0.rfind('/').map(|pos| {
            if pos == 0 {
                // Parent of "/foo" is "/" (root)
                Self("/".to_string())
            } else {
                // Parent of "/foo/bar" is "/foo"
                Self(self.0[..pos].to_string())
            }
        })
    }

    /// Checks if this path is a directory path.
    ///
    /// A path is considered a directory if it does not have a file extension.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsPath;
    ///
    /// let dir = VfsPath::new("/mcp-tools/servers")?;
    /// assert!(dir.is_dir_path());
    ///
    /// let file = VfsPath::new("/mcp-tools/manifest.json")?;
    /// assert!(!file.is_dir_path());
    /// # Ok::<(), mcp_vfs::VfsError>(())
    /// ```
    #[must_use]
    pub fn is_dir_path(&self) -> bool {
        // A path is a directory if it doesn't contain a '.' after the last '/'
        self.0
            .rfind('/')
            .is_some_and(|last_slash| !self.0[last_slash..].contains('.'))
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsRef<Path> for VfsPath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

/// A file in the virtual filesystem.
///
/// Contains file content as a string and metadata.
///
/// # Examples
///
/// ```
/// use mcp_vfs::VfsFile;
///
/// let file = VfsFile::new("console.log('hello');");
/// assert_eq!(file.content(), "console.log('hello');");
/// assert_eq!(file.size(), 21);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsFile {
    content: String,
}

impl VfsFile {
    /// Creates a new VFS file with the given content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsFile;
    ///
    /// let file = VfsFile::new("export const VERSION = '1.0';");
    /// assert_eq!(file.size(), 29);
    /// ```
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    /// Returns the file content as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsFile;
    ///
    /// let file = VfsFile::new("test content");
    /// assert_eq!(file.content(), "test content");
    /// ```
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns the size of the file content in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_vfs::VfsFile;
    ///
    /// let file = VfsFile::new("hello");
    /// assert_eq!(file.size(), 5);
    /// ```
    #[must_use]
    pub const fn size(&self) -> usize {
        self.content.len()
    }
}

/// Type alias for VFS operation results.
///
/// # Examples
///
/// ```
/// use mcp_vfs::{Result, VfsPath};
///
/// fn validate_path(path: &str) -> Result<VfsPath> {
///     VfsPath::new(path)
/// }
/// ```
pub type Result<T> = std::result::Result<T, VfsError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfs_path_new_valid() {
        let path = VfsPath::new("/mcp-tools/test.ts").unwrap();
        assert_eq!(path.as_str(), "/mcp-tools/test.ts");
    }

    #[test]
    fn test_vfs_path_new_relative_fails() {
        let result = VfsPath::new("relative/path");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_invalid_path());
    }

    #[test]
    fn test_vfs_path_new_parent_dir_fails() {
        let result = VfsPath::new("/parent/../escape");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_invalid_path());
    }

    #[test]
    fn test_vfs_path_new_empty_fails() {
        let result = VfsPath::new("");
        assert!(result.is_err());
    }

    #[test]
    fn test_vfs_path_parent() {
        let path = VfsPath::new("/mcp-tools/servers/test.ts").unwrap();
        let parent = path.parent().unwrap();
        assert_eq!(parent.as_str(), "/mcp-tools/servers");
    }

    #[test]
    fn test_vfs_path_parent_root() {
        let path = VfsPath::new("/test").unwrap();
        let parent = path.parent();
        assert!(parent.is_some());
    }

    #[test]
    fn test_vfs_path_is_dir_path() {
        let dir = VfsPath::new("/mcp-tools/servers").unwrap();
        assert!(dir.is_dir_path());

        let file = VfsPath::new("/mcp-tools/test.ts").unwrap();
        assert!(!file.is_dir_path());
    }

    #[test]
    fn test_vfs_path_display() {
        let path = VfsPath::new("/test.ts").unwrap();
        assert_eq!(format!("{path}"), "/test.ts");
    }

    #[test]
    fn test_vfs_file_new() {
        let file = VfsFile::new("test content");
        assert_eq!(file.content(), "test content");
        assert_eq!(file.size(), 12);
    }

    #[test]
    fn test_vfs_file_empty() {
        let file = VfsFile::new("");
        assert_eq!(file.content(), "");
        assert_eq!(file.size(), 0);
    }

    #[test]
    fn test_vfs_error_is_not_found() {
        let error = VfsError::FileNotFound {
            path: "/test".to_string(),
        };
        assert!(error.is_not_found());
        assert!(!error.is_not_directory());
        assert!(!error.is_invalid_path());
    }

    #[test]
    fn test_vfs_error_is_not_directory() {
        let error = VfsError::NotADirectory {
            path: "/file.txt".to_string(),
        };
        assert!(!error.is_not_found());
        assert!(error.is_not_directory());
        assert!(!error.is_invalid_path());
    }

    #[test]
    fn test_vfs_error_is_invalid_path() {
        let error = VfsError::InvalidPath {
            path: String::new(),
        };
        assert!(error.is_invalid_path());

        let error = VfsError::PathNotAbsolute {
            path: "relative".to_string(),
        };
        assert!(error.is_invalid_path());

        let error = VfsError::InvalidPathComponent {
            path: "../escape".to_string(),
        };
        assert!(error.is_invalid_path());
    }

    #[test]
    fn test_vfs_path_as_ref() {
        let vfs_path = VfsPath::new("/test.ts").unwrap();
        let path: &Path = vfs_path.as_ref();
        assert_eq!(path.to_str(), Some("/test.ts"));
    }
}
