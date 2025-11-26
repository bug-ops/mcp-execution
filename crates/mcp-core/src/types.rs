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
//! use mcp_core::{ServerId, ToolName};
//!
//! // Type-safe identifiers
//! let server = ServerId::new("my-server");
//! let tool = ToolName::new("execute_code");
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

/// Server identifier (newtype over String).
///
/// Represents a unique identifier for an MCP server. Using a strong type
/// prevents accidentally mixing server IDs with other string values.
///
/// # Examples
///
/// ```
/// use mcp_core::ServerId;
///
/// let id = ServerId::new("example-server");
/// assert_eq!(id.as_str(), "example-server");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerId(String);

impl ServerId {
    /// Creates a new server identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ServerId;
    ///
    /// let id = ServerId::new("my-server");
    /// let from_string = ServerId::new(String::from("my-server"));
    /// assert_eq!(id, from_string);
    /// ```
    #[inline]
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the server ID as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ServerId;
    ///
    /// let id = ServerId::new("test");
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
    /// use mcp_core::ServerId;
    ///
    /// let id = ServerId::new("test");
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

impl From<String> for ServerId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ServerId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Tool name identifier (newtype over String).
///
/// Represents the name of an MCP tool. Using a strong type ensures
/// tool names are not confused with other string values.
///
/// # Examples
///
/// ```
/// use mcp_core::ToolName;
///
/// let tool = ToolName::new("execute_code");
/// assert_eq!(tool.as_str(), "execute_code");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolName(String);

impl ToolName {
    /// Creates a new tool name.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ToolName;
    ///
    /// let name = ToolName::new("my_tool");
    /// assert_eq!(name.as_str(), "my_tool");
    /// ```
    #[inline]
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the tool name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ToolName;
    ///
    /// let name = ToolName::new("test_tool");
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
    /// use mcp_core::ToolName;
    ///
    /// let name = ToolName::new("tool");
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

impl From<String> for ToolName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ToolName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ServerId tests
    #[test]
    fn test_server_id_creation() {
        let id = ServerId::new("test-server");
        assert_eq!(id.as_str(), "test-server");
    }

    #[test]
    fn test_server_id_from_string() {
        let id = ServerId::from("test".to_string());
        assert_eq!(id.as_str(), "test");
    }

    #[test]
    fn test_server_id_into_inner() {
        let id = ServerId::new("test");
        let inner = id.into_inner();
        assert_eq!(inner, "test");
    }

    #[test]
    fn test_server_id_display() {
        let id = ServerId::new("display-test");
        assert_eq!(format!("{id}"), "display-test");
    }

    #[test]
    fn test_server_id_clone_eq() {
        let id1 = ServerId::new("same");
        let id2 = id1.clone();
        assert_eq!(id1, id2);
    }

    // ToolName tests
    #[test]
    fn test_tool_name_creation() {
        let name = ToolName::new("send_message");
        assert_eq!(name.as_str(), "send_message");
    }

    #[test]
    fn test_tool_name_from_string() {
        let name = ToolName::from("tool".to_string());
        assert_eq!(name.as_str(), "tool");
    }

    #[test]
    fn test_tool_name_into_inner() {
        let name = ToolName::new("test");
        let inner = name.into_inner();
        assert_eq!(inner, "test");
    }

    #[test]
    fn test_tool_name_display() {
        let name = ToolName::new("display_test");
        assert_eq!(format!("{name}"), "display_test");
    }

    #[test]
    fn test_tool_name_clone_eq() {
        let name1 = ToolName::new("same");
        let name2 = name1.clone();
        assert_eq!(name1, name2);
    }

    #[test]
    fn test_server_id_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ServerId>();
        assert_sync::<ServerId>();
    }

    #[test]
    fn test_tool_name_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ToolName>();
        assert_sync::<ToolName>();
    }
}
