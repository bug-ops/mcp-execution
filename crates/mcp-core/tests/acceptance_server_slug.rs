//! Acceptance tests of the `ServerSlug` newtype (#677): the frozen rule `[a-z0-9-]{1,64}`, with
//! no leading, trailing or consecutive-hyphen restriction, enforced at construction and at
//! deserialization.

use mcp_execution_core::{MAX_SERVER_ID_LENGTH, ServerIdSlugError, ServerSlug};

const fn is_slug_byte(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

#[test]
fn new_accepts_the_frozen_rule_for_every_ascii_character() {
    for code in 0u8..=127 {
        let c = char::from(code);
        let candidate = c.to_string();
        assert_eq!(
            ServerSlug::new(&candidate).is_ok(),
            is_slug_byte(c),
            "single character {c:?}"
        );
    }
}

#[test]
fn new_accepts_hyphens_in_any_position() {
    for id in ["-", "--a", "a-", "a--b", "github", "my-server-123", "0"] {
        let slug = ServerSlug::new(id).unwrap();
        assert_eq!(slug.as_str(), id);
        assert_eq!(slug.to_string(), id);
    }
}

#[test]
fn new_accepts_exactly_the_maximum_length() {
    let id = "a".repeat(MAX_SERVER_ID_LENGTH);
    assert_eq!(ServerSlug::new(&id).unwrap().as_str(), id);
}

#[test]
fn new_rejects_empty_input() {
    assert_eq!(ServerSlug::new("").unwrap_err(), ServerIdSlugError::Empty);
}

#[test]
fn new_rejects_input_over_the_maximum_length() {
    let id = "a".repeat(MAX_SERVER_ID_LENGTH + 1);
    assert_eq!(
        ServerSlug::new(&id).unwrap_err(),
        ServerIdSlugError::TooLong {
            len: MAX_SERVER_ID_LENGTH + 1,
            limit: MAX_SERVER_ID_LENGTH,
        }
    );
}

#[test]
fn new_rejects_each_class_of_foreign_character() {
    for id in [
        "GitHub",
        "my_server",
        "my.server",
        "a/b",
        "a@b",
        "a b",
        "caf\u{e9}",
        "\u{65e5}\u{672c}",
        "\u{ff41}",
        &"\u{e9}".repeat(32),
    ] {
        assert_eq!(
            ServerSlug::new(id).unwrap_err(),
            ServerIdSlugError::InvalidCharacters,
            "{id:?}"
        );
    }
}

#[test]
fn deserialize_gives_the_same_verdict_as_new() {
    let slug: ServerSlug = serde_json::from_str("\"my-server-1\"").unwrap();
    assert_eq!(slug.as_str(), "my-server-1");

    for bad in ["\"\"", "\"Bad\"", "\"a_b\"", "\"a.b\"", "\"\u{e9}\""] {
        assert!(serde_json::from_str::<ServerSlug>(bad).is_err(), "{bad}");
    }
    let too_long = format!("\"{}\"", "a".repeat(MAX_SERVER_ID_LENGTH + 1));
    assert!(serde_json::from_str::<ServerSlug>(&too_long).is_err());
}

#[test]
fn serialize_is_a_plain_string() {
    let slug = ServerSlug::new("github").unwrap();
    assert_eq!(serde_json::to_string(&slug).unwrap(), "\"github\"");
}
