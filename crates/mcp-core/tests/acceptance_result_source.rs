//! Acceptance tests of the single wire enum `helper_protocol::ResultSource` (#676): exactly the
//! two `snake_case` spellings, anything else rejected.

use mcp_execution_core::helper_protocol::ResultSource;

#[test]
fn serde_round_trips_both_spellings() {
    for (variant, wire) in [
        (ResultSource::Structured, "\"structured\""),
        (ResultSource::Content, "\"content\""),
    ] {
        assert_eq!(serde_json::to_string(&variant).unwrap(), wire);
        assert_eq!(serde_json::from_str::<ResultSource>(wire).unwrap(), variant);
    }
}

#[test]
fn deserialize_rejects_every_other_spelling() {
    for wire in [
        "\"Structured\"",
        "\"CONTENT\"",
        "\"text\"",
        "\"\"",
        "null",
        "0",
    ] {
        assert!(
            serde_json::from_str::<ResultSource>(wire).is_err(),
            "{wire}"
        );
    }
}
