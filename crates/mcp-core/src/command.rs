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
//! // Valid binary name (resolved via PATH) — `build()` validates internally,
//! // so `validate_server_config` is redundant here; shown for clarity.
//! let config = ServerConfig::builder()
//!     .command("docker".to_string())
//!     .arg("run".to_string())
//!     .build()
//!     .unwrap();
//! assert!(validate_server_config(&config).is_ok());
//!
//! // Invalid: shell metacharacters in arg — rejected by `build()` itself,
//! // so no `ServerConfig` carrying this arg can ever exist.
//! let err = ServerConfig::builder()
//!     .command("docker".to_string())
//!     .arg("run; rm -rf /".to_string())
//!     .build()
//!     .unwrap_err();
//! assert!(err.is_security_error());
//! ```

use crate::{Error, Result, ServerConfig, Transport};
use std::path::Path;
use std::time::Duration;

/// Shell metacharacters that indicate potential command injection.
const FORBIDDEN_CHARS: &[char] = &[';', '|', '&', '>', '<', '`', '$', '(', ')', '\n', '\r'];

/// Forbidden environment variable names that pose security risks.
///
/// Matched by [`validate_env_name`] using an ASCII-case-insensitive comparison — Windows
/// treats environment variable names as case-insensitive at the OS/`CreateProcess` level, so
/// a config carrying e.g. `Path` or `NODE_options` is rejected exactly like the canonical
/// spelling shown below.
///
/// # Threat Model — What This List Does and Does Not Protect Against
///
/// This is an **accidental/indirect-misconfiguration guard, not a sandbox
/// boundary**. It blocks the well-known names an interpreter or dynamic
/// linker consults to load extra code or redirect its own search paths —
/// covering the runtimes this bridge actually spawns (Node.js, Python, Ruby,
/// Perl, the JVM, and POSIX shells, in addition to the native dynamic
/// linker) — so that a config sourced from `mcp.json` or CLI flags cannot
/// silently turn an intended `docker`/`node`/`python` invocation into
/// arbitrary code execution via one of these documented hijack vectors:
///
/// - `LD_PRELOAD` / `LD_LIBRARY_PATH` / `LD_AUDIT`: Linux dynamic linker —
///   force-load an arbitrary shared object into the child process
/// - `DYLD_*`: macOS dynamic linker equivalents
/// - `PATH`: binary substitution for any bare (non-absolute) command
/// - `NODE_OPTIONS`: Node.js — inject interpreter flags such as `--require`
///   or `--experimental-loader` into any `node`/`npx` invocation
/// - `BASH_ENV`: sourced by non-interactive `bash` before running a
///   script/command, letting a config inject arbitrary shell code
/// - `PYTHONPATH` / `PYTHONSTARTUP`: Python — module search-path hijacking
///   and arbitrary code executed at interpreter startup
/// - `RUBYOPT`: Ruby — inject interpreter flags (`-r`, `-e`) to load
///   arbitrary code
/// - `PERL5OPT`: Perl — inject interpreter switches to run arbitrary code
/// - `JAVA_TOOL_OPTIONS`: JVM — inject arbitrary JVM arguments, including a
///   `-javaagent` for bytecode instrumentation
///
/// What it deliberately does **not** protect against: a command/binary that
/// is itself malicious, a compromised dependency, arbitrary code the spawned
/// server executes once running, or forbidden-adjacent variables not on this
/// exact-match/prefix list (e.g. an interpreter-specific vector this project
/// does not yet spawn). This list is reviewed and extended as new spawn
/// targets are added, not treated as exhaustive by construction.
const FORBIDDEN_ENV_NAMES: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "PATH",         // Block PATH override to prevent binary substitution
    "NODE_OPTIONS", // Lets a config inject e.g. `--require /tmp/evil.js` into any Node subprocess
    "BASH_ENV",     // Sourced by non-interactive `bash` before running a script/command
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "RUBYOPT",
    "PERL5OPT",
    "JAVA_TOOL_OPTIONS",
];

/// Environment-variable-name prefix rejected regardless of exact match: macOS's
/// dynamic-linker variable family (`DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`, ...).
///
/// Matched by [`validate_env_name`] case-insensitively, same as [`FORBIDDEN_ENV_NAMES`].
const FORBIDDEN_ENV_PREFIX: &str = "DYLD_";

/// Upper bound for `connect_timeout`/`discover_timeout`, matching the
/// 30-second defaults declared in `server_config.rs` with headroom for
/// slow-starting servers configured via `mcp.json`.
const MAX_TIMEOUT: Duration = Duration::from_mins(10);

/// Maximum number of positional arguments accepted in a `ServerConfig` (denial-of-service
/// protection, CWE-400).
///
/// An `mcp.json` entry or CLI invocation is expected to pass a short, fixed argv to the
/// spawned subprocess, so this is generous headroom rather than a realistic expectation.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::MAX_ARG_COUNT;
///
/// assert!(MAX_ARG_COUNT > 0);
/// ```
pub const MAX_ARG_COUNT: usize = 256;

/// Maximum byte length for a single command string, argument, or environment variable name.
///
/// A legitimate command/argument/env-name is always a short identifier or path, never
/// free-form text, so this ceiling exists purely as a resource-exhaustion backstop.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::MAX_ARG_LEN;
///
/// assert!(MAX_ARG_LEN > 0);
/// ```
pub const MAX_ARG_LEN: usize = 4096;

/// Maximum number of environment variables accepted in a `ServerConfig`.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::MAX_ENV_COUNT;
///
/// assert!(MAX_ENV_COUNT > 0);
/// ```
pub const MAX_ENV_COUNT: usize = 256;

/// Maximum byte length for a single environment variable value.
///
/// Wider than [`MAX_ARG_LEN`] since env values legitimately carry things like JSON
/// configuration blobs, not just short identifiers.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::MAX_ENV_VALUE_LEN;
///
/// assert!(MAX_ENV_VALUE_LEN > 0);
/// ```
pub const MAX_ENV_VALUE_LEN: usize = 32 * 1024;

/// Maximum number of HTTP headers accepted for Http/Sse transport.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::MAX_HEADER_COUNT;
///
/// assert!(MAX_HEADER_COUNT > 0);
/// ```
pub const MAX_HEADER_COUNT: usize = 128;

