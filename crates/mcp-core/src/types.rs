//! Strong domain types for MCP Code Execution.
//!
//! This module implements the newtype pattern to provide type safety for
//! domain primitives, following ADR-003 (strong types over primitives).
//!
//! # Type Safety Benefits
//!
//! Using strong types instead of primitives prevents:
//! - Mixing up parameters of the same primitive type
//! - Invalid values being passed
//! - Accidental type conversions
//!
//! # Examples
//!
//! ```
//! use mcp_execution_core::{ServerId, ToolName};
//!
//! // Type-safe identifiers
//! let server = ServerId::new("my-server").unwrap();
//! let tool = ToolName::new("execute_code").unwrap();
//! ```

use crate::path::validate_path_segment;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Error returned when a candidate string fails the invariant [`ServerId::new`] enforces.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ServerId, ServerIdError};
///
/// let err = ServerId::new("../etc").unwrap_err();
/// assert!(matches!(err, ServerIdError::InvalidFormat { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServerIdError {
    /// `id` is not a single non-empty path segment: it is empty, or contains a `..`, a path
    /// separator, or a root/prefix component.
    #[error(
        "invalid server id {id:?}: must be a single non-empty path segment with no `..` or path separator"
    )]
    InvalidFormat {
        /// The rejected input.
        id: String,
    },
}

/// Server identifier (newtype over String).
///
/// Represents a unique identifier for an MCP server. Using a strong type
/// prevents accidentally mixing server IDs with other string values, and
/// [`ServerId::new`] enforces that every `ServerId` is safe to use as a single
/// path segment (see [`validate_path_segment`](crate::validate_path_segment)),
/// since server IDs are ultimately confined into filesystem paths (e.g.
/// `~/.claude/servers/{server_id}`).
///
/// This baseline invariant is deliberately looser than [`validate_server_id_slug`]'s: a
/// `ServerId` may contain uppercase letters, underscores, or other path-segment-safe
/// characters that are not valid in a filesystem-safe *slug*. If you need the id to become a
/// directory name or generated-code identifier — not just a safe path segment — validate it
/// with [`validate_server_id_slug`] too, in addition to constructing it here.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ServerId;
///
/// let id = ServerId::new("example-server").unwrap();
/// assert_eq!(id.as_str(), "example-server");
/// assert!(ServerId::new("../escape").is_err());
/// ```
///
/// [`Deserialize`] is routed through [`ServerId::new`] (via `#[serde(try_from = "String")]`),
/// so `serde_json::from_str::<ServerId>(...)` — or any struct with a `ServerId` field, such as
/// `mcp_execution_introspector::ServerInfo` — enforces the same invariant as calling `new`
/// directly; there is no separate, unvalidated deserialization path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ServerId(String);

impl TryFrom<String> for ServerId {
    type Error = ServerIdError;

    /// Delegates to [`ServerId::new`] — the sole entry point [`Deserialize`] uses.
    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

impl ServerId {
    /// Creates a new server identifier, validating that `id` is a single non-empty path
    /// segment with no `..` or path separator.
    ///
    /// # Errors
    ///
    /// Returns [`ServerIdError::InvalidFormat`] if `id` is empty, or contains a `..`, a path
    /// separator, or a root/prefix component.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerId;
    ///
    /// let id = ServerId::new("my-server").unwrap();
    /// let from_string = ServerId::new(String::from("my-server")).unwrap();
    /// assert_eq!(id, from_string);
    /// assert!(ServerId::new("").is_err());
    /// assert!(ServerId::new("a/b").is_err());
    /// ```
    #[inline]
    pub fn new(id: impl Into<String>) -> Result<Self, ServerIdError> {
        let id = id.into();
        if validate_path_segment(&id).is_none() {
            return Err(ServerIdError::InvalidFormat { id });
        }
        Ok(Self(id))
    }

    /// Returns the server ID as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerId;
    ///
    /// let id = ServerId::new("test").unwrap();
    /// assert_eq!(id.as_str(), "test");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `ServerId` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerId;
    ///
    /// let id = ServerId::new("test").unwrap();
    /// let inner: String = id.into_inner();
    /// assert_eq!(inner, "test");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Maximum length, in bytes, of a slug-shaped server id (see [`validate_server_id_slug`]).
pub const MAX_SERVER_ID_LENGTH: usize = 64;

/// Errors returned by [`validate_server_id_slug`].
///
/// Every message names the parameter as `server_id` (matching the MCP tool JSON field and this
/// crate's other `server_id`-named APIs) rather than "server id" — callers surface these
/// messages verbatim to end users/MCP clients, so wording here must stay actionable and
/// consistent with every other layer that reports the same rejection.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServerIdSlugError {
    /// The candidate was empty.
    #[error("server_id must not be empty")]
    Empty,

