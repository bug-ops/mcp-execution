//! Acceptance tests for #568 wiring: stated annotations reach the generated tool file as fixed
//! `JSDoc` tags, and unstated ones do not.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod acceptance_support;

use acceptance_support::{file, generate, server_info, tool};
use mcp_execution_core::annotations::{AnnotationTitle, Hint, ToolAnnotations};
use serde_json::json;

#[test]
fn tool_file_carries_tags_for_stated_hints_and_title_only() {
    let mut deleter = tool("delete_file", json!({"type": "object"}), None);
    deleter.annotations = Some({
        let mut value = ToolAnnotations::default();
        value.read_only = Hint::StatedFalse;
        value.destructive = Hint::StatedTrue;

        value.title = AnnotationTitle::new("Delete file");
        value
    });
    let plain = tool("plain_tool", json!({"type": "object"}), None);

    let code = generate(&server_info(vec![deleter, plain]));
    let annotated = file(&code, "deleteFile.ts");
    let unannotated = file(&code, "plainTool.ts");

    for expected in [
        "@title Delete file",
        "@readOnlyHint false",
        "@destructiveHint true",
    ] {
        assert!(
            annotated.contains(expected),
            "missing `{expected}` in:\n{annotated}"
        );
    }
    for absent in ["@idempotentHint", "@openWorldHint"] {
        assert!(
            !annotated.contains(absent),
            "`{absent}` was not stated:\n{annotated}"
        );
    }
    assert!(
        !unannotated.contains("Hint") && !unannotated.contains("@title"),
        "a tool without annotations carries no annotation tags:\n{unannotated}"
    );
}

#[test]
fn tool_file_tags_cannot_terminate_the_doc_comment() {
    let mut hostile = tool("hostile", json!({"type": "object"}), None);
    hostile.annotations = Some({
        let mut value = ToolAnnotations::default();
        value.read_only = Hint::StatedTrue;

        value.title = AnnotationTitle::new("evil */ export const pwn = 1; /*");
        value
    });

    let code = generate(&server_info(vec![hostile]));
    let tool_file = file(&code, "hostile.ts");

    assert!(tool_file.contains("@readOnlyHint true"), "{tool_file}");
    assert!(!tool_file.contains("*/ export const pwn"), "{tool_file}");
}
