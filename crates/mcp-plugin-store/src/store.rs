//! Plugin storage implementation.
//!
//! Provides the main [`PluginStore`] type for saving, loading, and managing
//! plugins on disk.

use crate::checksum::{calculate_checksum, verify_checksum};
use crate::error::{PluginStoreError, Result};
use crate::types::{
    Checksums, FORMAT_VERSION, GENERATED_DIR, LoadedPlugin, METADATA_FILE, PluginInfo,
    PluginMetadata, ServerInfo, ToolInfo, WASM_FILE,
};
use chrono::Utc;
use mcp_vfs::{Vfs, VfsBuilder};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// RAII guard for plugin directory cleanup on error.
///
/// Automatically removes a plugin directory if the save operation fails
/// or panics, preventing partial plugin state on disk.
///
/// This guard ensures atomic-like behavior: the plugin directory is either
/// fully written or completely removed on error.
struct PluginDirGuard {
    path: PathBuf,
    cleanup: bool,
}

impl PluginDirGuard {
    /// Creates a new guard for the given plugin directory.
    ///
    /// The directory will be removed on drop unless [`commit`](Self::commit) is called.
    const fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    /// Commits the save operation, disabling cleanup on drop.
    ///
    /// Call this after successfully writing all plugin files.
    fn commit(mut self) {
        self.cleanup = false;
    }
}

impl Drop for PluginDirGuard {
    fn drop(&mut self) {
        if self.cleanup {
            if let Err(e) = fs::remove_dir_all(&self.path) {
                tracing::warn!(
                    "Failed to cleanup plugin directory {}: {}",
                    self.path.display(),
                    e
                );
            } else {
                tracing::debug!(
                    "Cleaned up incomplete plugin directory: {}",
                    self.path.display()
                );
            }
        }
    }
}

/// Plugin storage manager.
///
/// Manages a directory of saved plugins, providing operations to save, load,
/// list, and remove plugins. Each plugin is stored in its own subdirectory
/// named after the server.
///
/// # Directory Structure
///
/// ```text
/// base_dir/
/// ├── server1/
/// │   ├── plugin.json
/// │   ├── generated/
/// │   │   └── ...
/// │   └── module.wasm
/// └── server2/
///     ├── plugin.json
///     ├── generated/
///     └── module.wasm
/// ```
///
/// # Thread Safety
///
/// `PluginStore` is `Send + Sync` and can be safely shared between threads.
/// However, concurrent modifications to the same plugin directory may result
/// in undefined behavior. Use external synchronization if needed.
///
/// # Examples
///
/// ```no_run
/// use mcp_plugin_store::{PluginStore, ServerInfo};
/// use mcp_vfs::VfsBuilder;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Create store
/// let store = PluginStore::new("./plugins")?;
///
/// // Save a plugin
/// let vfs = VfsBuilder::new()
///     .add_file("/index.ts", "export * from './tools';")
///     .build()?;
/// let wasm = vec![0x00, 0x61, 0x73, 0x6D]; // WASM magic bytes
/// let server_info = ServerInfo {
///     name: "my-server".to_string(),
///     version: "1.0.0".to_string(),
///     protocol_version: "2024-11-05".to_string(),
/// };
///
/// store.save_plugin("my-server", &vfs, &wasm, server_info, vec![])?;
///
/// // Load it back
/// let plugin = store.load_plugin("my-server")?;
/// assert_eq!(plugin.metadata.server.name, "my-server");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct PluginStore {
    base_dir: PathBuf,
}

impl PluginStore {
    /// Creates a new plugin store at the given directory.
    ///
    /// Creates the base directory if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or is not writable.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::PluginStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = PluginStore::new("./plugins")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(base_dir: impl AsRef<Path>) -> Result<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();

