//! MCP tool annotations as captured from a server and carried through generation.
//!
//! An MCP server may advertise behavioural hints for each tool (`readOnlyHint`,
//! `destructiveHint`, `idempotentHint`, `openWorldHint`) and a display title. These are
//! **untrusted, advisory** values: the server is the only source and nothing here verifies
//! them. This module gives them a shared, strongly typed shape so that introspection
//! (producer), code generation and the `_meta.json` sidecar (carriers) and the skill and
//! server crates (consumers) agree on one representation.
//!
//! Each hint is three-valued ([`Hint`]): a server that says nothing is distinguishable from a
//! server that explicitly says `false`.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_core::annotations::{AnnotationTitle, Hint, ToolAnnotations};
//!
//! let annotations = { let mut value = ToolAnnotations::new(Hint::StatedFalse, Hint::StatedTrue, Hint::NotStated, Hint::from(Some(false))); value.title = AnnotationTitle::new("Delete file"); value };
//!
//! assert_eq!(annotations.destructive, Hint::StatedTrue);
//! assert_eq!(annotations.idempotent, Hint::NotStated);
//! ```

use crate::untrusted::{MAX_UNTRUSTED_FIELD_LEN, sanitize_untrusted_text};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A boolean annotation hint that distinguishes "the server said `false`" from "the server
/// said nothing".
///
/// # Examples
///
/// ```
/// use mcp_execution_core::annotations::Hint;
///
/// assert_eq!(Hint::from(Some(true)), Hint::StatedTrue);
/// assert_eq!(Hint::from(Some(false)), Hint::StatedFalse);
/// assert_eq!(Hint::from(None), Hint::NotStated);
/// ```
///
/// This is a closed set: adding a variant is a major change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hint {
    /// The server explicitly set the hint to `true`.
    StatedTrue,
    /// The server explicitly set the hint to `false`.
    StatedFalse,
    /// The server did not state the hint.
    NotStated,
}

impl From<Option<bool>> for Hint {
    fn from(value: Option<bool>) -> Self {
        match value {
            Some(true) => Self::StatedTrue,
            Some(false) => Self::StatedFalse,
            None => Self::NotStated,
        }
    }
}

/// Error returned when a raw title is empty once sanitized.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("annotation title is empty after sanitization")]
pub struct AnnotationTitleError;

/// A tool display title that has been through
/// [`sanitize_untrusted_text`] and is therefore safe to store and display.
///
/// The inner string is private so a value that skipped sanitization cannot exist: the
/// constructor and the deserializer both re-sanitize, and neither yields an empty title.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::annotations::AnnotationTitle;
///
/// let title = AnnotationTitle::new("Create issue").unwrap();
/// assert_eq!(title.as_str(), "Create issue");
/// assert!(AnnotationTitle::new("").is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AnnotationTitle(String);

impl AnnotationTitle {
    /// Sanitizes `raw` and wraps it, or returns `None` when nothing remains.
    #[must_use]
    pub fn new(raw: &str) -> Option<Self> {
        let sanitized = sanitize_untrusted_text(raw, MAX_UNTRUSTED_FIELD_LEN);
        (!sanitized.is_empty()).then_some(Self(sanitized))
    }

    /// Returns the sanitized title text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AnnotationTitle {
    type Error = AnnotationTitleError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value).ok_or(AnnotationTitleError)
    }
}

impl From<AnnotationTitle> for String {
    fn from(value: AnnotationTitle) -> Self {
        value.0
    }
}

/// The annotations an MCP server advertised for one tool.
///
/// Every field value is legal and there is deliberately no `Default`: the absence of
/// annotations is `Option::<ToolAnnotations>::None` on the carrying type, never an
/// all-[`Hint::NotStated`] value. Values are advisory and untrusted.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::annotations::{Hint, ToolAnnotations};
///
/// let annotations = ToolAnnotations::new(
///     Hint::StatedTrue,
///     Hint::NotStated,
///     Hint::NotStated,
///     Hint::NotStated,
/// );
///
/// let json = serde_json::to_string(&annotations).unwrap();
/// let back: ToolAnnotations = serde_json::from_str(&json).unwrap();
/// assert_eq!(back, annotations);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolAnnotations {
    /// Display title, from `Tool.title` falling back to `annotations.title`.
    pub title: Option<AnnotationTitle>,
    /// `readOnlyHint`: the tool does not modify its environment.
    pub read_only: Hint,
    /// `destructiveHint`: the tool may perform destructive updates.
    pub destructive: Hint,
    /// `idempotentHint`: repeated calls with the same arguments have no further effect.
    pub idempotent: Hint,
    /// `openWorldHint`: the tool interacts with an open set of external entities.
    pub open_world: Hint,
}

impl ToolAnnotations {
    /// Creates annotations from the four hints, with no display title; set [`Self::title`]
    /// afterwards when the server supplied one.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::annotations::{Hint, ToolAnnotations};
    ///
    /// let annotations = ToolAnnotations::new(
    ///     Hint::StatedTrue,
    ///     Hint::NotStated,
    ///     Hint::NotStated,
    ///     Hint::NotStated,
    /// );
    /// assert!(annotations.title.is_none());
    /// ```
    #[must_use]
    pub const fn new(
        read_only: Hint,
        destructive: Hint,
        idempotent: Hint,
        open_world: Hint,
    ) -> Self {
        Self {
            title: None,
            read_only,
            destructive,
            idempotent,
            open_world,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AnnotationTitle, Hint, ToolAnnotations};

    #[test]
    fn title_deserialization_rejects_empty_after_sanitization() {
        let result: Result<AnnotationTitle, _> = serde_json::from_str("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn title_deserialization_resanitizes() {
        let title: AnnotationTitle = serde_json::from_str("\"a\\u0007b\"").unwrap();
        assert!(!title.as_str().contains('\u{7}'));
    }

    #[test]
    fn annotations_round_trip_keeps_three_states_distinct() {
        let annotations = ToolAnnotations {
            title: AnnotationTitle::new("t"),
            read_only: Hint::StatedFalse,
            destructive: Hint::StatedTrue,
            idempotent: Hint::NotStated,
            open_world: Hint::StatedFalse,
        };
        let json = serde_json::to_string(&annotations).unwrap();
        let back: ToolAnnotations = serde_json::from_str(&json).unwrap();
        assert_eq!(back, annotations);
    }
}
