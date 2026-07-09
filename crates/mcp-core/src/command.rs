//! Command validation and sanitization for secure subprocess execution.
//!
//! This module provides security-focused validation of server configurations before
//! they are executed as subprocesses, preventing command injection attacks.
//!
//! # Security
//!
//! The validation enforces:
//! - Command validation (absolute path or binary name)
//! - Argument sanitization (no shell metacharacters)
//! - Environment variable validation (block dangerous names)
//! - Executable permission checks (for absolute paths)
//!
//! # Examples
//!
//! ```
//! use mcp_execution_core::{ServerConfig, validate_server_config};
//!
//! // Valid binary name (resolved via PATH)
//! let config = ServerConfig::builder()
//!     .command("docker".to_string())
//!     .arg("run".to_string())
//!     .build();
//! assert!(validate_server_config(&config).is_ok());
//!
//! // Invalid: shell metacharacters in arg
//! let config = ServerConfig::builder()
//!     .command("docker".to_string())
//!     .arg("run; rm -rf /".to_string())
//!     .build();
//! assert!(validate_server_config(&config).is_err());
//! ```

use crate::{Error, Result, ServerConfig};
use std::path::Path;
use std::time::Duration;

/// Shell metacharacters that indicate potential command injection.
const FORBIDDEN_CHARS: &[char] = &[';', '|', '&', '>', '<', '`', '$', '(', ')', '\n', '\r'];

/// Forbidden environment variable names that pose security risks.
const FORBIDDEN_ENV_NAMES: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "PATH", // Block PATH override to prevent binary substitution
];

/// Upper bound for `connect_timeout`/`discover_timeout`, matching the
/// 30-second defaults declared in `server_config.rs` with headroom for
/// slow-starting servers configured via `mcp.json`.
const MAX_TIMEOUT: Duration = Duration::from_mins(10);

/// Validates a `ServerConfig` for safe subprocess execution.
///
/// This function performs comprehensive security validation to prevent
/// command injection attacks. It validates:
///
/// 1. **Command**: Can be absolute path (with existence/permission checks) or binary name
/// 2. **Arguments**: Each arg checked for shell metacharacters
/// 3. **Environment**: Variables checked for dangerous names
/// 4. **Timeouts**: `connect_timeout`/`discover_timeout` checked against bounds
///
/// # Security Rules
///
/// - **Forbidden chars in command/args**: `;`, `|`, `&`, `>`, `<`, `` ` ``, `$`, `(`, `)`, `\n`, `\r`
/// - **Forbidden env names**: `LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_*`, `PATH`
/// - **Absolute paths**: Must exist and be executable
/// - **Binary names**: Allowed (resolved via PATH at runtime)
/// - **Timeout bounds**: `connect_timeout`/`discover_timeout` must be greater than zero and at
///   most `MAX_TIMEOUT` (600s)
///
/// # Errors
///
/// Returns `Error::SecurityViolation` if:
/// - Command is empty or whitespace
/// - Command/args contain shell metacharacters
/// - Absolute path does not exist or is not executable
/// - Environment variable name is forbidden
///
/// Returns `Error::ValidationError` if:
/// - `connect_timeout` or `discover_timeout` is zero
/// - `connect_timeout` or `discover_timeout` exceeds `MAX_TIMEOUT` (600s)
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ServerConfig, validate_server_config};
///
/// // Valid: binary name
/// let config = ServerConfig::builder()
///     .command("docker".to_string())
///     .build();
/// assert!(validate_server_config(&config).is_ok());
///
/// // Invalid: forbidden env var
/// let config = ServerConfig::builder()
///     .command("docker".to_string())
///     .env("LD_PRELOAD".to_string(), "/evil.so".to_string())
///     .build();
/// assert!(validate_server_config(&config).is_err());
/// ```
///
/// # Security Considerations
///
/// - Binary names are allowed and resolved via PATH at runtime
/// - Absolute paths undergo strict validation (existence, permissions)
/// - All arguments are validated separately to prevent injection
/// - Environment variables are checked against forbidden names
/// - There is no infinite-timeout option: `0` is always rejected, since an
///   unbounded wait would let a hung server block this non-interactive tool
///   forever (see the `validate_timeout` design note in this module)
pub fn validate_server_config(config: &ServerConfig) -> Result<()> {
    // Validate command
    validate_command_string(&config.command, "command")?;

    // If command is absolute path, perform additional checks
    let command_path = Path::new(&config.command);
    if command_path.is_absolute() {
        validate_absolute_path(&config.command)?;
    }
    // If not absolute, it's a binary name (to be resolved via PATH) - this is OK

    // Validate each argument separately
    for (idx, arg) in config.args.iter().enumerate() {
        validate_command_string(arg, &format!("argument {idx}"))?;
    }

    // Validate environment variable names
    for env_name in config.env.keys() {
        validate_env_name(env_name)?;
    }

    // Validate timeout bounds. Zero fires immediately and breaks all
    // discovery; an infinite timeout is deliberately unsupported (see
    // `validate_timeout` doc comment) because it would let a hung or
    // malicious server block this non-interactive CLI tool forever,
    // re-opening the DoS window these timeouts were introduced to close.
    validate_timeout(config.connect_timeout(), "connect_timeout")?;
    validate_timeout(config.discover_timeout(), "discover_timeout")?;

    Ok(())
}

