//! Acceptance tests for #566: `render_catalog` output shape, ordering, bounds and degradation.

use super::*;

fn entry(name: &str, params: &str, result: &str) -> CatalogEntry {
    CatalogEntry {
        typescript_name: name.to_string(),
        description: format!("Does {name}"),
        params_type: params.to_string(),
        result_type: result.to_string(),
        hints: None,
    }
}

fn rendered(entries: Vec<CatalogEntry>) -> String {
    render_catalog("acme", entries)
        .expect("catalog renders")
        .content
}

fn signature_names(content: &str, names: &[&str]) -> Vec<usize> {
    names
        .iter()
        .map(|n| {
            content
                .find(&format!("{n}(params:"))
                .unwrap_or_else(|| panic!("entry {n} missing from catalog:\n{content}"))
        })
        .collect()
}

#[test]
fn render_catalog_path_is_the_catalog_file_name() {
    let file = render_catalog("acme", vec![entry("a", "{}", "unknown")]).unwrap();
    assert_eq!(file.path, CATALOG_FILE_NAME);
    assert!(
        file.content.contains("export interface Catalog"),
        "{}",
        file.content
    );
}

#[test]
fn render_catalog_entries_are_sorted_by_name_one_per_input_and_deterministic() {
    let make = || {
        vec![
            entry("zeta", "{ q: string }", "string"),
            entry("alpha", "{ n?: number }", "unknown"),
            entry("mid", "Record<string, unknown>", "unknown"),
        ]
    };
    let content = rendered(make());

    let positions = signature_names(&content, &["alpha", "mid", "zeta"]);
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{content}");
    assert_eq!(content.matches("Promise<").count(), 3);
    assert_eq!(content, rendered(make()), "byte-identical across runs");
}

#[test]
fn render_catalog_entry_is_a_typed_method_signature() {
    let content = rendered(vec![entry(
        "createIssue",
        "{ title: string }",
        "{ id: number } | undefined",
    )]);

    assert!(
        content.contains(
            "createIssue(params: { title: string }): Promise<{ id: number } | undefined>;"
        ),
        "{content}"
    );
}

#[test]
fn render_catalog_imports_nothing() {
    let content = rendered(vec![entry("a", "{}", "unknown")]);

    assert!(
        !content
            .lines()
            .any(|l| l.trim_start().starts_with("import ")),
        "{content}"
    );
    assert!(content.contains("export interface Catalog"));
}

#[test]
fn render_catalog_header_names_the_three_detail_levels() {
    let content = rendered(vec![entry("a", "{}", "unknown")]);
    let header = content.split("export interface Catalog").next().unwrap();

    for level in ["index.ts", "catalog.ts", "<tool>.ts"] {
        assert!(
            header.contains(level),
            "header must name {level}:\n{header}"
        );
    }
    assert!(header.len() <= CATALOG_HEADER_MAX_BYTES);
}

#[test]
fn render_catalog_description_stays_on_one_short_comment_line() {
    let mut e = entry("a", "{}", "unknown");
    e.description = format!("first line\nsecond line {}", "x".repeat(400));
    let content = rendered(vec![e]);

    assert_eq!(content.matches("/**").count(), 1, "{content}");
    let comment = content
        .lines()
        .find(|l| l.contains("first line"))
        .expect("the description's first line is kept");
    assert!(
        comment.contains("*/"),
        "comment closes on its own line: {comment}"
    );
    assert!(
        !comment.contains("second line") || comment.len() <= 160,
        "{comment}"
    );
    assert!(
        !content.contains(&"x".repeat(400)),
        "long descriptions are truncated"
    );
}

#[test]
fn render_catalog_hints_appear_only_for_entries_that_have_them() {
    let mut with_hints = entry("a", "{}", "unknown");
    with_hints.hints = Some("@hints readOnly=true destructive=false".to_string());
    let content = rendered(vec![with_hints, entry("b", "{}", "unknown")]);

    assert_eq!(content.matches("@hints").count(), 1, "{content}");
    assert!(content.contains("@hints readOnly=true destructive=false"));
}

#[test]
fn render_catalog_oversized_entry_degrades_to_generic_signature_and_is_not_dropped() {
    let huge = format!(
        "{{ {} }}",
        "field: string; ".repeat(CATALOG_ENTRY_MAX_BYTES)
    );
    let content = rendered(vec![
        entry("big", &huge, "unknown"),
        entry("small", "{ q: string }", "unknown"),
    ]);

    assert!(
        content.contains("big(params: Record<string, unknown>): Promise<unknown>;"),
        "{content}"
    );
    assert!(content.contains("@see ./big.ts"), "{content}");
    assert!(content.contains("small(params: { q: string }): Promise<unknown>;"));
    assert!(content.len() < CATALOG_HEADER_MAX_BYTES + 2 * CATALOG_ENTRY_MAX_BYTES);
}

#[test]
fn render_catalog_with_the_maximum_tool_count_stays_within_the_file_bound() {
    let huge = "x".repeat(2 * CATALOG_ENTRY_MAX_BYTES);
    let entries: Vec<CatalogEntry> = (0..MAX_TOOL_COUNT)
        .map(|i| entry(&format!("tool{i:04}"), &huge, &huge))
        .collect();

    let content = rendered(entries);

    assert_eq!(content.matches("Promise<").count(), MAX_TOOL_COUNT);
    assert!(
        content.len() <= MAX_CATALOG_BYTES,
        "{} bytes",
        content.len()
    );
}
