//! Cache management for internal MCP execution data.
//!
//! This module provides the [`CacheManager`] type for managing the internal cache
//! directory structure (`~/.mcp-execution/cache/`) which stores:
//! - Compiled WASM modules
//! - Generated VFS code
//! - Build metadata (timestamps, checksums)
//!
//! The cache is system-managed and can be safely deleted and regenerated.
//!
//! # Examples
//!
//! ```
//! use mcp_core::CacheManager;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create cache manager with default directory
//! let cache = CacheManager::new()?;
//!
//! // Get paths for a specific skill
//! let wasm_path = cache.wasm_path("vkteams-bot")?;
//! let vfs_path = cache.vfs_path("vkteams-bot")?;
//!
//! // Check if skill is cached
//! if cache.has_wasm("vkteams-bot")? {
//!     println!("WASM module is cached");
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Cache manager for internal MCP execution data.
///
/// Manages the `~/.mcp-execution/cache/` directory with three subdirectories:
/// - `wasm/`: Compiled WASM modules
/// - `vfs/`: Generated TypeScript/code files
/// - `metadata/`: Build timestamps and checksums
///
/// # Thread Safety
///
/// `CacheManager` is `Send + Sync` and can be safely shared between threads.
/// However, concurrent modifications to the same cached skill may result in
/// race conditions. Use external synchronization if needed.
///
/// # Examples
///
/// ```
/// use mcp_core::CacheManager;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let cache = CacheManager::new()?;
///
/// // Check cache statistics
/// let stats = cache.stats()?;
/// println!("Total WASM files: {}", stats.total_wasm_files);
/// println!("Total size: {} bytes", stats.total_size_bytes);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CacheManager {
    cache_root: PathBuf,
}