    /// The candidate exceeded [`MAX_SERVER_ID_LENGTH`].
    #[error("server_id too long: {len} chars exceeds {limit} limit")]
    TooLong {
        /// Actual length of the rejected id, in bytes (`str::len`, not `chars().count()`).
        len: usize,
        /// Maximum allowed length ([`MAX_SERVER_ID_LENGTH`]).
        limit: usize,
    },

    /// The candidate contained a character other than a lowercase ASCII letter, digit, or
    /// hyphen.
    #[error("server_id must contain only lowercase letters, digits, and hyphens")]
    InvalidCharacters,
}

/// Validates that `id` is a filesystem-safe server id *slug*: 1-64 lowercase ASCII letters,
/// digits, or hyphens (`^[a-z0-9-]+$`).
///
/// This is a stricter, opt-in invariant layered on top of [`ServerId::new`]'s own baseline
/// (single non-empty path segment, no `..` or path separator): every slug-shaped id is
/// automatically a valid [`ServerId`], but not every valid `ServerId` is slug-shaped — e.g. a
/// raw `mcp.json` key like `claude_ai_Gmail` (mixed case, underscore) is a legitimate
/// `ServerId` but not a valid slug. [`ServerId::new`] deliberately does not enforce this
/// tighter rule itself, since callers that only need a safe path segment (e.g. looking up an
/// existing `mcp.json` entry) must keep accepting non-slug-shaped ids. Callers that need the id
/// to become a directory name or generated-code identifier — where entry validation and
/// filesystem confinement must agree on the exact same rule — should call this function too,
/// in addition to [`ServerId::new`].
///
/// # Errors
///
/// Returns [`ServerIdSlugError`] if `id` is empty, exceeds [`MAX_SERVER_ID_LENGTH`] bytes, or
/// contains a character other than a lowercase ASCII letter, digit, or hyphen.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::validate_server_id_slug;
///
/// assert!(validate_server_id_slug("github").is_ok());
/// assert!(validate_server_id_slug("my-server-123").is_ok());
/// assert!(validate_server_id_slug("").is_err());
/// assert!(validate_server_id_slug("GitHub").is_err());
/// assert!(validate_server_id_slug("my_server").is_err());
/// ```
pub fn validate_server_id_slug(id: &str) -> Result<(), ServerIdSlugError> {
    if id.is_empty() {
        return Err(ServerIdSlugError::Empty);
    }
    if id.len() > MAX_SERVER_ID_LENGTH {
        return Err(ServerIdSlugError::TooLong {
            len: id.len(),
            limit: MAX_SERVER_ID_LENGTH,
        });
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ServerIdSlugError::InvalidCharacters);
    }
    Ok(())
}

/// Error returned when a candidate string fails the invariant [`ToolName::new`] enforces.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ToolName, ToolNameError};
///
/// let err = ToolName::new("").unwrap_err();
/// assert!(matches!(err, ToolNameError::InvalidFormat { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolNameError {
    /// `name` is not a single non-empty path segment: it is empty, or contains a `..`, a path
    /// separator, or a root/prefix component.
    #[error(
        "invalid tool name {name:?}: must be a single non-empty path segment with no `..` or path separator"
    )]
    InvalidFormat {
        /// The rejected input.
        name: String,
    },
}

/// Tool name identifier (newtype over String).
///
/// Represents the name of an MCP tool. Using a strong type ensures tool names are not
/// confused with other string values, and [`ToolName::new`] enforces the same baseline
/// shape as [`ServerId::new`] (see [`validate_path_segment`](crate::validate_path_segment)),
/// since generated tool names are ultimately used to derive output file names.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ToolName;
///
/// let tool = ToolName::new("execute_code").unwrap();
/// assert_eq!(tool.as_str(), "execute_code");
/// assert!(ToolName::new("").is_err());
/// ```
///
/// [`Deserialize`] is routed through [`ToolName::new`] (via `#[serde(try_from = "String")]`),
/// so `serde_json::from_str::<ToolName>(...)` — or any struct with a `ToolName` field, such as
/// `mcp_execution_introspector::ToolInfo` — enforces the same invariant as calling `new`
/// directly; there is no separate, unvalidated deserialization path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ToolName(String);

