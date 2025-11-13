//! Security configuration and limits for WASM execution.
//!
//! Provides configurable security boundaries including memory limits,
//! CPU fuel limits, filesystem restrictions, and network isolation.
//!
//! # Examples
//!
//! ```
//! use mcp_wasm_runtime::security::SecurityConfig;
//!
//! let config = SecurityConfig::default();
//! assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024); // 256MB
//! ```

use mcp_core::MemoryLimit;
use std::path::PathBuf;
use std::time::Duration;

/// Security configuration for WASM execution.
///
/// Defines all security boundaries and limits enforced by the runtime.
///
/// # Examples
///
/// ```
/// use mcp_wasm_runtime::security::SecurityConfig;
/// use std::time::Duration;
///
/// let config = SecurityConfig::builder()
///     .memory_limit_mb(512)
///     .execution_timeout(Duration::from_secs(30))
///     .max_fuel(10_000_000)
///     .build();
///
/// assert_eq!(config.memory_limit_bytes(), 512 * 1024 * 1024);
/// ```
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Maximum memory available to WASM module
    memory_limit: MemoryLimit,

    /// Maximum execution time before timeout
    execution_timeout: Duration,

    /// CPU fuel limit (prevents infinite loops)
    max_fuel: Option<u64>,

    /// Allowed preopened directories for WASI
    preopened_dirs: Vec<PathBuf>,

    /// Allow network access via host functions only
    allow_network: bool,

    /// Maximum number of host function calls
    max_host_calls: Option<usize>,
}

impl SecurityConfig {
    /// Default memory limit: 256MB
    pub const DEFAULT_MEMORY_LIMIT_MB: usize = 256;

    /// Default execution timeout: 60 seconds
    pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

    /// Default fuel limit: 10 million instructions
    pub const DEFAULT_FUEL: u64 = 10_000_000;

    /// Creates a new security configuration builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::builder()
    ///     .memory_limit_mb(512)
    ///     .build();
    /// ```
    #[inline]
    #[must_use]
    pub fn builder() -> SecurityConfigBuilder {
        SecurityConfigBuilder::default()
    }

    /// Returns memory limit in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::security::SecurityConfig;
    ///
    /// let config = SecurityConfig::default();
    /// assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024);
    /// ```
    #[inline]
    #[must_use]
    pub fn memory_limit_bytes(&self) -> usize {
        self.memory_limit.bytes()
    }

    /// Returns execution timeout.
    #[inline]
    #[must_use]
    pub fn execution_timeout(&self) -> Duration {
        self.execution_timeout
    }

    /// Returns CPU fuel limit.
    #[inline]
    #[must_use]
    pub fn max_fuel(&self) -> Option<u64> {
        self.max_fuel
    }

    /// Returns preopened directories.
    #[inline]
    #[must_use]
    pub fn preopened_dirs(&self) -> &[PathBuf] {
        &self.preopened_dirs
    }

    /// Returns whether network access is allowed.
    #[inline]
    #[must_use]
    pub fn allow_network(&self) -> bool {
        self.allow_network
    }

    /// Returns maximum number of host function calls.
    #[inline]
    #[must_use]
    pub fn max_host_calls(&self) -> Option<usize> {
        self.max_host_calls
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            memory_limit: MemoryLimit::from_mb(Self::DEFAULT_MEMORY_LIMIT_MB)
                .expect("default memory limit is valid"),
            execution_timeout: Duration::from_secs(Self::DEFAULT_TIMEOUT_SECS),
            // Fuel is disabled by default due to Wasmtime 37.0 API limitations
            // Use execution_timeout for CPU exhaustion protection
            max_fuel: None,
            preopened_dirs: Vec::new(),
            allow_network: false, // Secure by default
            max_host_calls: Some(1000),
        }
    }
}

/// Builder for security configuration.
///
/// # Examples
///
/// ```
/// use mcp_wasm_runtime::security::SecurityConfig;
/// use std::time::Duration;
/// use std::path::PathBuf;
///
/// let config = SecurityConfig::builder()
///     .memory_limit_mb(512)
///     .execution_timeout(Duration::from_secs(30))
///     .max_fuel(5_000_000)
///     .preopen_dir(PathBuf::from("/tmp/wasm"))
///     .allow_network(true)
///     .max_host_calls(500)
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct SecurityConfigBuilder {
    memory_limit_mb: Option<usize>,
    execution_timeout: Option<Duration>,
    max_fuel: Option<Option<u64>>,
    preopened_dirs: Vec<PathBuf>,
    allow_network: Option<bool>,
    max_host_calls: Option<Option<usize>>,
}