impl CacheManager {
    /// Creates a new cache manager with default directory.
    ///
    /// Default location: `~/.mcp-execution/cache/` on Unix-like systems,
    /// `%LOCALAPPDATA%\mcp-execution\cache` on Windows.
    ///
    /// Creates the cache directory and subdirectories if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Home/cache directory cannot be determined
    /// - Cache directory cannot be created
    /// - Insufficient permissions
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// assert!(cache.wasm_dir().exists());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| Error::CacheError {
                message: "Cannot determine cache directory".to_string(),
            })?
            .join("mcp-execution");

        Self::with_directory(cache_dir)
    }

    /// Creates a cache manager with a custom directory.
    ///
    /// Useful for testing or when a non-standard cache location is needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or accessed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let temp = TempDir::new()?;
    /// let cache = CacheManager::with_directory(temp.path())?;
    /// assert!(cache.wasm_dir().exists());
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_directory(path: impl AsRef<Path>) -> Result<Self> {
        let cache_root = path.as_ref().to_path_buf();
        let manager = Self { cache_root };
        manager.ensure_cache_structure()?;
        Ok(manager)
    }

    /// Ensures the cache directory structure exists (wasm, vfs, metadata).
    ///
    /// Creates the three subdirectories if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if any directory cannot be created.
    fn ensure_cache_structure(&self) -> Result<()> {
        fs::create_dir_all(self.cache_root.join("wasm")).map_err(|e| Error::CacheError {
            message: format!("Failed to create wasm directory: {e}"),
        })?;

        fs::create_dir_all(self.cache_root.join("vfs")).map_err(|e| Error::CacheError {
            message: format!("Failed to create vfs directory: {e}"),
        })?;

        fs::create_dir_all(self.cache_root.join("metadata")).map_err(|e| Error::CacheError {
            message: format!("Failed to create metadata directory: {e}"),
        })?;

        Ok(())
    }

    /// Returns the root cache directory path.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let root = cache.cache_root();
    /// assert!(root.ends_with("mcp-execution"));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Returns path to WASM directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let wasm_dir = cache.wasm_dir();
    /// assert!(wasm_dir.ends_with("wasm"));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn wasm_dir(&self) -> PathBuf {
        self.cache_root.join("wasm")
    }

    /// Returns path to VFS directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let vfs_dir = cache.vfs_dir();
    /// assert!(vfs_dir.ends_with("vfs"));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn vfs_dir(&self) -> PathBuf {
        self.cache_root.join("vfs")
    }

    /// Returns path to metadata directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let metadata_dir = cache.metadata_dir();
    /// assert!(metadata_dir.ends_with("metadata"));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn metadata_dir(&self) -> PathBuf {
        self.cache_root.join("metadata")
    }

    /// Gets path to WASM module for a skill.
    ///
    /// # Errors
    ///
    /// Returns an error if the skill name is invalid (contains path separators,
    /// "..", null bytes, or is empty).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let wasm_path = cache.wasm_path("vkteams-bot")?;
    /// assert!(wasm_path.ends_with("vkteams-bot.wasm"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn wasm_path(&self, skill_name: &str) -> Result<PathBuf> {
        Self::validate_skill_name(skill_name)?;
        Ok(self.wasm_dir().join(format!("{skill_name}.wasm")))
    }

    /// Gets path to VFS directory for a skill.
    ///
    /// # Errors
    ///
    /// Returns an error if the skill name is invalid (contains path separators,
    /// "..", null bytes, or is empty).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let vfs_path = cache.vfs_path("vkteams-bot")?;
    /// assert!(vfs_path.ends_with("vkteams-bot"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn vfs_path(&self, skill_name: &str) -> Result<PathBuf> {
        Self::validate_skill_name(skill_name)?;
        Ok(self.vfs_dir().join(skill_name))
    }

    /// Gets path to metadata file for a skill.
    ///
    /// # Errors
    ///
    /// Returns an error if the skill name is invalid (contains path separators,
    /// "..", null bytes, or is empty).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let metadata_path = cache.metadata_path("vkteams-bot")?;
    /// assert!(metadata_path.ends_with("vkteams-bot.json"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn metadata_path(&self, skill_name: &str) -> Result<PathBuf> {
        Self::validate_skill_name(skill_name)?;
        Ok(self.metadata_dir().join(format!("{skill_name}.json")))
    }

    /// Validates a skill name to prevent path traversal and invalid filenames.
    ///
    /// # Errors
    ///
    /// Returns a `CacheError` if the skill name:
    /// - Is empty
    /// - Contains path separators (`/` or `\`)
    /// - Contains `..` (directory traversal)
    /// - Contains null bytes
    ///
    /// This validation is tested through public APIs like `wasm_path()`, `vfs_path()`, etc.
    fn validate_skill_name(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(Error::CacheError {
                message: "Skill name cannot be empty".to_string(),
            });
        }

        if name.contains('/') || name.contains('\\') {
            return Err(Error::CacheError {
                message: format!("Skill name cannot contain path separators: {name:?}"),
            });
        }

        if name.contains("..") {
            return Err(Error::CacheError {
                message: format!("Skill name cannot contain '..': {name:?}"),
            });
        }

        if name.contains('\0') {
            return Err(Error::CacheError {
                message: format!("Skill name cannot contain null bytes: {name:?}"),
            });
        }

        Ok(())
    }
    /// Checks if WASM module exists in cache for a skill.
    ///
    /// # Errors
    ///
    /// Returns an error if the skill name is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// if !cache.has_wasm("vkteams-bot")? {
    ///     println!("WASM not cached");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn has_wasm(&self, skill_name: &str) -> Result<bool> {
        Ok(self.wasm_path(skill_name)?.exists())
    }

    /// Checks if VFS cache exists for a skill.
    ///
    /// # Errors
    ///
    /// Returns an error if the skill name is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// if !cache.has_vfs("vkteams-bot")? {
    ///     println!("VFS not cached");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn has_vfs(&self, skill_name: &str) -> Result<bool> {
        Ok(self.vfs_path(skill_name)?.exists())
    }

    /// Checks if metadata exists for a skill.
    ///
    /// # Errors
    ///
    /// Returns an error if the skill name is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// if !cache.has_metadata("vkteams-bot")? {
    ///     println!("Metadata not cached");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn has_metadata(&self, skill_name: &str) -> Result<bool> {
        Ok(self.metadata_path(skill_name)?.exists())
    }

    /// Clears all cache data.
    ///
    /// Removes and recreates the entire cache directory. This is a destructive
    /// operation, but the cache can be regenerated from public skills.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be removed or recreated.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let temp_dir = TempDir::new()?;
    /// let cache = CacheManager::with_directory(temp_dir.path())?;
    /// cache.clear_all()?;
    /// assert!(cache.wasm_dir().exists());
    /// # Ok(())
    /// # }
    /// ```
    pub fn clear_all(&self) -> Result<()> {
        if self.cache_root.exists() {
            fs::remove_dir_all(&self.cache_root).map_err(|e| Error::CacheError {
                message: format!("Failed to clear cache: {e}"),
            })?;
        }

        // Recreate directory structure
        self.ensure_cache_structure()?;

        tracing::info!("Cleared all cache data from: {}", self.cache_root.display());
        Ok(())
    }

    /// Clears cache for a specific skill.
    ///
    /// Removes WASM module, VFS directory, and metadata for the given skill.
    ///
    /// # Errors
    ///
    /// Returns an error if files cannot be removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// cache.clear_skill("vkteams-bot")?;
    /// assert!(!cache.has_wasm("vkteams-bot")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn clear_skill(&self, skill_name: &str) -> Result<()> {
        // Remove WASM module
        let wasm_path = self.wasm_path(skill_name)?;
        if wasm_path.exists() {
            fs::remove_file(&wasm_path).map_err(|e| Error::CacheError {
                message: format!("Failed to remove WASM module: {e}"),
            })?;
            tracing::debug!("Removed WASM module: {}", wasm_path.display());
        }

        // Remove VFS directory
        let vfs_path = self.vfs_path(skill_name)?;
        if vfs_path.exists() {
            fs::remove_dir_all(&vfs_path).map_err(|e| Error::CacheError {
                message: format!("Failed to remove VFS directory: {e}"),
            })?;
            tracing::debug!("Removed VFS directory: {}", vfs_path.display());
        }

        // Remove metadata
        let metadata_path = self.metadata_path(skill_name)?;
        if metadata_path.exists() {
            fs::remove_file(&metadata_path).map_err(|e| Error::CacheError {
                message: format!("Failed to remove metadata: {e}"),
            })?;
            tracing::debug!("Removed metadata: {}", metadata_path.display());
        }

        tracing::info!("Cleared cache for skill: {skill_name}");
        Ok(())
    }

    /// Gets cache statistics.
    ///
    /// Returns information about cache usage including file counts and
    /// total storage size.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be read.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CacheManager;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache = CacheManager::new()?;
    /// let stats = cache.stats()?;
    /// println!("Cache size: {} bytes", stats.total_size_bytes);
    /// # Ok(())
    /// # }
    /// ```
    pub fn stats(&self) -> Result<CacheStats> {
        let mut total_wasm_files = 0;
        let mut total_vfs_files = 0;
        let mut total_metadata_files = 0;
        let mut total_size_bytes = 0u64;

        // Count WASM files
        if let Ok(entries) = fs::read_dir(self.wasm_dir()) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|ext| ext == "wasm") {
                    total_wasm_files += 1;
                    if let Ok(metadata) = entry.metadata() {
                        total_size_bytes = total_size_bytes.saturating_add(metadata.len());
                    }
                }
            }
        }

        // Count VFS directories and files
        if let Ok(entries) = fs::read_dir(self.vfs_dir()) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    total_vfs_files += 1;
                    // Count size of all files in VFS directory
                    if let Ok(size) = dir_size(&entry.path()) {
                        total_size_bytes = total_size_bytes.saturating_add(size);
                    }
                }
            }
        }

        // Count metadata files
        if let Ok(entries) = fs::read_dir(self.metadata_dir()) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|ext| ext == "json") {
                    total_metadata_files += 1;
                    if let Ok(metadata) = entry.metadata() {
                        total_size_bytes = total_size_bytes.saturating_add(metadata.len());
                    }
                }
            }
        }

        Ok(CacheStats {
            total_wasm_files,
            total_vfs_files,
            total_metadata_files,
            total_size_bytes,
        })
    }
}

