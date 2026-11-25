//! Acceptance tests for #568: annotation tags and catalog hints for stated hints only.

use super::*;
use mcp_execution_core::annotations::{AnnotationTitle, Hint};

fn annotations(
    title: Option<&str>,
    read_only: Hint,
    destructive: Hint,
    idempotent: Hint,
    open_world: Hint,
) -> ToolAnnotations {
    {
        let mut value = ToolAnnotations::new(read_only, destructive, idempotent, open_world);
        value.title = title.and_then(AnnotationTitle::new);
        value
    }
}

#[test]
fn jsdoc_tags_emit_every_stated_hint_and_the_title() {
    let tags = jsdoc_tags(&annotations(
        Some("Delete file"),
        Hint::StatedFalse,
        Hint::StatedTrue,
        Hint::StatedTrue,
        Hint::StatedFalse,
    ));

    assert_eq!(
        tags,
        [
            "@title Delete file",
            "@readOnlyHint false",
            "@destructiveHint true",
            "@idempotentHint true",
            "@openWorldHint false",
        ]
    );
}

#[test]
fn jsdoc_tags_emit_nothing_for_hints_that_were_not_stated() {
    let tags = jsdoc_tags(&annotations(
        None,
        Hint::StatedTrue,
        Hint::NotStated,
        Hint::NotStated,
        Hint::NotStated,
    ));

    assert_eq!(tags, ["@readOnlyHint true"]);
}

#[test]
fn jsdoc_tags_keep_read_only_plus_destructive_as_advertised() {
    let tags = jsdoc_tags(&annotations(
        None,
        Hint::StatedTrue,
        Hint::StatedTrue,
        Hint::NotStated,
        Hint::NotStated,
    ));

    assert_eq!(tags, ["@readOnlyHint true", "@destructiveHint true"]);
}

#[test]
fn jsdoc_tags_title_cannot_terminate_the_comment_and_is_capped() {
    let hostile = format!("evil */ export const x = 1; {}", "t".repeat(500));
    let tags = jsdoc_tags(&annotations(
        Some(&hostile),
        Hint::StatedTrue,
        Hint::NotStated,
        Hint::NotStated,
        Hint::NotStated,
    ));

    let title = tags
        .iter()
        .find(|t| t.starts_with("@title"))
        .expect("a stated title is emitted");
    assert!(!title.contains("*/"), "{title}");
    assert!(
        title.chars().count() <= "@title ".len() + 256,
        "{} chars",
        title.chars().count()
    );
}

#[test]
fn catalog_hints_list_stated_hints_in_a_fixed_order() {
    let hints = catalog_hints(&annotations(
        Some("ignored in hints"),
        Hint::StatedTrue,
        Hint::StatedFalse,
        Hint::NotStated,
        Hint::NotStated,
    ));

    assert_eq!(
        hints.as_deref(),
        Some("@hints readOnly=true destructive=false")
    );
}

#[test]
fn catalog_hints_cover_all_four_hints_and_none_when_nothing_is_stated() {
    let all = catalog_hints(&annotations(
        None,
        Hint::StatedTrue,
        Hint::StatedFalse,
        Hint::StatedTrue,
        Hint::StatedFalse,
    ));
    assert_eq!(
        all.as_deref(),
        Some("@hints readOnly=true destructive=false idempotent=true openWorld=false")
    );

    let nothing = annotations(
        Some("only a title"),
        Hint::NotStated,
        Hint::NotStated,
        Hint::NotStated,
        Hint::NotStated,
    );
    assert_eq!(catalog_hints(&nothing), None);
}
