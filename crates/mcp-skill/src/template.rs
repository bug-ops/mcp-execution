//! Template rendering for skill generation.
//!
//! Uses Handlebars templates to render the skill generation prompt and
//! the final SKILL.md file. Both templates are embedded at compile time.

use std::sync::LazyLock;

use handlebars::Handlebars;
use thiserror::Error;

use crate::types::GenerateSkillResult;

/// Errors that can occur during template rendering.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// Template rendering failed.
    #[error("template rendering failed: {0}")]
    RenderFailed(#[from] handlebars::RenderError),

    /// Template registration failed.
    #[error("template registration failed: {0}")]
    RegistrationFailed(#[from] handlebars::TemplateError),
}

/// Embedded Handlebars template for the LLM skill generation prompt.
const SKILL_GENERATION_TEMPLATE: &str = include_str!("templates/skill-generation.hbs");

/// Embedded Handlebars template that renders SKILL.md directly (no LLM required).
const SKILL_MD_TEMPLATE: &str = include_str!("templates/skill-md.hbs");

/// Handlebars instance with pre-registered templates.
///
/// Initialized once per process using `LazyLock` for optimal performance.
/// Templates are parsed and validated on first access.
static HANDLEBARS: LazyLock<Handlebars<'static>> = LazyLock::new(|| {
    let mut hb = Handlebars::new();
    hb.register_template_string("skill", SKILL_GENERATION_TEMPLATE)
        .expect("embedded skill-generation template must be valid Handlebars syntax");
    hb.register_template_string("skill-md", SKILL_MD_TEMPLATE)
        .expect("embedded skill-md template must be valid Handlebars syntax");
    hb
});

/// Wraps a string in YAML double-quote scalars, escaping `\`, `"`, and newlines.
///
/// Produces a value that can be embedded directly after a YAML key as a
/// quoted scalar — safe against `:` in the middle, leading `-`, and
/// newline injection.
fn yaml_quote(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "");
    format!("\"{escaped}\"")
}

/// Render the skill generation prompt.
///
/// Takes the `GenerateSkillResult` context and renders it using
/// the embedded Handlebars template.
///
/// # Arguments
///
/// * `context` - Skill generation context from `build_skill_context`
///
/// # Returns
///
/// Rendered prompt string for the LLM.
///
/// # Errors
///
/// Returns `TemplateError` if template rendering fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::{build_skill_context, render_generation_prompt};
///
/// let context = build_skill_context("github", &[], None);
/// let prompt = render_generation_prompt(&context).unwrap();
/// ```
pub fn render_generation_prompt(context: &GenerateSkillResult) -> Result<String, TemplateError> {
    let rendered = HANDLEBARS.render("skill", context)?;
    Ok(rendered)
}