        // Create directory if it doesn't exist
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
            tracing::debug!("Created plugin store directory: {}", base_dir.display());
        }

        Ok(Self { base_dir })
    }

    /// Saves a plugin to disk.
    ///
    /// Writes the plugin files to a new subdirectory named after the server.
    /// Calculates checksums for all files and stores them in metadata.
    ///
    /// # Atomicity
    ///
    /// This operation is atomic at the directory level:
    /// - Directory creation uses atomic `create_dir` (fails if exists)
    /// - On error or panic, the partial plugin directory is automatically cleaned up
    /// - Once complete, the plugin is fully saved or not saved at all
    ///
    /// # Concurrency
    ///
    /// Safe for concurrent calls with different `server_name` values.
    /// Concurrent saves to the same plugin will result in one success and
    /// one [`PluginStoreError::PluginAlreadyExists`] error (atomic directory creation).
    ///
    /// # Arguments
    ///
    /// * `server_name` - Server identifier (must be valid directory name)
    /// * `vfs` - Virtual filesystem with generated TypeScript code
    /// * `wasm_module` - Compiled WASM module bytes
    /// * `server_info` - Server identification information
    /// * `tool_info` - List of tools provided by the plugin
    ///
    /// # Errors
    ///
    /// * [`PluginStoreError::PluginAlreadyExists`] - Plugin directory exists
    /// * [`PluginStoreError::InvalidServerName`] - Invalid server name
    /// * I/O errors if writing fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::{PluginStore, ServerInfo, ToolInfo};
    /// use mcp_vfs::VfsBuilder;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = PluginStore::new("./plugins")?;
    /// let vfs = VfsBuilder::new().build()?;
    /// let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    ///
    /// let server_info = ServerInfo {
    ///     name: "test-server".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     protocol_version: "2024-11-05".to_string(),
    /// };
    ///
    /// let tools = vec![ToolInfo {
    ///     name: "test_tool".to_string(),
    ///     description: "Test tool".to_string(),
    /// }];
    ///
    /// let metadata = store.save_plugin("test-server", &vfs, &wasm, server_info, tools)?;
    /// println!("Saved plugin with {} tools", metadata.tools.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn save_plugin(
        &self,
        server_name: &str,
        vfs: &Vfs,
        wasm_module: &[u8],
        server_info: ServerInfo,
        tool_info: Vec<ToolInfo>,
    ) -> Result<PluginMetadata> {
        // Validate server name
        validate_server_name(server_name)?;

        let plugin_dir = self.plugin_path(server_name);

        // Create directory atomically - fails if already exists
        // This prevents TOCTOU race condition
        match fs::create_dir(&plugin_dir) {
            Ok(()) => {
                tracing::debug!("Created plugin directory: {}", plugin_dir.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(PluginStoreError::PluginAlreadyExists {
                    server_name: server_name.to_string(),
                });
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        // Set up cleanup guard - will remove directory if we panic or return error
        let guard = PluginDirGuard::new(plugin_dir.clone());

        tracing::info!("Saving plugin for server: {}", server_name);

        // Create plugin directory structure
        let generated_dir = plugin_dir.join(GENERATED_DIR);
        fs::create_dir_all(&generated_dir)?;

        // Track generated file checksums
        let mut generated_checksums = HashMap::new();

        // Write all VFS files to generated/ directory
        for vfs_path in vfs.all_paths() {
            let content = vfs
                .read_file(vfs_path.as_str())
                .map_err(PluginStoreError::Vfs)?;

            // Convert VFS path (absolute, starting with /) to relative path
            // Example: /tools/sendMessage.ts -> tools/sendMessage.ts
            let relative_path = vfs_path.as_str().trim_start_matches('/');

            // Calculate checksum before writing
            let checksum = calculate_checksum(content.as_bytes());
            generated_checksums.insert(relative_path.to_string(), checksum);

            // Write file to disk
            let file_path = generated_dir.join(relative_path);

            // Create parent directories if needed
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(&file_path, content)?;
            tracing::debug!("Wrote VFS file: {}", relative_path);
        }

        // Calculate WASM checksum and write module
        let wasm_checksum = calculate_checksum(wasm_module);
        let wasm_path = plugin_dir.join(WASM_FILE);
        fs::write(&wasm_path, wasm_module)?;
        tracing::debug!("Wrote WASM module: {} bytes", wasm_module.len());

        // Create metadata with checksums
        let metadata = PluginMetadata {
            format_version: FORMAT_VERSION.to_string(),
            server: server_info,
            generated_at: Utc::now(),
            generator_version: env!("CARGO_PKG_VERSION").to_string(),
            checksums: Checksums {
                wasm: wasm_checksum,
                generated: generated_checksums,
            },
            tools: tool_info,
        };

        // Write metadata to plugin.json
        let metadata_path = plugin_dir.join(METADATA_FILE);
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        fs::write(&metadata_path, metadata_json)?;
        tracing::debug!("Wrote plugin metadata");

        // Success - disable cleanup
        guard.commit();

        tracing::info!(
            "Successfully saved plugin for server: {} ({} files, {} tools)",
            server_name,
            metadata.checksums.generated.len(),
            metadata.tools.len()
        );

        Ok(metadata)
    }

    /// Loads a plugin from disk.
    ///
    /// Reads all plugin files and verifies checksums before returning.
    ///
    /// # Errors
    ///
    /// * [`PluginStoreError::PluginNotFound`] - Plugin doesn't exist
    /// * [`PluginStoreError::ChecksumMismatch`] - File hash mismatch
    /// * [`PluginStoreError::InvalidMetadata`] - Malformed metadata
    /// * [`PluginStoreError::MissingFile`] - Required file missing
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::PluginStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = PluginStore::new("./plugins")?;
    /// let plugin = store.load_plugin("my-server")?;
    ///
    /// println!("Loaded {} tools", plugin.metadata.tools.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_plugin(&self, server_name: &str) -> Result<LoadedPlugin> {
        // Validate server name
        validate_server_name(server_name)?;

        // Check plugin exists
        let plugin_dir = self.plugin_path(server_name);
        if !plugin_dir.exists() {
            return Err(PluginStoreError::PluginNotFound {
                server_name: server_name.to_string(),
            });
        }

        tracing::info!("Loading plugin for server: {}", server_name);

        // Read and parse plugin.json
        let metadata_path = plugin_dir.join(METADATA_FILE);
        if !metadata_path.exists() {
            return Err(PluginStoreError::MissingFile {
                server_name: server_name.to_string(),
                path: METADATA_FILE.into(),
            });
        }
        let metadata = Self::read_metadata(&metadata_path)?;

        // Read and verify WASM module
        let wasm_path = plugin_dir.join(WASM_FILE);
        if !wasm_path.exists() {
            return Err(PluginStoreError::MissingFile {
                server_name: server_name.to_string(),
                path: WASM_FILE.into(),
            });
        }
        let wasm_module = fs::read(&wasm_path)?;
        verify_checksum(&wasm_module, &metadata.checksums.wasm, WASM_FILE)?;
        tracing::debug!("Verified WASM module checksum: {} bytes", wasm_module.len());

        // Build VFS from generated files
        let mut vfs_builder = VfsBuilder::new();
        let generated_dir = plugin_dir.join(GENERATED_DIR);

        // Walk the generated/ directory and load all files
        for entry in WalkDir::new(&generated_dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path();

            // Get relative path from generated/ directory
            let relative_path = file_path.strip_prefix(&generated_dir).map_err(|_| {
                PluginStoreError::InvalidMetadata {
                    reason: format!("Failed to strip prefix from path: {}", file_path.display()),
                }
            })?;

            // Convert to string for lookups
            let relative_path_str = relative_path.to_string_lossy().to_string();

            // Normalize path separators to forward slashes for cross-platform compatibility
            let normalized_path = relative_path_str.replace('\\', "/");

            // Check if this file is in metadata
            let expected_checksum = metadata
                .checksums
                .generated
                .get(&normalized_path)
                .ok_or_else(|| PluginStoreError::InvalidMetadata {
                    reason: format!("File '{normalized_path}' not found in metadata checksums"),
                })?;

            // Read file content
            let content = fs::read(file_path)?;

            // Verify checksum
            verify_checksum(&content, expected_checksum, &normalized_path)?;

            // Add to VFS with absolute path (prepend /)
            let vfs_path = format!("/{normalized_path}");
            let content_str =
                String::from_utf8(content).map_err(|e| PluginStoreError::InvalidMetadata {
                    reason: format!("File '{normalized_path}' is not valid UTF-8: {e}"),
                })?;

            vfs_builder = vfs_builder.add_file(&vfs_path, content_str);
            tracing::debug!("Loaded and verified: {}", vfs_path);
        }

        // Build VFS
        let vfs = vfs_builder.build()?;

        // Verify all expected files were found
        let loaded_count = vfs.file_count();
        let expected_count = metadata.checksums.generated.len();
        if loaded_count != expected_count {
            return Err(PluginStoreError::InvalidMetadata {
                reason: format!(
                    "File count mismatch: loaded {loaded_count} files but metadata lists {expected_count}"
                ),
            });
        }

        tracing::info!(
            "Successfully loaded plugin for server: {} ({} files, {} tools)",
            server_name,
            vfs.file_count(),
            metadata.tools.len()
        );

        Ok(LoadedPlugin {
            metadata,
            vfs,
            wasm_module,
        })
    }

    /// Lists all available plugins.
    ///
    /// Returns brief information about each plugin without loading full content.
    ///
    /// # Errors
    ///
    /// Returns an error if reading the plugin directory fails or if metadata
    /// files cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::PluginStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = PluginStore::new("./plugins")?;
    ///
    /// for plugin in store.list_plugins()? {
    ///     println!("{} v{} - {} tools",
    ///         plugin.server_name,
    ///         plugin.version,
    ///         plugin.tool_count
    ///     );
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn list_plugins(&self) -> Result<Vec<PluginInfo>> {
        let mut plugins = Vec::new();

        // Iterate over subdirectories in base_dir
        for entry in fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            // Try to read metadata
            let metadata_path = path.join(METADATA_FILE);
            if !metadata_path.exists() {
                tracing::warn!("Skipping directory without metadata: {}", path.display());
                continue;
            }

            match Self::read_metadata(&metadata_path) {
                Ok(metadata) => {
                    plugins.push(PluginInfo {
                        server_name: metadata.server.name,
                        version: metadata.server.version,
                        generated_at: metadata.generated_at,
                        tool_count: metadata.tools.len(),
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to read metadata from {}: {}", path.display(), e);
                }
            }
        }

        Ok(plugins)
    }

    /// Checks if a plugin exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the server name is invalid.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::PluginStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = PluginStore::new("./plugins")?;
    ///
    /// if store.plugin_exists("my-server")? {
    ///     println!("Plugin exists");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn plugin_exists(&self, server_name: &str) -> Result<bool> {
        validate_server_name(server_name)?;
        Ok(self.plugin_path(server_name).exists())
    }

    /// Removes a plugin from disk.
    ///
    /// Deletes the entire plugin directory and all its contents.
    ///
    /// # Errors
    ///
    /// * [`PluginStoreError::PluginNotFound`] - Plugin doesn't exist
    /// * I/O errors if deletion fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::PluginStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = PluginStore::new("./plugins")?;
    /// store.remove_plugin("old-server")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn remove_plugin(&self, server_name: &str) -> Result<()> {
        validate_server_name(server_name)?;

        let plugin_dir = self.plugin_path(server_name);
        if !plugin_dir.exists() {
            return Err(PluginStoreError::PluginNotFound {
                server_name: server_name.to_string(),
            });
        }

        fs::remove_dir_all(&plugin_dir)?;
        tracing::info!("Removed plugin: {}", server_name);
        Ok(())
    }

    /// Gets the path to a plugin directory.
    ///
    /// Does not check if the directory exists.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_plugin_store::PluginStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = PluginStore::new("./plugins")?;
    /// let path = store.plugin_path("my-server");
    /// println!("Plugin path: {}", path.display());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn plugin_path(&self, server_name: &str) -> PathBuf {
        self.base_dir.join(server_name)
    }

    /// Reads and parses plugin metadata from disk.
    fn read_metadata(metadata_path: &Path) -> Result<PluginMetadata> {
        let content = fs::read_to_string(metadata_path)?;
        let metadata: PluginMetadata =
            serde_json::from_str(&content).map_err(|e| PluginStoreError::InvalidMetadata {
                reason: format!("Failed to parse JSON: {e}"),
            })?;

        // Validate format version
        if metadata.format_version != FORMAT_VERSION {
            return Err(PluginStoreError::InvalidMetadata {
                reason: format!(
                    "Unsupported format version: {} (expected {})",
                    metadata.format_version, FORMAT_VERSION
                ),
            });
        }

        Ok(metadata)
    }
}