/// Statistics about cache usage.
///
/// Contains information about the number of cached files and total storage size.
///
/// # Examples
///
/// ```
/// use mcp_core::CacheManager;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let cache = CacheManager::new()?;
/// let stats = cache.stats()?;
///
/// if stats.total_wasm_files > 0 {
///     let avg_size = stats.total_size_bytes / stats.total_wasm_files as u64;
///     println!("Average WASM size: {} bytes", avg_size);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    /// Number of cached WASM modules
    pub total_wasm_files: usize,
    /// Number of cached VFS directories
    pub total_vfs_files: usize,
    /// Number of cached metadata files
    pub total_metadata_files: usize,
    /// Total storage used by cache in bytes
    pub total_size_bytes: u64,
}

/// Build metadata for cached skills.
///
/// Stores information about when a skill was built, what version of the
/// generator was used, and checksums for integrity verification.
///
/// # Examples
///
/// ```
/// use mcp_core::BuildMetadata;
/// use std::collections::HashMap;
///
/// let metadata = BuildMetadata {
///     skill_name: "vkteams-bot".to_string(),
///     built_at: chrono::Utc::now(),
///     generator_version: "0.2.0".to_string(),
///     wasm_checksum: "blake3:abc123...".to_string(),
///     vfs_checksums: HashMap::new(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Name of the skill
    pub skill_name: String,
    /// Timestamp when the skill was built
    pub built_at: DateTime<Utc>,
    /// Version of the code generator used
    pub generator_version: String,
    /// Blake3 checksum of the WASM module
    pub wasm_checksum: String,
    /// Blake3 checksums of VFS files (path -> checksum)
    pub vfs_checksums: HashMap<String, String>,
}