/// Validates that a timeout is within `(0, MAX_TIMEOUT]`.
///
/// # Design Note: No Infinite Timeout
///
/// A timeout of zero is permanently rejected rather than treated as a
/// sentinel for "no timeout". This tool spawns subprocesses and connects to
/// servers non-interactively (CLI and MCP-server modes); an unbounded
/// connect/discover wait would let a hung or malicious server block the
/// caller indefinitely, which is exactly the denial-of-service exposure
/// these timeouts were added to close. Callers that need a longer wait
/// should raise the value up to `MAX_TIMEOUT` (10 minutes) instead.
fn validate_timeout(timeout: Duration, field: &str) -> Result<()> {
    if timeout.is_zero() {
        return Err(Error::ValidationError {
            field: field.to_string(),
            reason: "timeout must be greater than zero".to_string(),
        });
    }
    if timeout > MAX_TIMEOUT {
        return Err(Error::ValidationError {
            field: field.to_string(),
            reason: format!("timeout {timeout:?} exceeds maximum allowed {MAX_TIMEOUT:?}"),
        });
    }
    Ok(())
}

/// Validates a command string for forbidden shell metacharacters.
///
/// This is an internal helper that checks a string (command or argument)
/// for dangerous shell metacharacters.
fn validate_command_string(value: &str, context: &str) -> Result<()> {
    // Check for empty
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::SecurityViolation {
            reason: format!("{context} cannot be empty"),
        });
    }

    // Check for shell metacharacters
    for forbidden in FORBIDDEN_CHARS {
        if value.contains(*forbidden) {
            return Err(Error::SecurityViolation {
                reason: format!(
                    "{context} contains forbidden shell metacharacter '{forbidden}': {value}"
                ),
            });
        }
    }

    Ok(())
}

/// Validates an absolute path command for existence and executability.
///
/// This is an internal helper that performs file system checks on
/// absolute path commands.
fn validate_absolute_path(command: &str) -> Result<()> {
    let path = Path::new(command);

    // Verify file exists
    if !path.exists() {
        return Err(Error::SecurityViolation {
            reason: format!("Command file does not exist: {command}"),
        });
    }

    // Verify it's a file (not a directory)
    if !path.is_file() {
        return Err(Error::SecurityViolation {
            reason: format!("Command path is not a file: {command}"),
        });
    }

    // Verify executable permissions (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path).map_err(|e| Error::SecurityViolation {
            reason: format!("Cannot read command metadata: {e}"),
        })?;
        let permissions = metadata.permissions();
        let mode = permissions.mode();

        // Check if any execute bit is set (owner, group, or other)
        if mode & 0o111 == 0 {
            return Err(Error::SecurityViolation {
                reason: format!("Command file is not executable: {command}"),
            });
        }
    }

    Ok(())
}