impl TryFrom<String> for ToolName {
    type Error = ToolNameError;

    /// Delegates to [`ToolName::new`] — the sole entry point [`Deserialize`] uses.
    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::new(name)
    }
}

impl ToolName {
    /// Creates a new tool name, validating that `name` is a single non-empty path segment
    /// with no `..` or path separator.
    ///
    /// # Errors
    ///
    /// Returns [`ToolNameError::InvalidFormat`] if `name` is empty, or contains a `..`, a
    /// path separator, or a root/prefix component.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ToolName;
    ///
    /// let name = ToolName::new("my_tool").unwrap();
    /// assert_eq!(name.as_str(), "my_tool");
    /// assert!(ToolName::new("a/b").is_err());
    /// ```
    #[inline]
    pub fn new(name: impl Into<String>) -> Result<Self, ToolNameError> {
        let name = name.into();
        if validate_path_segment(&name).is_none() {
            return Err(ToolNameError::InvalidFormat { name });
        }
        Ok(Self(name))
    }

    /// Returns the tool name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ToolName;
    ///
    /// let name = ToolName::new("test_tool").unwrap();
    /// assert_eq!(name.as_str(), "test_tool");
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `ToolName` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ToolName;
    ///
    /// let name = ToolName::new("tool").unwrap();
    /// let inner: String = name.into_inner();
    /// assert_eq!(inner, "tool");
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ServerId tests
    #[test]
    fn test_server_id_creation() {
        let id = ServerId::new("test-server").unwrap();
        assert_eq!(id.as_str(), "test-server");
    }

    #[test]
    fn test_server_id_into_inner() {
        let id = ServerId::new("test").unwrap();
        let inner = id.into_inner();
        assert_eq!(inner, "test");
    }

    #[test]
    fn test_server_id_display() {
        let id = ServerId::new("display-test").unwrap();
        assert_eq!(format!("{id}"), "display-test");
    }

