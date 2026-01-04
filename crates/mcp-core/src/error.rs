//! Error types for MCP Code Execution.
//!
//! This module provides a comprehensive error hierarchy with contextual information
//! following Microsoft Rust Guidelines for error handling.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_core::{Error, Result};
//!
//! fn connect_to_server(name: &str) -> Result<()> {
//!     if name.is_empty() {
//!         return Err(Error::ConfigError {
//!             message: "Server name cannot be empty".to_string(),
//!         });
//!     }
//!     Ok(())
//! }
//!
//! let err = connect_to_server("").unwrap_err();
//! assert!(err.is_config_error());
//! ```

use thiserror::Error;

/// Main error type for MCP Code Execution.
///
/// All errors in the system use this type, providing consistent error handling
/// across all crates in the workspace.
#[derive(Error, Debug)]
pub enum Error {
    /// MCP server connection failed.
    ///
    /// This error occurs when attempting to connect to an MCP server and
    /// the connection fails due to network issues, authentication failures,
    /// or server unavailability.
    #[error("MCP server connection failed: {server}")]
    ConnectionFailed {
        /// Name or identifier of the server that failed to connect
        server: String,
        /// Underlying error cause
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Security policy violation.
    ///
    /// Raised when an operation violates configured security policies,
    /// such as attempting to access forbidden resources or exceeding
    /// resource limits.
    #[error("Security policy violation: {reason}")]
    SecurityViolation {
        /// Description of the security violation
        reason: String,
    },

    /// Resource not found error.
    ///
    /// Occurs when attempting to access a resource (file, tool, server)
    /// that does not exist.
    #[error("Resource not found: {resource}")]
    ResourceNotFound {
        /// Identifier of the missing resource
        resource: String,
    },

    /// Configuration error.
    ///
    /// Raised when configuration is invalid, missing required fields,
    /// or contains contradictory settings.
    #[error("Configuration error: {message}")]
    ConfigError {
        /// Description of the configuration problem
        message: String,
    },

    /// Timeout error.
    ///
    /// Occurs when an operation exceeds its configured timeout limit.
    #[error("Operation timed out after {duration_secs}s: {operation}")]
    Timeout {
        /// Name of the operation that timed out
        operation: String,
        /// Duration in seconds before timeout occurred
        duration_secs: u64,
    },

    /// Serialization/deserialization error.
    ///
    /// Raised when JSON or other data format conversion fails.
    #[error("Serialization error: {message}")]
    SerializationError {
        /// Description of the serialization failure
        message: String,
        /// Underlying serde error
        #[source]
        source: Option<serde_json::Error>,
    },

    /// Invalid argument error.
    ///
    /// Raised when CLI arguments or function parameters are invalid.
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Validation error for domain types.
    ///
    /// Raised when creating or validating domain types like `SkillName`,
    /// `SkillDescription`, etc. that have specific format requirements.
    #[error("Validation error in {field}: {reason}")]
    ValidationError {
        /// The field that failed validation
        field: String,
        /// Detailed reason for the validation failure
        reason: String,
    },

    /// Script generation failed.
    ///
    /// Raised when generating TypeScript scripts from tool schemas fails.
    #[error("Script generation failed for tool '{tool}': {message}")]
    ScriptGenerationError {
        /// The tool name that failed to generate
        tool: String,
        /// Description of the generation failure
        message: String,
        /// Optional underlying error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl Error {
    /// Returns `true` if this is a connection error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::Error;
    ///
    /// let err = Error::ConnectionFailed {
    ///     server: "test".to_string(),
    ///     source: "connection refused".into(),
    /// };
    /// assert!(err.is_connection_error());
    /// ```
    #[must_use]
    pub const fn is_connection_error(&self) -> bool {
        matches!(self, Self::ConnectionFailed { .. })
    }

    /// Returns `true` if this is a security violation error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::Error;
    ///
    /// let err = Error::SecurityViolation {
    ///     reason: "Unauthorized access".to_string(),
    /// };
    /// assert!(err.is_security_error());
    /// ```
    #[must_use]
    pub const fn is_security_error(&self) -> bool {
        matches!(self, Self::SecurityViolation { .. })
    }

    /// Returns `true` if this is a resource not found error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::Error;
    ///
    /// let err = Error::ResourceNotFound {
    ///     resource: "tool:example".to_string(),
    /// };
    /// assert!(err.is_not_found());
    /// ```
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(self, Self::ResourceNotFound { .. })
    }

    /// Returns `true` if this is a configuration error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::Error;
    ///
    /// let err = Error::ConfigError {
    ///     message: "Invalid port".to_string(),
    /// };
    /// assert!(err.is_config_error());
    /// ```
    #[must_use]
    pub const fn is_config_error(&self) -> bool {
        matches!(self, Self::ConfigError { .. })
    }

    /// Returns `true` if this is a timeout error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::Error;
    ///
    /// let err = Error::Timeout {
    ///     operation: "execute_code".to_string(),
    ///     duration_secs: 30,
    /// };
    /// assert!(err.is_timeout());
    /// ```
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Returns `true` if this is a validation error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::Error;
    ///
    /// let err = Error::ValidationError {
    ///     field: "skill_name".to_string(),
    ///     reason: "Invalid characters".to_string(),
    /// };
    /// assert!(err.is_validation_error());
    /// ```
    #[must_use]
    pub const fn is_validation_error(&self) -> bool {
        matches!(self, Self::ValidationError { .. })
    }

    /// Returns `true` if this is a script generation error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::Error;
    ///
    /// let err = Error::ScriptGenerationError {
    ///     tool: "send_message".to_string(),
    ///     message: "Template rendering failed".to_string(),
    ///     source: None,
    /// };
    /// assert!(err.is_script_generation_error());
    /// ```
    #[must_use]
    pub const fn is_script_generation_error(&self) -> bool {
        matches!(self, Self::ScriptGenerationError { .. })
    }
}

/// Result type alias for MCP operations.
///
/// This is a convenience alias for `Result<T, Error>` used throughout
/// the codebase.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{Result, Error};
///
/// fn validate_input(value: i32) -> Result<i32> {
///     if value < 0 {
///         return Err(Error::ConfigError {
///             message: "Value must be non-negative".to_string(),
///         });
///     }
///     Ok(value)
/// }
///
/// assert!(validate_input(5).is_ok());
/// assert!(validate_input(-1).is_err());
/// ```
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_error_detection() {
        let err = Error::ConnectionFailed {
            server: "test-server".to_string(),
            source: "network error".into(),
        };
        assert!(err.is_connection_error());
        assert!(!err.is_security_error());
    }

