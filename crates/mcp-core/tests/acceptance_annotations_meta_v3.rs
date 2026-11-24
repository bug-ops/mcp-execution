//! Acceptance tests for #568: annotations in the provenance tool digest.
//!
//! Types, serde shape and the schema-version constant are delivered by the W0 seam and are
//! type-enforced; what remains observable here is the digest preimage rule: stated hints are
//! hashed with three-valued framing and the title is not.

use mcp_execution_core::annotations::{AnnotationTitle, Hint, ToolAnnotations};
use mcp_execution_core::provenance::{ToolDigest, ToolDigestEntry};
use serde_json::{Value, json};

fn annotations(title: Option<&str>, read_only: Hint, destructive: Hint) -> ToolAnnotations {
    ToolAnnotations {
        title: title.and_then(AnnotationTitle::new),
        read_only,
        destructive,
        idempotent: Hint::NotStated,
        open_world: Hint::NotStated,
    }
}

fn digest_with(schema: &Value, annotations: Option<&ToolAnnotations>) -> ToolDigest {
    ToolDigest::compute(&[ToolDigestEntry {
        name: "delete_file",
        description: "Deletes a file",
        input_schema: schema,
        output_schema: None,
        annotations,
    }])
}

#[test]
fn tool_digest_changes_when_only_a_hint_changes() {
    let schema = json!({"type": "object"});
    let read_only = annotations(None, Hint::StatedTrue, Hint::NotStated);
    let writes = annotations(None, Hint::StatedFalse, Hint::NotStated);

    assert_ne!(
        digest_with(&schema, Some(&read_only)),
        digest_with(&schema, Some(&writes)),
        "an annotation-only change must alter the tool digest"
    );
}

#[test]
fn tool_digest_distinguishes_absent_not_stated_false_and_true() {
    let schema = json!({"type": "object"});
    let not_stated = annotations(None, Hint::NotStated, Hint::NotStated);
    let stated_false = annotations(None, Hint::StatedFalse, Hint::NotStated);
    let stated_true = annotations(None, Hint::StatedTrue, Hint::NotStated);

    let digests = [
        digest_with(&schema, None),
        digest_with(&schema, Some(&not_stated)),
        digest_with(&schema, Some(&stated_false)),
        digest_with(&schema, Some(&stated_true)),
    ];

    for (i, a) in digests.iter().enumerate() {
        for b in &digests[i + 1..] {
            assert_ne!(
                a, b,
                "absent, not stated, false and true must all hash differently"
            );
        }
    }
}

#[test]
fn tool_digest_ignores_title_but_not_hints() {
    let schema = json!({"type": "object"});
    let first = annotations(Some("First title"), Hint::StatedTrue, Hint::NotStated);
    let second = annotations(Some("Second title"), Hint::StatedTrue, Hint::NotStated);
    let flipped = annotations(Some("First title"), Hint::StatedFalse, Hint::NotStated);

    assert_eq!(
        digest_with(&schema, Some(&first)),
        digest_with(&schema, Some(&second)),
        "the display title must not affect the digest"
    );
    assert_ne!(
        digest_with(&schema, Some(&first)),
        digest_with(&schema, Some(&flipped)),
        "hints must affect the digest"
    );
}

#[test]
fn tool_digest_keeps_contradictory_hints_as_advertised() {
    let schema = json!({"type": "object"});
    let both = annotations(None, Hint::StatedTrue, Hint::StatedTrue);
    let read_only_only = annotations(None, Hint::StatedTrue, Hint::NotStated);

    assert_ne!(
        digest_with(&schema, Some(&both)),
        digest_with(&schema, Some(&read_only_only)),
        "read-only plus destructive is recorded as advertised, not normalized away"
    );
}