/// Maximum byte length for a single HTTP header value.
///
/// Wider than [`MAX_ARG_LEN`] since header values legitimately carry things like long
/// bearer tokens.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::MAX_HEADER_VALUE_LEN;
///
/// assert!(MAX_HEADER_VALUE_LEN > 0);
/// ```
pub const MAX_HEADER_VALUE_LEN: usize = 8 * 1024;

/// Maximum byte length for the HTTP/Sse transport `url`.
///
/// Generous headroom over any realistic endpoint URL (including a long query string), while
/// still bounding a hostile or hand-edited `mcp.json` entry (denial-of-service protection,
/// CWE-400).
///
/// # Examples
///
/// ```
/// use mcp_execution_core::MAX_URL_LEN;
///
/// assert!(MAX_URL_LEN > 0);
/// ```
pub const MAX_URL_LEN: usize = 8 * 1024;

/// Returns the shell metacharacters considered forbidden in a command or argument string.
///
/// Exposed so downstream consumers that must mirror this exact rule outside this function —
/// currently, the generated TypeScript runtime bridge
/// (`crates/mcp-codegen/templates/progressive/runtime-bridge.ts.hbs`) — can render their copy
/// directly from this constant at code-generation time instead of hand-copying it, which
/// would otherwise silently drift out of sync.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::forbidden_chars;
///
/// assert!(forbidden_chars().contains(&';'));
/// ```
#[must_use]
pub const fn forbidden_chars() -> &'static [char] {
    FORBIDDEN_CHARS
}

/// Returns the exact-match forbidden environment variable names.
///
/// Does not include the `DYLD_` prefix rule — see [`forbidden_env_prefix`] for that. Exposed
/// for the same drift-elimination reason as [`forbidden_chars`]; see its documentation.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::forbidden_env_names;
///
/// assert!(forbidden_env_names().contains(&"LD_PRELOAD"));
/// ```
#[must_use]
pub const fn forbidden_env_names() -> &'static [&'static str] {
    FORBIDDEN_ENV_NAMES
}

/// Returns the environment-variable-name prefix rejected regardless of exact match
/// (currently `DYLD_`, macOS's dynamic-linker variable family).
///
/// # Examples
///
/// ```
/// use mcp_execution_core::forbidden_env_prefix;
///
/// assert_eq!(forbidden_env_prefix(), "DYLD_");
/// ```
#[must_use]
pub const fn forbidden_env_prefix() -> &'static str {
    FORBIDDEN_ENV_PREFIX
}

