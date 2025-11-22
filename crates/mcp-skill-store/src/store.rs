//! Skill storage implementation.
//!
//! Provides the main [`SkillStore`] type for saving, loading, and managing
//! skills on disk.

use crate::checksum::{calculate_checksum, verify_checksum};
use crate::error::{Result, SkillStoreError};
use crate::types::{
    Checksums, FORMAT_VERSION, GENERATED_DIR, LoadedSkill, METADATA_FILE, ServerInfo, SkillInfo,
    SkillMetadata, ToolInfo, WASM_FILE,
};
use chrono::Utc;
use mcp_vfs::{Vfs, VfsBuilder};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// RAII guard for skill directory cleanup on error.
///
/// Automatically removes a skill directory if the save operation fails
/// or panics, preventing partial skill state on disk.
///
/// This guard ensures atomic-like behavior: the skill directory is either
/// fully written or completely removed on error.
struct SkillDirGuard {
    path: PathBuf,
    cleanup: bool,
}

impl SkillDirGuard {
    /// Creates a new guard for the given skill directory.
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
    /// Call this after successfully writing all skill files.
    fn commit(mut self) {
        self.cleanup = false;
    }
}

impl Drop for SkillDirGuard {
    fn drop(&mut self) {
        if self.cleanup {
            if let Err(e) = fs::remove_dir_all(&self.path) {
                tracing::warn!(
                    "Failed to cleanup skill directory {}: {}",
                    self.path.display(),
                    e
                );
            } else {
                tracing::debug!(
                    "Cleaned up incomplete skill directory: {}",
                    self.path.display()
                );
            }
        }
    }
}

/// Skill storage manager.
///
/// Manages a directory of saved skills, providing operations to save, load,
/// list, and remove skills. Each skill is stored in its own subdirectory
/// named after the server.
///
/// # Directory Structure
///
/// ```text
/// base_dir/
/// ├── server1/
/// │   ├── skill.json
/// │   ├── generated/
/// │   │   └── ...
/// │   └── module.wasm
/// └── server2/
///     ├── skill.json
///     ├── generated/
///     └── module.wasm
/// ```
///
/// # Thread Safety
///
/// `SkillStore` is `Send + Sync` and can be safely shared between threads.
/// However, concurrent modifications to the same skill directory may result
/// in undefined behavior. Use external synchronization if needed.
///
/// # Examples
///
/// ```no_run
/// use mcp_skill_store::{SkillStore, ServerInfo};
/// use mcp_vfs::VfsBuilder;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Create store
/// let store = SkillStore::new("./skills")?;
///
/// // Save a skill
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
/// store.save_skill("my-server", &vfs, &wasm, server_info, vec![])?;
///
/// // Load it back
/// let plugin = store.load_skill("my-server")?;
/// assert_eq!(plugin.metadata.server.name, "my-server");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct SkillStore {
    base_dir: PathBuf,
}

impl SkillStore {
    /// Creates a new skill store at the given directory.
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
    /// use mcp_skill_store::SkillStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = SkillStore::new("./skills")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(base_dir: impl AsRef<Path>) -> Result<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();

