//! The bare-identifier predicate and its pattern text are one rule: the pattern the generated
//! runtime bridge receives must accept exactly what the Rust predicate accepts.

use mcp_execution_core::untrusted::{BARE_IDENTIFIER_PATTERN, is_bare_identifier};
use regex::Regex;

const SHARED_VECTORS: &[(&str, bool)] = &[
    ("id", true),
    ("_private", true),
    ("$ref", true),
    ("a1", true),
    ("A_b$", true),
    ("", false),
    ("1st", false),
    ("per-page", false),
    ("a.b", false),
    ("two words", false),
    ("naïve", false),
    ("x*/y", false),
    ("trailing\n", false),
];

#[test]
fn predicate_and_pattern_agree_on_every_shared_vector() {
    let pattern = Regex::new(BARE_IDENTIFIER_PATTERN).expect("pattern is a valid regex");

    for &(key, expected) in SHARED_VECTORS {
        assert_eq!(is_bare_identifier(key), expected, "predicate on {key:?}");
        assert_eq!(pattern.is_match(key), expected, "pattern on {key:?}");
    }
}
