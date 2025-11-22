//! Error types for plugin store operations.

use std::path::PathBuf;

/// Result type for plugin store operations.
pub type Result<T> = std::result::Result<T, SkillStoreError>;

/// Errors that can occur during plugin store operations.
#[derive(thiserror::Error, Debug)]
pub enum SkillStoreError {
    /// Skill was not found in the store.
    ///
    /// This error occurs when attempting to load, remove, or access a skill
    /// that doesn't exist in the skill directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_store::{SkillStore, SkillStoreError};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let temp = tempfile::tempdir()?;
    /// let store = SkillStore::new(temp.path())?;
    ///
    /// let result = store.load_skill("nonexistent");
    /// assert!(matches!(result, Err(SkillStoreError::PluginNotFound { .. })));
    /// # Ok(())
    /// # }
    /// ```
    #[error("Skill not found: {server_name}")]
    PluginNotFound {
        /// Name of the server whose skill was not found
        server_name: String,
    },

    /// Skill already exists in the store.
    ///
    /// This error occurs when attempting to save a skill with a server name
    /// that already has a skill saved. To overwrite, remove the existing
    /// plugin first.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::{SkillStore, SkillStoreError, ServerInfo};
    /// use mcp_vfs::VfsBuilder;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let temp = tempfile::tempdir()?;
    /// let store = SkillStore::new(temp.path())?;
    /// let vfs = VfsBuilder::new().build()?;
    /// let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    /// let server_info = ServerInfo {
    ///     name: "test".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     protocol_version: "2024-11-05".to_string(),
    /// };
    ///
    /// // First save succeeds
    /// store.save_skill("test", &vfs, &wasm, server_info.clone(), vec![])?;
    ///
    /// // Second save fails
    /// let result = store.save_skill("test", &vfs, &wasm, server_info, vec![]);
    /// assert!(matches!(result, Err(SkillStoreError::PluginAlreadyExists { .. })));
    /// # Ok(())
    /// # }
    /// ```
    #[error("Skill already exists: {server_name}")]
    PluginAlreadyExists {
        /// Name of the server whose skill already exists
        server_name: String,
    },

    /// Checksum verification failed during plugin load.
    ///
    /// This error indicates that a file's content hash doesn't match the
    /// expected value in the metadata, suggesting file corruption or tampering.
    ///
    /// # Security
    ///
    /// While Blake3 provides good integrity checking, this is not a security
    /// boundary against adversarial attacks. For untrusted skills, additional
    /// cryptographic signatures would be required.
    #[error("Checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Path of the file with mismatched checksum
        path: String,
        /// Expected checksum from metadata
        expected: String,
        /// Actual checksum calculated from file
        actual: String,
    },

    /// Plugin metadata is invalid or malformed.
    ///
    /// This error occurs when `skill.json` cannot be parsed or contains
    /// invalid data (e.g., missing required fields, invalid format version).
    #[error("Invalid metadata format: {reason}")]
    InvalidMetadata {
        /// Description of why the metadata is invalid
        reason: String,
    },

    /// Server name contains invalid characters or path traversal attempts.
    ///
    /// Server names must be valid directory names without path separators,
    /// parent directory references, or other special characters that could
    /// enable directory traversal attacks.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::{SkillStore, SkillStoreError};
    /// use mcp_vfs::VfsBuilder;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let temp = tempfile::tempdir()?;
    /// let store = SkillStore::new(temp.path())?;
    /// let vfs = VfsBuilder::new().build()?;
    /// let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    ///
    /// // These server names are invalid
    /// assert!(store.skill_exists("../escape").unwrap_err().to_string().contains("Invalid"));
    /// assert!(store.skill_exists("/absolute").unwrap_err().to_string().contains("Invalid"));
    /// assert!(store.skill_exists("sub/dir").unwrap_err().to_string().contains("Invalid"));
    /// # Ok(())
    /// # }
    /// ```
    #[error("Invalid server name: {server_name} ({reason})")]
    InvalidServerName {
        /// The invalid server name
        server_name: String,
        /// Why the server name is invalid
        reason: String,
    },

    /// I/O error occurred during file operations.
    ///
    /// This wraps standard I/O errors from file system operations like
    /// reading, writing, creating directories, etc.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    ///
    /// This wraps errors from parsing or generating JSON, typically from
    /// `skill.json` metadata files.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// VFS error occurred during VFS operations.
    ///
    /// This wraps errors from the VFS layer when building or manipulating
    /// the virtual filesystem.
    #[error("VFS error: {0}")]
    Vfs(#[from] mcp_vfs::VfsError),

    /// File is missing from skill directory.
    ///
    /// This indicates a required file (like `skill.json` or `module.wasm`)
    /// is missing from the skill directory, suggesting an incomplete or
    /// corrupted plugin.
    #[error("Missing file in plugin {server_name}: {path}")]
    MissingFile {
        /// Server name of the skill with missing file
        server_name: String,
        /// Path of the missing file relative to skill directory
        path: PathBuf,
    },
}

