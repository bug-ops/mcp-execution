//! JSON Schema to TypeScript type conversion with bounds and diagnostics.
//!
//! [`convert_schema`] walks a schema once and yields a [`ConvertedType`] that renders in two
//! styles ([`RenderStyle`]): the multi-line, commented `Declaration` form used in tool files and
//! the one-line `Compact` form used in the catalog and stored texts.
//!
//! The converter never panics, never errors and never fetches a remote reference: every bound it
//! hits degrades the smallest enclosing node to `unknown` and records a [`SchemaDiagnostic`].

use crate::common::typescript::json_schema_to_typescript;
use mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES;
use serde_json::Value;

/// Maximum number of members rendered in one union before the node degrades.
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub(crate) const MAX_UNION_MEMBERS: usize = 64;

/// Maximum number of members rendered in one intersection before the node degrades.
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub(crate) const MAX_INTERSECTION_MEMBERS: usize = 8;

/// Maximum number of `$ref` expansions performed per root schema.
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub(crate) const MAX_REF_EXPANSIONS: usize = 64;

/// Maximum number of diagnostics recorded per conversion; further ones are only counted.
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub(crate) const MAX_DIAGNOSTICS: usize = 32;

/// Maximum number of characters kept in a [`SchemaPath`].
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub(crate) const MAX_SCHEMA_PATH_CHARS: usize = 256;

/// Maximum size in bytes of a rendered `Declaration` type before the converter degrades.
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub(crate) const MAX_RENDERED_TYPE_BYTES: usize = MAX_SCHEMA_SIZE_BYTES;

/// How a [`ConvertedType`] is rendered.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
/// use serde_json::json;
///
/// let converted = convert_schema(&json!({"type": "string"}));
/// assert_eq!(converted.render(RenderStyle::Compact), "string");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStyle {
    /// Multi-line with member `JSDoc`, for tool files.
    Declaration,
    /// One line without `JSDoc`, for the catalog and stored texts.
    Compact,
}

/// A JSON pointer into the converted schema, sanitized and capped at
/// [`MAX_SCHEMA_PATH_CHARS`] characters; the empty path is the schema root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaPath(pub(crate) String);

/// What went wrong at one [`SchemaPath`] during conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub(crate) enum DiagnosticKind {
    /// The node is not a schema object or uses an unsupported `type`.
    UnsupportedType,
    /// Nesting exceeded `MAX_SCHEMA_RECURSION_DEPTH`.
    DepthExceeded,
    /// A union exceeded [`MAX_UNION_MEMBERS`].
    UnionTooLarge,
    /// An intersection exceeded [`MAX_INTERSECTION_MEMBERS`].
    IntersectionTooLarge,
    /// More than [`MAX_REF_EXPANSIONS`] references were expanded.
    RefExpansionLimit,
    /// The rendered type exceeded [`MAX_RENDERED_TYPE_BYTES`].
    SizeExceeded,
    /// A reference cycle was cut.
    CyclicRef,
    /// A reference does not point into the root document.
    NonLocalRef,
    /// A local reference could not be resolved.
    UnresolvedRef,
    /// Sibling keywords next to a `$ref` were ignored.
    RefSiblingsIgnored,
    /// A narrowing-only keyword was ignored, keeping the superset type.
    NarrowingKeywordIgnored,
    /// Tuple `items` or `prefixItems` rendered as `unknown[]`.
    TupleItems,
    /// A string literal could not be represented exactly.
    LiteralNotRepresentable,
    /// A property key could not be represented exactly and was omitted.
    KeyNotRepresentable,
}

/// One conversion diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaDiagnostic {
    /// Where in the schema the problem occurred.
    pub(crate) path: SchemaPath,
    /// What the problem was.
    pub(crate) kind: DiagnosticKind,
}

/// One representable member of an object-like root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemberSummary {
    /// The raw schema key.
    pub(crate) name: String,
    /// The member's type in `Compact` form.
    pub(crate) type_text: String,
    /// Whether the member is required.
    pub(crate) required: bool,
}

