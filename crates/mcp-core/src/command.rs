//! Command validation and sanitization for secure subprocess execution.
//!
//! This module provides security-focused validation of command strings before
//! they are executed as subprocesses, preventing command injection attacks.
//!
//! # Security
//!
//! The validation enforces:
//! - Absolute paths only (prevents PATH manipulation)
//! - File existence verification
//! - Forbidden shell metacharacters blocking
//! - Executable permission checks
//!
//! # Examples
//!
//! ```
//! use mcp_core::validate_command;
//!
//! // Valid absolute path
//! # let temp_file = if cfg!(windows) {
//! #     std::env::temp_dir().join("test-mcp-server.exe")
//! # } else {
//! #     std::path::PathBuf::from("/tmp/test-mcp-server")
//! # };
//! # std::fs::write(&temp_file, "#!/bin/sh\n").unwrap();
//! # #[cfg(unix)]
//! # {
//! # use std::os::unix::fs::PermissionsExt;
//! # let mut perms = std::fs::metadata(&temp_file).unwrap().permissions();
//! # perms.set_mode(0o755);
//! # std::fs::set_permissions(&temp_file, perms).unwrap();
//! # }
//! let result = validate_command(temp_file.to_str().unwrap());
//! # std::fs::remove_file(&temp_file).ok();
//! # if result.is_err() {
//! #     // On some systems, execution permission check might fail
//! #     return;
//! # }
//! assert!(result.is_ok());
//!
//! // Invalid: relative path
//! assert!(validate_command("./server").is_err());
//!
//! // Invalid: shell metacharacters
//! assert!(validate_command("/usr/bin/server; rm -rf /").is_err());
//! ```

use crate::{Error, Result};
use std::path::Path;

/// Shell metacharacters that indicate potential command injection.
const FORBIDDEN_CHARS: &[char] = &[';', '|', '&', '>', '<', '`', '$', '(', ')', '\n', '\r'];

/// Validates a command string for safe subprocess execution.
///
/// This function performs comprehensive security validation to prevent
/// command injection attacks. It checks:
///
/// 1. **Absolute Path**: Command must start with `/` (Unix) or drive letter (Windows)
/// 2. **File Existence**: The file must exist at the specified path
/// 3. **Executable**: The file must have execute permissions
/// 4. **No Shell Metacharacters**: Forbidden characters are rejected
///
/// # Errors
///
/// Returns `Error::SecurityViolation` if:
/// - Command is empty or whitespace
/// - Command is not an absolute path
/// - Command contains shell metacharacters
/// - File does not exist
/// - File is not executable
///
/// # Examples
///
/// ```no_run
/// use mcp_core::validate_command;
///
/// // Valid command
/// match validate_command("/usr/local/bin/mcp-server") {
///     Ok(()) => println!("Command is safe"),
///     Err(e) => eprintln!("Validation failed: {}", e),
/// }
/// ```
///
/// # Security Considerations
///
/// This function only validates the command path itself. Callers should:
/// - Never pass untrusted user input as command arguments
/// - Use `Command::arg()` for arguments (which properly escapes them)
/// - Consider using a whitelist of allowed commands
/// - Run commands with minimal privileges
pub fn validate_command(command: &str) -> Result<()> {
    // Check for empty command
    let command = command.trim();
    if command.is_empty() {
        return Err(Error::SecurityViolation {
            reason: "Command cannot be empty".into(),
        });
    }

    // Check for shell metacharacters
    for forbidden in FORBIDDEN_CHARS {
        if command.contains(*forbidden) {
            return Err(Error::SecurityViolation {
                reason: format!("Command contains forbidden shell metacharacter: '{forbidden}'"),
            });
        }
    }

    // Verify absolute path
    let path = Path::new(command);
    if !path.is_absolute() {
        return Err(Error::SecurityViolation {
            reason: format!("Command must be an absolute path, got: {command}"),
        });
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn test_validate_command_empty() {
        assert!(validate_command("").is_err());
        assert!(validate_command("   ").is_err());
    }

    #[test]
    fn test_validate_command_relative_path() {
        assert!(validate_command("./server").is_err());
        assert!(validate_command("server").is_err());
        assert!(validate_command("../bin/server").is_err());
    }

    #[test]
    fn test_validate_command_shell_metacharacters() {
        let dangerous = vec![
            "/usr/bin/server; rm -rf /",
            "/usr/bin/server | cat",
            "/usr/bin/server && echo pwned",
            "/usr/bin/server > /tmp/out",
            "/usr/bin/server < /tmp/in",
            "/usr/bin/server `whoami`",
            "/usr/bin/server $(whoami)",
            "/usr/bin/server & background",
            "/usr/bin/server\nrm -rf /",
        ];

        for cmd in dangerous {
            let result = validate_command(cmd);
            assert!(result.is_err(), "Should reject dangerous command: {cmd}");
            if let Err(Error::SecurityViolation { reason }) = result {
                assert!(
                    reason.contains("forbidden") || reason.contains("metacharacter"),
                    "Error should mention forbidden character: {reason}"
                );
            }
        }
    }

    #[test]
    fn test_validate_command_nonexistent() {
        #[cfg(unix)]
        let nonexistent_path = "/absolutely/nonexistent/path/to/server";
        #[cfg(windows)]
        let nonexistent_path = "C:\\absolutely\\nonexistent\\path\\to\\server.exe";

        let result = validate_command(nonexistent_path);
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("does not exist"));
        } else {
            panic!("Expected SecurityViolation error");
        }
    }

    #[test]
    fn test_validate_command_directory() {
        // Use /tmp which should exist on Unix and %TEMP% on Windows
        #[cfg(unix)]
        let dir_path = "/tmp";
        #[cfg(windows)]
        let dir_path = "C:\\Windows\\Temp";

        let result = validate_command(dir_path);
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("not a file"));
        } else {
            panic!("Expected SecurityViolation error");
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_validate_command_not_executable() {
        use std::os::unix::fs::PermissionsExt;

        // Create a temporary non-executable file
        let temp_file = "/tmp/test-mcp-nonexec";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();

        // Remove execute permissions
        let mut perms = fs::metadata(temp_file).unwrap().permissions();
        perms.set_mode(0o644); // rw-r--r--
        fs::set_permissions(temp_file, perms).unwrap();

        let result = validate_command(temp_file);
        fs::remove_file(temp_file).ok();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("not executable"));
        } else {
            panic!("Expected SecurityViolation error");
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_validate_command_valid() {
        use std::os::unix::fs::PermissionsExt;

        // Create a temporary executable file
        let temp_file = "/tmp/test-mcp-exec";
        let mut file = fs::File::create(temp_file).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();

        // Set execute permissions
        let mut perms = fs::metadata(temp_file).unwrap().permissions();
        perms.set_mode(0o755); // rwxr-xr-x
        fs::set_permissions(temp_file, perms).unwrap();

        let result = validate_command(temp_file);
        fs::remove_file(temp_file).ok();

        assert!(result.is_ok());
    }

    #[test]
    fn test_is_security_error() {
        let error = validate_command("./relative").unwrap_err();
        assert!(error.is_security_error());
    }
}
