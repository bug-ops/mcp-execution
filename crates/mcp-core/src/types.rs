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
