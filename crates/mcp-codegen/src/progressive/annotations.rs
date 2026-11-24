//! Rendering of tool annotations into generated TypeScript.
//!
//! Annotations are untrusted server-advertised hints. Nothing is emitted for a hint the server
//! did not state.

use mcp_execution_core::annotations::ToolAnnotations;

/// `JSDoc` tag lines (without the leading `*`) for the hints a server stated.
pub const fn jsdoc_tags(_annotations: &ToolAnnotations) -> Vec<String> {
    Vec::new()
}

/// One-line `@hints` text for the catalog, or `None` when no hint was stated.
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub const fn catalog_hints(_annotations: &ToolAnnotations) -> Option<String> {
    None
}

#[cfg(test)]
mod acceptance_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::annotations::Hint;

    /// Keeps the stub reachable in test builds until Track 1 wires it.
    #[test]
    fn seam_surface_is_reachable() {
        let annotations = ToolAnnotations {
            title: None,
            read_only: Hint::NotStated,
            destructive: Hint::NotStated,
            idempotent: Hint::NotStated,
            open_world: Hint::NotStated,
        };
        assert_eq!(jsdoc_tags(&annotations).len(), 0);
        drop(catalog_hints(&annotations));
    }
}