/// Validates a `ServerConfig` for safe execution, dispatching on transport type.
///
/// This function performs comprehensive security validation before a config is
/// used to connect to a server. It validates:
///
/// 1. **Stdio transport**: command (absolute path or binary name), arguments, and
///    environment variables.
/// 2. **Http/Sse transport**: URL presence and scheme, and HTTP header names/values.
/// 3. **Timeouts**: `connect_timeout`/`discover_timeout` checked against bounds,
///    for all transports.
///
/// # Security Rules
///
/// - **Forbidden chars in command/args**: `;`, `|`, `&`, `>`, `<`, `` ` ``, `$`, `(`, `)`, `\n`, `\r`
/// - **Env name charset**: must match `[A-Za-z_][A-Za-z0-9_]*` (POSIX/Windows environment
///   variable identifier convention); rejects non-ASCII Unicode confusables (e.g. U+0131 `ı`,
///   U+017F `ſ`) that could otherwise dodge the ASCII-only case-insensitive comparison below
///   while still resolving as the forbidden name on Windows
/// - **Forbidden env names**: dynamic-linker (`LD_PRELOAD`, `LD_LIBRARY_PATH`,
///   `LD_AUDIT`, `DYLD_*`), `PATH`, and interpreter hijack vectors
///   (`NODE_OPTIONS`, `BASH_ENV`, `PYTHONPATH`, `PYTHONSTARTUP`, `RUBYOPT`,
///   `PERL5OPT`, `JAVA_TOOL_OPTIONS`), matched case-insensitively — see the
///   `FORBIDDEN_ENV_NAMES` constant's doc comment in this module's source for
///   the full threat-model note
/// - **Absolute paths**: Must exist and be executable
/// - **Binary names**: Allowed (resolved via PATH at runtime)
/// - **URL scheme**: Must be `http://` or `https://`
/// - **Header names/values**: Must not contain control characters
/// - **Timeout bounds**: `connect_timeout`/`discover_timeout` must be greater than zero and at
///   most `MAX_TIMEOUT` (600s)
/// - **Element counts/lengths** (denial-of-service protection, CWE-400) — see this module's
///   `validate_stdio_size_bounds`/`validate_network_size_bounds`: at most `MAX_ARG_COUNT`
///   args, `MAX_ENV_COUNT` env vars, and `MAX_HEADER_COUNT` headers; at most `MAX_ARG_LEN`
///   bytes per command/argument/env-name/header-name, `MAX_ENV_VALUE_LEN` bytes per env
///   value, `MAX_HEADER_VALUE_LEN` bytes per header value, and `MAX_URL_LEN` bytes for the
///   `url` field
///
/// # Errors
///
/// Returns `Error::SecurityViolation` if:
/// - Command is empty or whitespace
/// - Command/args contain shell metacharacters
/// - Absolute path does not exist or is not executable
/// - Environment variable name is forbidden, or outside the `[A-Za-z_][A-Za-z0-9_]*` charset
/// - URL scheme is not `http://`/`https://`, or a header name/value contains control characters
///
/// Returns `Error::ValidationError` if:
/// - URL is missing for Http/Sse transport
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
///     .build()
///     .unwrap();
/// assert!(validate_server_config(&config).is_ok());
///
/// // Invalid: forbidden env var — `ServerConfigBuilder::build()` already
/// // rejects this, so no unvalidated `ServerConfig` reaches this function.
/// let err = ServerConfig::builder()
///     .command("docker".to_string())
///     .env("LD_PRELOAD".to_string(), "/evil.so".to_string())
///     .build()
///     .unwrap_err();
/// assert!(err.is_security_error());
///
/// // Valid: HTTP transport
/// let config = ServerConfig::builder()
///     .http_transport("https://api.example.com/mcp".to_string())
///     .build()
///     .unwrap();
/// assert!(validate_server_config(&config).is_ok());
/// ```
///
/// # Security Considerations
///
/// - Binary names are allowed and resolved via PATH at runtime
/// - Absolute paths undergo strict validation (existence, permissions)
/// - All arguments are validated separately to prevent injection
/// - Environment variables are checked against forbidden names
/// - Header values are never echoed into error messages, since they routinely
///   carry secrets such as bearer tokens
/// - Header *names* are never echoed either, once rejected: a `Name=Value` or
///   `Name: Value` CLI argument can be mis-split on the wrong separator,
///   leaving a full secret value in the "name" position — the token-charset
///   error, the duplicate-header-name error, and the header-value
///   control-character error all omit the name for this reason
/// - There is no infinite-timeout option: `0` is always rejected, since an
///   unbounded wait would let a hung server block this non-interactive tool
///   forever (see the `validate_timeout` design note in this module)
pub fn validate_server_config(config: &ServerConfig) -> Result<()> {
    match config.transport() {
        Transport::Stdio {
            command, args, env, ..
        } => {
            // Element counts/lengths (denial-of-service protection, CWE-400) are bounded
            // before the command-injection-specific checks below.
            validate_stdio_size_bounds(command, args, env)?;
            validate_stdio_config(command, args, env)?;
        }
        Transport::Http { url, headers } | Transport::Sse { url, headers } => {
            validate_network_size_bounds(url, headers)?;
            validate_network_config(url, headers)?;
        }
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

/// Bounds `command`'s length and `args`'/`env`'s counts/lengths (denial-of-service
/// protection, CWE-400) for a [`Transport::Stdio`] config.
///
/// Since #313, `Transport::Http`/`Transport::Sse` have no `command`/`args`/`env` fields at
/// all — the cross-transport bypass this once guarded against (issue #198 S2: a hostile
/// `mcp.json` populating `args`/`env` for a non-stdio transport) is unrepresentable rather
/// than merely unchecked, so this only needs to run for the `Stdio` variant.
///
/// Deliberately does not check for shell metacharacters or forbidden environment variable
/// names — that remains [`validate_stdio_config`]'s responsibility, since it is only
/// meaningful for a config that is actually used to spawn a subprocess.
fn validate_stdio_size_bounds(
    command: &str,
    args: &[String],
    env: &std::collections::HashMap<String, String>,
) -> Result<()> {
    if command.len() > MAX_ARG_LEN {
        return Err(Error::SecurityViolation {
            reason: format!(
                "command too long: {} bytes exceeds the {MAX_ARG_LEN} limit",
                command.len()
            ),
        });
    }

    if args.len() > MAX_ARG_COUNT {
        return Err(Error::SecurityViolation {
            reason: format!(
                "too many arguments: {} exceeds the {MAX_ARG_COUNT} limit",
                args.len()
            ),
        });
    }
    for (idx, arg) in args.iter().enumerate() {
        if arg.len() > MAX_ARG_LEN {
            return Err(Error::SecurityViolation {
                reason: format!(
                    "argument {idx} too long: {} bytes exceeds the {MAX_ARG_LEN} limit",
                    arg.len()
                ),
            });
        }
    }

    if env.len() > MAX_ENV_COUNT {
        return Err(Error::SecurityViolation {
            reason: format!(
                "too many environment variables: {} exceeds the {MAX_ENV_COUNT} limit",
                env.len()
            ),
        });
    }
    for (env_name, env_value) in env {
        if env_name.len() > MAX_ARG_LEN {
            return Err(Error::SecurityViolation {
                reason: format!(
                    "environment variable name too long: {} bytes exceeds the {MAX_ARG_LEN} \
                     limit",
                    env_name.len()
                ),
            });
        }
        if env_value.len() > MAX_ENV_VALUE_LEN {
            return Err(Error::SecurityViolation {
                reason: format!(
                    "environment variable '{env_name}' value too long: {} bytes exceeds the \
                     {MAX_ENV_VALUE_LEN} limit",
                    env_value.len()
                ),
            });
        }
    }

    Ok(())
}

/// Bounds `url`'s length and `headers`' count/lengths (denial-of-service protection,
/// CWE-400) for a [`Transport::Http`]/[`Transport::Sse`] config.
///
/// See [`validate_stdio_size_bounds`]'s doc comment for why this only needs to run for its
/// own variant family since #313.
fn validate_network_size_bounds(
    url: &str,
    headers: &std::collections::HashMap<String, String>,
) -> Result<()> {
    if url.len() > MAX_URL_LEN {
        return Err(Error::SecurityViolation {
            reason: format!(
                "url too long: {} bytes exceeds the {MAX_URL_LEN} limit",
                url.len()
            ),
        });
    }

    if headers.len() > MAX_HEADER_COUNT {
        return Err(Error::SecurityViolation {
            reason: format!(
                "too many headers: {} exceeds the {MAX_HEADER_COUNT} limit",
                headers.len()
            ),
        });
    }
    for (name, value) in headers {
        if name.len() > MAX_ARG_LEN {
            return Err(Error::SecurityViolation {
                reason: format!(
                    "header name too long: {} bytes exceeds the {MAX_ARG_LEN} limit",
                    name.len()
                ),
            });
        }
        if value.len() > MAX_HEADER_VALUE_LEN {
            return Err(Error::SecurityViolation {
                reason: format!(
                    "header value too long: {} bytes exceeds the {MAX_HEADER_VALUE_LEN} limit",
                    value.len()
                ),
            });
        }
    }

    Ok(())
}

/// Validates the stdio-transport-specific fields of a `ServerConfig`.
///
/// Checks the command (absolute path or binary name), arguments, and environment variables
/// for command-injection risks. Element counts/lengths are already bounded unconditionally by
/// [`validate_stdio_size_bounds`] before this runs; this function only adds the checks that
/// are meaningful specifically because this config will be used to spawn a subprocess.
fn validate_stdio_config(
    command: &str,
    args: &[String],
    env: &std::collections::HashMap<String, String>,
) -> Result<()> {
    // Validate command
    validate_command_string(command, "command")?;

    // If command is absolute path, perform additional checks
    let command_path = Path::new(command);
    if command_path.is_absolute() {
        validate_absolute_path(command)?;
    }
    // If not absolute, it's a binary name (to be resolved via PATH) - this is OK

    // Validate each argument separately
    for (idx, arg) in args.iter().enumerate() {
        validate_command_string(arg, &format!("argument {idx}"))?;
    }

    // Validate environment variable names
    for env_name in env.keys() {
        validate_env_name(env_name)?;
    }

    Ok(())
}

/// Validates the Http/Sse-transport-specific fields of a `ServerConfig`.
///
/// `url` is a required field of [`Transport::Http`]/[`Transport::Sse`] (see #313), so unlike
/// before, a config missing it cannot reach this function at all — that gap is now closed at
/// deserialization/construction time rather than here.
///
/// `headers`'/`url`'s element counts/lengths are already bounded unconditionally by
/// [`validate_network_size_bounds`] before this runs; this function only adds the checks that
/// are meaningful specifically because this config will be used to send an HTTP request
/// (header name charset, control characters, scheme, duplicate names).
fn validate_network_config(
    url: &str,
    headers: &std::collections::HashMap<String, String>,
) -> Result<()> {
    validate_url_scheme(url)?;

    // `http::HeaderName` lowercases on parse, so two headers that differ only
    // in case (e.g. "Authorization" and "authorization") collapse into a
    // single entry with a nondeterministic winner once converted — reject
    // that here rather than letting it silently drop a header downstream.
    let mut seen_header_names = std::collections::HashSet::new();
    for (name, value) in headers {
        validate_header_name_string(name)?;
        validate_header_value_string(value)?;
        if !seen_header_names.insert(name.to_ascii_lowercase()) {
            return Err(Error::SecurityViolation {
                reason: "duplicate header name (case-insensitive); name omitted as it may \
                         be secret-shaped"
                    .to_string(),
            });
        }
    }

    Ok(())
}

/// Validates that a URL uses the `http://` or `https://` scheme.
///
/// This is defense in depth: rejects `file://`, `unix://`, and similar
/// schemes at the `mcp-core` validation boundary rather than relying on the
/// HTTP client to reject them. The scheme comparison is case-insensitive per
/// RFC 3986 (`HTTP://host` is a valid URL, not a different scheme).
///
/// This is a minimal, string-based scheme check — it does not validate the
/// rest of the URL's structure (e.g. it does not require a host). It is
/// exposed publicly so that other crates checking URL validity for the same
/// http/sse transport (e.g. `mcp-execution-cli`'s server status/validation
/// commands) can share this exact rule instead of drifting from it with a
/// second, differently-behaved check.
///
/// # Errors
///
/// Returns [`Error::SecurityViolation`] if `url` does not start with an
/// `http://` or `https://` scheme (case-insensitive).
///
/// # Examples
///
/// ```
/// use mcp_execution_core::validate_url_scheme;
///
/// assert!(validate_url_scheme("https://example.com/mcp").is_ok());
/// assert!(validate_url_scheme("HTTP://example.com").is_ok());
/// assert!(validate_url_scheme("ftp://example.com").is_err());
/// assert!(validate_url_scheme("  https://example.com").is_err());
/// ```
pub fn validate_url_scheme(url: &str) -> Result<()> {
    let is_valid = url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    });
    if is_valid {
        Ok(())
    } else {
        Err(Error::SecurityViolation {
            reason: "url must use the http:// or https:// scheme".to_string(),
        })
    }
}