/// Render SKILL.md content directly from skill context.
///
/// Produces the final SKILL.md file content without requiring an LLM. Uses the
/// embedded `skill-md.hbs` template with the same [`GenerateSkillResult`] context
/// as [`render_generation_prompt`].
///
/// Tool descriptions are rendered with triple-stash (`{{{...}}}`) to avoid
/// HTML-escaping characters such as `<`, `>`, and `&`.
///
/// YAML frontmatter scalars (`description`) are pre-quoted so that special
/// characters in MCP server metadata (`:`, newlines, leading `-`) cannot
/// corrupt the frontmatter or inject additional YAML keys (S3).
///
/// # Arguments
///
/// * `context` - Skill generation context from [`crate::build_skill_context`]
///
/// # Returns
///
/// Rendered SKILL.md string ready to write to disk.
///
/// # Panics
///
/// Does not panic in practice: `serde_json::to_value` is infallible for
/// `GenerateSkillResult` because all fields are standard Rust types with
/// derived `Serialize` implementations.
///
/// # Errors
///
/// Returns [`TemplateError`] if Handlebars rendering fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::{build_skill_context, render_skill_md};
///
/// let context = build_skill_context("github", &[], None);
/// let md = render_skill_md(&context).unwrap();
/// assert!(md.starts_with("---\n"));
/// ```
pub fn render_skill_md(context: &GenerateSkillResult) -> Result<String, TemplateError> {
    // SAFETY: `GenerateSkillResult` derives `Serialize` with only primitive and
    // standard-library types — `to_value` is infallible for this type.
    let mut value =
        serde_json::to_value(context).expect("GenerateSkillResult serialization is infallible");

    // YAML-quote server_description so that `:`, newlines, and leading `-` in
    // MCP server metadata cannot corrupt the frontmatter or inject keys (S3).
    if let Some(desc) = value
        .get("server_description")
        .and_then(|v| v.as_str())
        .map(yaml_quote)
    {
        value["server_description"] = serde_json::Value::String(desc);
    }

    let rendered = HANDLEBARS.render("skill-md", &value)?;
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SkillCategory, SkillTool, ToolExample};

    fn create_test_context() -> GenerateSkillResult {
        GenerateSkillResult {
            server_id: "test".to_string(),
            skill_name: "test-progressive".to_string(),
            server_description: Some("Test server".to_string()),
            categories: vec![SkillCategory {
                name: "test".to_string(),
                display_name: "Test".to_string(),
                tools: vec![SkillTool {
                    name: "test_tool".to_string(),
                    typescript_name: "testTool".to_string(),
                    description: "Test tool description".to_string(),
                    keywords: vec!["test".to_string()],
                    required_params: vec!["param1".to_string()],
                    optional_params: vec![],
                }],
            }],
            tool_count: 1,
            example_tools: vec![ToolExample {
                tool_name: "test_tool".to_string(),
                description: "Test tool".to_string(),
                cli_command: "node test.ts".to_string(),
                params_json: "{}".to_string(),
            }],
            generation_prompt: "Pre-built prompt".to_string(),
            output_path: "~/.claude/skills/test/SKILL.md".to_string(),
        }
    }

    #[test]
    fn test_render_generation_prompt() {
        let context = create_test_context();
        let result = render_generation_prompt(&context);

        match result {
            Ok(prompt) => {
                assert!(prompt.contains("test"));
                assert!(prompt.contains("SKILL.md"));
            }
            Err(e) => panic!("Template rendering failed: {e}"),
        }
    }

    #[test]
    fn test_render_skill_md() {
        let context = create_test_context();
        let result = render_skill_md(&context);

        match result {
            Ok(md) => {
                assert!(md.starts_with("---\n"), "must start with YAML frontmatter");
                assert!(md.contains("name: test-progressive"));
                assert!(md.contains("# test-progressive"));
                assert!(md.contains("~/.claude/servers/test/"));
                assert!(md.contains("testTool"));
            }
            Err(e) => panic!("render_skill_md failed: {e}"),
        }
    }

    #[test]
    fn test_render_skill_md_html_special_chars_not_escaped() {
        let mut context = create_test_context();
        context.categories = vec![SkillCategory {
            name: "test".to_string(),
            display_name: "Test".to_string(),
            tools: vec![SkillTool {
                name: "my_tool".to_string(),
                typescript_name: "myTool".to_string(),
                description: "Create & update <items> with \"quotes\"".to_string(),
                keywords: vec![],
                required_params: vec![],
                optional_params: vec![],
            }],
        }];

        let md = render_skill_md(&context).unwrap();
        // Triple-stash in template must prevent HTML escaping.
        assert!(md.contains('&'), "& must not be HTML-escaped");
        assert!(md.contains('<'), "< must not be HTML-escaped");
    }

    #[test]
    fn test_render_skill_md_yaml_frontmatter_safe() {
        // S3: malicious server_description must not inject YAML keys or corrupt frontmatter.
        let mut context = create_test_context();
        context.server_description = Some("GitHub: issues & CI\nname: injected".to_string());

        let md = render_skill_md(&context).unwrap();

        // Extract frontmatter block (between the two "---" markers).
        let after_open = md.strip_prefix("---\n").expect("must start with ---");
        let fm_end = after_open.find("\n---").expect("must have closing ---");
        let frontmatter = &after_open[..fm_end];

        // There must be exactly one `name:` key — no injected sibling.
        let name_count = frontmatter
            .lines()
            .filter(|l| l.starts_with("name:"))
            .count();
        assert_eq!(
            name_count, 1,
            "YAML key injection detected in: {frontmatter}"
        );

        // The description value must be quoted (YAML double-quoted scalar).
        let desc_line = frontmatter
            .lines()
            .find(|l| l.starts_with("description:"))
            .expect("description key must be present");
        assert!(
            desc_line.contains('"'),
            "description must be YAML-quoted: {desc_line}"
        );
    }

    #[test]
    fn test_yaml_quote() {
        assert_eq!(yaml_quote("simple"), "\"simple\"");
        assert_eq!(yaml_quote("GitHub: issues"), "\"GitHub: issues\"");
        assert_eq!(yaml_quote("line1\nline2"), "\"line1\\nline2\"");
        assert_eq!(yaml_quote(r#"has "quotes""#), r#""has \"quotes\"""#);
        assert_eq!(yaml_quote("has \\backslash"), "\"has \\\\backslash\"");
    }
}
