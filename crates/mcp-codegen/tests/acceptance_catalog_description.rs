//! Acceptance test for #566 / #578-style injection defence: untrusted tool descriptions and
//! categorization short descriptions reach `catalog.ts` neutralized, end to end through
//! `generate_with_categories`.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod acceptance_support;

use acceptance_support::{file, server_info, tool};
use mcp_execution_codegen::ProgressiveGenerator;
use mcp_execution_codegen::progressive::ToolCategorization;
use serde_json::json;
use std::collections::HashMap;

const HOSTILE: &str =
    "marker */ export const pwn = 1; /* \u{1}\u{7}\u{1b}[31mred\u{2028}line\nsecond line\r\nthird";

fn catalog_for_hostile_descriptions() -> String {
    let mut described = tool("described_tool", json!({"type": "object"}), None);
    described.description = format!("{HOSTILE} {}", "y".repeat(5000));
    let categorized = tool("categorized_tool", json!({"type": "object"}), None);
    let categorizations = HashMap::from([(
        "categorized_tool".to_string(),
        ToolCategorization {
            category: "issues".to_string(),
            keywords: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            short_description: format!("{HOSTILE} {}", "z".repeat(5000)),
        },
    )]);
    let server = server_info(vec![described, categorized]);
    let code = {
        let config = mcp_execution_core::ServerConfig::builder()
            .command("test-command".to_string())
            .build()
            .unwrap();
        ProgressiveGenerator::new()
            .unwrap()
            .generate_with_categories(&server, &config, &categorizations)
            .expect("generation succeeds")
    };
    file(&code, "catalog.ts").to_string()
}

#[test]
fn catalog_descriptions_are_one_neutralized_control_free_comment_line_each() {
    let catalog = catalog_for_hostile_descriptions();

    assert!(!catalog.contains("*/ export const pwn"), "{catalog}");
    let comments: Vec<&str> = catalog
        .lines()
        .filter(|l| l.trim_start().starts_with("/**"))
        .collect();
    assert_eq!(comments.len(), 2, "one doc comment per tool: {catalog}");
    for comment in &comments {
        assert!(
            comment.contains("marker"),
            "the first line of the text is kept: {comment}"
        );
        assert_eq!(
            comment.matches("*/").count(),
            1,
            "only the closing terminator: {comment}"
        );
        assert!(comment.trim_end().ends_with("*/"), "{comment}");
        assert!(comment.len() < 400, "long text is cut: {} B", comment.len());
    }
    let bad: Vec<char> = catalog
        .chars()
        .filter(|c| (c.is_control() && *c != '\n') || *c == '\u{2028}' || *c == '\u{2029}')
        .collect();
    assert!(bad.is_empty(), "control characters survived: {bad:?}");
}