    #[test]
    fn test_server_id_clone_eq() {
        let id1 = ServerId::new("same").unwrap();
        let id2 = id1.clone();
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_server_id_rejects_empty() {
        assert!(matches!(
            ServerId::new(""),
            Err(ServerIdError::InvalidFormat { .. })
        ));
    }

    #[test]
    fn test_server_id_rejects_parent_traversal() {
        assert!(ServerId::new("..").is_err());
        assert!(ServerId::new("../escape").is_err());
    }

    #[test]
    fn test_server_id_rejects_path_separator() {
        assert!(ServerId::new("a/b").is_err());
    }

    /// Regression guard: `#[serde(try_from = "String")]` must route `Deserialize` through
    /// `new`'s invariant, not just `From`/a bare `#[derive(Deserialize)]`. Without this, a
    /// struct holding a `ServerId` field (e.g. `mcp_execution_introspector::ServerInfo`) could
    /// deserialize an unvalidated id straight from JSON.
    #[test]
    fn test_server_id_deserialize_rejects_invalid_value() {
        let result: std::result::Result<ServerId, _> = serde_json::from_str(r#""../escape""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_server_id_deserialize_accepts_valid_value() {
        let id: ServerId = serde_json::from_str(r#""my-server""#).unwrap();
        assert_eq!(id.as_str(), "my-server");
    }

    // ToolName tests
    #[test]
    fn test_tool_name_creation() {
        let name = ToolName::new("send_message").unwrap();
        assert_eq!(name.as_str(), "send_message");
    }

    #[test]
    fn test_tool_name_into_inner() {
        let name = ToolName::new("test").unwrap();
        let inner = name.into_inner();
        assert_eq!(inner, "test");
    }

    #[test]
    fn test_tool_name_display() {
        let name = ToolName::new("display_test").unwrap();
        assert_eq!(format!("{name}"), "display_test");
    }

    #[test]
    fn test_tool_name_clone_eq() {
        let name1 = ToolName::new("same").unwrap();
        let name2 = name1.clone();
        assert_eq!(name1, name2);
    }

    #[test]
    fn test_tool_name_rejects_empty() {
        assert!(matches!(
            ToolName::new(""),
            Err(ToolNameError::InvalidFormat { .. })
        ));
    }

    #[test]
    fn test_tool_name_rejects_path_separator() {
        assert!(ToolName::new("a/b").is_err());
    }

    /// Mirrors `test_server_id_deserialize_rejects_invalid_value`: `ToolName`'s `Deserialize`
    /// must also route through `new`, not bypass it — see that test's doc comment.
    #[test]
    fn test_tool_name_deserialize_rejects_invalid_value() {
        let result: std::result::Result<ToolName, _> = serde_json::from_str(r#""a/b""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_name_deserialize_accepts_valid_value() {
        let name: ToolName = serde_json::from_str(r#""send_message""#).unwrap();
        assert_eq!(name.as_str(), "send_message");
    }

    #[test]
    fn test_server_id_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ServerId>();
        assert_sync::<ServerId>();
        assert_send::<ServerIdError>();
        assert_sync::<ServerIdError>();
    }

    // validate_server_id_slug tests
    #[test]
    fn test_validate_server_id_slug_valid() {
        assert!(validate_server_id_slug("github").is_ok());
        assert!(validate_server_id_slug("my-server").is_ok());
        assert!(validate_server_id_slug("server123").is_ok());
        assert!(validate_server_id_slug("my-server-123").is_ok());
    }

    #[test]
    fn test_validate_server_id_slug_empty() {
        assert_eq!(validate_server_id_slug(""), Err(ServerIdSlugError::Empty));
    }

    #[test]
    fn test_validate_server_id_slug_uppercase() {
        assert_eq!(
            validate_server_id_slug("GitHub"),
            Err(ServerIdSlugError::InvalidCharacters)
        );
    }

    #[test]
    fn test_validate_server_id_slug_underscore() {
        assert_eq!(
            validate_server_id_slug("my_server"),
            Err(ServerIdSlugError::InvalidCharacters)
        );
    }

    #[test]
    fn test_validate_server_id_slug_special_chars() {
        assert_eq!(
            validate_server_id_slug("my@server"),
            Err(ServerIdSlugError::InvalidCharacters)
        );
    }

    #[test]
    fn test_validate_server_id_slug_too_long() {
        let long_id = "a".repeat(65);
        assert_eq!(
            validate_server_id_slug(&long_id),
            Err(ServerIdSlugError::TooLong {
                len: 65,
                limit: MAX_SERVER_ID_LENGTH
            })
        );
    }

    #[test]
    fn test_validate_server_id_slug_max_length() {
        let max_id = "a".repeat(64);
        assert!(validate_server_id_slug(&max_id).is_ok());
    }

    /// A slug-shaped id must always also satisfy `ServerId::new`'s own (looser) baseline
    /// invariant — the slug charset is a strict subset of what a plain path segment allows.
    #[test]
    fn test_valid_slug_is_always_a_valid_server_id() {
        // Every single character in the slug charset, individually — not just a handful of
        // hand-picked compositions, so a future charset change to `validate_server_id_slug`
        // that quietly adds a character `ServerId::new` would reject (e.g. a path separator)
        // is caught here rather than only in a hand-picked example.
        const CHARSET: &str = "abcdefghijklmnopqrstuvwxyz0123456789-";
        for c in CHARSET.chars() {
            let single = c.to_string();
            assert!(validate_server_id_slug(&single).is_ok(), "{single:?}");
            assert!(ServerId::new(&single).is_ok(), "{single:?}");
        }
        for candidate in [
            "github",
            "my-server",
            "server123",
            "my-server-123",
            "-leading-hyphen",
            "trailing-hyphen-",
            "0123456789",
            &"a".repeat(MAX_SERVER_ID_LENGTH),
        ] {
            assert!(validate_server_id_slug(candidate).is_ok(), "{candidate:?}");
            assert!(ServerId::new(candidate).is_ok(), "{candidate:?}");
        }
    }

    /// The converse of the test above: `ServerId::new`'s baseline is strictly looser than
    /// `validate_server_id_slug`'s charset, not merely equal to it in practice. Mixed case and
    /// underscores are legitimate path segments (e.g. a raw `mcp.json` key like
    /// `claude_ai_Gmail`, issue #311) even though they fail the slug charset — this must keep
    /// holding after #401, which deliberately left `ServerId::new` untouched rather than
    /// tightening it to the slug rule.
    #[test]
    fn test_server_id_new_accepts_non_slug_shaped_baseline() {
        for candidate in ["GitHub", "my_server", "claude_ai_Gmail"] {
            assert!(validate_server_id_slug(candidate).is_err());
            assert!(ServerId::new(candidate).is_ok());
        }
    }

    #[test]
    fn test_server_id_slug_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ServerIdSlugError>();
        assert_sync::<ServerIdSlugError>();
    }

    #[test]
    fn test_tool_name_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ToolName>();
        assert_sync::<ToolName>();
        assert_send::<ToolNameError>();
        assert_sync::<ToolNameError>();
    }
}
