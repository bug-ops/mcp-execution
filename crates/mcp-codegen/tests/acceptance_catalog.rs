//! Acceptance tests for #566 (the typed `catalog.ts`) and its #568 hints, observed through
//! `generate`.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod acceptance_support;
mod support;

use acceptance_support::{file, generate, server_info, tool, write_package};
use mcp_execution_core::annotations::{AnnotationTitle, Hint, ToolAnnotations};
use mcp_execution_introspector::ToolInfo;
use serde_json::json;
use std::process::Command;
use support::{install_declared_dev_dependencies, require_ts_toolchain, tsc_program};

const NAMES: [&str; 16] = [
    "search_code",
    "create_issue",
    "get_issue",
    "list_issues",
    "update_issue",
    "close_issue",
    "add_comment",
    "list_comments",
    "get_user",
    "list_repos",
    "create_branch",
    "merge_pull",
    "list_commits",
    "get_file",
    "delete_file",
    "star_repo",
];

fn typical_tool(name: &str) -> ToolInfo {
    tool(
        name,
        json!({
            "type": "object",
            "properties": {
                "repo": {"type": "string", "description": "Repository name"},
                "title": {"type": "string"},
                "page": {"type": "integer"}
            },
            "required": ["repo"]
        }),
        None,
    )
}

fn sixteen_tools() -> Vec<ToolInfo> {
    NAMES.iter().rev().map(|n| typical_tool(n)).collect()
}

fn catalog_of(tools: Vec<ToolInfo>) -> String {
    file(&generate(&server_info(tools)), "catalog.ts").to_string()
}

fn entry_sizes(catalog: &str) -> Vec<usize> {
    let body = catalog
        .split("export interface Catalog {")
        .nth(1)
        .unwrap_or_else(|| panic!("no Catalog interface in:\n{catalog}"));
    let mut sizes = Vec::new();
    let mut current = String::new();
    for line in body.lines().map(str::trim).take_while(|l| *l != "}") {
        if line.is_empty() {
            continue;
        }
        current.push_str(line);
        current.push('\n');
        if line.ends_with(';') {
            sizes.push(current.len());
            current.clear();
        }
    }
    sizes
}

fn signature_names(catalog: &str) -> Vec<String> {
    catalog
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("(params:") && !l.starts_with("/**") && !l.starts_with('*'))
        .map(|l| l.split('(').next().unwrap().to_string())
        .collect()
}

#[test]
fn catalog_has_one_sorted_type_only_entry_per_tool_within_the_size_target() {
    let code = generate(&server_info(sixteen_tools()));
    assert_eq!(
        code.files.iter().filter(|f| f.path == "catalog.ts").count(),
        1,
        "exactly one catalog.ts"
    );
    let catalog = file(&code, "catalog.ts");

    let names = signature_names(catalog);
    assert_eq!(names.len(), 16, "{catalog}");
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "entries are sorted by TypeScript name");
    assert!(
        !catalog
            .lines()
            .any(|l| l.trim_start().starts_with("import ")),
        "the catalog imports nothing"
    );
    let mut sizes = entry_sizes(catalog);
    sizes.sort_unstable();
    assert_eq!(sizes.len(), 16);
    assert!(sizes[8] <= 250, "median entry is {} B", sizes[8]);
    for level in ["index.ts", "catalog.ts"] {
        assert!(catalog.contains(level), "header must name {level}");
    }
}

#[test]
fn catalog_is_byte_identical_across_runs_and_tool_order() {
    let forward: Vec<ToolInfo> = NAMES.iter().map(|n| typical_tool(n)).collect();

    let first = catalog_of(forward.clone());
    let second = catalog_of(sixteen_tools());

    assert_eq!(signature_names(&first).len(), 16, "{first}");
    assert_eq!(first, second);
    assert_eq!(first, catalog_of(forward));
}