// Ensure error type follows Microsoft Rust Guidelines
impl SkillStoreError {
    /// Returns true if this error is recoverable.
    ///
    /// Recoverable errors are typically user errors (invalid names, missing
    /// plugins) rather than system errors (I/O failures).
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::PluginNotFound { .. }
                | Self::PluginAlreadyExists { .. }
                | Self::InvalidServerName { .. }
                | Self::InvalidMetadata { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_plugin_not_found_display() {
        let error = SkillStoreError::PluginNotFound {
            server_name: "test-server".to_string(),
        };

        let display = format!("{error}");
        assert!(display.contains("Skill not found"));
        assert!(display.contains("test-server"));
    }

    #[test]
    fn test_plugin_already_exists_display() {
        let error = SkillStoreError::PluginAlreadyExists {
            server_name: "existing-server".to_string(),
        };

        let display = format!("{error}");
        assert!(display.contains("Skill already exists"));
        assert!(display.contains("existing-server"));
    }

    #[test]
    fn test_checksum_mismatch_display() {
        let error = SkillStoreError::ChecksumMismatch {
            path: "/path/to/file.wasm".to_string(),
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        };

        let display = format!("{error}");
        assert!(display.contains("Checksum mismatch"));
        assert!(display.contains("file.wasm"));
        assert!(display.contains("abc123"));
        assert!(display.contains("def456"));
    }

    #[test]
    fn test_invalid_metadata_display() {
        let error = SkillStoreError::InvalidMetadata {
            reason: "missing required field 'version'".to_string(),
        };

        let display = format!("{error}");
        assert!(display.contains("Invalid metadata format"));
        assert!(display.contains("missing required field"));
    }

    #[test]
    fn test_invalid_server_name_display() {
        let error = SkillStoreError::InvalidServerName {
            server_name: "../escape".to_string(),
            reason: "contains path traversal".to_string(),
        };

        let display = format!("{error}");
        assert!(display.contains("Invalid server name"));
        assert!(display.contains("../escape"));
        assert!(display.contains("path traversal"));
    }

    #[test]
    fn test_missing_file_display() {
        let error = SkillStoreError::MissingFile {
            server_name: "test".to_string(),
            path: PathBuf::from("skill.json"),
        };

        let display = format!("{error}");
        assert!(display.contains("Missing file"));
        assert!(display.contains("test"));
        assert!(display.contains("skill.json"));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let error: SkillStoreError = io_error.into();

        let display = format!("{error}");
        assert!(display.contains("IO error"));
        assert!(display.contains("file not found"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json_str = "{invalid json";
        let json_error = serde_json::from_str::<serde_json::Value>(json_str).unwrap_err();
        let error: SkillStoreError = json_error.into();

        let display = format!("{error}");
        assert!(display.contains("JSON error"));
    }

    #[test]
    fn test_is_recoverable_plugin_not_found() {
        let error = SkillStoreError::PluginNotFound {
            server_name: "test".to_string(),
        };
        assert!(error.is_recoverable());
    }

    #[test]
    fn test_is_recoverable_plugin_already_exists() {
        let error = SkillStoreError::PluginAlreadyExists {
            server_name: "test".to_string(),
        };
        assert!(error.is_recoverable());
    }

    #[test]
    fn test_is_recoverable_invalid_server_name() {
        let error = SkillStoreError::InvalidServerName {
            server_name: "../test".to_string(),
            reason: "path traversal".to_string(),
        };
        assert!(error.is_recoverable());
    }

    #[test]
    fn test_is_recoverable_invalid_metadata() {
        let error = SkillStoreError::InvalidMetadata {
            reason: "missing field".to_string(),
        };
        assert!(error.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_io_error() {
        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let error: SkillStoreError = io_error.into();
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_checksum_mismatch() {
        let error = SkillStoreError::ChecksumMismatch {
            path: "test.wasm".to_string(),
            expected: "abc".to_string(),
            actual: "def".to_string(),
        };
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_is_not_recoverable_missing_file() {
        let error = SkillStoreError::MissingFile {
            server_name: "test".to_string(),
            path: PathBuf::from("skill.json"),
        };
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_error_debug() {
        let error = SkillStoreError::PluginNotFound {
            server_name: "test".to_string(),
        };

        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("PluginNotFound"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    #[allow(clippy::unnecessary_wraps)]
    fn test_result_type_alias() {
        fn returns_result() -> Result<i32> {
            Ok(42)
        }

        let result = returns_result();
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_multiple_error_variants_debug() {
        let errors = vec![
            SkillStoreError::PluginNotFound {
                server_name: "test1".to_string(),
            },
            SkillStoreError::PluginAlreadyExists {
                server_name: "test2".to_string(),
            },
            SkillStoreError::InvalidServerName {
                server_name: "../test3".to_string(),
                reason: "invalid".to_string(),
            },
            SkillStoreError::ChecksumMismatch {
                path: "file.wasm".to_string(),
                expected: "abc".to_string(),
                actual: "def".to_string(),
            },
            SkillStoreError::InvalidMetadata {
                reason: "bad format".to_string(),
            },
            SkillStoreError::MissingFile {
                server_name: "test4".to_string(),
                path: PathBuf::from("missing.json"),
            },
        ];

        for error in &errors {
            let debug = format!("{error:?}");
            let display = format!("{error}");
            assert!(!debug.is_empty());
            assert!(!display.is_empty());
        }
    }

    #[test]
    fn test_checksum_mismatch_with_long_hashes() {
        let error = SkillStoreError::ChecksumMismatch {
            path: "module.wasm".to_string(),
            expected: "a".repeat(64),
            actual: "b".repeat(64),
        };

        let display = format!("{error}");
        assert!(display.contains(&"a".repeat(64)));
        assert!(display.contains(&"b".repeat(64)));
    }

    #[test]
    fn test_invalid_server_name_empty() {
        let error = SkillStoreError::InvalidServerName {
            server_name: String::new(),
            reason: "empty name".to_string(),
        };

        assert!(error.is_recoverable());
        let display = format!("{error}");
        assert!(display.contains("empty name"));
    }

    #[test]
    fn test_missing_file_with_nested_path() {
        let error = SkillStoreError::MissingFile {
            server_name: "test".to_string(),
            path: PathBuf::from("subdir/nested/file.txt"),
        };

        let display = format!("{error}");
        assert!(display.contains("subdir"));
        assert!(display.contains("file.txt"));
    }

    #[test]
    fn test_invalid_metadata_various_reasons() {
        let reasons = vec![
            "missing 'version' field",
            "invalid JSON syntax",
            "unsupported format version",
            "corrupted data",
        ];

        for reason in reasons {
            let error = SkillStoreError::InvalidMetadata {
                reason: reason.to_string(),
            };
            assert!(error.is_recoverable());
            assert!(format!("{error}").contains(reason));
        }
    }

    #[test]
    fn test_error_source_chain() {
        use std::error::Error;

        let io_error = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let error: SkillStoreError = io_error.into();

        // Test that error can be used with source trait
        let source = error.source();
        assert!(source.is_some());
    }
}
