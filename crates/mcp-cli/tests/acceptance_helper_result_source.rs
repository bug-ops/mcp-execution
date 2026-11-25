//! Acceptance tests of the helper `Request` wire shape after the `ResultSource` merge (#676):
//! the `result` field is the core enum, with the same two spellings and nothing else.

use mcp_execution_cli::runtime::protocol::Request;
use mcp_execution_core::helper_protocol::ResultSource;

fn line(result: &str) -> String {
    format!(r#"{{"id":1,"server":"demo","tool":"echo","arguments":{{}},"result":"{result}"}}"#)
}

#[test]
fn request_parses_both_result_spellings_into_the_core_enum() {
    let structured: Request =
        serde_json::from_str(&line("structured")).expect("structured spelling parses");
    assert_eq!(structured.result, ResultSource::Structured);

    let content: Request = serde_json::from_str(&line("content")).expect("content spelling parses");
    assert_eq!(content.result, ResultSource::Content);
}

#[test]
fn request_rejects_an_unknown_result_spelling() {
    assert!(serde_json::from_str::<Request>(&line("text")).is_err());
}
