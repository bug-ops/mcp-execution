//! Acceptance tests of the per-tool digest (#688): `ToolEntryDigest` and the optional
//! `tools[].digest` of a `schema_version` 3 `_meta.json`.

#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::assert_matches;

use mcp_execution_core::ToolName;
use mcp_execution_core::annotations::{AnnotationTitle, Hint, ToolAnnotations};
use mcp_execution_core::metadata::{
    MetadataReadError, ResultTypeOrigin, ToolMetadata, read_server_metadata,
};
use mcp_execution_core::provenance::{ToolDigestEntry, ToolEntryDigest};
use serde_json::{Value, json};

/// SHA-256 of the `tool-entry:v2` preimage of [`golden_schema`] under name `delete_file`.
const GOLDEN: &str = "791b8bb408d3f1cc7356ebbebee28e4f993e5a5a0bdc04eefb874f57aec4cf20";
const HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn golden_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}}})
}

fn digest(entry: &ToolDigestEntry<'_>) -> String {
    serde_json::to_value(ToolEntryDigest::compute(entry))
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}

fn annotated(title: &str, read_only: Hint) -> ToolAnnotations {
    let mut annotations = ToolAnnotations::not_stated();
    annotations.read_only = read_only;
    annotations.title = AnnotationTitle::new(title);
    annotations
}

#[test]
fn compute_matches_the_golden_vector() {
    let schema = golden_schema();

    let actual = digest(&ToolDigestEntry::new(
        "delete_file",
        "Deletes a file",
        &schema,
    ));

    assert_eq!(actual, GOLDEN);
}

#[test]
fn compute_changes_with_every_digested_field() {
    let schema = golden_schema();
    let other = json!({"type": "object"});
    let read_only = annotated("t", Hint::StatedTrue);
    let base = ToolDigestEntry::new("delete_file", "Deletes a file", &schema);

    let mut with_output = base;
    with_output.output_schema = Some(&other);
    let mut with_hint = base;
    with_hint.annotations = Some(&read_only);
    let variants = [
        base,
        ToolDigestEntry::new("remove_file", "Deletes a file", &schema),
        ToolDigestEntry::new("delete_file", "Removes a file", &schema),
        ToolDigestEntry::new("delete_file", "Deletes a file", &other),
        with_output,
        with_hint,
    ];

    let digests: std::collections::BTreeSet<String> = variants.iter().map(digest).collect();
    assert_eq!(digests.len(), variants.len());
}

#[test]
fn compute_ignores_a_title_only_change() {
    let schema = golden_schema();
    let first = annotated("First", Hint::StatedTrue);
    let second = annotated("Second", Hint::StatedTrue);
    let mut a = ToolDigestEntry::new("delete_file", "Deletes a file", &schema);
    let mut b = a;
    a.annotations = Some(&first);
    b.annotations = Some(&second);

    assert_eq!(digest(&a), digest(&b));
}

#[test]
fn try_from_accepts_only_64_lowercase_hex_characters() {
    assert!(ToolEntryDigest::try_from(GOLDEN.to_string()).is_ok());
    for bad in [
        String::new(),
        "abc".to_string(),
        GOLDEN.to_uppercase(),
        format!("{GOLDEN}0"),
        GOLDEN.replacen('7', "g", 1),
    ] {
        assert!(ToolEntryDigest::try_from(bad.clone()).is_err(), "{bad:?}");
        assert!(
            serde_json::from_value::<ToolEntryDigest>(json!(bad)).is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn tool_metadata_new_records_no_digest() {
    let tool = ToolMetadata::new(
        ToolName::new("echo").unwrap(),
        "echo".to_string(),
        vec![],
        vec![],
        ResultTypeOrigin::Undeclared,
        0,
    );

    assert!(tool.digest.is_none());
    let wire = serde_json::to_value(&tool).unwrap();
    assert!(wire.get("digest").is_none(), "{wire}");
}

/// A v3 sidecar as written before per-tool digests existed; `digest` is added when given.
fn v3_sidecar(digest: Option<&str>) -> Value {
    let mut tool = json!({
        "name": "echo",
        "typescript_name": "echo",
        "category": null,
        "keywords": [],
        "description": "Echoes",
        "parameters": [],
        "annotations": null,
        "result_type": null,
        "result_type_origin": "undeclared",
        "manifest_size_bytes": 10
    });
    if let Some(digest) = digest {
        tool["digest"] = json!(digest);
    }
    json!({
        "schema_version": 3,
        "server_id": "fixture",
        "server_name": "fixture",
        "server_version": "1.0.0",
        "tools": [tool],
        "provenance": {
            "generated_at": "2026-11-01T00:00:00Z",
            "config_fingerprint": HEX,
            "tool_digest": HEX
        }
    })
}

fn read(
    sidecar: &Value,
) -> Result<mcp_execution_core::metadata::ServerMetadata, MetadataReadError> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("_meta.json"), sidecar.to_string()).unwrap();
    read_server_metadata(dir.path())
}

#[test]
fn read_server_metadata_accepts_a_v3_file_without_digests() {
    let meta = read(&v3_sidecar(None)).unwrap();

    assert_eq!(meta.schema_version, 3);
    assert!(meta.tools[0].digest.is_none());
}

#[test]
fn read_server_metadata_keeps_a_recorded_digest() {
    let meta = read(&v3_sidecar(Some(GOLDEN))).unwrap();

    let recorded = meta.tools[0].digest.clone().unwrap();
    assert_eq!(serde_json::to_value(recorded).unwrap(), json!(GOLDEN));
}

#[test]
fn read_server_metadata_rejects_a_malformed_digest_as_malformed() {
    let result = read(&v3_sidecar(Some("not-a-digest")));

    assert_matches!(result, Err(MetadataReadError::Malformed { .. }));
}