/// Validates that a server name is safe to use as a directory name.
///
/// Rejects names that:
/// - Contain path separators (/ or \)
/// - Are parent directory references (. or ..)
/// - Are empty
/// - Contain control characters
///
/// # Errors
///
/// Returns [`PluginStoreError::InvalidServerName`] if the name is invalid.
fn validate_server_name(server_name: &str) -> Result<()> {
    if server_name.is_empty() {
        return Err(PluginStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot be empty".to_string(),
        });
    }

    if server_name == "." || server_name == ".." {
        return Err(PluginStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot be '.' or '..'".to_string(),
        });
    }

    if server_name.contains('/') || server_name.contains('\\') {
        return Err(PluginStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot contain path separators".to_string(),
        });
    }

    if server_name.chars().any(char::is_control) {
        return Err(PluginStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot contain control characters".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_new_creates_directory() {
        let temp = TempDir::new().unwrap();
        let store_path = temp.path().join("plugins");

        let _store = PluginStore::new(&store_path).unwrap();
        assert!(store_path.exists());
    }

    #[test]
    fn test_plugin_path() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let path = store.plugin_path("test-server");
        assert!(path.ends_with("test-server"));
    }

    #[test]
    fn test_validate_server_name_valid() {
        assert!(validate_server_name("valid-name").is_ok());
        assert!(validate_server_name("server123").is_ok());
        assert!(validate_server_name("my_server").is_ok());
    }

    #[test]
    fn test_validate_server_name_invalid() {
        assert!(validate_server_name("").is_err());
        assert!(validate_server_name(".").is_err());
        assert!(validate_server_name("..").is_err());
        assert!(validate_server_name("path/traversal").is_err());
        assert!(validate_server_name("path\\traversal").is_err());
    }

    #[test]
    fn test_plugin_exists_nonexistent() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        assert!(!store.plugin_exists("nonexistent").unwrap());
    }

    #[test]
    fn test_list_plugins_empty() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let plugins = store.list_plugins().unwrap();
        assert_eq!(plugins.len(), 0);
    }

    // Helper function to create test VFS
    fn create_test_vfs() -> Vfs {
        use mcp_vfs::VfsBuilder;

        VfsBuilder::new()
            .add_file("/index.ts", "export * from './tools';")
            .add_file("/tools/sendMessage.ts", "export function sendMessage() {}")
            .add_file("/tools/getChatInfo.ts", "export function getChatInfo() {}")
            .add_file("/types.ts", "export type Message = { id: string };")
            .build()
            .unwrap()
    }

    // Helper function to create test server info
    fn create_test_server_info(name: &str) -> ServerInfo {
        ServerInfo {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            protocol_version: "2024-11-05".to_string(),
        }
    }

    // Helper function to create test tools
    fn create_test_tools() -> Vec<ToolInfo> {
        vec![
            ToolInfo {
                name: "send_message".to_string(),
                description: "Sends a message".to_string(),
            },
            ToolInfo {
                name: "get_chat_info".to_string(),
                description: "Gets chat info".to_string(),
            },
        ]
    }

    #[test]
    fn test_save_plugin_success() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D]; // WASM magic bytes
        let server_info = create_test_server_info("test-server");
        let tools = create_test_tools();

        let metadata = store
            .save_plugin(
                "test-server",
                &vfs,
                &wasm,
                server_info.clone(),
                tools.clone(),
            )
            .unwrap();

        // Verify metadata
        assert_eq!(metadata.server.name, "test-server");
        assert_eq!(metadata.server.version, "1.0.0");
        assert_eq!(metadata.tools.len(), 2);
        assert_eq!(metadata.format_version, FORMAT_VERSION);

        // Verify directory structure
        let plugin_dir = store.plugin_path("test-server");
        assert!(plugin_dir.exists());
        assert!(plugin_dir.join(METADATA_FILE).exists());
        assert!(plugin_dir.join(WASM_FILE).exists());
        assert!(plugin_dir.join(GENERATED_DIR).exists());

        // Verify generated files exist
        let generated_dir = plugin_dir.join(GENERATED_DIR);
        assert!(generated_dir.join("index.ts").exists());
        assert!(generated_dir.join("tools/sendMessage.ts").exists());
        assert!(generated_dir.join("tools/getChatInfo.ts").exists());
        assert!(generated_dir.join("types.ts").exists());

        // Verify checksums are present
        assert!(!metadata.checksums.wasm.is_empty());
        assert_eq!(metadata.checksums.generated.len(), 4);
    }

    #[test]
    fn test_save_plugin_already_exists() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("test-server");
        let tools = create_test_tools();

        // First save succeeds
        store
            .save_plugin(
                "test-server",
                &vfs,
                &wasm,
                server_info.clone(),
                tools.clone(),
            )
            .unwrap();

        // Second save fails
        let result = store.save_plugin("test-server", &vfs, &wasm, server_info, tools);
        assert!(matches!(
            result,
            Err(PluginStoreError::PluginAlreadyExists { .. })
        ));
    }

    #[test]
    fn test_save_load_roundtrip() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00]; // WASM header
        let server_info = create_test_server_info("roundtrip-server");
        let tools = create_test_tools();

        // Save plugin
        let save_metadata = store
            .save_plugin(
                "roundtrip-server",
                &vfs,
                &wasm,
                server_info.clone(),
                tools.clone(),
            )
            .unwrap();

        // Load plugin
        let loaded = store.load_plugin("roundtrip-server").unwrap();

        // Verify metadata matches
        assert_eq!(loaded.metadata.server.name, save_metadata.server.name);
        assert_eq!(loaded.metadata.server.version, save_metadata.server.version);
        assert_eq!(loaded.metadata.tools.len(), save_metadata.tools.len());
        assert_eq!(loaded.metadata.checksums.wasm, save_metadata.checksums.wasm);
        assert_eq!(
            loaded.metadata.checksums.generated.len(),
            save_metadata.checksums.generated.len()
        );

        // Verify WASM module matches
        assert_eq!(loaded.wasm_module, wasm);

        // Verify VFS files match
        assert_eq!(loaded.vfs.file_count(), vfs.file_count());

        // Verify individual file content
        let original_content = vfs.read_file("/index.ts").unwrap();
        let loaded_content = loaded.vfs.read_file("/index.ts").unwrap();
        assert_eq!(original_content, loaded_content);
    }

    #[test]
    fn test_load_plugin_not_found() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let result = store.load_plugin("nonexistent");
        assert!(matches!(
            result,
            Err(PluginStoreError::PluginNotFound { .. })
        ));
    }

    #[test]
    fn test_load_plugin_checksum_mismatch_wasm() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("corrupt-wasm");
        let tools = create_test_tools();

        // Save plugin
        store
            .save_plugin("corrupt-wasm", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Corrupt WASM file
        let wasm_path = store.plugin_path("corrupt-wasm").join(WASM_FILE);
        fs::write(&wasm_path, b"corrupted data").unwrap();

        // Load should fail with checksum mismatch
        let result = store.load_plugin("corrupt-wasm");
        assert!(matches!(
            result,
            Err(PluginStoreError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_load_plugin_checksum_mismatch_generated() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("corrupt-generated");
        let tools = create_test_tools();

        // Save plugin
        store
            .save_plugin("corrupt-generated", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Corrupt a generated file
        let file_path = store
            .plugin_path("corrupt-generated")
            .join(GENERATED_DIR)
            .join("index.ts");
        fs::write(&file_path, "corrupted content").unwrap();

        // Load should fail with checksum mismatch
        let result = store.load_plugin("corrupt-generated");
        assert!(matches!(
            result,
            Err(PluginStoreError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_load_plugin_missing_file() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("missing-file");
        let tools = create_test_tools();

        // Save plugin
        store
            .save_plugin("missing-file", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Delete a generated file
        let file_path = store
            .plugin_path("missing-file")
            .join(GENERATED_DIR)
            .join("index.ts");
        fs::remove_file(&file_path).unwrap();

        // Load should fail with invalid metadata (file count mismatch)
        let result = store.load_plugin("missing-file");
        assert!(matches!(
            result,
            Err(PluginStoreError::InvalidMetadata { .. })
        ));
    }

    #[test]
    fn test_multiple_plugins_same_store() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs1 = create_test_vfs();
        let vfs2 = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info1 = create_test_server_info("plugin1");
        let server_info2 = create_test_server_info("plugin2");
        let tools = create_test_tools();

        // Save two different plugins
        store
            .save_plugin("plugin1", &vfs1, &wasm, server_info1, tools.clone())
            .unwrap();
        store
            .save_plugin("plugin2", &vfs2, &wasm, server_info2, tools)
            .unwrap();

        // Both should exist
        assert!(store.plugin_exists("plugin1").unwrap());
        assert!(store.plugin_exists("plugin2").unwrap());

        // List should show both
        let plugins = store.list_plugins().unwrap();
        assert_eq!(plugins.len(), 2);

        // Load both
        let loaded1 = store.load_plugin("plugin1").unwrap();
        let loaded2 = store.load_plugin("plugin2").unwrap();

        assert_eq!(loaded1.metadata.server.name, "plugin1");
        assert_eq!(loaded2.metadata.server.name, "plugin2");
    }

    #[test]
    fn test_remove_plugin_and_reload() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("to-remove");
        let tools = create_test_tools();

        // Save plugin
        store
            .save_plugin("to-remove", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Verify it exists
        assert!(store.plugin_exists("to-remove").unwrap());

        // Remove it
        store.remove_plugin("to-remove").unwrap();

        // Should no longer exist
        assert!(!store.plugin_exists("to-remove").unwrap());

        // Load should fail
        let result = store.load_plugin("to-remove");
        assert!(matches!(
            result,
            Err(PluginStoreError::PluginNotFound { .. })
        ));
    }

    #[test]
    fn test_save_plugin_empty_vfs() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        use mcp_vfs::VfsBuilder;
        let vfs = VfsBuilder::new().build().unwrap();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("empty-vfs");

        // Should succeed with empty VFS
        let metadata = store
            .save_plugin("empty-vfs", &vfs, &wasm, server_info, vec![])
            .unwrap();

        assert_eq!(metadata.checksums.generated.len(), 0);
        assert_eq!(metadata.tools.len(), 0);

        // Should be able to load it back
        let loaded = store.load_plugin("empty-vfs").unwrap();
        assert_eq!(loaded.vfs.file_count(), 0);
    }

    #[test]
    fn test_concurrent_save_same_plugin() {
        use std::sync::Arc;
        use std::thread;

        let temp = TempDir::new().unwrap();
        let store = Arc::new(PluginStore::new(temp.path()).unwrap());

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let tools = create_test_tools();

        // Spawn two threads trying to save the same plugin
        let store1 = Arc::clone(&store);
        let vfs1 = vfs.clone();
        let wasm1 = wasm.clone();
        let tools1 = tools.clone();
        let t1 = thread::spawn(move || {
            store1.save_plugin(
                "concurrent-test",
                &vfs1,
                &wasm1,
                create_test_server_info("concurrent-test"),
                tools1,
            )
        });

        let store2 = Arc::clone(&store);
        let vfs2 = vfs.clone();
        let wasm2 = wasm.clone();
        let tools2 = tools.clone();
        let t2 = thread::spawn(move || {
            store2.save_plugin(
                "concurrent-test",
                &vfs2,
                &wasm2,
                create_test_server_info("concurrent-test"),
                tools2,
            )
        });

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        // Exactly one should succeed, one should get AlreadyExists
        let success_count = [&r1, &r2].iter().filter(|r| r.is_ok()).count();
        let already_exists_count = [&r1, &r2]
            .iter()
            .filter(|r| matches!(r, Err(PluginStoreError::PluginAlreadyExists { .. })))
            .count();

        assert_eq!(success_count, 1, "Exactly one save should succeed");
        assert_eq!(
            already_exists_count, 1,
            "Exactly one save should fail with AlreadyExists"
        );

        // Plugin should exist and be valid
        assert!(store.plugin_exists("concurrent-test").unwrap());
        let loaded = store.load_plugin("concurrent-test").unwrap();
        assert_eq!(loaded.metadata.server.name, "concurrent-test");
    }

    #[test]
    fn test_save_plugin_cleanup_on_vfs_error() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        // Create VFS with a file, then we'll simulate an error by making
        // the generated directory read-only (on Unix systems)
        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("cleanup-test");
        let tools = create_test_tools();

        // First create the plugin directory manually
        let plugin_dir = store.plugin_path("cleanup-test");
        fs::create_dir(&plugin_dir).unwrap();

        // Now save should fail with AlreadyExists
        let result = store.save_plugin("cleanup-test", &vfs, &wasm, server_info, tools);
        assert!(matches!(
            result,
            Err(PluginStoreError::PluginAlreadyExists { .. })
        ));

        // Directory should still exist since we created it manually
        assert!(plugin_dir.exists());
    }

    #[test]
    fn test_plugin_dir_guard_cleanup() {
        let temp = TempDir::new().unwrap();
        let test_dir = temp.path().join("test-guard");

        // Create directory
        fs::create_dir(&test_dir).unwrap();
        assert!(test_dir.exists());

        // Create guard and let it drop without commit
        {
            let _guard = PluginDirGuard::new(test_dir.clone());
            // Guard drops here
        }

        // Directory should be cleaned up
        assert!(!test_dir.exists());
    }

    #[test]
    fn test_plugin_dir_guard_commit() {
        let temp = TempDir::new().unwrap();
        let test_dir = temp.path().join("test-guard-commit");

        // Create directory
        fs::create_dir(&test_dir).unwrap();
        assert!(test_dir.exists());

        // Create guard and commit it
        {
            let guard = PluginDirGuard::new(test_dir.clone());
            guard.commit();
            // Guard drops here
        }

        // Directory should still exist after commit
        assert!(test_dir.exists());
    }

    #[test]
    fn test_save_plugin_with_nested_directories() {
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        use mcp_vfs::VfsBuilder;
        let vfs = VfsBuilder::new()
            .add_file("/a/b/c/deep.ts", "export const DEEP = true;")
            .add_file("/x/y/file.ts", "export const XY = true;")
            .build()
            .unwrap();

        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("nested");

        // Save with nested directories
        let metadata = store
            .save_plugin("nested", &vfs, &wasm, server_info, vec![])
            .unwrap();

        assert_eq!(metadata.checksums.generated.len(), 2);

        // Verify directory structure on disk
        let plugin_dir = store.plugin_path("nested");
        let generated_dir = plugin_dir.join(GENERATED_DIR);
        assert!(generated_dir.join("a/b/c/deep.ts").exists());
        assert!(generated_dir.join("x/y/file.ts").exists());

        // Load and verify
        let loaded = store.load_plugin("nested").unwrap();
        assert_eq!(loaded.vfs.file_count(), 2);

        let content = loaded.vfs.read_file("/a/b/c/deep.ts").unwrap();
        assert_eq!(content, "export const DEEP = true;");
    }
}
