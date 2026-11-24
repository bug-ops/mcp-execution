//! Rendering of tool annotations into generated TypeScript.
//!
//! Annotations are untrusted server-advertised hints. Nothing is emitted for a hint the server
//! did not state, and contradictory hints are rendered as advertised.

use crate::common::typescript::sanitize_jsdoc;
use mcp_execution_core::annotations::{Hint, ToolAnnotations};

/// Display names of the four hints, in the fixed order they are emitted.
const fn hint_entries(annotations: &ToolAnnotations) -> [(&'static str, Hint); 4] {
    [
        ("readOnly", annotations.read_only),
        ("destructive", annotations.destructive),
        ("idempotent", annotations.idempotent),
        ("openWorld", annotations.open_world),
    ]
}

const fn stated(hint: Hint) -> Option<bool> {
    match hint {
        Hint::StatedTrue => Some(true),
        Hint::StatedFalse => Some(false),
        Hint::NotStated => None,
    }
}

/// `JSDoc` tag lines (without the leading `*`) for the title and the hints a server stated.
///
/// The title goes through `sanitize_jsdoc`, so it cannot close the comment.
pub fn jsdoc_tags(annotations: &ToolAnnotations) -> Vec<String> {
    let title = annotations
        .title
        .as_ref()
        .map(|title| format!("@title {}", sanitize_jsdoc(title.as_str(), 256)));
    let hints = hint_entries(annotations)
        .into_iter()
        .filter_map(|(name, hint)| stated(hint).map(|value| format!("@{name}Hint {value}")));
    title.into_iter().chain(hints).collect()
}

/// One-line `@hints` text for the catalog, or `None` when no hint was stated.
pub fn catalog_hints(annotations: &ToolAnnotations) -> Option<String> {
    let stated_hints: Vec<String> = hint_entries(annotations)
        .into_iter()
        .filter_map(|(name, hint)| stated(hint).map(|value| format!("{name}={value}")))
        .collect();
    (!stated_hints.is_empty()).then(|| format!("@hints {}", stated_hints.join(" ")))
}

#[cfg(test)]
mod acceptance_tests;