/// Helper function to calculate directory size recursively.
fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut size = 0u64;

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;

            if metadata.is_dir() {
                size = size.saturating_add(dir_size(&entry.path())?);
            } else {
                size = size.saturating_add(metadata.len());
            }
        }
    }

    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_manager_creation() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        assert!(cache.wasm_dir().exists());
        assert!(cache.vfs_dir().exists());
        assert!(cache.metadata_dir().exists());
    }

    #[test]
    fn test_cache_paths() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        let wasm_path = cache.wasm_path("test-skill").unwrap();
        assert!(wasm_path.to_string_lossy().contains("wasm"));
        assert!(wasm_path.ends_with("test-skill.wasm"));

        let vfs_path = cache.vfs_path("test-skill").unwrap();
        assert!(vfs_path.to_string_lossy().contains("vfs"));
        assert!(vfs_path.ends_with("test-skill"));

        let metadata_path = cache.metadata_path("test-skill").unwrap();
        assert!(metadata_path.to_string_lossy().contains("metadata"));
        assert!(metadata_path.ends_with("test-skill.json"));
    }

    #[test]
    fn test_cache_existence_checks() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        // Initially, nothing should exist
        assert!(!cache.has_wasm("test-skill").unwrap());
        assert!(!cache.has_vfs("test-skill").unwrap());
        assert!(!cache.has_metadata("test-skill").unwrap());

        // Create WASM file
        let wasm_path = cache.wasm_path("test-skill").unwrap();
        fs::write(&wasm_path, b"fake wasm").unwrap();
        assert!(cache.has_wasm("test-skill").unwrap());

        // Create VFS directory
        let vfs_path = cache.vfs_path("test-skill").unwrap();
        fs::create_dir(&vfs_path).unwrap();
        assert!(cache.has_vfs("test-skill").unwrap());

        // Create metadata file
        let metadata_path = cache.metadata_path("test-skill").unwrap();
        fs::write(&metadata_path, b"{}").unwrap();
        assert!(cache.has_metadata("test-skill").unwrap());
    }

    #[test]
    fn test_clear_skill() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        // Create cache files
        fs::write(cache.wasm_path("test-skill").unwrap(), b"wasm").unwrap();
        fs::create_dir(cache.vfs_path("test-skill").unwrap()).unwrap();
        fs::write(cache.metadata_path("test-skill").unwrap(), b"{}").unwrap();

        // Verify they exist
        assert!(cache.has_wasm("test-skill").unwrap());
        assert!(cache.has_vfs("test-skill").unwrap());
        assert!(cache.has_metadata("test-skill").unwrap());

        // Clear cache for skill
        cache.clear_skill("test-skill").unwrap();

        // Verify they're gone
        assert!(!cache.has_wasm("test-skill").unwrap());
        assert!(!cache.has_vfs("test-skill").unwrap());
        assert!(!cache.has_metadata("test-skill").unwrap());
    }

    #[test]
    fn test_clear_all() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        // Create multiple cached skills
        for skill in &["skill1", "skill2", "skill3"] {
            fs::write(cache.wasm_path(skill).unwrap(), b"wasm").unwrap();
            fs::create_dir(cache.vfs_path(skill).unwrap()).unwrap();
            fs::write(cache.metadata_path(skill).unwrap(), b"{}").unwrap();
        }

        // Clear all
        cache.clear_all().unwrap();

        // Verify directories still exist but are empty
        assert!(cache.wasm_dir().exists());
        assert!(cache.vfs_dir().exists());
        assert!(cache.metadata_dir().exists());

        // Verify all cache is cleared
        for skill in &["skill1", "skill2", "skill3"] {
            assert!(!cache.has_wasm(skill).unwrap());
            assert!(!cache.has_vfs(skill).unwrap());
            assert!(!cache.has_metadata(skill).unwrap());
        }
    }

    #[test]
    fn test_cache_stats() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        // Empty cache
        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_wasm_files, 0);
        assert_eq!(stats.total_vfs_files, 0);
        assert_eq!(stats.total_metadata_files, 0);
        assert_eq!(stats.total_size_bytes, 0);

        // Add some cache files
        fs::write(cache.wasm_path("skill1").unwrap(), b"wasm1").unwrap();
        fs::write(cache.wasm_path("skill2").unwrap(), b"wasm2").unwrap();
        fs::create_dir(cache.vfs_path("skill1").unwrap()).unwrap();
        fs::write(cache.metadata_path("skill1").unwrap(), b"{}").unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_wasm_files, 2);
        assert_eq!(stats.total_vfs_files, 1);
        assert_eq!(stats.total_metadata_files, 1);
        assert!(stats.total_size_bytes > 0);
    }

    #[test]
    fn test_cache_root() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        assert_eq!(cache.cache_root(), temp.path());
    }

    #[test]
    fn test_clear_nonexistent_skill() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        // Should not error when clearing nonexistent skill
        cache.clear_skill("nonexistent").unwrap();
    }

    #[test]
    fn test_dir_size_calculation() {
        let temp = TempDir::new().unwrap();

        // Create nested directory structure
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("file1.txt"), b"hello").unwrap();
        fs::write(nested.join("file2.txt"), b"world").unwrap();

        let size = dir_size(&nested).unwrap();
        assert_eq!(size, 10); // "hello" (5) + "world" (5)
    }

    #[test]
    fn test_build_metadata_serialization() {
        let metadata = BuildMetadata {
            skill_name: "test-skill".to_string(),
            built_at: Utc::now(),
            generator_version: "0.2.0".to_string(),
            wasm_checksum: "blake3:abc123".to_string(),
            vfs_checksums: HashMap::new(),
        };

        // Should serialize and deserialize
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: BuildMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata.skill_name, deserialized.skill_name);
        assert_eq!(metadata.generator_version, deserialized.generator_version);
    }

    #[test]
    fn test_path_traversal_prevention() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        // Unix-style path traversal attempts
        let result = cache.wasm_path("../../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(".."));

        // Windows-style path traversal attempts
        let result = cache.wasm_path("..\\..\\system32\\file");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(".."));

        // Path with forward slash
        let result = cache.vfs_path("path/with/slash");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path separators"));

        // Path with backslash
        let result = cache.metadata_path("path\\with\\backslash");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path separators"));
    }

    #[test]
    fn test_empty_skill_name_validation() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        let result = cache.wasm_path("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_null_byte_validation() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        let result = cache.wasm_path("skill\0name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));
    }

    #[test]
    fn test_valid_skill_names() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();

        // These should all be valid
        assert!(cache.wasm_path("valid-skill").is_ok());
        assert!(cache.wasm_path("skill_123").is_ok());
        assert!(cache.wasm_path("vkteams-bot").is_ok());
        assert!(cache.wasm_path("my.skill.name").is_ok());
        assert!(cache.wasm_path("UPPERCASE").is_ok());
    }
}