impl SecurityConfigBuilder {
    /// Sets memory limit in megabytes.
    #[must_use]
    pub fn memory_limit_mb(mut self, mb: usize) -> Self {
        self.memory_limit_mb = Some(mb);
        self
    }

    /// Sets execution timeout.
    #[must_use]
    pub fn execution_timeout(mut self, timeout: Duration) -> Self {
        self.execution_timeout = Some(timeout);
        self
    }

    /// Sets CPU fuel limit.
    #[must_use]
    pub fn max_fuel(mut self, fuel: u64) -> Self {
        self.max_fuel = Some(Some(fuel));
        self
    }

    /// Disables fuel limit (allows unlimited execution).
    ///
    /// # Security Warning
    ///
    /// Disabling fuel limit removes protection against infinite loops
    /// and CPU exhaustion attacks. Only use in trusted environments.
    #[must_use]
    pub fn unlimited_fuel(mut self) -> Self {
        self.max_fuel = Some(None);
        self
    }

    /// Adds a preopened directory for WASI filesystem access.
    #[must_use]
    pub fn preopen_dir(mut self, path: PathBuf) -> Self {
        self.preopened_dirs.push(path);
        self
    }

    /// Sets whether network access is allowed via host functions.
    #[must_use]
    pub fn allow_network(mut self, allow: bool) -> Self {
        self.allow_network = Some(allow);
        self
    }

    /// Sets maximum number of host function calls.
    #[must_use]
    pub fn max_host_calls(mut self, max: usize) -> Self {
        self.max_host_calls = Some(Some(max));
        self
    }

    /// Disables host call limit.
    #[must_use]
    pub fn unlimited_host_calls(mut self) -> Self {
        self.max_host_calls = Some(None);
        self
    }

    /// Builds the security configuration.
    ///
    /// # Panics
    ///
    /// Panics if memory limit is invalid.
    #[must_use]
    pub fn build(self) -> SecurityConfig {
        let memory_limit_mb = self
            .memory_limit_mb
            .unwrap_or(SecurityConfig::DEFAULT_MEMORY_LIMIT_MB);

        SecurityConfig {
            memory_limit: MemoryLimit::from_mb(memory_limit_mb)
                .expect("memory limit must be valid"),
            execution_timeout: self
                .execution_timeout
                .unwrap_or_else(|| Duration::from_secs(SecurityConfig::DEFAULT_TIMEOUT_SECS)),
            max_fuel: self.max_fuel.unwrap_or(Some(SecurityConfig::DEFAULT_FUEL)),
            preopened_dirs: self.preopened_dirs,
            allow_network: self.allow_network.unwrap_or(false),
            max_host_calls: self.max_host_calls.unwrap_or(Some(1000)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SecurityConfig::default();
        assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(60));
        // Fuel disabled by default due to Wasmtime 37.0 API limitations
        assert_eq!(config.max_fuel(), None);
        assert!(!config.allow_network());
        assert_eq!(config.max_host_calls(), Some(1000));
    }

    #[test]
    fn test_builder() {
        let config = SecurityConfig::builder()
            .memory_limit_mb(512)
            .execution_timeout(Duration::from_secs(30))
            .max_fuel(5_000_000)
            .allow_network(true)
            .max_host_calls(500)
            .build();

        assert_eq!(config.memory_limit_bytes(), 512 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(30));
        assert_eq!(config.max_fuel(), Some(5_000_000));
        assert!(config.allow_network());
        assert_eq!(config.max_host_calls(), Some(500));
    }

    #[test]
    fn test_unlimited_fuel() {
        let config = SecurityConfig::builder().unlimited_fuel().build();

        assert_eq!(config.max_fuel(), None);
    }

    #[test]
    fn test_preopened_dirs() {
        let config = SecurityConfig::builder()
            .preopen_dir(PathBuf::from("/tmp/test1"))
            .preopen_dir(PathBuf::from("/tmp/test2"))
            .build();

        assert_eq!(config.preopened_dirs().len(), 2);
    }

    #[test]
    fn test_unlimited_host_calls() {
        let config = SecurityConfig::builder().unlimited_host_calls().build();

        assert_eq!(config.max_host_calls(), None);
    }
}