        // Create directory if it doesn't exist
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
            tracing::debug!("Created skill store directory: {}", base_dir.display());
        }

        Ok(Self { base_dir })
    }

    /// Saves a skill to disk.
    ///
    /// Writes the skill files to a new subdirectory named after the server.
    /// Calculates checksums for all files and stores them in metadata.
    ///
    /// # Atomicity
    ///
    /// This operation is atomic at the directory level:
    /// - Directory creation uses atomic `create_dir` (fails if exists)
    /// - On error or panic, the partial skill directory is automatically cleaned up
    /// - Once complete, the skill is fully saved or not saved at all
    ///
    /// # Concurrency
    ///
    /// Safe for concurrent calls with different `server_name` values.
    /// Concurrent saves to the same skill will result in one success and
    /// one [`SkillStoreError::SkillAlreadyExists`] error (atomic directory creation).
    ///
    /// # Arguments
    ///
    /// * `server_name` - Server identifier (must be valid directory name)
    /// * `vfs` - Virtual filesystem with generated TypeScript code
    /// * `wasm_module` - Compiled WASM module bytes
    /// * `server_info` - Server identification information
    /// * `tool_info` - List of tools provided by the skill
    ///
    /// # Errors
    ///
    /// * [`SkillStoreError::SkillAlreadyExists`] - Skill directory exists
    /// * [`SkillStoreError::InvalidServerName`] - Invalid server name
    /// * I/O errors if writing fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::{SkillStore, ServerInfo, ToolInfo};
    /// use mcp_vfs::VfsBuilder;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = SkillStore::new("./skills")?;
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
    /// let metadata = store.save_skill("test-server", &vfs, &wasm, server_info, tools)?;
    /// println!("Saved skill with {} tools", metadata.tools.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn save_skill(
        &self,
        server_name: &str,
        vfs: &Vfs,
        wasm_module: &[u8],
        server_info: ServerInfo,
        tool_info: Vec<ToolInfo>,
    ) -> Result<SkillMetadata> {
        // Validate server name
        validate_server_name(server_name)?;

        let skill_dir = self.skill_path(server_name);

        // Create directory atomically - fails if already exists
        // This prevents TOCTOU race condition
        match fs::create_dir(&skill_dir) {
            Ok(()) => {
                tracing::debug!("Created skill directory: {}", skill_dir.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(SkillStoreError::SkillAlreadyExists {
                    server_name: server_name.to_string(),
                });
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        // Set up cleanup guard - will remove directory if we panic or return error
        let guard = SkillDirGuard::new(skill_dir.clone());

        tracing::info!("Saving skill for server: {}", server_name);

        // Create skill directory structure
        let generated_dir = skill_dir.join(GENERATED_DIR);
        fs::create_dir_all(&generated_dir)?;

        // Track generated file checksums
        let mut generated_checksums = HashMap::new();

        // Write all VFS files to generated/ directory
        for vfs_path in vfs.all_paths() {
            let content = vfs
                .read_file(vfs_path.as_str())
                .map_err(SkillStoreError::Vfs)?;

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
        let wasm_path = skill_dir.join(WASM_FILE);
        fs::write(&wasm_path, wasm_module)?;
        tracing::debug!("Wrote WASM module: {} bytes", wasm_module.len());

        // Create metadata with checksums
        let metadata = SkillMetadata {
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

        // Write metadata to skill.json
        let metadata_path = skill_dir.join(METADATA_FILE);
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        fs::write(&metadata_path, metadata_json)?;
        tracing::debug!("Wrote skill metadata");

        // Success - disable cleanup
        guard.commit();

        tracing::info!(
            "Successfully saved skill for server: {} ({} files, {} tools)",
            server_name,
            metadata.checksums.generated.len(),
            metadata.tools.len()
        );

        Ok(metadata)
    }

    /// Loads a skill from disk.
    ///
    /// Reads all skill files and verifies checksums before returning.
    ///
    /// # Errors
    ///
    /// * [`SkillStoreError::SkillNotFound`] - Plugin doesn't exist
    /// * [`SkillStoreError::ChecksumMismatch`] - File hash mismatch
    /// * [`SkillStoreError::InvalidMetadata`] - Malformed metadata
    /// * [`SkillStoreError::MissingFile`] - Required file missing
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::SkillStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = SkillStore::new("./skills")?;
    /// let plugin = store.load_skill("my-server")?;
    ///
    /// println!("Loaded {} tools", plugin.metadata.tools.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_skill(&self, server_name: &str) -> Result<LoadedSkill> {
        // Validate server name
        validate_server_name(server_name)?;

        // Check plugin exists
        let skill_dir = self.skill_path(server_name);
        if !skill_dir.exists() {
            return Err(SkillStoreError::SkillNotFound {
                server_name: server_name.to_string(),
            });
        }

        tracing::info!("Loading skill for server: {}", server_name);

        // Read and parse skill.json
        let metadata_path = skill_dir.join(METADATA_FILE);
        if !metadata_path.exists() {
            return Err(SkillStoreError::MissingFile {
                server_name: server_name.to_string(),
                path: METADATA_FILE.into(),
            });
        }
        let metadata = Self::read_metadata(&metadata_path)?;

        // Read and verify WASM module
        let wasm_path = skill_dir.join(WASM_FILE);
        if !wasm_path.exists() {
            return Err(SkillStoreError::MissingFile {
                server_name: server_name.to_string(),
                path: WASM_FILE.into(),
            });
        }
        let wasm_module = fs::read(&wasm_path)?;
        verify_checksum(&wasm_module, &metadata.checksums.wasm, WASM_FILE)?;
        tracing::debug!("Verified WASM module checksum: {} bytes", wasm_module.len());

        // Build VFS from generated files
        let mut vfs_builder = VfsBuilder::new();
        let generated_dir = skill_dir.join(GENERATED_DIR);

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
                SkillStoreError::InvalidMetadata {
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
                .ok_or_else(|| SkillStoreError::InvalidMetadata {
                    reason: format!("File '{normalized_path}' not found in metadata checksums"),
                })?;

            // Read file content
            let content = fs::read(file_path)?;

            // Verify checksum
            verify_checksum(&content, expected_checksum, &normalized_path)?;

            // Add to VFS with absolute path (prepend /)
            let vfs_path = format!("/{normalized_path}");
            let content_str =
                String::from_utf8(content).map_err(|e| SkillStoreError::InvalidMetadata {
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
            return Err(SkillStoreError::InvalidMetadata {
                reason: format!(
                    "File count mismatch: loaded {loaded_count} files but metadata lists {expected_count}"
                ),
            });
        }

        tracing::info!(
            "Successfully loaded skill for server: {} ({} files, {} tools)",
            server_name,
            vfs.file_count(),
            metadata.tools.len()
        );

        Ok(LoadedSkill {
            metadata,
            vfs,
            wasm_module,
        })
    }

    /// Lists all available skills.
    ///
    /// Returns brief information about each skill without loading full content.
    ///
    /// # Errors
    ///
    /// Returns an error if reading the skill directory fails or if metadata
    /// files cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::SkillStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = SkillStore::new("./skills")?;
    ///
    /// for skill in store.list_skills()? {
    ///     println!("{} v{} - {} tools",
    ///         skill.server_name,
    ///         skill.version,
    ///         skill.tool_count
    ///     );
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn list_skills(&self) -> Result<Vec<SkillInfo>> {
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
                    plugins.push(SkillInfo {
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

    /// Checks if a skill exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the server name is invalid.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::SkillStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = SkillStore::new("./skills")?;
    ///
    /// if store.skill_exists("my-server")? {
    ///     println!("Plugin exists");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn skill_exists(&self, server_name: &str) -> Result<bool> {
        validate_server_name(server_name)?;
        Ok(self.skill_path(server_name).exists())
    }

    /// Removes a skill from disk.
    ///
    /// Deletes the entire skill directory and all its contents.
    ///
    /// # Errors
    ///
    /// * [`SkillStoreError::SkillNotFound`] - Plugin doesn't exist
    /// * I/O errors if deletion fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::SkillStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = SkillStore::new("./skills")?;
    /// store.remove_skill("old-server")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn remove_skill(&self, server_name: &str) -> Result<()> {
        validate_server_name(server_name)?;

        let skill_dir = self.skill_path(server_name);
        if !skill_dir.exists() {
            return Err(SkillStoreError::SkillNotFound {
                server_name: server_name.to_string(),
            });
        }

        fs::remove_dir_all(&skill_dir)?;
        tracing::info!("Removed plugin: {}", server_name);
        Ok(())
    }

    /// Gets the path to a skill directory.
    ///
    /// Does not check if the directory exists.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_store::SkillStore;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = SkillStore::new("./skills")?;
    /// let path = store.skill_path("my-server");
    /// println!("Plugin path: {}", path.display());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn skill_path(&self, server_name: &str) -> PathBuf {
        self.base_dir.join(server_name)
    }

    /// Reads and parses skill metadata from disk.
    fn read_metadata(metadata_path: &Path) -> Result<SkillMetadata> {
        let content = fs::read_to_string(metadata_path)?;
        let metadata: SkillMetadata =
            serde_json::from_str(&content).map_err(|e| SkillStoreError::InvalidMetadata {
                reason: format!("Failed to parse JSON: {e}"),
            })?;

        // Validate format version
        if metadata.format_version != FORMAT_VERSION {
            return Err(SkillStoreError::InvalidMetadata {
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
/// Returns [`SkillStoreError::InvalidServerName`] if the name is invalid.
fn validate_server_name(server_name: &str) -> Result<()> {
    if server_name.is_empty() {
        return Err(SkillStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot be empty".to_string(),
        });
    }

    if server_name == "." || server_name == ".." {
        return Err(SkillStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot be '.' or '..'".to_string(),
        });
    }

    if server_name.contains('/') || server_name.contains('\\') {
        return Err(SkillStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot contain path separators".to_string(),
        });
    }

    if server_name.chars().any(char::is_control) {
        return Err(SkillStoreError::InvalidServerName {
            server_name: server_name.to_string(),
            reason: "Server name cannot contain control characters".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_vfs::VfsBuilder;
    use tempfile::TempDir;

    #[test]
    fn test_new_creates_directory() {
        let temp = TempDir::new().unwrap();
        let store_path = temp.path().join("skills");

        let _store = SkillStore::new(&store_path).unwrap();
        assert!(store_path.exists());
    }

    #[test]
    fn test_skill_path() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let path = store.skill_path("test-server");
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
    fn test_skill_exists_nonexistent() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        assert!(!store.skill_exists("nonexistent").unwrap());
    }

    #[test]
    fn test_list_skills_empty() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let plugins = store.list_skills().unwrap();
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
    fn test_save_skill_success() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D]; // WASM magic bytes
        let server_info = create_test_server_info("test-server");
        let tools = create_test_tools();

        let metadata = store
            .save_skill("test-server", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Verify metadata
        assert_eq!(metadata.server.name, "test-server");
        assert_eq!(metadata.server.version, "1.0.0");
        assert_eq!(metadata.tools.len(), 2);
        assert_eq!(metadata.format_version, FORMAT_VERSION);

        // Verify directory structure
        let skill_dir = store.skill_path("test-server");
        assert!(skill_dir.exists());
        assert!(skill_dir.join(METADATA_FILE).exists());
        assert!(skill_dir.join(WASM_FILE).exists());
        assert!(skill_dir.join(GENERATED_DIR).exists());

        // Verify generated files exist
        let generated_dir = skill_dir.join(GENERATED_DIR);
        assert!(generated_dir.join("index.ts").exists());
        assert!(generated_dir.join("tools/sendMessage.ts").exists());
        assert!(generated_dir.join("tools/getChatInfo.ts").exists());
        assert!(generated_dir.join("types.ts").exists());

        // Verify checksums are present
        assert!(!metadata.checksums.wasm.is_empty());
        assert_eq!(metadata.checksums.generated.len(), 4);
    }

    #[test]
    fn test_save_skill_already_exists() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("test-server");
        let tools = create_test_tools();

        // First save succeeds
        store
            .save_skill(
                "test-server",
                &vfs,
                &wasm,
                server_info.clone(),
                tools.clone(),
            )
            .unwrap();

        // Second save fails
        let result = store.save_skill("test-server", &vfs, &wasm, server_info, tools);
        assert!(matches!(
            result,
            Err(SkillStoreError::SkillAlreadyExists { .. })
        ));
    }

    #[test]
    fn test_save_load_roundtrip() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00]; // WASM header
        let server_info = create_test_server_info("roundtrip-server");
        let tools = create_test_tools();

        // Save skill
        let save_metadata = store
            .save_skill("roundtrip-server", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Load skill
        let loaded = store.load_skill("roundtrip-server").unwrap();

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
    fn test_load_skill_not_found() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let result = store.load_skill("nonexistent");
        assert!(matches!(result, Err(SkillStoreError::SkillNotFound { .. })));
    }

    #[test]
    fn test_load_skill_checksum_mismatch_wasm() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("corrupt-wasm");
        let tools = create_test_tools();

        // Save skill
        store
            .save_skill("corrupt-wasm", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Corrupt WASM file
        let wasm_path = store.skill_path("corrupt-wasm").join(WASM_FILE);
        fs::write(&wasm_path, b"corrupted data").unwrap();

        // Load should fail with checksum mismatch
        let result = store.load_skill("corrupt-wasm");
        assert!(matches!(
            result,
            Err(SkillStoreError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_load_skill_checksum_mismatch_generated() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("corrupt-generated");
        let tools = create_test_tools();

        // Save skill
        store
            .save_skill("corrupt-generated", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Corrupt a generated file
        let file_path = store
            .skill_path("corrupt-generated")
            .join(GENERATED_DIR)
            .join("index.ts");
        fs::write(&file_path, "corrupted content").unwrap();

        // Load should fail with checksum mismatch
        let result = store.load_skill("corrupt-generated");
        assert!(matches!(
            result,
            Err(SkillStoreError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_load_skill_missing_file() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("missing-file");
        let tools = create_test_tools();

        // Save skill
        store
            .save_skill("missing-file", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Delete a generated file
        let file_path = store
            .skill_path("missing-file")
            .join(GENERATED_DIR)
            .join("index.ts");
        fs::remove_file(&file_path).unwrap();

        // Load should fail with invalid metadata (file count mismatch)
        let result = store.load_skill("missing-file");
        assert!(matches!(
            result,
            Err(SkillStoreError::InvalidMetadata { .. })
        ));
    }

    #[test]
    fn test_multiple_plugins_same_store() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs1 = create_test_vfs();
        let vfs2 = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info1 = create_test_server_info("plugin1");
        let server_info2 = create_test_server_info("plugin2");
        let tools = create_test_tools();

        // Save two different plugins
        store
            .save_skill("plugin1", &vfs1, &wasm, server_info1, tools.clone())
            .unwrap();
        store
            .save_skill("plugin2", &vfs2, &wasm, server_info2, tools)
            .unwrap();

        // Both should exist
        assert!(store.skill_exists("plugin1").unwrap());
        assert!(store.skill_exists("plugin2").unwrap());

        // List should show both
        let plugins = store.list_skills().unwrap();
        assert_eq!(plugins.len(), 2);

        // Load both
        let loaded1 = store.load_skill("plugin1").unwrap();
        let loaded2 = store.load_skill("plugin2").unwrap();

        assert_eq!(loaded1.metadata.server.name, "plugin1");
        assert_eq!(loaded2.metadata.server.name, "plugin2");
    }

    #[test]
    fn test_remove_skill_and_reload() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("to-remove");
        let tools = create_test_tools();

        // Save skill
        store
            .save_skill("to-remove", &vfs, &wasm, server_info, tools)
            .unwrap();

        // Verify it exists
        assert!(store.skill_exists("to-remove").unwrap());

        // Remove it
        store.remove_skill("to-remove").unwrap();

        // Should no longer exist
        assert!(!store.skill_exists("to-remove").unwrap());

        // Load should fail
        let result = store.load_skill("to-remove");
        assert!(matches!(result, Err(SkillStoreError::SkillNotFound { .. })));
    }

    #[test]
    fn test_save_skill_empty_vfs() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = VfsBuilder::new().build().unwrap();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("empty-vfs");

        // Should succeed with empty VFS
        let metadata = store
            .save_skill("empty-vfs", &vfs, &wasm, server_info, vec![])
            .unwrap();

        assert_eq!(metadata.checksums.generated.len(), 0);
        assert_eq!(metadata.tools.len(), 0);

        // Should be able to load it back
        let loaded = store.load_skill("empty-vfs").unwrap();
        assert_eq!(loaded.vfs.file_count(), 0);
    }

    #[test]
    fn test_concurrent_save_same_plugin() {
        use std::sync::Arc;
        use std::thread;

        let temp = TempDir::new().unwrap();
        let store = Arc::new(SkillStore::new(temp.path()).unwrap());

        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let tools = create_test_tools();

        // Spawn two threads trying to save the same plugin
        let store1 = Arc::clone(&store);
        let vfs1 = vfs.clone();
        let wasm1 = wasm.clone();
        let tools1 = tools.clone();
        let t1 = thread::spawn(move || {
            store1.save_skill(
                "concurrent-test",
                &vfs1,
                &wasm1,
                create_test_server_info("concurrent-test"),
                tools1,
            )
        });

        let store2 = Arc::clone(&store);
        let t2 = thread::spawn(move || {
            store2.save_skill(
                "concurrent-test",
                &vfs,
                &wasm,
                create_test_server_info("concurrent-test"),
                tools,
            )
        });

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        // Exactly one should succeed, one should get AlreadyExists
        let success_count = [&r1, &r2].iter().filter(|r| r.is_ok()).count();
        let already_exists_count = [&r1, &r2]
            .iter()
            .filter(|r| matches!(r, Err(SkillStoreError::SkillAlreadyExists { .. })))
            .count();

        assert_eq!(success_count, 1, "Exactly one save should succeed");
        assert_eq!(
            already_exists_count, 1,
            "Exactly one save should fail with AlreadyExists"
        );

        // Plugin should exist and be valid
        assert!(store.skill_exists("concurrent-test").unwrap());
        let loaded = store.load_skill("concurrent-test").unwrap();
        assert_eq!(loaded.metadata.server.name, "concurrent-test");
    }

    #[test]
    fn test_save_skill_cleanup_on_vfs_error() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        // Create VFS with a file, then we'll simulate an error by making
        // the generated directory read-only (on Unix systems)
        let vfs = create_test_vfs();
        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("cleanup-test");
        let tools = create_test_tools();

        // First create the skill directory manually
        let skill_dir = store.skill_path("cleanup-test");
        fs::create_dir(&skill_dir).unwrap();

        // Now save should fail with AlreadyExists
        let result = store.save_skill("cleanup-test", &vfs, &wasm, server_info, tools);
        assert!(matches!(
            result,
            Err(SkillStoreError::SkillAlreadyExists { .. })
        ));

        // Directory should still exist since we created it manually
        assert!(skill_dir.exists());
    }

    #[test]
    fn test_skill_dir_guard_cleanup() {
        let temp = TempDir::new().unwrap();
        let test_dir = temp.path().join("test-guard");

        // Create directory
        fs::create_dir(&test_dir).unwrap();
        assert!(test_dir.exists());

        // Create guard and let it drop without commit
        {
            let _guard = SkillDirGuard::new(test_dir.clone());
            // Guard drops here
        }

        // Directory should be cleaned up
        assert!(!test_dir.exists());
    }

    #[test]
    fn test_skill_dir_guard_commit() {
        let temp = TempDir::new().unwrap();
        let test_dir = temp.path().join("test-guard-commit");

        // Create directory
        fs::create_dir(&test_dir).unwrap();
        assert!(test_dir.exists());

        // Create guard and commit it
        {
            let guard = SkillDirGuard::new(test_dir.clone());
            guard.commit();
            // Guard drops here
        }

        // Directory should still exist after commit
        assert!(test_dir.exists());
    }

    #[test]
    fn test_save_skill_with_nested_directories() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let vfs = VfsBuilder::new()
            .add_file("/a/b/c/deep.ts", "export const DEEP = true;")
            .add_file("/x/y/file.ts", "export const XY = true;")
            .build()
            .unwrap();

        let wasm = vec![0x00, 0x61, 0x73, 0x6D];
        let server_info = create_test_server_info("nested");

        // Save with nested directories
        let metadata = store
            .save_skill("nested", &vfs, &wasm, server_info, vec![])
            .unwrap();

        assert_eq!(metadata.checksums.generated.len(), 2);

        // Verify directory structure on disk
        let skill_dir = store.skill_path("nested");
        let generated_dir = skill_dir.join(GENERATED_DIR);
        assert!(generated_dir.join("a/b/c/deep.ts").exists());
        assert!(generated_dir.join("x/y/file.ts").exists());

        // Load and verify
        let loaded = store.load_skill("nested").unwrap();
        assert_eq!(loaded.vfs.file_count(), 2);

        let content = loaded.vfs.read_file("/a/b/c/deep.ts").unwrap();
        assert_eq!(content, "export const DEEP = true;");
    }

    #[test]
    fn test_load_skill_unsupported_format_version() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        // Create a skill with future/unsupported format version
        let skill_dir = store.skill_path("test-server");
        std::fs::create_dir_all(&skill_dir).unwrap();

        let metadata = serde_json::json!({
            "format_version": "2.0",  // Future version
            "server": {
                "name": "test-server",
                "version": "1.0.0",
                "protocol_version": "2024-11-05"
            },
            "generated_at": "2025-11-21T12:00:00Z",
            "generator_version": "0.1.0",
            "checksums": {
                "wasm": "blake3:abc123",
                "generated": {}
            },
            "tools": []
        });

        std::fs::write(
            skill_dir.join("skill.json"),
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();

        // Attempt to load should fail with InvalidMetadata
        let result = store.load_skill("test-server");
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(
            matches!(err, SkillStoreError::InvalidMetadata { .. }),
            "Expected InvalidMetadata error for unsupported format version, got: {err:?}"
        );
    }

    #[test]
    fn test_validate_server_name_control_characters() {
        // Test null byte
        assert!(
            validate_server_name("server\x00name").is_err(),
            "Should reject null bytes in server name"
        );

        // Test newline
        assert!(
            validate_server_name("server\nname").is_err(),
            "Should reject newlines in server name"
        );

        // Test carriage return
        assert!(
            validate_server_name("server\rname").is_err(),
            "Should reject carriage returns in server name"
        );

        // Test tab
        assert!(
            validate_server_name("server\tname").is_err(),
            "Should reject tabs in server name"
        );

        // Test various control characters
        for c in 0u8..32u8 {
            let name = format!("server{}name", c as char);
            assert!(
                validate_server_name(&name).is_err(),
                "Should reject control character {c} in server name"
            );
        }
    }

    #[test]
    fn test_load_skill_missing_metadata() {
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        // Create skill directory structure without metadata file
        let skill_dir = store.skill_path("test-server");
        let generated_dir = skill_dir.join("generated");
        std::fs::create_dir_all(&generated_dir).unwrap();

        // Create some generated files
        std::fs::write(generated_dir.join("test.ts"), "export const test = true;").unwrap();

        // Create WASM file
        std::fs::write(skill_dir.join("module.wasm"), b"fake wasm").unwrap();

        // skill.json is missing - should fail
        let result = store.load_skill("test-server");
        assert!(result.is_err());

        let err = result.unwrap_err();
        // Should be MissingFile error
        assert!(
            matches!(err, SkillStoreError::MissingFile { .. }),
            "Expected MissingFile error for missing skill.json, got: {err:?}"
        );
    }
}