#[test]
fn catalog_degrades_an_oversized_tool_without_dropping_it() {
    let mut properties = serde_json::Map::new();
    for i in 0..300 {
        properties.insert(format!("field_{i:04}"), json!({"type": "string"}));
    }
    let huge = tool(
        "huge_tool",
        json!({"type": "object", "properties": properties}),
        None,
    );
    let mut tools = sixteen_tools();
    tools.push(huge);

    let catalog = catalog_of(tools);

    let names = signature_names(&catalog);
    assert_eq!(names.len(), 17, "no tool is dropped: {names:?}");
    assert!(
        catalog.contains("hugeTool(params: Record<string, unknown>): Promise<unknown>;"),
        "{catalog}"
    );
    assert!(catalog.contains("@see ./hugeTool.ts"));
    assert!(
        !catalog.contains("field_0001"),
        "the oversized type text is not embedded"
    );
}

#[test]
fn catalog_entries_show_declared_results_undefined_and_annotation_hints() {
    let mut deleter = tool(
        "delete_file",
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
        Some(
            json!({"type": "object", "properties": {"deleted": {"type": "boolean"}}, "required": ["deleted"]}),
        ),
    );
    deleter.annotations = Some(ToolAnnotations {
        title: AnnotationTitle::new("Delete file"),
        read_only: Hint::StatedFalse,
        destructive: Hint::StatedTrue,
        idempotent: Hint::NotStated,
        open_world: Hint::NotStated,
    });
    let catalog = catalog_of(vec![deleter, typical_tool("plain_tool")]);

    let delete_entry = catalog
        .lines()
        .find(|l| l.contains("deleteFile(params:"))
        .unwrap_or_else(|| panic!("{catalog}"));
    assert!(
        delete_entry.contains("params: { path: string; [key: string]: unknown }"),
        "{delete_entry}"
    );
    assert!(delete_entry.contains("deleted: boolean"), "{delete_entry}");
    assert!(
        catalog.contains("@hints readOnly=false destructive=true"),
        "{catalog}"
    );
    assert_eq!(
        catalog.matches("@hints").count(),
        1,
        "hints only for the tool that stated them"
    );
    let plain_entry = catalog
        .lines()
        .find(|l| l.contains("plainTool(params:"))
        .unwrap_or_else(|| panic!("{catalog}"));
    assert!(
        plain_entry.ends_with("| undefined>;"),
        "an empty success is visible in every type: {plain_entry}"
    );
}

#[test]
fn catalog_tool_names_are_disambiguated_from_the_catalog_file() {
    let tools = vec![
        typical_tool("catalog"),
        typical_tool("Catalog"),
        typical_tool("index"),
        typical_tool("other"),
    ];
    let code = generate(&server_info(tools));

    let paths: Vec<String> = code
        .files
        .iter()
        .map(|f| f.path.to_ascii_lowercase())
        .collect();
    let mut unique = paths.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        paths.len(),
        unique.len(),
        "case-insensitively unique paths: {paths:?}"
    );
    assert_eq!(paths.iter().filter(|p| *p == "catalog.ts").count(), 1);
    assert!(file(&code, "catalog.ts").contains("export interface Catalog"));
    assert_eq!(code.file_count(), 4 + 7);
    let names = signature_names(file(&code, "catalog.ts"));
    assert_eq!(names.len(), 4, "{names:?}");
}

#[test]
fn catalog_passes_tsc_with_the_rest_of_the_package() {
    let test_name = "catalog_passes_tsc_with_the_rest_of_the_package";
    let code = generate(&server_info(sixteen_tools()));
    assert_eq!(signature_names(file(&code, "catalog.ts")).len(), 16);
    if !require_ts_toolchain(test_name, true) {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    write_package(&code, dir.path());
    if !install_declared_dev_dependencies(dir.path(), test_name) {
        return;
    }
    let output = Command::new(tsc_program())
        .arg("--noEmit")
        .arg("-p")
        .arg(dir.path().join("tsconfig.json"))
        .output()
        .expect("run tsc");

    assert!(
        output.status.success(),
        "tsc failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
