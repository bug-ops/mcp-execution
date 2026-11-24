//! A tool file run directly keeps the argument conventions of the inline block it replaced:
//! a missing or empty argument means `{}`.

mod acceptance_support;
mod support;

use acceptance_support::{
    REPLY_ECHO_ARGUMENTS, canonical_tempdir, generate, run_node, server_info, tool, write_home,
    write_package,
};
use serde_json::json;
use support::require_node_strip_types;

#[test]
fn run_tool_with_an_empty_argument_sends_empty_arguments() {
    if !require_node_strip_types("run_tool_with_an_empty_argument_sends_empty_arguments") {
        return;
    }
    let root = canonical_tempdir();
    let pkg = root.path().join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    let server = server_info(vec![tool(
        "get_thing",
        json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        None,
    )]);
    write_package(&generate(&server), &pkg);
    let home = write_home(
        root.path(),
        &acceptance_support::fake_server_js(REPLY_ECHO_ARGUMENTS),
    );

    let run = run_node(&pkg.join("getThing.ts"), root.path(), &home, Some(""), &[]);

    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "{}\n");
}