/// Returns `true` if `value` contains an ASCII or Unicode control character
/// (including `\r`, `\n`, and NUL), which could otherwise be used to smuggle
/// extra header lines into an HTTP request.
fn contains_control_char(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// Returns `true` if `c` is a valid RFC 7230 `tchar` (the charset allowed in
/// an HTTP header field name).
const fn is_header_name_tchar(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '.'
                | '^'
                | '_'
                | '`'
                | '|'
                | '~'
        )
}

/// Validates an HTTP header name against the RFC 7230 `token` charset.
///
/// A plain control-character check is not tight enough: a space, `:`, or `@`
/// is not a control character but is still an invalid header-name character
/// that would otherwise pass here and fail later inside `http::HeaderName`
/// construction with an opaque error.
///
/// # Security
///
/// The rejected name is never echoed into the error message. A `Name=Value`
/// or `Name: Value` CLI argument can be mis-split on the wrong separator,
/// leaving a full secret value in the "name" position; that value only needs
/// one non-`tchar` byte to reach this branch, so it must be treated the same
/// as a secret — mirroring the duplicate-header-name check below, which
/// redacts for the same reason.
fn validate_header_name_string(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::SecurityViolation {
            reason: "header name cannot be empty".to_string(),
        });
    }
    if !name.chars().all(is_header_name_tchar) {
        return Err(Error::SecurityViolation {
            reason: "header name contains characters outside the allowed HTTP token charset"
                .to_string(),
        });
    }
    Ok(())
}

