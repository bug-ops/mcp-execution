//! The top-level `catalog.ts`: one type-only signature per tool.
//!
//! The catalog is the second detail level between `index.ts` (names) and the per-tool files
//! (full types). It is a `.ts` file, not a `.d.ts`, so the existing tsconfig type-checks it
//! (`skipLibCheck` would skip a declaration file), and it sits at the top level next to
//! `index.ts` so a directory listing shows the detail levels side by side.

use crate::common::types::GeneratedFile;
use mcp_execution_core::metadata::CATALOG_FILE_NAME;
use mcp_execution_core::{Error, ResourceKind, Result};
use mcp_execution_introspector::MAX_TOOL_COUNT;

/// Maximum size in bytes of one catalog entry; a larger entry degrades to a generic signature.
pub const CATALOG_ENTRY_MAX_BYTES: usize = 4096;

/// Maximum size in bytes of the catalog header.
pub const CATALOG_HEADER_MAX_BYTES: usize = 4096;

/// Maximum size in bytes of the whole catalog file.
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

/// Longest description kept in a catalog entry's comment, in characters. Callers sanitize and
/// cap descriptions to this length before building a [`CatalogEntry`].
pub const CATALOG_DESCRIPTION_CHARS: usize = 120;

/// Signature parts of an entry that no longer fits [`CATALOG_ENTRY_MAX_BYTES`].
const GENERIC_PARAMS: &str = "Record<string, unknown>";
const GENERIC_RESULT: &str = "unknown";

fn header(server_name: &str) -> String {
    format!(
        "// Catalog of the tools of server \"{server_name}\": one type-only signature per tool.\n\
         //\n\
         // Detail levels, cheapest first:\n\
         //   1. index.ts   - the tool names\n\
         //   2. catalog.ts - this file: the parameter and result type of every tool\n\
         //   3. <tool>.ts  - one tool in full: documentation, types and the call\n\
         //\n\
         // Read the next level only for the tools you need. Nothing in this file is executable.\n\n"
    )
}

/// Description cut to its first line and [`CATALOG_DESCRIPTION_CHARS`], so one entry never
/// spans several comment lines even for a caller that skipped sanitizing.
fn one_line_description(description: &str) -> String {
    description
        .chars()
        .take_while(|c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}'))
        .take(CATALOG_DESCRIPTION_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

fn format_entry(name: &str, comment_parts: &[&str], params: &str, result: &str) -> String {
    let text = comment_parts
        .iter()
        .copied()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let comment = if text.is_empty() {
        String::new()
    } else {
        format!("  /** {text} */\n")
    };
    format!("{comment}  {name}(params: {params}): Promise<{result}>;\n")
}

fn render_entry(entry: &CatalogEntry) -> String {
    let description = one_line_description(&entry.description);
    let hints = entry.hints.as_deref().unwrap_or_default();
    let full = format_entry(
        &entry.typescript_name,
        &[&description, hints],
        &entry.params_type,
        &entry.result_type,
    );
    if full.len() <= CATALOG_ENTRY_MAX_BYTES {
        return full;
    }

    let see = format!("@see ./{}.ts", entry.typescript_name);
    format_entry(
        &entry.typescript_name,
        &[&description, &see],
        GENERIC_PARAMS,
        GENERIC_RESULT,
    )
}

/// Renders `catalog.ts` for `server_name` from `entries`.
///
/// The file is a header naming the three detail levels followed by `export interface Catalog`
/// with one method signature per entry, sorted by TypeScript name. An entry whose text would
/// exceed [`CATALOG_ENTRY_MAX_BYTES`] degrades to the generic signature with a `@see` pointer
/// to its tool file; no entry is ever dropped, so the file stays within [`MAX_CATALOG_BYTES`]
/// for at most `MAX_TOOL_COUNT` entries. The output imports nothing and is deterministic.
///
/// `server_name` and every entry string must already be sanitized display text.
///
/// # Errors
///
/// Returns [`Error::ResourceLimitExceeded`] if the rendered file would exceed
/// [`MAX_CATALOG_BYTES`], which only happens for more than `MAX_TOOL_COUNT` entries or an
/// oversized `server_name`.
pub fn render_catalog(server_name: &str, mut entries: Vec<CatalogEntry>) -> Result<GeneratedFile> {
    entries.sort_by(|a, b| a.typescript_name.cmp(&b.typescript_name));

    let mut content = header(server_name);
    content.push_str("export interface Catalog {\n");
    for entry in &entries {
        content.push_str(&render_entry(entry));
    }
    content.push_str("}\n");

    if content.len() > MAX_CATALOG_BYTES {
        return Err(Error::ResourceLimitExceeded {
            resource: ResourceKind::GeneratedOutputSize,
            actual: content.len(),
            limit: MAX_CATALOG_BYTES,
        });
    }

    Ok(GeneratedFile {
        path: CATALOG_FILE_NAME.to_string(),
        content,
    })
}

#[cfg(test)]
mod acceptance_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(description: &str, hints: Option<&str>) -> CatalogEntry {
        CatalogEntry {
            typescript_name: "tool".to_string(),
            description: description.to_string(),
            params_type: "{ a: string }".to_string(),
            result_type: "unknown".to_string(),
            hints: hints.map(str::to_string),
        }
    }

    #[test]
    fn render_catalog_rejects_more_entries_than_the_file_bound_allows() {
        let entries: Vec<CatalogEntry> = (0..MAX_TOOL_COUNT + 100)
            .map(|i| CatalogEntry {
                typescript_name: format!("tool{i:04}"),
                description: String::new(),
                params_type: "x".repeat(CATALOG_ENTRY_MAX_BYTES - 64),
                result_type: String::new(),
                hints: None,
            })
            .collect();

        assert!(matches!(
            render_catalog("acme", entries),
            Err(Error::ResourceLimitExceeded { .. })
        ));
    }

    #[test]
    fn render_entry_cuts_the_description_at_the_first_line_and_the_character_cap() {
        let rendered = render_entry(&entry(&format!("first\nsecond {}", "x".repeat(300)), None));

        assert!(rendered.contains("/** first */"), "{rendered}");

        let long = render_entry(&entry(&"y".repeat(300), None));
        assert!(
            long.contains(&"y".repeat(CATALOG_DESCRIPTION_CHARS)),
            "{long}"
        );
        assert!(
            !long.contains(&"y".repeat(CATALOG_DESCRIPTION_CHARS + 1)),
            "{long}"
        );
    }

    #[test]
    fn render_entry_without_description_or_hints_has_no_comment() {
        assert_eq!(
            render_entry(&entry("", None)),
            "  tool(params: { a: string }): Promise<unknown>;\n"
        );
    }

    #[test]
    fn render_entry_degraded_form_keeps_description_and_points_to_the_tool_file() {
        let mut oversized = entry("Does a thing", Some("@hints readOnly=true"));
        oversized.params_type = "x".repeat(CATALOG_ENTRY_MAX_BYTES);

        assert_eq!(
            render_entry(&oversized),
            "  /** Does a thing @see ./tool.ts */\n  tool(params: Record<string, unknown>): Promise<unknown>;\n"
        );
    }
}
