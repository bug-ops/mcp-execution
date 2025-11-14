//! Configuration types for MCP Code Execution.
//!
//! This module provides runtime configuration for the execution environment,
//! including memory limits, timeouts, security policies, and connection pooling.
//!
//! # Examples
//!
//! ```
//! use mcp_core::{RuntimeConfig, SecurityPolicy, MemoryLimit};
//! use std::time::Duration;
//!
//! // Use default configuration
//! let config = RuntimeConfig::default();
//! assert_eq!(config.memory_limit.megabytes(), 256);
//!
//! // Create custom configuration
//! let custom = RuntimeConfig {
//!     memory_limit: MemoryLimit::from_mb(512).unwrap(),
//!     execution_timeout: Duration::from_secs(60),
//!     ..Default::default()
//! };
//! ```

use crate::{MemoryLimit, ServerId};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

/// Runtime configuration for code execution environment.
///
/// This structure controls all aspects of the WASM runtime behavior,
/// including resource limits, security policies, and operational parameters.
///
/// # Examples
///
/// ```
/// use mcp_core::{RuntimeConfig, MemoryLimit};
/// use std::time::Duration;
///
/// let config = RuntimeConfig {
///     memory_limit: MemoryLimit::from_mb(128).unwrap(),
///     execution_timeout: Duration::from_secs(30),
///     enable_cache: true,
///     ..Default::default()
/// };
///
/// assert_eq!(config.memory_limit.megabytes(), 128);
/// assert_eq!(config.execution_timeout.as_secs(), 30);
/// ```
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Maximum memory per WASM instance in bytes.
    ///
    /// This limit is enforced by the WASM runtime to prevent memory exhaustion.
    /// Default: 256MB
    pub memory_limit: MemoryLimit,

    /// Execution timeout for WASM code.
    ///
    /// If execution exceeds this duration, the operation is cancelled.
    /// Default: 30 seconds
    pub execution_timeout: Duration,

    /// Path to cache directory for compiled WASM modules.
    ///
    /// If `None`, caching is disabled. When enabled, compiled modules are
    /// cached to disk for faster subsequent executions.
    /// Default: None (disabled)
    pub cache_dir: Option<PathBuf>,

    /// Whitelist of allowed MCP servers.
    ///
    /// If `Some`, only servers in this list can be called. If `None`,
    /// all servers are allowed.
    /// Default: None (all allowed)
    pub allowed_servers: Option<HashSet<ServerId>>,

    /// Enable persistent state storage.
    ///
    /// When enabled, WASM code can store and retrieve state across
    /// multiple executions using the state storage API.
    /// Default: false
    pub enable_state: bool,

    /// Enable result caching.
    ///
    /// When enabled, tool call results are cached to reduce redundant
    /// calls to MCP servers.
    /// Default: true
    pub enable_cache: bool,

    /// Maximum size of the connection pool.
    ///
    /// Controls how many concurrent connections to MCP servers can be
    /// maintained.
    /// Default: 10
    pub connection_pool_size: usize,

    /// Maximum fuel units for WASM execution.
    ///
    /// Fuel limits prevent infinite loops by metering instruction execution.
    /// If `None`, fuel metering is disabled (not recommended for untrusted code).
    /// Default: `Some(10_000_000)`
    pub max_fuel: Option<u64>,

    /// Security policy configuration.
    ///
    /// Defines security boundaries and restrictions for code execution.
    pub security: SecurityPolicy,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            memory_limit: MemoryLimit::default(),
            execution_timeout: Duration::from_secs(30),
            cache_dir: None,
            allowed_servers: None,
            enable_state: false,
            enable_cache: true,
            connection_pool_size: 10,
            max_fuel: Some(10_000_000),
            security: SecurityPolicy::default(),
        }
    }
}

impl RuntimeConfig {
    /// Creates a new configuration builder.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{RuntimeConfig, MemoryLimit};
    /// use std::time::Duration;
    ///
    /// let config = RuntimeConfig::builder()
    ///     .memory_limit(MemoryLimit::from_mb(512).unwrap())
    ///     .execution_timeout(Duration::from_secs(60))
    ///     .enable_cache(true)
    ///     .build();
    ///
    /// assert_eq!(config.memory_limit.megabytes(), 512);
    /// ```
    #[must_use]
    pub fn builder() -> RuntimeConfigBuilder {
        RuntimeConfigBuilder::new()
    }

    /// Validates the configuration.
    ///
    /// Ensures all configuration values are within acceptable ranges and
    /// that no conflicting options are set.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Connection pool size is zero
    /// - Execution timeout is zero
    /// - Cache directory path is invalid
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::RuntimeConfig;
    ///
    /// let config = RuntimeConfig::default();
    /// assert!(config.validate().is_ok());
    ///
    /// let mut invalid = RuntimeConfig::default();
    /// invalid.connection_pool_size = 0;
    /// assert!(invalid.validate().is_err());
    /// ```
    pub fn validate(&self) -> Result<(), String> {
        if self.connection_pool_size == 0 {
            return Err("Connection pool size must be greater than zero".to_string());
        }

        if self.execution_timeout.is_zero() {
            return Err("Execution timeout must be greater than zero".to_string());
        }

        if let Some(cache_dir) = &self.cache_dir
            && cache_dir.as_os_str().is_empty()
        {
            return Err("Cache directory path cannot be empty".to_string());
        }

        Ok(())
    }
}