/// Validates an environment variable name.
///
/// This is an internal helper that checks if an environment variable
/// name is in the forbidden list.
fn validate_env_name(name: &str) -> Result<()> {
    // Check for forbidden env names (exact match)
    if FORBIDDEN_ENV_NAMES.contains(&name) {
        return Err(Error::SecurityViolation {
            reason: format!("Forbidden environment variable name: {name}"),
        });
    }

    // Check for DYLD_* prefix (macOS dynamic linker variables)
    if name.starts_with("DYLD_") {
        return Err(Error::SecurityViolation {
            reason: format!("Forbidden environment variable prefix DYLD_: {name}"),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn test_validate_server_config_binary_name() {
        // Binary names (not absolute paths) should be valid
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build();
        assert!(validate_server_config(&config).is_ok());

        let config = ServerConfig::builder()
            .command("python".to_string())
            .build();
        assert!(validate_server_config(&config).is_ok());

        let config = ServerConfig::builder().command("node".to_string()).build();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_server_config_binary_with_args() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .arg("--rm".to_string())
            .arg("mcp-server".to_string())
            .build();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_server_config_empty_command() {
        // Empty command should fail during build
        let result = ServerConfig::builder().command(String::new()).try_build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));

        // Whitespace-only command should fail during build
        let result = ServerConfig::builder()
            .command("   ".to_string())
            .try_build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_validate_server_config_command_with_metacharacters() {
        let dangerous_commands = vec![
            "docker; rm -rf /",
            "docker | cat",
            "docker && echo pwned",
            "docker > /tmp/out",
            "docker < /tmp/in",
            "docker `whoami`",
            "docker $(whoami)",
            "docker & background",
            "docker\nrm -rf /",
        ];

        for cmd in dangerous_commands {
            let config = ServerConfig::builder().command(cmd.to_string()).build();
            let result = validate_server_config(&config);
            assert!(
                result.is_err(),
                "Should reject command with metacharacters: {cmd}"
            );
            if let Err(Error::SecurityViolation { reason }) = result {
                assert!(
                    reason.contains("forbidden") || reason.contains("metacharacter"),
                    "Error should mention forbidden character: {reason}"
                );
            }
        }
    }

    #[test]
    fn test_validate_server_config_args_with_metacharacters() {
        let dangerous_args = vec![
            "run; rm -rf /",
            "run | cat",
            "run && echo pwned",
            "run > /tmp/out",
            "run < /tmp/in",
            "run `whoami`",
            "run $(whoami)",
            "run & background",
            "run\nrm -rf /",
        ];

        for arg in dangerous_args {
            let config = ServerConfig::builder()
                .command("docker".to_string())
                .arg(arg.to_string())
                .build();
            let result = validate_server_config(&config);
            assert!(
                result.is_err(),
                "Should reject arg with metacharacters: {arg}"
            );
            if let Err(Error::SecurityViolation { reason }) = result {
                assert!(
                    reason.contains("argument")
                        && (reason.contains("forbidden") || reason.contains("metacharacter")),
                    "Error should mention argument and forbidden character: {reason}"
                );
            }
        }
    }

    #[test]
    fn test_validate_server_config_empty_arg() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .arg(String::new())
            .build();
        assert!(validate_server_config(&config).is_err());
    }

    #[test]
    fn test_validate_server_config_forbidden_env_ld_preload() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .env("LD_PRELOAD".to_string(), "/evil.so".to_string())
            .build();
        let result = validate_server_config(&config);
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("LD_PRELOAD"));
        }
    }

    #[test]
    fn test_validate_server_config_forbidden_env_ld_library_path() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .env("LD_LIBRARY_PATH".to_string(), "/evil".to_string())
            .build();
        let result = validate_server_config(&config);
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("LD_LIBRARY_PATH"));
        }
    }

    #[test]
    fn test_validate_server_config_forbidden_env_dyld() {
        let dyld_vars = vec![
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
            "DYLD_FRAMEWORK_PATH",
            "DYLD_PRINT_TO_FILE",
            "DYLD_CUSTOM_VAR",
        ];

        for var in dyld_vars {
            let config = ServerConfig::builder()
                .command("docker".to_string())
                .env(var.to_string(), "/evil".to_string())
                .build();
            let result = validate_server_config(&config);
            assert!(result.is_err(), "Should reject DYLD_* variable: {var}");
            if let Err(Error::SecurityViolation { reason }) = result {
                assert!(
                    reason.contains("DYLD_"),
                    "Error should mention DYLD_: {reason}"
                );
            }
        }
    }

    #[test]
    fn test_validate_server_config_forbidden_env_path() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .env("PATH".to_string(), "/evil:/usr/bin".to_string())
            .build();
        let result = validate_server_config(&config);
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("PATH"));
        }
    }

    #[test]
    fn test_validate_server_config_safe_env() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .env("LOG_LEVEL".to_string(), "debug".to_string())
            .env("DEBUG".to_string(), "1".to_string())
            .env("HOME".to_string(), "/home/user".to_string())
            .env("MY_CUSTOM_VAR".to_string(), "value".to_string())
            .build();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_validate_server_config_absolute_path_valid() {
        use std::os::unix::fs::PermissionsExt;

        // Create a temporary executable file
        let temp_file = "/tmp/test-mcp-server-config";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();

        // Set execute permissions
        let mut perms = fs::metadata(temp_file).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(temp_file, perms).unwrap();

        let config = ServerConfig::builder()
            .command(temp_file.to_string())
            .arg("--port".to_string())
            .arg("8080".to_string())
            .build();

        let result = validate_server_config(&config);
        fs::remove_file(temp_file).ok();

        assert!(result.is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_validate_server_config_absolute_path_not_executable() {
        use std::os::unix::fs::PermissionsExt;

        // Create a temporary non-executable file
        let temp_file = "/tmp/test-mcp-server-config-noexec";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();

        // Remove execute permissions
        let mut perms = fs::metadata(temp_file).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(temp_file, perms).unwrap();

        let config = ServerConfig::builder()
            .command(temp_file.to_string())
            .build();

        let result = validate_server_config(&config);
        fs::remove_file(temp_file).ok();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("not executable"));
        }
    }

    #[test]
    fn test_validate_server_config_absolute_path_nonexistent() {
        #[cfg(unix)]
        let nonexistent = "/absolutely/nonexistent/path/to/server";
        #[cfg(windows)]
        let nonexistent = "C:\\absolutely\\nonexistent\\path\\to\\server.exe";

        let config = ServerConfig::builder()
            .command(nonexistent.to_string())
            .build();

        let result = validate_server_config(&config);
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("does not exist"));
        }
    }

    #[test]
    fn test_validate_server_config_with_cwd() {
        // cwd doesn't affect validation (it's not security-critical)
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .cwd(std::path::PathBuf::from("/tmp"))
            .build();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_server_config_complex_valid() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .arg("--rm".to_string())
            .arg("-e".to_string())
            .arg("DEBUG=1".to_string())
            .arg("mcp-server".to_string())
            .env("LOG_LEVEL".to_string(), "info".to_string())
            .env("CACHE_DIR".to_string(), "/var/cache".to_string())
            .cwd(std::path::PathBuf::from("/opt/app"))
            .build();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_server_config_default_timeouts_pass() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_server_config_zero_connect_timeout_rejected() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .connect_timeout(std::time::Duration::ZERO)
            .build();
        let result = validate_server_config(&config);
        assert!(result.is_err());
        if let Err(Error::ValidationError { field, reason }) = result {
            assert_eq!(field, "connect_timeout");
            assert!(reason.contains("greater than zero"));
        } else {
            panic!("expected ValidationError");
        }
    }

    #[test]
    fn test_validate_server_config_zero_discover_timeout_rejected() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .discover_timeout(std::time::Duration::ZERO)
            .build();
        let result = validate_server_config(&config);
        assert!(result.is_err());
        if let Err(Error::ValidationError { field, .. }) = result {
            assert_eq!(field, "discover_timeout");
        } else {
            panic!("expected ValidationError");
        }
    }

    #[test]
    fn test_validate_server_config_above_max_timeout_rejected() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .connect_timeout(std::time::Duration::from_secs(601))
            .build();
        let result = validate_server_config(&config);
        assert!(result.is_err());
        if let Err(Error::ValidationError { field, reason }) = result {
            assert_eq!(field, "connect_timeout");
            assert!(reason.contains("exceeds maximum"));
        } else {
            panic!("expected ValidationError");
        }
    }

    #[test]
    fn test_validate_server_config_in_bounds_timeout_accepted() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .connect_timeout(std::time::Duration::from_mins(1))
            .discover_timeout(std::time::Duration::from_mins(10))
            .build();
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_env_name_edge_cases() {
        // Test exact matches and prefix matches
        assert!(validate_env_name("LD_PRELOAD").is_err());
        assert!(validate_env_name("DYLD_TEST").is_err());
        assert!(validate_env_name("PATH").is_err());

        // These should be OK (not in forbidden list)
        assert!(validate_env_name("LD_DEBUG").is_ok()); // Not in list
        assert!(validate_env_name("MY_PATH").is_ok()); // Not exact match
        assert!(validate_env_name("DYLD").is_ok()); // No underscore, not prefix match
    }
}
