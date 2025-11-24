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

/// Predefined security configuration profiles.
///
/// Provides three standard security levels for different use cases:
/// - **Strict**: Maximum security for untrusted code
/// - **Moderate**: Balanced security for typical use (default)
/// - **Permissive**: Relaxed security for trusted environments
///
/// # Examples
///
/// ```
/// use mcp_wasm_runtime::security::{SecurityConfig, SecurityProfile};
///
/// // Use strict profile for untrusted code
/// let strict = SecurityConfig::from_profile(SecurityProfile::Strict);
/// assert_eq!(strict.memory_limit_bytes(), 128 * 1024 * 1024);
///
/// // Use moderate profile (recommended default)
/// let moderate = SecurityConfig::from_profile(SecurityProfile::Moderate);
/// assert_eq!(moderate.memory_limit_bytes(), 256 * 1024 * 1024);
///
/// // Use permissive profile for trusted code
/// let permissive = SecurityConfig::from_profile(SecurityProfile::Permissive);
/// assert_eq!(permissive.memory_limit_bytes(), 512 * 1024 * 1024);
/// assert!(permissive.allow_network());
/// ```
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecurityProfile {
    /// Maximum security with minimal permissions.
    ///
    /// - Memory: 128MB
    /// - Timeout: 30 seconds
    /// - Network: Disabled
    /// - Host calls: 100 max
    ///
    /// Use for untrusted or unknown code.
    Strict,

    /// Balanced security for typical use cases (recommended).
    ///
    /// - Memory: 256MB
    /// - Timeout: 60 seconds
    /// - Network: Disabled
    /// - Host calls: 1000 max
    ///
    /// Use for general-purpose skills and tools.
    Moderate,

    /// Relaxed security for trusted environments.
    ///
    /// - Memory: 512MB
    /// - Timeout: 120 seconds
    /// - Network: Enabled
    /// - Host calls: 5000 max
    ///
    /// Use only for code from trusted sources.
    Permissive,
}

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

    /// Creates a security configuration from a predefined profile.
    ///
    /// This is a convenience method for creating configurations with
    /// common security settings. For custom settings, use [`builder()`](Self::builder).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::security::{SecurityConfig, SecurityProfile};
    ///
    /// // Create strict configuration
    /// let config = SecurityConfig::from_profile(SecurityProfile::Strict);
    /// assert_eq!(config.memory_limit_bytes(), 128 * 1024 * 1024);
    /// assert_eq!(config.max_host_calls(), Some(100));
    ///
    /// // Create moderate configuration (same as default)
    /// let config = SecurityConfig::from_profile(SecurityProfile::Moderate);
    /// assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024);
    ///
    /// // Create permissive configuration
    /// let config = SecurityConfig::from_profile(SecurityProfile::Permissive);
    /// assert!(config.allow_network());
    /// ```
    #[inline]
    #[must_use]
    pub fn from_profile(profile: SecurityProfile) -> Self {
        match profile {
            SecurityProfile::Strict => Self::strict(),
            SecurityProfile::Moderate => Self::moderate(),
            SecurityProfile::Permissive => Self::permissive(),
        }
    }

    /// Creates a strict security profile with maximum restrictions.
    ///
    /// Suitable for untrusted or unknown code with minimal trust.
    ///
    /// # Configuration
    ///
    /// - Memory limit: 128MB
    /// - Execution timeout: 30 seconds
    /// - Network access: Disabled
    /// - Max host calls: 100
    /// - Fuel limit: None (timeout-based protection)
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use std::time::Duration;
    ///
    /// let config = SecurityConfig::strict();
    ///
    /// assert_eq!(config.memory_limit_bytes(), 128 * 1024 * 1024);
    /// assert_eq!(config.execution_timeout(), Duration::from_secs(30));
    /// assert!(!config.allow_network());
    /// assert_eq!(config.max_host_calls(), Some(100));
    /// ```
    #[must_use]
    pub fn strict() -> Self {
        Self::builder()
            .memory_limit_mb(128)
            .execution_timeout(Duration::from_secs(30))
            .allow_network(false)
            .max_host_calls(100)
            .unlimited_fuel() // Use timeout-based protection
            .build()
    }

    /// Creates a moderate security profile with balanced restrictions.
    ///
    /// Recommended for general-purpose skills and typical use cases.
    /// This is equivalent to the default configuration.
    ///
    /// # Configuration
    ///
    /// - Memory limit: 256MB
    /// - Execution timeout: 60 seconds
    /// - Network access: Disabled
    /// - Max host calls: 1000
    /// - Fuel limit: None (timeout-based protection)
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use std::time::Duration;
    ///
    /// let config = SecurityConfig::moderate();
    ///
    /// assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024);
    /// assert_eq!(config.execution_timeout(), Duration::from_secs(60));
    /// assert!(!config.allow_network());
    /// assert_eq!(config.max_host_calls(), Some(1000));
    ///
    /// // Moderate profile matches default
    /// let default = SecurityConfig::default();
    /// assert_eq!(config.memory_limit_bytes(), default.memory_limit_bytes());
    /// assert_eq!(config.execution_timeout(), default.execution_timeout());
    /// ```
    #[must_use]
    pub fn moderate() -> Self {
        Self::builder()
            .memory_limit_mb(256)
            .execution_timeout(Duration::from_secs(60))
            .allow_network(false)
            .max_host_calls(1000)
            .unlimited_fuel() // Use timeout-based protection
            .build()
    }

    /// Creates a permissive security profile with relaxed restrictions.
    ///
    /// Suitable for trusted code from known sources. Enables network
    /// access and provides more resources.
    ///
    /// # Security Warning
    ///
    /// Only use this profile for code from trusted sources. The relaxed
    /// restrictions may allow potentially harmful operations.
    ///
    /// # Configuration
    ///
    /// - Memory limit: 512MB
    /// - Execution timeout: 120 seconds
    /// - Network access: Enabled
    /// - Max host calls: 5000
    /// - Fuel limit: None (timeout-based protection)
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use std::time::Duration;
    ///
    /// let config = SecurityConfig::permissive();
    ///
    /// assert_eq!(config.memory_limit_bytes(), 512 * 1024 * 1024);
    /// assert_eq!(config.execution_timeout(), Duration::from_secs(120));
    /// assert!(config.allow_network());
    /// assert_eq!(config.max_host_calls(), Some(5000));
    /// ```
    #[must_use]
    pub fn permissive() -> Self {
        Self::builder()
            .memory_limit_mb(512)
            .execution_timeout(Duration::from_secs(120))
            .allow_network(true)
            .max_host_calls(5000)
            .unlimited_fuel() // Use timeout-based protection
            .build()
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

    // Security Profile Tests

    #[test]
    fn test_security_profile_strict() {
        let config = SecurityConfig::strict();

        assert_eq!(config.memory_limit_bytes(), 128 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(30));
        assert!(!config.allow_network());
        assert_eq!(config.max_host_calls(), Some(100));
        assert_eq!(config.max_fuel(), None);
    }

    #[test]
    fn test_security_profile_moderate() {
        let config = SecurityConfig::moderate();

        assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(60));
        assert!(!config.allow_network());
        assert_eq!(config.max_host_calls(), Some(1000));
        assert_eq!(config.max_fuel(), None);
    }

    #[test]
    fn test_security_profile_permissive() {
        let config = SecurityConfig::permissive();

        assert_eq!(config.memory_limit_bytes(), 512 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(120));
        assert!(config.allow_network());
        assert_eq!(config.max_host_calls(), Some(5000));
        assert_eq!(config.max_fuel(), None);
    }

    #[test]
    fn test_from_profile_strict() {
        let config = SecurityConfig::from_profile(SecurityProfile::Strict);

        assert_eq!(config.memory_limit_bytes(), 128 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(30));
        assert!(!config.allow_network());
        assert_eq!(config.max_host_calls(), Some(100));
    }

    #[test]
    fn test_from_profile_moderate() {
        let config = SecurityConfig::from_profile(SecurityProfile::Moderate);

        assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(60));
        assert!(!config.allow_network());
        assert_eq!(config.max_host_calls(), Some(1000));
    }

    #[test]
    fn test_from_profile_permissive() {
        let config = SecurityConfig::from_profile(SecurityProfile::Permissive);

        assert_eq!(config.memory_limit_bytes(), 512 * 1024 * 1024);
        assert_eq!(config.execution_timeout(), Duration::from_secs(120));
        assert!(config.allow_network());
        assert_eq!(config.max_host_calls(), Some(5000));
    }

    #[test]
    fn test_moderate_matches_default() {
        let moderate = SecurityConfig::moderate();
        let default = SecurityConfig::default();

        assert_eq!(moderate.memory_limit_bytes(), default.memory_limit_bytes());
        assert_eq!(moderate.execution_timeout(), default.execution_timeout());
        assert_eq!(moderate.allow_network(), default.allow_network());
        assert_eq!(moderate.max_host_calls(), default.max_host_calls());
    }

    #[test]
    fn test_profile_ordering_security() {
        let strict = SecurityConfig::strict();
        let moderate = SecurityConfig::moderate();
        let permissive = SecurityConfig::permissive();

        // Verify security ordering: strict < moderate < permissive
        assert!(strict.memory_limit_bytes() < moderate.memory_limit_bytes());
        assert!(moderate.memory_limit_bytes() < permissive.memory_limit_bytes());

        assert!(strict.execution_timeout() < moderate.execution_timeout());
        assert!(moderate.execution_timeout() < permissive.execution_timeout());

        assert!(strict.max_host_calls() < moderate.max_host_calls());
        assert!(moderate.max_host_calls() < permissive.max_host_calls());
    }

    #[test]
    fn test_profile_network_permissions() {
        let strict = SecurityConfig::strict();
        let moderate = SecurityConfig::moderate();
        let permissive = SecurityConfig::permissive();

        // Only permissive allows network
        assert!(!strict.allow_network());
        assert!(!moderate.allow_network());
        assert!(permissive.allow_network());
    }

    #[test]
    fn test_security_profile_equality() {
        assert_eq!(SecurityProfile::Strict, SecurityProfile::Strict);
        assert_eq!(SecurityProfile::Moderate, SecurityProfile::Moderate);
        assert_eq!(SecurityProfile::Permissive, SecurityProfile::Permissive);

        assert_ne!(SecurityProfile::Strict, SecurityProfile::Moderate);
        assert_ne!(SecurityProfile::Moderate, SecurityProfile::Permissive);
        assert_ne!(SecurityProfile::Strict, SecurityProfile::Permissive);
    }

    #[test]
    fn test_security_profile_debug() {
        let profile = SecurityProfile::Strict;
        let debug_str = format!("{:?}", profile);
        assert!(debug_str.contains("Strict"));
    }

    #[test]
    fn test_security_profile_clone() {
        let profile = SecurityProfile::Moderate;
        let cloned = profile;
        assert_eq!(profile, cloned);
    }

    #[test]
    fn test_security_profile_copy() {
        let profile = SecurityProfile::Permissive;
        let copied = profile;
        // Both should be usable after copy
        assert_eq!(profile, SecurityProfile::Permissive);
        assert_eq!(copied, SecurityProfile::Permissive);
    }

    #[test]
    fn test_profile_custom_override() {
        // Start with strict, but override some settings
        let config = SecurityConfig::builder()
            .memory_limit_mb(128)
            .execution_timeout(Duration::from_secs(30))
            .allow_network(false)
            .max_host_calls(100)
            .max_fuel(1_000_000) // Add custom fuel limit
            .build();

        assert_eq!(config.memory_limit_bytes(), 128 * 1024 * 1024);
        assert_eq!(config.max_fuel(), Some(1_000_000));
    }

    #[test]
    fn test_profile_preopened_dirs_empty() {
        // All profiles start with no preopened directories
        let strict = SecurityConfig::strict();
        let moderate = SecurityConfig::moderate();
        let permissive = SecurityConfig::permissive();

        assert!(strict.preopened_dirs().is_empty());
        assert!(moderate.preopened_dirs().is_empty());
        assert!(permissive.preopened_dirs().is_empty());
    }

    #[test]
    fn test_strict_profile_minimal_permissions() {
        let config = SecurityConfig::strict();

        // Verify strict has the most restrictive settings
        assert_eq!(config.memory_limit_bytes(), 128 * 1024 * 1024); // Minimum
        assert_eq!(config.execution_timeout(), Duration::from_secs(30)); // Shortest
        assert!(!config.allow_network()); // No network
        assert_eq!(config.max_host_calls(), Some(100)); // Fewest calls
    }

    #[test]
    fn test_permissive_profile_maximum_permissions() {
        let config = SecurityConfig::permissive();

        // Verify permissive has the most relaxed settings
        assert_eq!(config.memory_limit_bytes(), 512 * 1024 * 1024); // Maximum
        assert_eq!(config.execution_timeout(), Duration::from_secs(120)); // Longest
        assert!(config.allow_network()); // Network allowed
        assert_eq!(config.max_host_calls(), Some(5000)); // Most calls
    }

    #[test]
    fn test_profile_use_case_untrusted_code() {
        // For untrusted code, use strict profile
        let config = SecurityConfig::from_profile(SecurityProfile::Strict);

        // Verify it's suitable for untrusted code
        assert!(!config.allow_network()); // No external access
        assert!(config.max_host_calls().is_some()); // Limited host calls
        assert!(config.memory_limit_bytes() <= 128 * 1024 * 1024); // Limited memory
    }

    #[test]
    fn test_profile_use_case_trusted_code() {
        // For trusted code, permissive profile is available
        let config = SecurityConfig::from_profile(SecurityProfile::Permissive);

        // Verify it has more permissions
        assert!(config.allow_network()); // Can access network
        assert!(config.memory_limit_bytes() >= 512 * 1024 * 1024); // More memory
    }

    #[test]
    fn test_profile_use_case_general_purpose() {
        // For general use, moderate is recommended
        let config = SecurityConfig::from_profile(SecurityProfile::Moderate);

        // Verify balanced settings
        assert!(!config.allow_network()); // Secure by default
        assert_eq!(config.memory_limit_bytes(), 256 * 1024 * 1024); // Balanced
        assert_eq!(config.max_host_calls(), Some(1000)); // Reasonable limit
    }

    #[test]
    fn test_all_profiles_have_fuel_disabled_by_default() {
        // All profiles use timeout-based protection instead of fuel
        let strict = SecurityConfig::strict();
        let moderate = SecurityConfig::moderate();
        let permissive = SecurityConfig::permissive();

        assert_eq!(strict.max_fuel(), None);
        assert_eq!(moderate.max_fuel(), None);
        assert_eq!(permissive.max_fuel(), None);
    }

    #[test]
    fn test_profile_documentation_accuracy() {
        // Verify documented values match actual implementation

        // Strict documentation claims
        let strict = SecurityConfig::strict();
        assert_eq!(strict.memory_limit_bytes(), 128 * 1024 * 1024);
        assert_eq!(strict.execution_timeout(), Duration::from_secs(30));
        assert_eq!(strict.max_host_calls(), Some(100));

        // Moderate documentation claims
        let moderate = SecurityConfig::moderate();
        assert_eq!(moderate.memory_limit_bytes(), 256 * 1024 * 1024);
        assert_eq!(moderate.execution_timeout(), Duration::from_secs(60));
        assert_eq!(moderate.max_host_calls(), Some(1000));

        // Permissive documentation claims
        let permissive = SecurityConfig::permissive();
        assert_eq!(permissive.memory_limit_bytes(), 512 * 1024 * 1024);
        assert_eq!(permissive.execution_timeout(), Duration::from_secs(120));
        assert_eq!(permissive.max_host_calls(), Some(5000));
    }
}
