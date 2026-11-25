//! Pins `sanitize_untrusted_text` to hand-written vectors (#624).
//!
//! `fixtures/untrusted_text_vectors.json` holds each input with the output the documented rules
//! of the sanitizer require (controls and bidi overrides to a space; bidi marks, Tags block,
//! U+FEFF and U+2060-U+2064 removed; U+200B to a space; ZWJ/ZWNJ kept; variation selectors
//! dropped past a run of 2 or a total of 16; cut at `max_len` chars). The expected values were
//! derived from those rules, never captured from the function, so a disagreement is either an
//! error in the expectation or a sanitizer defect to investigate before either side changes.

#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use mcp_execution_core::untrusted::{MAX_UNTRUSTED_FIELD_LEN, sanitize_untrusted_text};
use serde_json::Value;

const VECTORS: &str = include_str!("fixtures/untrusted_text_vectors.json");

/// Number of vectors in the fixture; a change must be deliberate.
const CASE_COUNT: usize = 19;

fn fixture() -> Value {
    serde_json::from_str(VECTORS).unwrap()
}

#[test]
fn untrusted_text_fixture_holds_every_vector_with_the_default_limit() {
    let fixture = fixture();

    assert_eq!(
        fixture["max_len"].as_u64().unwrap(),
        MAX_UNTRUSTED_FIELD_LEN as u64
    );
    assert_eq!(fixture["cases"].as_array().unwrap().len(), CASE_COUNT);
}

#[test]
fn sanitize_untrusted_text_produces_the_expected_output_for_every_vector() {
    let fixture = fixture();
    let max_len = usize::try_from(fixture["max_len"].as_u64().unwrap()).unwrap();

    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let expected = case["expected"].as_str().unwrap();

        let actual = sanitize_untrusted_text(input, max_len);

        assert_eq!(actual, expected, "vector {name}");
    }
}