/// Security policy configuration.
///
/// Defines security boundaries and restrictions for WASM code execution,
/// including resource access controls and operation restrictions.
///
/// # Examples
///
/// ```
/// use mcp_core::SecurityPolicy;
///
/// // Strict security (default)
/// let strict = SecurityPolicy::default();
/// assert!(!strict.allow_network);
/// assert!(!strict.allow_filesystem);
///
/// // Development mode (permissive)
/// let dev = SecurityPolicy::development();
/// assert!(dev.allow_network);
/// assert!(dev.allow_filesystem);
/// ```
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct SecurityPolicy {
    /// Allow network access from WASM code.
    ///
    /// When disabled, WASM code can only access MCP servers through
    /// the bridge API. Direct network access is blocked.
    /// Default: false
    pub allow_network: bool,

    /// Allow filesystem access from WASM code.
    ///
    /// When disabled, WASM code can only access files through the
    /// virtual filesystem API. Direct filesystem access is blocked.
    /// Default: false
    pub allow_filesystem: bool,

    /// Allow environment variable access.
    ///
    /// When disabled, WASM code cannot read environment variables.
    /// Default: false
    pub allow_env: bool,

    /// Allow spawning child processes.
    ///
    /// When disabled, WASM code cannot spawn subprocesses.
    /// Default: false (always recommended)
    pub allow_spawn: bool,

    /// Maximum rate limit for tool calls (calls per second).
    ///
    /// If `None`, no rate limiting is applied.
    /// Default: Some(10)
    pub max_calls_per_second: Option<u32>,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            allow_network: false,
            allow_filesystem: false,
            allow_env: false,
            allow_spawn: false,
            max_calls_per_second: Some(10),
        }
    }
}

impl SecurityPolicy {
    /// Creates a strict security policy.
    ///
    /// All capabilities are disabled. This is the most secure option
    /// and is equivalent to `SecurityPolicy::default()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SecurityPolicy;
    ///
    /// let policy = SecurityPolicy::strict();
    /// assert!(!policy.allow_network);
    /// assert!(!policy.allow_filesystem);
    /// assert!(!policy.allow_env);
    /// assert!(!policy.allow_spawn);
    /// ```
    #[must_use]
    pub fn strict() -> Self {
        Self::default()
    }

    /// Creates a permissive policy for development.
    ///
    /// Enables network and filesystem access. Use only in trusted
    /// development environments.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SecurityPolicy;
    ///
    /// let policy = SecurityPolicy::development();
    /// assert!(policy.allow_network);
    /// assert!(policy.allow_filesystem);
    /// assert!(!policy.allow_spawn); // Still disabled for safety
    /// ```
    #[must_use]
    pub const fn development() -> Self {
        Self {
            allow_network: true,
            allow_filesystem: true,
            allow_env: true,
            allow_spawn: false,         // Never enable by default
            max_calls_per_second: None, // No rate limiting in dev
        }
    }
}

/// Builder for `RuntimeConfig`.
///
/// Provides a fluent interface for constructing runtime configurations
/// with custom values.
///
/// # Examples
///
/// ```
/// use mcp_core::{RuntimeConfig, MemoryLimit, SecurityPolicy};
/// use std::time::Duration;
///
/// let config = RuntimeConfig::builder()
///     .memory_limit(MemoryLimit::from_mb(256).unwrap())
///     .execution_timeout(Duration::from_secs(45))
///     .enable_cache(true)
///     .enable_state(false)
///     .security(SecurityPolicy::strict())
///     .build();
///
/// assert_eq!(config.memory_limit.megabytes(), 256);
/// assert_eq!(config.execution_timeout.as_secs(), 45);
/// ```
#[derive(Debug)]
pub struct RuntimeConfigBuilder {
    config: RuntimeConfig,
}