/// The result of converting one JSON Schema to TypeScript.
///
/// Opaque: obtain one from [`convert_schema`] and render it with [`ConvertedType::render`].
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
/// use serde_json::json;
///
/// let converted = convert_schema(&json!({"type": "boolean"}));
/// assert_eq!(converted.render(RenderStyle::Declaration), "boolean");
/// ```
#[derive(Debug, Clone)]
pub struct ConvertedType {
    declaration: String,
    diagnostics: Vec<SchemaDiagnostic>,
    members: Vec<MemberSummary>,
}

impl ConvertedType {
    /// Renders the converted type in the requested style.
    ///
    /// `Compact` output has no newline and no `/**`, and is never longer than `Declaration`.
    #[must_use]
    pub fn render(&self, _style: RenderStyle) -> String {
        self.declaration.clone()
    }

    /// Diagnostics recorded during conversion, at most [`MAX_DIAGNOSTICS`].
    #[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
    pub(crate) fn diagnostics(&self) -> &[SchemaDiagnostic] {
        &self.diagnostics
    }

    /// Whether the root itself degraded, that is a diagnostic exists at the empty path.
    #[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
    pub(crate) fn root_degraded(&self) -> bool {
        self.diagnostics.iter().any(|d| d.path.0.is_empty())
    }

    /// Whether the root is an object, or a union or intersection of object-like members.
    #[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
    pub(crate) const fn is_object_like(&self) -> bool {
        !self.members.is_empty()
    }

    /// The root's representable members in declared order.
    #[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
    pub(crate) fn members(&self) -> &[MemberSummary] {
        &self.members
    }
}

/// Converts a JSON Schema to a [`ConvertedType`].
///
/// Never panics, never errors and never fetches a remote reference; the output is
/// deterministic.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
/// use serde_json::json;
///
/// let converted = convert_schema(&json!({"type": "number"}));
/// assert_eq!(converted.render(RenderStyle::Compact), "number");
/// ```
#[must_use]
pub fn convert_schema(schema: &Value) -> ConvertedType {
    ConvertedType {
        declaration: json_schema_to_typescript(schema),
        diagnostics: Vec::new(),
        members: Vec::new(),
    }
}

#[cfg(test)]
mod acceptance_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Keeps every seam item reachable in test builds until Track 1 wires it.
    #[test]
    fn seam_surface_is_reachable() {
        let converted = convert_schema(&json!({"type": "string"}));
        assert_eq!(converted.diagnostics().len(), 0);
        assert!(!converted.root_degraded());
        assert!(!converted.is_object_like());
        assert_eq!(converted.members().len(), 0);

        let all_kinds = [
            DiagnosticKind::UnsupportedType,
            DiagnosticKind::DepthExceeded,
            DiagnosticKind::UnionTooLarge,
            DiagnosticKind::IntersectionTooLarge,
            DiagnosticKind::RefExpansionLimit,
            DiagnosticKind::SizeExceeded,
            DiagnosticKind::CyclicRef,
            DiagnosticKind::NonLocalRef,
            DiagnosticKind::UnresolvedRef,
            DiagnosticKind::RefSiblingsIgnored,
            DiagnosticKind::NarrowingKeywordIgnored,
            DiagnosticKind::TupleItems,
            DiagnosticKind::LiteralNotRepresentable,
            DiagnosticKind::KeyNotRepresentable,
        ];
        let diagnostic = SchemaDiagnostic {
            path: SchemaPath(String::new()),
            kind: all_kinds[0],
        };
        let member = MemberSummary {
            name: String::new(),
            type_text: String::new(),
            required: false,
        };
        assert_eq!(member.name, member.type_text);
        assert!(!member.required);
        assert_eq!(diagnostic.kind, DiagnosticKind::UnsupportedType);

        let bounds = [
            MAX_UNION_MEMBERS,
            MAX_INTERSECTION_MEMBERS,
            MAX_REF_EXPANSIONS,
            MAX_DIAGNOSTICS,
            MAX_SCHEMA_PATH_CHARS,
            MAX_RENDERED_TYPE_BYTES,
        ];
        assert!(bounds.iter().all(|&bound| bound > 0));
    }
}
