//! Acceptance tests of the script-first skill (#578, FR-043): one script-first workflow taught by
//! the deterministic render, a body whose size does not depend on the
//! tool count, and the privilege statement. Server text staying inert (FR-043.9) already holds and is
//! pinned by the existing skill tests, so no new test is added for it here.
//!
//! The default name (FR-043.5) and `SkillName` validation (FR-043.6) are already unit-tested at
//! their implementation; the "name differs from the directory" rule is pinned through the CLI in
//! `crates/mcp-cli/tests/acceptance_scripts_cmd.rs`.

use std::collections::BTreeSet;

use mcp_execution_skill::{ParsedParameter, ParsedToolFile, build_skill_context, render_skill_md};

fn tool(index: usize, category: &str, description: &str) -> ParsedToolFile {
    ParsedToolFile {
        name: format!("tool_{index:04}"),
        typescript_name: format!("tool{index:04}"),
        server_id: "acme".to_string(),
        category: Some(category.to_string()),
        keywords: vec!["thing".to_string()],
        description: Some(description.to_string()),
        parameters: vec![ParsedParameter {
            name: "repo".to_string(),
            typescript_type: "string".to_string(),
            required: true,
            description: None,
        }],
    }
}

fn tools(count: usize) -> Vec<ParsedToolFile> {
    (0..count)
        .map(|i| tool(i, "issues", "Does a thing"))
        .collect()
}

/// The CLI subcommands a text tells the reader to run: every word after `mcp-execution-cli `.
fn commands(text: &str) -> BTreeSet<String> {
    text.split("mcp-execution-cli ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .map(|word| {
            word.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_string()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

#[test]
fn render_teaches_the_direct_call_and_the_script_runner_commands() {
    let context = build_skill_context("acme", &tools(12), None, None);

    let rendered = render_skill_md(&context).unwrap();

    let taught = commands(&rendered);
    for command in ["call", "run"] {
        assert!(
            taught.contains(command),
            "the workflow teaches `{command}`: {taught:?}"
        );
    }
    assert!(
        rendered.contains("call --from-config acme --"),
        "direct call form: {rendered}"
    );
    assert!(
        rendered.contains("run script.ts"),
        "runner form: {rendered}"
    );
}

#[test]
fn rendered_body_size_does_not_depend_on_the_tool_count() {
    let small = render_skill_md(&build_skill_context("acme", &tools(10), None, None)).unwrap();
    let large = render_skill_md(&build_skill_context("acme", &tools(1000), None, None)).unwrap();

    assert!(
        small.len().abs_diff(large.len()) <= 256,
        "10 tools render {} bytes, 1000 tools render {} bytes",
        small.len(),
        large.len()
    );
}

#[test]
fn rendered_skill_states_the_privilege_level_and_when_not_to_script() {
    let rendered = render_skill_md(&build_skill_context("acme", &tools(3), None, None))
        .unwrap()
        .to_lowercase();

    assert!(rendered.contains("sandbox"), "{rendered}");
    assert!(rendered.contains("privileges"), "{rendered}");
    assert!(
        rendered.contains("single lookup") || rendered.contains("one lookup"),
        "the skill must say when a direct call is enough: {rendered}"
    );
}
