//! Core types for the virtual filesystem.
//!
//! This module defines strong types for VFS paths, files, and errors,
//! following Microsoft Rust Guidelines for type safety and error handling.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_files::{FilePath, FileEntry};
//!
//! let path = FilePath::new("/mcp-tools/servers/github/manifest.json").unwrap();
//! let file = FileEntry::new("{}");
//!
//! assert_eq!(path.as_str(), "/mcp-tools/servers/github/manifest.json");
//! assert_eq!(file.content(), "{}");
//! ```

use std::fmt;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during VFS operations.
///
/// All error variants include contextual information for diagnostics.
///
/// # Examples
///
/// ```
/// use mcp_execution_files::FilesError;
///
/// let error = FilesError::FileNotFound {
///     path: "/missing.txt".to_string(),
/// };
///
/// assert!(matches!(error, FilesError::FileNotFound { .. }));
/// ```
#[derive(Error, Debug)]
pub enum FilesError {
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

    /// I/O operation failed during filesystem export
    #[error("I/O error at {path}: {source}")]
    IoError {
        /// The path where the I/O error occurred
        path: String,
        /// The underlying I/O error
        source: std::io::Error,
    },

    /// The virtual filesystem being exported exceeds a configured file-count or total-byte-size
    /// limit (denial-of-service protection, CWE-400).
    #[error("export exceeds resource limit for {resource}: {actual} exceeds limit of {limit}")]
    ResourceLimitExceeded {
        /// Human-readable name of the bounded resource (e.g. "export file count").
        resource: String,
        /// The actual observed size/count that triggered the rejection.
        actual: usize,
        /// The configured maximum allowed for this resource.
        limit: usize,
    },
}

impl FilesError {
    /// Returns `true` if this is a resource-limit-exceeded error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FilesError;
    ///
    /// let error = FilesError::ResourceLimitExceeded {
    ///     resource: "export file count".to_string(),
    ///     actual: 3000,
    ///     limit: 2000,
    /// };
    ///
    /// assert!(error.is_resource_limit_exceeded());
    /// ```
    #[must_use]
    pub const fn is_resource_limit_exceeded(&self) -> bool {
        matches!(self, Self::ResourceLimitExceeded { .. })
    }
}

/// A validated virtual filesystem path.
///
/// `FilePath` ensures paths use Unix-style conventions on all platforms:
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
/// use mcp_execution_files::FilePath;
///
/// let path = FilePath::new("/mcp-tools/servers/test/file.ts").unwrap();
/// assert_eq!(path.as_str(), "/mcp-tools/servers/test/file.ts");
/// ```
///
/// ```
/// use mcp_execution_files::FilePath;
///
/// // Invalid paths are rejected
/// assert!(FilePath::new("relative/path").is_err());
/// assert!(FilePath::new("/parent/../escape").is_err());
/// assert!(FilePath::new("//doubled-slash.ts").is_err());
/// assert!(FilePath::new("/./dot-component.ts").is_err());
/// assert!(FilePath::new("/trailing-slash/").is_err());
/// ```
///
/// On Windows, Unix-style paths like "/mcp-tools/servers/test" are accepted
/// (not Windows paths like "C:\mcp-tools\servers\test").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FilePath(String);