    #[test]
    fn test_security_error_detection() {
        let err = Error::SecurityViolation {
            reason: "Access denied".to_string(),
        };
        assert!(err.is_security_error());
        assert!(!err.is_connection_error());
    }

    #[test]
    fn test_not_found_error_detection() {
        let err = Error::ResourceNotFound {
            resource: "missing-tool".to_string(),
        };
        assert!(err.is_not_found());
        assert!(!err.is_timeout());
    }

    #[test]
    fn test_config_error_detection() {
        let err = Error::ConfigError {
            message: "Invalid configuration".to_string(),
        };
        assert!(err.is_config_error());
        assert!(!err.is_timeout());
    }

    #[test]
    fn test_timeout_error_detection() {
        let err = Error::Timeout {
            operation: "long_operation".to_string(),
            duration_secs: 60,
        };
        assert!(err.is_timeout());
        assert!(!err.is_not_found());
    }

    #[test]
    fn test_error_display() {
        let err = Error::SecurityViolation {
            reason: "Unauthorized".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("Security policy violation"));
        assert!(display.contains("Unauthorized"));
    }

    #[test]
    fn test_result_alias() {
        #[allow(clippy::unnecessary_wraps)]
        fn returns_ok() -> Result<i32> {
            Ok(42)
        }

        fn returns_err() -> Result<i32> {
            Err(Error::ConfigError {
                message: "test error".to_string(),
            })
        }

        assert_eq!(returns_ok().unwrap(), 42);
        assert!(returns_err().is_err());
    }
}