/// Validates an HTTP header value for control characters.
///
/// # Security
///
/// The header *value* routinely carries secrets (e.g. bearer tokens), so it
/// must never appear in the returned error's reason string. The header
/// *name* is not echoed either: this runs after `validate_header_name_string`
/// has already accepted it as RFC 7230 `token`-charset-only, the same
/// "may still be secret-shaped input from a misparsed argument" condition
/// that the tchar-violation and duplicate-header-name errors above already
/// treat as untrusted.
fn validate_header_value_string(value: &str) -> Result<()> {
    if contains_control_char(value) {
        return Err(Error::SecurityViolation {
            reason: "header value contains control characters".to_string(),
        });
    }
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
/// for dangerous shell metacharacters. Length is already bounded unconditionally by
/// [`validate_stdio_size_bounds`] before this runs.
///
/// # Security
///
/// The offending value is never echoed into the error message. `context` is
/// `"argument {idx}"` for CLI arguments, which routinely carry secrets in a
/// `--api-key sk-...`-style value; the same "may be secret-shaped" treatment
/// as `validate_header_value_string` and the duplicate-header-name check
/// applies here.
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
                    "{context} contains forbidden shell metacharacter '{forbidden}'; \
                     value omitted as it may be secret-shaped"
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
/// This is an internal helper that checks if an environment variable name is in the
/// forbidden list. Length is already bounded unconditionally by [`validate_stdio_size_bounds`]
/// before this runs.
///
/// The comparison is ASCII-case-insensitive: Windows treats environment variable names as
/// case-insensitive at the OS/`CreateProcess` level (and so does std's `Command` environment
/// block on that platform), so a case-varied spelling such as `Path` or `path` would otherwise
/// bypass this list while still functioning as a real override when the subprocess is spawned.
///
/// Before that comparison, the name is required to match the conventional POSIX/Windows
/// environment-variable-name charset `[A-Za-z_][A-Za-z0-9_]*`. Windows' own name comparison
/// folds case using the OS's Unicode uppercase table, which is broader than the ASCII-only
/// folding `eq_ignore_ascii_case` performs here — e.g. `ı` (U+0131, Turkish dotless i)
/// uppercases to `I` and `ſ` (U+017F, long s) uppercases to `S` on Windows, so a forbidden name
/// spelled with one of these in place of the ASCII letter (e.g. `NODE_OPTıONS`) would pass the
/// ASCII-only comparison yet still resolve as the forbidden name once handed to the OS
/// environment block. Rather than chase every such Unicode confusable, any name outside the
/// conventional identifier charset is rejected outright, since it is not a valid environment
/// variable name to begin with.
fn validate_env_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let is_valid_charset = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !is_valid_charset {
        return Err(Error::SecurityViolation {
            reason: format!(
                "environment variable name is not a valid identifier (expected \
                 [A-Za-z_][A-Za-z0-9_]*): {name}"
            ),
        });
    }

    // Check for forbidden env names (exact match, case-insensitive)
    if FORBIDDEN_ENV_NAMES
        .iter()
        .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
    {
        return Err(Error::SecurityViolation {
            reason: format!("Forbidden environment variable name: {name}"),
        });
    }

    // Check for DYLD_* prefix (macOS dynamic linker variables), case-insensitive.
    // Compared as bytes (not `str` slicing) so a multi-byte UTF-8 name can never
    // panic on a non-char-boundary split.
    if name
        .as_bytes()
        .get(..FORBIDDEN_ENV_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(FORBIDDEN_ENV_PREFIX.as_bytes()))
    {
        return Err(Error::SecurityViolation {
            reason: format!("Forbidden environment variable prefix DYLD_: {name}"),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write;

    #[test]
    fn test_validate_server_config_binary_name() {
        // Binary names (not absolute paths) should be valid
        assert!(
            ServerConfig::builder()
                .command("docker".to_string())
                .build()
                .is_ok()
        );
        assert!(
            ServerConfig::builder()
                .command("python".to_string())
                .build()
                .is_ok()
        );
        assert!(
            ServerConfig::builder()
                .command("node".to_string())
                .build()
                .is_ok()
        );
    }

    #[test]
    fn test_validate_server_config_binary_with_args() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .arg("--rm".to_string())
            .arg("mcp-server".to_string())
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_empty_command() {
        // Empty command should fail during build
        let result = ServerConfig::builder().command(String::new()).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));

        // Whitespace-only command should fail during build
        let result = ServerConfig::builder().command("   ".to_string()).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
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
            // `build()` now runs security validation internally, so a config
            // carrying a shell metacharacter is rejected at construction.
            let result = ServerConfig::builder().command(cmd.to_string()).build();
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
            let result = ServerConfig::builder()
                .command("docker".to_string())
                .arg(arg.to_string())
                .build();
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
    fn test_validate_server_config_arg_with_metacharacter_does_not_leak_secret() {
        // Regression test for #229: a rejected arg is routinely a
        // misparsed `--api-key sk-...`-style secret; the metacharacter
        // error must never echo the raw value.
        let secret_shaped_arg = "--api-key sk-live-supersecretvalue1234567890;whoami";
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .arg(secret_shaped_arg.to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(!reason.contains(secret_shaped_arg));
            assert!(!reason.contains("sk-live-supersecretvalue1234567890"));
        }
    }

    #[test]
    fn test_validate_server_config_empty_arg() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .arg(String::new())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_server_config_forbidden_env_ld_preload() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env("LD_PRELOAD".to_string(), "/evil.so".to_string())
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("LD_PRELOAD"));
        }
    }

    #[test]
    fn test_validate_server_config_forbidden_env_ld_library_path() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env("LD_LIBRARY_PATH".to_string(), "/evil".to_string())
            .build();
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
            let result = ServerConfig::builder()
                .command("docker".to_string())
                .env(var.to_string(), "/evil".to_string())
                .build();
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
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env("PATH".to_string(), "/evil:/usr/bin".to_string())
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("PATH"));
        }
    }

    /// #221.1 — the interpreter hijack vectors added alongside the original
    /// dynamic-linker/`PATH` entries must also be rejected.
    #[test]
    fn test_validate_server_config_forbidden_env_interpreter_hijack_vectors() {
        // NODE_OPTIONS and BASH_ENV have their own dedicated tests above.
        let interpreter_vars = vec![
            "PYTHONPATH",
            "PYTHONSTARTUP",
            "RUBYOPT",
            "PERL5OPT",
            "JAVA_TOOL_OPTIONS",
            "LD_AUDIT",
        ];

        for var in interpreter_vars {
            let result = ServerConfig::builder()
                .command("docker".to_string())
                .env(var.to_string(), "evil".to_string())
                .build();
            assert!(result.is_err(), "Should reject variable: {var}");
            if let Err(Error::SecurityViolation { reason }) = result {
                assert!(reason.contains(var), "Error should mention {var}: {reason}");
            }
        }
    }

    #[test]
    fn test_validate_server_config_forbidden_env_node_options() {
        // NODE_OPTIONS lets a config inject e.g. `--require /tmp/evil.js` into any Node
        // subprocess the server itself spawns.
        let result = ServerConfig::builder()
            .command("node".to_string())
            .env(
                "NODE_OPTIONS".to_string(),
                "--require /tmp/evil.js".to_string(),
            )
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("NODE_OPTIONS"));
        }
    }

    #[test]
    fn test_validate_server_config_forbidden_env_bash_env() {
        // BASH_ENV is sourced by non-interactive `bash` before running a script or command.
        let result = ServerConfig::builder()
            .command("bash".to_string())
            .env("BASH_ENV".to_string(), "/tmp/evil.sh".to_string())
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("BASH_ENV"));
        }
    }

    #[test]
    fn test_validate_server_config_safe_env() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env("LOG_LEVEL".to_string(), "debug".to_string())
            .env("DEBUG".to_string(), "1".to_string())
            .env("HOME".to_string(), "/home/user".to_string())
            .env("MY_CUSTOM_VAR".to_string(), "value".to_string())
            .build();
        assert!(result.is_ok());
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

        let result = ServerConfig::builder()
            .command(temp_file.to_string())
            .arg("--port".to_string())
            .arg("8080".to_string())
            .build();

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

        let result = ServerConfig::builder()
            .command(temp_file.to_string())
            .build();

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

        let result = ServerConfig::builder()
            .command(nonexistent.to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("does not exist"));
        }
    }

    #[test]
    fn test_validate_server_config_with_cwd() {
        // cwd doesn't affect validation (it's not security-critical)
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .cwd(std::path::PathBuf::from("/tmp"))
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_complex_valid() {
        let result = ServerConfig::builder()
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
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_default_timeouts_pass() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_zero_connect_timeout_rejected() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .connect_timeout(std::time::Duration::ZERO)
            .build();
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
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .discover_timeout(std::time::Duration::ZERO)
            .build();
        assert!(result.is_err());
        if let Err(Error::ValidationError { field, .. }) = result {
            assert_eq!(field, "discover_timeout");
        } else {
            panic!("expected ValidationError");
        }
    }

    #[test]
    fn test_validate_server_config_above_max_timeout_rejected() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .connect_timeout(std::time::Duration::from_secs(601))
            .build();
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
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .connect_timeout(std::time::Duration::from_mins(1))
            .discover_timeout(std::time::Duration::from_mins(10))
            .build();
        assert!(result.is_ok());
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

    #[test]
    fn test_validate_env_name_case_insensitive() {
        // Windows treats env var names as case-insensitive, so any casing of an
        // exact-match forbidden name must be rejected, not just the canonical spelling.
        assert!(validate_env_name("Path").is_err());
        assert!(validate_env_name("path").is_err());
        assert!(validate_env_name("PATH").is_err());
        assert!(validate_env_name("PaTh").is_err());

        assert!(validate_env_name("Ld_Preload").is_err());
        assert!(validate_env_name("ld_preload").is_err());

        assert!(validate_env_name("Node_Options").is_err());
        assert!(validate_env_name("node_options").is_err());

        // Prefix match must also be case-insensitive.
        assert!(validate_env_name("dyld_insert_libraries").is_err());
        assert!(validate_env_name("Dyld_Insert_Libraries").is_err());
        assert!(validate_env_name("dYlD_anything").is_err());

        // Sanity: names that are not case variants of a forbidden entry stay allowed.
        assert!(validate_env_name("MyPath").is_ok());
        assert!(validate_env_name("dyl").is_ok()); // too short for DYLD_ prefix, not a match
    }

    #[test]
    fn test_validate_env_name_prefix_check_does_not_panic_on_utf8_boundary() {
        // "DYL€_REST": the euro sign ('\u{20AC}') is a 3-byte UTF-8 sequence occupying byte
        // indices 3..6, so byte index `FORBIDDEN_ENV_PREFIX.len()` (5) falls strictly inside
        // it and is not a char boundary. A naive `name[..5]` slice would panic here. The
        // charset check now rejects this name (non-ASCII characters are never valid identifier
        // characters) before the byte-slice `eq_ignore_ascii_case` prefix comparison even runs,
        // but that comparison must still not panic if reached directly.
        let name = "DYL\u{20AC}_REST";
        assert_eq!(name.len(), 3 + 3 + 5);
        assert!(!name.is_char_boundary(5));
        assert!(validate_env_name(name).is_err());
    }

    /// #438 — Windows' own environment-name comparison folds case using the OS's Unicode
    /// uppercase table, which is broader than `eq_ignore_ascii_case`. A forbidden name spelled
    /// with a Unicode confusable in place of an ASCII letter must be rejected outright by the
    /// charset check, since it is not a valid `[A-Za-z_][A-Za-z0-9_]*` identifier to begin with.
    #[test]
    fn test_validate_env_name_rejects_unicode_case_confusables() {
        // U+0131 LATIN SMALL LETTER DOTLESS I uppercases to 'I' on Windows.
        assert!(validate_env_name("NODE_OPT\u{0131}ONS").is_err());
        // U+017F LATIN SMALL LETTER LONG S uppercases to 'S' on Windows.
        assert!(validate_env_name("JAVA_TOOL_OPTION\u{017F}").is_err());
    }

    #[test]
    fn test_validate_env_name_charset_accepts_valid_ascii_identifiers() {
        assert!(validate_env_name("MY_VAR_1").is_ok());
        assert!(validate_env_name("_LEADING_UNDERSCORE").is_ok());
        assert!(validate_env_name("A").is_ok());
        assert!(validate_env_name("a1").is_ok());
    }

    #[test]
    fn test_validate_env_name_charset_rejects_empty_and_leading_digit() {
        assert!(validate_env_name("").is_err());
        assert!(validate_env_name("1FOO").is_err());
        assert!(validate_env_name("9").is_err());
    }

    #[test]
    fn test_forbidden_env_constants_are_already_ascii_uppercase() {
        // The generated TS runtime bridge (`runtime-bridge.ts.hbs`) renders these constants
        // verbatim and upper-cases only the *input* name before comparing, relying on
        // `FORBIDDEN_ENV_NAMES`/`FORBIDDEN_ENV_PREFIX` already being upper-case. This invariant
        // protects that render-from-Rust drift guarantee.
        for forbidden in FORBIDDEN_ENV_NAMES {
            assert_eq!(*forbidden, forbidden.to_ascii_uppercase());
        }
        assert_eq!(
            FORBIDDEN_ENV_PREFIX,
            FORBIDDEN_ENV_PREFIX.to_ascii_uppercase()
        );
    }

    // ── Http/Sse transport validation ────────────────────────────────────────

    #[test]
    fn test_validate_server_config_http_valid() {
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_sse_valid() {
        let result = ServerConfig::builder()
            .sse_transport("https://api.example.com/sse".to_string())
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_http_with_valid_headers() {
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), "Bearer token123".to_string())
            .build();
        assert!(result.is_ok());
    }

    /// #313 — `url` is a required (non-`#[serde(default)]`) field of `Transport::Http`, so a
    /// hand-edited `mcp.json` with `"transport": "http"` and no `url` key now fails to
    /// deserialize at all, rather than producing an incomplete `ServerConfig` that only
    /// `validate_server_config` would have caught downstream (see also
    /// `server_config::tests::test_deserialize_http_config_missing_url_is_rejected`).
    #[test]
    fn test_validate_server_config_http_missing_url_rejected() {
        let result: std::result::Result<ServerConfig, _> =
            serde_json::from_str(r#"{"transport": "http"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_server_config_sse_missing_url_rejected() {
        let result: std::result::Result<ServerConfig, _> =
            serde_json::from_str(r#"{"transport": "sse"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_server_config_http_rejects_non_http_scheme() {
        for url in [
            "file:///etc/passwd",
            "unix:///tmp/socket",
            "ftp://host/path",
        ] {
            let result = ServerConfig::builder()
                .http_transport(url.to_string())
                .build();
            assert!(result.is_err(), "should reject scheme: {url}");
            if let Err(Error::SecurityViolation { reason }) = result {
                assert!(reason.contains("http://") || reason.contains("https://"));
            } else {
                panic!("expected SecurityViolation for url: {url}");
            }
        }
    }

    #[test]
    fn test_validate_server_config_http_accepts_case_insensitive_scheme() {
        for url in ["HTTP://api.example.com/mcp", "HTTPS://api.example.com/mcp"] {
            let result = ServerConfig::builder()
                .http_transport(url.to_string())
                .build();
            assert!(
                result.is_ok(),
                "should accept case-insensitive scheme: {url}"
            );
        }
    }

    #[test]
    fn test_validate_server_config_http_rejects_scheme_lookalike() {
        // "httpsomething" must not be accepted as a loose prefix match of "http".
        let result = ServerConfig::builder()
            .http_transport("httpsomething://api.example.com/mcp".to_string())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_server_config_http_rejects_duplicate_header_case_insensitive() {
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), "Bearer one".to_string())
            .header("authorization".to_string(), "Bearer two".to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("duplicate header"));
            assert!(!reason.contains("Authorization"));
            assert!(!reason.to_ascii_lowercase().contains("authorization"));
        } else {
            panic!("expected SecurityViolation for duplicate header name");
        }
    }

    #[test]
    fn test_validate_server_config_http_rejects_duplicate_header_secret_shaped_name() {
        // A misparsed `Name: Value`-style CLI argument can leave a "key" that
        // is entirely RFC 7230 token-charset (alphanumerics plus
        // `!#$%&'*+-.^_`|~`), e.g. a hex-encoded key or JWT-like value using
        // only `A-Za-z0-9-_.`. Such a name passes `validate_header_name_string`
        // and must not be echoed if it collides case-insensitively.
        let secret_name = "eyJhbGciOiJIUzI1NiJ9.super-secret-token-material";
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(secret_name.to_string(), "value one".to_string())
            .header(secret_name.to_ascii_uppercase(), "value two".to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("duplicate header"));
            assert!(!reason.contains(secret_name));
            assert!(
                !reason
                    .to_ascii_lowercase()
                    .contains(&secret_name.to_ascii_lowercase())
            );
        } else {
            panic!("expected SecurityViolation for duplicate header name");
        }
    }

    #[test]
    fn test_validate_server_config_http_rejects_header_name_with_invalid_tchar() {
        // Space, ':', and '@' are not control characters but are still
        // invalid HTTP header-name characters (outside RFC 7230's `token`).
        for bad_name in ["X Bad Header", "X:Bad", "X@Bad"] {
            let result = ServerConfig::builder()
                .http_transport("https://api.example.com/mcp".to_string())
                .header(bad_name.to_string(), "value".to_string())
                .build();

            assert!(result.is_err(), "should reject header name: {bad_name}");
            if let Err(Error::SecurityViolation { reason }) = result {
                assert!(reason.contains("header name"));
                assert!(!reason.contains(bad_name));
            } else {
                panic!("expected SecurityViolation for header name: {bad_name}");
            }
        }
    }

    #[test]
    fn test_validate_server_config_http_rejects_secret_shaped_header_name_without_leaking_it() {
        // Reproduces the #215 leak vector: a `Name=Value` CLI argument
        // mis-split on the wrong `=` leaves a base64-encoded secret in the
        // "name" position. It only needs one non-tchar byte (here `/`) to
        // reach `validate_header_name_string`'s tchar-violation branch,
        // which must not echo it back.
        let secret_name = "aGVsbG8/d29ybGQK=supersecretpayload";
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(secret_name.to_string(), "value".to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("header name"));
            assert!(!reason.contains(secret_name));
            assert!(!reason.contains("aGVsbG8"));
        } else {
            panic!("expected SecurityViolation for header name");
        }
    }

    #[test]
    fn test_validate_server_config_http_rejects_control_char_in_header_name() {
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("X-Bad\r\nHeader".to_string(), "value".to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("header name"));
            assert!(!reason.contains("X-Bad"));
        } else {
            panic!("expected SecurityViolation for header name");
        }
    }

    #[test]
    fn test_validate_server_config_http_rejects_control_char_in_header_value() {
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(
                "Authorization".to_string(),
                "Bearer sekrit\r\nX-Injected: evil".to_string(),
            )
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("header value"));
            // Neither the header value nor its (ordinary, non-secret-shaped
            // here) name need to appear — the name is withheld unconditionally
            // since this path cannot distinguish an ordinary name from a
            // secret-shaped one.
            assert!(!reason.contains("Authorization"));
            assert!(!reason.contains("sekrit"));
            assert!(!reason.contains("X-Injected"));
        } else {
            panic!("expected SecurityViolation for header value");
        }
    }

    #[test]
    fn test_validate_server_config_http_rejects_control_char_in_value_with_secret_shaped_name() {
        // Reproduces the critic's S5 repro: a JWT-shaped header *name* (fully
        // RFC 7230 token-charset, so it clears `validate_header_name_string`)
        // paired with a control character in the *value*. Both the name and
        // the control-char-bearing value must be absent from the error.
        let secret_name = "eyJhbGciOiJIUzI1NiJ9.abc-secret_material";
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(secret_name.to_string(), "x\ry".to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("header value"));
            assert!(!reason.contains(secret_name));
            assert!(!reason.contains("eyJhbGciOiJIUzI1NiJ9"));
        } else {
            panic!("expected SecurityViolation for header value");
        }
    }

    // ── Resource-exhaustion bounds (issue #198) ──────────────────────────────

    #[test]
    fn test_validate_server_config_rejects_too_many_args() {
        let args = (0..=MAX_ARG_COUNT).map(|i| format!("a{i}")).collect();
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .args(args)
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too many arguments"));
        } else {
            panic!("expected SecurityViolation for too many arguments");
        }
    }

    #[test]
    fn test_validate_server_config_accepts_max_arg_count() {
        let args = (0..MAX_ARG_COUNT).map(|i| format!("a{i}")).collect();
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .args(args)
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_rejects_oversized_arg() {
        let long_arg = "a".repeat(MAX_ARG_LEN + 1);
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .arg(long_arg)
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too long"));
        } else {
            panic!("expected SecurityViolation for oversized argument");
        }
    }

    #[test]
    fn test_validate_server_config_accepts_arg_at_max_len() {
        let arg_at_cap = "a".repeat(MAX_ARG_LEN);
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .arg(arg_at_cap)
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_rejects_oversized_command() {
        let long_command = "a".repeat(MAX_ARG_LEN + 1);
        let result = ServerConfig::builder().command(long_command).build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too long"));
        } else {
            panic!("expected SecurityViolation for oversized command");
        }
    }

    #[test]
    fn test_validate_server_config_rejects_too_many_env_vars() {
        let env: HashMap<String, String> = (0..=MAX_ENV_COUNT)
            .map(|i| (format!("VAR_{i}"), "value".to_string()))
            .collect();
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .environment(env)
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too many environment variables"));
        } else {
            panic!("expected SecurityViolation for too many env vars");
        }
    }

    #[test]
    fn test_validate_server_config_accepts_max_env_count() {
        let env: HashMap<String, String> = (0..MAX_ENV_COUNT)
            .map(|i| (format!("VAR_{i}"), "value".to_string()))
            .collect();
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .environment(env)
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_rejects_oversized_env_value() {
        let long_value = "v".repeat(MAX_ENV_VALUE_LEN + 1);
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env("MY_VAR".to_string(), long_value)
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too long"));
        } else {
            panic!("expected SecurityViolation for oversized env value");
        }
    }

    #[test]
    fn test_validate_server_config_accepts_env_value_at_max_len() {
        let value_at_cap = "v".repeat(MAX_ENV_VALUE_LEN);
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env("MY_VAR".to_string(), value_at_cap)
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_rejects_oversized_env_name() {
        let long_name = "V".repeat(MAX_ARG_LEN + 1);
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env(long_name, "value".to_string())
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too long"));
        } else {
            panic!("expected SecurityViolation for oversized env name");
        }
    }

    #[test]
    fn test_validate_server_config_rejects_too_many_headers() {
        let headers: HashMap<String, String> = (0..=MAX_HEADER_COUNT)
            .map(|i| (format!("X-Header-{i}"), "value".to_string()))
            .collect();
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .headers(headers)
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too many headers"));
        } else {
            panic!("expected SecurityViolation for too many headers");
        }
    }

    #[test]
    fn test_validate_server_config_accepts_max_header_count() {
        let headers: HashMap<String, String> = (0..MAX_HEADER_COUNT)
            .map(|i| (format!("X-Header-{i}"), "value".to_string()))
            .collect();
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .headers(headers)
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_rejects_oversized_header_value() {
        let long_value = "v".repeat(MAX_HEADER_VALUE_LEN + 1);
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), long_value)
            .build();
        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("too long"));
        } else {
            panic!("expected SecurityViolation for oversized header value");
        }
    }

    #[test]
    fn test_validate_server_config_accepts_header_value_at_max_len() {
        let value_at_cap = "v".repeat(MAX_HEADER_VALUE_LEN);
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), value_at_cap)
            .build();
        assert!(result.is_ok());
    }

    // ── #313: cross-transport fields are now unrepresentable ────────────────────
    //
    // The S2/N1 bypass this section used to guard against (a hand-edited `mcp.json`
    // populating `args`/`env`/`headers`/`url`/`command` for the "wrong" transport, since every
    // field used to exist unconditionally at the type level) is closed by construction as of
    // #313: `Transport::Http`/`Transport::Sse` have no `command`/`args`/`env`/`cwd` fields, and
    // `Transport::Stdio` has no `url`/`headers` fields. A JSON key that doesn't belong to the
    // deserialized variant has no field to populate, so `serde` simply ignores it — the same as
    // any other unrecognized key — rather than it being a bypass.

    #[test]
    fn test_deserialize_ignores_cross_transport_command_field() {
        let json = serde_json::json!({
            "transport": "http",
            "url": "https://api.example.com/mcp",
            "command": "a".repeat(MAX_ARG_LEN + 1),
        });
        let config: ServerConfig = serde_json::from_value(json).expect("valid ServerConfig JSON");

        // An Http config has no `command` field to populate, so the oversized value was never
        // stored anywhere and is not a resource-exhaustion vector.
        assert!(config.command().is_none());
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_deserialize_ignores_cross_transport_headers_field() {
        let headers: HashMap<String, String> = (0..=MAX_HEADER_COUNT)
            .map(|i| (format!("X-Header-{i}"), "value".to_string()))
            .collect();
        let json = serde_json::json!({
            "transport": "stdio",
            "command": "docker",
            "headers": headers,
        });
        let config: ServerConfig = serde_json::from_value(json).expect("valid ServerConfig JSON");

        assert!(config.headers().is_empty());
        assert!(validate_server_config(&config).is_ok());
    }

    #[test]
    fn test_validate_server_config_http_rejects_url_too_long() {
        let long_url = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        let result = ServerConfig::builder().http_transport(long_url).build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("url too long"));
        } else {
            panic!("expected SecurityViolation for oversized url");
        }
    }

    #[test]
    fn test_validate_server_config_http_accepts_url_at_max_len() {
        let prefix = "https://example.com/";
        let padding_len = MAX_URL_LEN - prefix.len();
        let url_at_cap = format!("{prefix}{}", "a".repeat(padding_len));
        assert_eq!(url_at_cap.len(), MAX_URL_LEN);

        let result = ServerConfig::builder().http_transport(url_at_cap).build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_http_rejects_header_name_too_long() {
        let long_name = format!("X-{}", "a".repeat(MAX_ARG_LEN));
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(long_name, "value".to_string())
            .build();

        assert!(result.is_err());
        if let Err(Error::SecurityViolation { reason }) = result {
            assert!(reason.contains("header name too long"));
        } else {
            panic!("expected SecurityViolation for oversized header name");
        }
    }

    #[test]
    fn test_validate_server_config_http_accepts_header_name_at_max_len() {
        let name_at_cap = "a".repeat(MAX_ARG_LEN);
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(name_at_cap, "value".to_string())
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_server_config_http_timeout_bounds_still_enforced() {
        let result = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .connect_timeout(std::time::Duration::ZERO)
            .build();

        assert!(result.is_err());
        if let Err(Error::ValidationError { field, .. }) = result {
            assert_eq!(field, "connect_timeout");
        } else {
            panic!("expected ValidationError for connect_timeout");
        }
    }
}
