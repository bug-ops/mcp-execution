//! Golden vectors pinning the `tool-entry:v2` digest preimage (#568): a change to the domain tag,
//! the field framing or the annotation encoding changes these values on purpose and must be
//! reviewed as a breaking change to stored provenance digests.

use mcp_execution_core::annotations::{AnnotationTitle, Hint, ToolAnnotations};
use mcp_execution_core::provenance::{ToolDigest, ToolDigestEntry};
use serde_json::json;

const WITHOUT_ANNOTATIONS: &str =
    "9db0db8e933193fedde9aecc177b808854498ec85de25f9ffa2dc1c48b5803a3";
const WITH_ANNOTATIONS: &str = "e4d7bbba68888ffabaef92b59756a94649c256cd58be2ceb24cecac9a0c9607a";

#[test]
fn tool_digest_golden_vector_without_annotations() {
    let schema = json!({"type": "object", "properties": {"path": {"type": "string"}}});
    let digest = ToolDigest::compute(&[ToolDigestEntry::new(
        "delete_file",
        "Deletes a file",
        &schema,
    )]);

    assert_eq!(digest.as_str(), WITHOUT_ANNOTATIONS);
}

#[test]
fn tool_digest_golden_vector_with_annotations_ignores_the_title() {
    let schema = json!({"type": "object", "properties": {"path": {"type": "string"}}});
    let annotations = {
        let mut value = ToolAnnotations::not_stated();
        value.read_only = Hint::StatedFalse;
        value.destructive = Hint::StatedTrue;
        value.open_world = Hint::StatedFalse;

        value.title = AnnotationTitle::new("Any title");
        value
    };
    let digest = ToolDigest::compute(&[{
        let mut value = ToolDigestEntry::new("delete_file", "Deletes a file", &schema);
        value.annotations = Some(&annotations);
        value
    }]);

    assert_eq!(digest.as_str(), WITH_ANNOTATIONS);
}