impl FilePath {
    /// Creates a new `FilePath` from a path-like type.
    ///
    /// The path must be absolute (start with '/'), must not contain parent
    /// directory references ('..'), and must not contain empty or '.'
    /// components (e.g. a doubled or trailing '/', or a literal `/./`
    /// segment) — the root path `/` itself has no components and remains
    /// valid.
    ///
    /// `FilePath` uses Unix-style path conventions on all platforms, ensuring
    /// consistent behavior on Linux, macOS, and Windows. Paths are validated
    /// using string-based checks rather than platform-specific `Path::is_absolute()`,
    /// which enables cross-platform compatibility.
    ///
    /// # Errors
    ///
    /// Returns `FilesError::PathNotAbsolute` if the path does not start with '/'.
    /// Returns `FilesError::InvalidPathComponent` if the path contains '..', or an
    /// empty or '.' component (e.g. `//`, a trailing `/`, or `/./`).
    /// Returns `FilesError::InvalidPath` if the path is empty or not UTF-8 valid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FilePath;
    ///
    /// let path = FilePath::new("/mcp-tools/test.ts")?;
    /// assert_eq!(path.as_str(), "/mcp-tools/test.ts");
    ///
    /// // Works on all platforms (Unix-style paths)
    /// let path = FilePath::new("/mcp-tools/servers/test/manifest.json")?;
    /// # Ok::<(), mcp_execution_files::FilesError>(())
    /// ```
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Convert to string for platform-independent validation
        let path_str = path.to_str().ok_or_else(|| FilesError::InvalidPath {
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
            return Err(FilesError::InvalidPath {
                path: String::new(),
            });
        }

        // Check if absolute using Unix-style path rules (starts with '/')
        // VFS uses Unix-style paths on all platforms
        if !normalized_str.starts_with('/') {
            return Err(FilesError::PathNotAbsolute {
                path: normalized_str,
            });
        }

        // Check for '..' components in the path string
        if normalized_str.contains("..") {
            return Err(FilesError::InvalidPathComponent {
                path: normalized_str,
            });
        }

        // Reject empty or '.' path components (e.g. "//x.ts", "/./x.ts", "/github/"):
        // an empty component from a doubled or trailing separator would make
        // `Path::join`/`PathBuf::join` treat a later absolute-looking remainder
        // as escaping its base, and both would let a single-file group name
        // collide with (and in `FilesBuilder::build_and_export`'s per-group
        // export, swap) an unintended target — including, for an empty root
        // component, the shared `base_path` itself. `normalized_str[1..]` is a
        // valid string slice because `normalized_str` is guaranteed to start
        // with the single ASCII byte `/` by the check above; it is only
        // inspected when non-empty; the root path `/` itself has no
        // components to check and remains valid.
        let remainder = &normalized_str[1..];
        if !remainder.is_empty()
            && remainder
                .split('/')
                .any(|component| component.is_empty() || component == ".")
        {
            return Err(FilesError::InvalidPathComponent {
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
    /// use mcp_execution_files::FilePath;
    ///
    /// let vfs_path = FilePath::new("/test.ts")?;
    /// let path = vfs_path.as_path();
    /// assert_eq!(path.to_str(), Some("/test.ts"));
    /// # Ok::<(), mcp_execution_files::FilesError>(())
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
    /// use mcp_execution_files::FilePath;
    ///
    /// let path = FilePath::new("/mcp-tools/file.ts")?;
    /// assert_eq!(path.as_str(), "/mcp-tools/file.ts");
    /// # Ok::<(), mcp_execution_files::FilesError>(())
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
    /// use mcp_execution_files::FilePath;
    ///
    /// let path = FilePath::new("/mcp-tools/servers/test.ts")?;
    /// let parent = path.parent().unwrap();
    /// assert_eq!(parent.as_str(), "/mcp-tools/servers");
    /// # Ok::<(), mcp_execution_files::FilesError>(())
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
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsRef<Path> for FilePath {
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
/// use mcp_execution_files::FileEntry;
///
/// let file = FileEntry::new("console.log('hello');");
/// assert_eq!(file.content(), "console.log('hello');");
/// assert_eq!(file.size(), 21);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    content: String,
}

impl FileEntry {
    /// Creates a new VFS file with the given content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_files::FileEntry;
    ///
    /// let file = FileEntry::new("export const VERSION = '1.0';");
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
    /// use mcp_execution_files::FileEntry;
    ///
    /// let file = FileEntry::new("test content");
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
    /// use mcp_execution_files::FileEntry;
    ///
    /// let file = FileEntry::new("hello");
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
/// use mcp_execution_files::{Result, FilePath};
///
/// fn validate_path(path: &str) -> Result<FilePath> {
///     FilePath::new(path)
/// }
/// ```
pub type Result<T> = std::result::Result<T, FilesError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfs_path_new_valid() {
        let path = FilePath::new("/mcp-tools/test.ts").unwrap();
        assert_eq!(path.as_str(), "/mcp-tools/test.ts");
    }

    #[test]
    fn test_vfs_path_new_relative_fails() {
        let result = FilePath::new("relative/path");
        assert!(matches!(result, Err(FilesError::PathNotAbsolute { .. })));
    }

    #[test]
    fn test_vfs_path_new_parent_dir_fails() {
        let result = FilePath::new("/parent/../escape");
        assert!(matches!(
            result,
            Err(FilesError::InvalidPathComponent { .. })
        ));
    }

    #[test]
    fn test_vfs_path_new_doubled_slash_fails() {
        // Regression test: an empty top-level component (from a doubled
        // leading slash) previously slipped through validation and made
        // `base.join("")` resolve to `base` itself downstream, letting a
        // single-file "group" swap the shared base directory.
        let result = FilePath::new("//x.ts");
        assert!(matches!(
            result,
            Err(FilesError::InvalidPathComponent { .. })
        ));
    }

    #[test]
    fn test_vfs_path_new_dot_component_fails() {
        let result = FilePath::new("/./x.ts");
        assert!(matches!(
            result,
            Err(FilesError::InvalidPathComponent { .. })
        ));
    }

    #[test]
    fn test_vfs_path_new_trailing_slash_fails() {
        let result = FilePath::new("/github/");
        assert!(matches!(
            result,
            Err(FilesError::InvalidPathComponent { .. })
        ));
    }

    #[test]
    fn test_vfs_path_new_root_is_valid() {
        // The root path itself has no components to check and must remain valid
        // even after rejecting empty/'.' components elsewhere in the path.
        let result = FilePath::new("/");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "/");
    }

    #[test]
    fn test_vfs_path_new_empty_fails() {
        let result = FilePath::new("");
        assert!(result.is_err());
    }

    #[test]
    fn test_vfs_path_parent() {
        let path = FilePath::new("/mcp-tools/servers/test.ts").unwrap();
        let parent = path.parent().unwrap();
        assert_eq!(parent.as_str(), "/mcp-tools/servers");
    }

    #[test]
    fn test_vfs_path_parent_root() {
        let path = FilePath::new("/test").unwrap();
        let parent = path.parent();
        assert!(parent.is_some());
    }

    #[test]
    fn test_vfs_path_display() {
        let path = FilePath::new("/test.ts").unwrap();
        assert_eq!(format!("{path}"), "/test.ts");
    }

    #[test]
    fn test_vfs_file_new() {
        let file = FileEntry::new("test content");
        assert_eq!(file.content(), "test content");
        assert_eq!(file.size(), 12);
    }

    #[test]
    fn test_vfs_file_empty() {
        let file = FileEntry::new("");
        assert_eq!(file.content(), "");
        assert_eq!(file.size(), 0);
    }

    #[test]
    fn test_vfs_path_as_ref() {
        let vfs_path = FilePath::new("/test.ts").unwrap();
        let path: &Path = vfs_path.as_ref();
        assert_eq!(path.to_str(), Some("/test.ts"));
    }
}
