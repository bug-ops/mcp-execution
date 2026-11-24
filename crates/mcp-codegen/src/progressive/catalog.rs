//! The top-level `catalog.ts`: one type-only signature per tool.
//!
//! The catalog is the second detail level between `index.ts` (names) and the per-tool files
//! (full types). It is a `.ts` file so the existing tsconfig type-checks it.

use crate::common::types::GeneratedFile;
use mcp_execution_core::Result;
use mcp_execution_core::metadata::CATALOG_FILE_NAME;
use mcp_execution_introspector::MAX_TOOL_COUNT;

/// Maximum size in bytes of one catalog entry; a larger entry degrades to a generic signature.
pub const CATALOG_ENTRY_MAX_BYTES: usize = 4096;

/// Maximum size in bytes of the catalog header.
pub const CATALOG_HEADER_MAX_BYTES: usize = 4096;

/// Maximum size in bytes of the whole catalog file.
#[cfg_attr(not(test), expect(dead_code, reason = "wired by Track 1 in this PR"))]
pub const MAX_CATALOG_BYTES: usize =
    CATALOG_HEADER_MAX_BYTES + MAX_TOOL_COUNT * CATALOG_ENTRY_MAX_BYTES;

/// Display text of one catalog entry; every string is already sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// The tool's TypeScript name; entries are sorted by it.
    pub typescript_name: String,
    /// One-line description.
    pub description: String,
    /// Params type in `Compact` form.
    pub params_type: String,
    /// Result type in `Compact` form.
    pub result_type: String,
    /// `@hints` text, if the server stated any hint.
    pub hints: Option<String>,
}

/// Renders `catalog.ts` for `server_name` from `entries`.
///
/// # Errors
///
/// Returns an error if rendering fails.
#[expect(
    clippy::unnecessary_wraps,
    reason = "stub with the final signature; Track 2 renders the entries and can fail"
)]
pub fn render_catalog(_server_name: &str, _entries: Vec<CatalogEntry>) -> Result<GeneratedFile> {
    Ok(GeneratedFile {
        path: CATALOG_FILE_NAME.to_string(),
        content: "export {};\n".to_string(),
    })
}

#[cfg(test)]
mod acceptance_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps every seam item reachable in test builds until Track 1 wires it.
    #[test]
    fn seam_surface_is_reachable() {
        let entry = CatalogEntry {
            typescript_name: "tool".to_string(),
            description: String::new(),
            params_type: "Record<string, unknown>".to_string(),
            result_type: "unknown".to_string(),
            hints: None,
        };
        assert_eq!(entry.hints, None);
        assert_eq!(entry.description, "");
        assert_ne!(entry.typescript_name, "");
        assert_ne!(entry.params_type, entry.result_type);
        assert_ne!(MAX_CATALOG_BYTES, CATALOG_HEADER_MAX_BYTES);
    }
}
