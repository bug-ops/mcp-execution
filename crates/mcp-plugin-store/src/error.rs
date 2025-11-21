//! Error types for plugin store operations.

use std::path::PathBuf;

/// Result type for plugin store operations.
pub type Result<T> = std::result::Result<T, PluginStoreError>;

/// Errors that can occur during plugin store operations.
#[derive(thiserror::Error, Debug)]
pub enum PluginStoreError {
    /// Plugin was not found in the store.
    ///
    /// This error occurs when attempting to load, remove, or access a plugin
    /// that doesn't exist in the plugin directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_plugin_store::{PluginStore, PluginStoreError};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let temp = tempfile::tempdir()?;
    /// let store = PluginStore::new(temp.path())?;
    ///
    /// let result = store.load_plugin("nonexistent");
    /// assert!(matches!(result, Err(PluginStoreError::PluginNotFound { .. })));
    /// # Ok(())
    /// # }
    /// ```
    #[error("Plugin not found: {server_name}")]
    PluginNotFound {
        /// Name of the server whose plugin was not found
        server_name: String,
    },

    /// Plugin already exists in the store.
    ///
    /// This error occurs when attempting to save a plugin with a server name
    /// that already has a plugin saved. To overwrite, remove the existing
    /// plugin first.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::{PluginStore, PluginStoreError, ServerInfo};
    /// use mcp_vfs::VfsBuilder;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let temp = tempfile::tempdir()?;
    /// let store = PluginStore::new(temp.path())?;
    /// let vfs = VfsBuilder::new().build()?;
    /// let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    /// let server_info = ServerInfo {
    ///     name: "test".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     protocol_version: "2024-11-05".to_string(),
    /// };
    ///
    /// // First save succeeds
    /// store.save_plugin("test", &vfs, &wasm, server_info.clone(), vec![])?;
    ///
    /// // Second save fails
    /// let result = store.save_plugin("test", &vfs, &wasm, server_info, vec![]);
    /// assert!(matches!(result, Err(PluginStoreError::PluginAlreadyExists { .. })));
    /// # Ok(())
    /// # }
    /// ```
    #[error("Plugin already exists: {server_name}")]
    PluginAlreadyExists {
        /// Name of the server whose plugin already exists
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
    /// boundary against adversarial attacks. For untrusted plugins, additional
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
    /// This error occurs when `plugin.json` cannot be parsed or contains
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
    /// use mcp_plugin_store::{PluginStore, PluginStoreError};
    /// use mcp_vfs::VfsBuilder;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let temp = tempfile::tempdir()?;
    /// let store = PluginStore::new(temp.path())?;
    /// let vfs = VfsBuilder::new().build()?;
    /// let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    ///
    /// // These server names are invalid
    /// assert!(store.plugin_exists("../escape").unwrap_err().to_string().contains("Invalid"));
    /// assert!(store.plugin_exists("/absolute").unwrap_err().to_string().contains("Invalid"));
    /// assert!(store.plugin_exists("sub/dir").unwrap_err().to_string().contains("Invalid"));
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
    /// `plugin.json` metadata files.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// VFS error occurred during VFS operations.
    ///
    /// This wraps errors from the VFS layer when building or manipulating
    /// the virtual filesystem.
    #[error("VFS error: {0}")]
    Vfs(#[from] mcp_vfs::VfsError),

    /// File is missing from plugin directory.
    ///
    /// This indicates a required file (like `plugin.json` or `module.wasm`)
    /// is missing from the plugin directory, suggesting an incomplete or
    /// corrupted plugin.
    #[error("Missing file in plugin {server_name}: {path}")]
    MissingFile {
        /// Server name of the plugin with missing file
        server_name: String,
        /// Path of the missing file relative to plugin directory
        path: PathBuf,
    },
}

// Ensure error type follows Microsoft Rust Guidelines
impl PluginStoreError {
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