impl RuntimeConfigBuilder {
    /// Creates a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: RuntimeConfig::default(),
        }
    }

    /// Sets the memory limit.
    #[must_use]
    pub const fn memory_limit(mut self, limit: MemoryLimit) -> Self {
        self.config.memory_limit = limit;
        self
    }

    /// Sets the execution timeout.
    #[must_use]
    pub const fn execution_timeout(mut self, timeout: Duration) -> Self {
        self.config.execution_timeout = timeout;
        self
    }

    /// Sets the cache directory.
    #[must_use]
    pub fn cache_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.cache_dir = Some(path.into());
        self
    }

    /// Sets the allowed servers whitelist.
    #[must_use]
    pub fn allowed_servers(mut self, servers: HashSet<ServerId>) -> Self {
        self.config.allowed_servers = Some(servers);
        self
    }

    /// Enables or disables state storage.
    #[must_use]
    pub const fn enable_state(mut self, enable: bool) -> Self {
        self.config.enable_state = enable;
        self
    }

    /// Enables or disables result caching.
    #[must_use]
    pub const fn enable_cache(mut self, enable: bool) -> Self {
        self.config.enable_cache = enable;
        self
    }

    /// Sets the connection pool size.
    #[must_use]
    pub const fn connection_pool_size(mut self, size: usize) -> Self {
        self.config.connection_pool_size = size;
        self
    }

    /// Sets the maximum fuel units.
    #[must_use]
    pub const fn max_fuel(mut self, fuel: Option<u64>) -> Self {
        self.config.max_fuel = fuel;
        self
    }

    /// Sets the security policy.
    #[must_use]
    pub const fn security(mut self, policy: SecurityPolicy) -> Self {
        self.config.security = policy;
        self
    }

    /// Builds the configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::RuntimeConfig;
    ///
    /// let config = RuntimeConfig::builder()
    ///     .enable_cache(true)
    ///     .build();
    ///
    /// assert!(config.enable_cache);
    /// ```
    #[must_use]
    pub fn build(self) -> RuntimeConfig {
        self.config
    }
}

impl Default for RuntimeConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RuntimeConfig::default();

        assert_eq!(config.memory_limit, MemoryLimit::default());
        assert_eq!(config.execution_timeout.as_secs(), 30);
        assert!(config.cache_dir.is_none());
        assert!(config.allowed_servers.is_none());
        assert!(!config.enable_state);
        assert!(config.enable_cache);
        assert_eq!(config.connection_pool_size, 10);
        assert_eq!(config.max_fuel, Some(10_000_000));
    }

    #[test]
    fn test_config_validation() {
        let config = RuntimeConfig::default();
        assert!(config.validate().is_ok());

        let invalid = RuntimeConfig {
            connection_pool_size: 0,
            ..Default::default()
        };
        assert!(invalid.validate().is_err());

        let invalid2 = RuntimeConfig {
            execution_timeout: Duration::from_secs(0),
            ..Default::default()
        };
        assert!(invalid2.validate().is_err());
    }

    #[test]
    fn test_config_builder() {
        let config = RuntimeConfig::builder()
            .memory_limit(MemoryLimit::from_mb(512).unwrap())
            .execution_timeout(Duration::from_secs(60))
            .enable_cache(false)
            .enable_state(true)
            .connection_pool_size(20)
            .build();

        assert_eq!(config.memory_limit.megabytes(), 512);
        assert_eq!(config.execution_timeout.as_secs(), 60);
        assert!(!config.enable_cache);
        assert!(config.enable_state);
        assert_eq!(config.connection_pool_size, 20);
    }

    #[test]
    fn test_security_policy_default() {
        let policy = SecurityPolicy::default();

        assert!(!policy.allow_network);
        assert!(!policy.allow_filesystem);
        assert!(!policy.allow_env);
        assert!(!policy.allow_spawn);
        assert_eq!(policy.max_calls_per_second, Some(10));
    }

    #[test]
    fn test_security_policy_strict() {
        let policy = SecurityPolicy::strict();

        assert!(!policy.allow_network);
        assert!(!policy.allow_filesystem);
        assert!(!policy.allow_env);
        assert!(!policy.allow_spawn);
    }

    #[test]
    fn test_security_policy_development() {
        let policy = SecurityPolicy::development();

        assert!(policy.allow_network);
        assert!(policy.allow_filesystem);
        assert!(policy.allow_env);
        assert!(!policy.allow_spawn); // Should always be false
        assert!(policy.max_calls_per_second.is_none());
    }

    #[test]
    fn test_builder_with_cache_dir() {
        let config = RuntimeConfig::builder().cache_dir("/tmp/mcp-cache").build();

        assert_eq!(
            config.cache_dir.as_ref().map(|p| p.to_str().unwrap()),
            Some("/tmp/mcp-cache")
        );
    }

    #[test]
    fn test_builder_with_allowed_servers() {
        let mut servers = HashSet::new();
        servers.insert(ServerId::new("server1"));
        servers.insert(ServerId::new("server2"));

        let config = RuntimeConfig::builder()
            .allowed_servers(servers.clone())
            .build();

        assert_eq!(config.allowed_servers, Some(servers));
    }

    #[test]
    fn test_builder_fluent_interface() {
        let config = RuntimeConfig::builder()
            .memory_limit(MemoryLimit::from_mb(128).unwrap())
            .execution_timeout(Duration::from_secs(15))
            .enable_cache(true)
            .enable_state(false)
            .connection_pool_size(5)
            .max_fuel(Some(5_000_000))
            .security(SecurityPolicy::strict())
            .build();

        assert_eq!(config.memory_limit.megabytes(), 128);
        assert_eq!(config.execution_timeout.as_secs(), 15);
        assert!(config.enable_cache);
        assert!(!config.enable_state);
        assert_eq!(config.connection_pool_size, 5);
        assert_eq!(config.max_fuel, Some(5_000_000));
        assert!(!config.security.allow_network);
    }

    #[test]
    fn test_validation_with_empty_cache_dir() {
        let config = RuntimeConfig {
            cache_dir: Some(PathBuf::from("")),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
