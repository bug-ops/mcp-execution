//! Template rendering for skill generation.
//!
//! Uses Handlebars templates to render the skill generation prompt and
//! the final SKILL.md file. Both templates are embedded at compile time.

use std::sync::LazyLock;

use handlebars::Handlebars;
use mcp_execution_core::untrusted::{MAX_UNTRUSTED_FIELD_LEN, sanitize_untrusted_text};
use serde::Serialize;
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

    // Strict mode: fail on missing variables
    hb.set_strict_mode(true);

    hb.register_template_string("skill", SKILL_GENERATION_TEMPLATE)
        .expect("embedded skill-generation template must be valid Handlebars syntax");
    hb.register_template_string("skill-md", SKILL_MD_TEMPLATE)
        .expect("embedded skill-md template must be valid Handlebars syntax");
    hb
});

/// Whole shape of `SKILL.md`'s YAML frontmatter, serialized as a single unit so both
/// fields go through one `serde-saphyr` emitter pass (see [`render_skill_md`]).
///
/// Field declaration order is emission order — `serde`'s struct serialization streams
/// fields in the order declared, unlike a `BTreeMap` (which would sort them
/// alphabetically) — so `name` is written before `description`, matching the
/// frontmatter's conventional layout.
#[derive(Serialize)]
struct Frontmatter<'a> {
    name: &'a str,
    description: &'a str,
}

impl Frontmatter<'_> {
    /// Renders this frontmatter as the raw text to splice between the `SKILL.md`
    /// template's `---` delimiters, delegating all escaping to `serde-saphyr`'s YAML
    /// emitter instead of a hand-maintained escape table — so it covers `:`, a
    /// leading `-`, embedded newlines, and C0 control characters (NUL, BEL, ESC, ...)
    /// that a narrower hand-rolled escaper would miss. `serde-saphyr` is YAML-1.2-correct:
    /// a `U+2028`/`U+2029` line/paragraph separator in the input is emitted as a `\L`/`\P`
    /// escape inside a double-quoted scalar, which both `serde-saphyr` and libyaml-based
    /// readers parse back exactly — a net improvement over the previous emitter, which
    /// rendered these as a literal line break with a 2-space fold indent (a narrow
    /// regression for strict YAML-1.2 external consumers).
    ///
    /// Injection safety (S3) rests on a structural property of the emitter, not on
    /// escaping alone: every emitted block-scalar body line carries at least 2
    /// (`indent_step`) leading spaces, so no attacker-controlled `description` can ever
    /// produce a line starting with `---` at column 0 that would prematurely close the
    /// frontmatter block. This is an internal property of `serde-saphyr`'s serializer
    /// with no stability contract, so it is pinned by a direct assertion on the emitted
    /// text in the round-trip test below rather than trusted blindly.
    ///
    /// `serde_saphyr::to_string` always ends the document in exactly one `\n`. When
    /// `description` (the last-declared, and so last-emitted, field) itself ends in
    /// `\n`, that final `\n` is not a plain document terminator but part of the
    /// scalar's semantic content — a multi-line value emitted as a YAML block literal
    /// (`|`, `|-`, `|+`, ...) uses a trailing newline to signal "clip"/"keep"
    /// chomping. No compensation is applied for this, and none is needed: it is
    /// unobservable through our own round-trip, since `granit-parser` 1.0.1's scanner
    /// (`scanner.rs:2981-2989`) applies EOF-chomping leniency to a block scalar
    /// terminated by end-of-input, recovering the content newline regardless. It is
    /// also unobservable by a plausible external YAML-1.2 consumer extracting the
    /// frontmatter block by region-capture or by a classic
    /// `^---\n([\s\S]*?)\n---` regex: `serde-saphyr` indents `|+` blank body lines
    /// (unlike a libyaml-based emitter, which renders them truly empty), so the final
    /// `\n` is never load-bearing under either extraction method.
    fn to_yaml_block(&self) -> String {
        // PANIC: serializing two `&str` fields to YAML has no fallible step (no
        // recursion, no I/O) — this cannot fail. `serde-saphyr`'s only
        // `Error::custom` call site in the serializer (`ser/serializer.rs:766`) is
        // caught locally with a quoting fallback (`:877`); its remaining
        // `Error::custom` sites live in unused `Rc`/`Arc` recursive-wrapper code paths.
        serde_saphyr::to_string(self).expect("YAML serialization of Frontmatter is infallible")
    }
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
/// `skill_name` is rendered with triple-stash (`{{{skill_name}}}`), matching
/// [`render_skill_md`]'s body heading, and sanitized with
/// [`sanitize_untrusted_text`] before rendering for the same reason: it's
/// attacker-controlled (the caller's `--skill-name` flag or an MCP tool call
/// argument) and appears both as this example structure's `name:` line and as
/// its `# {{{skill_name}}}` heading — a value containing a newline could
/// otherwise inject Markdown structure into the prompt itself (issue #411).
///
/// # Errors
///
/// Returns `TemplateError` if template rendering fails, including when
/// `context` is missing a field the template references — Handlebars strict
/// mode is enabled, so a missing variable hard-fails instead of silently
/// rendering an empty string.
///
/// # Panics
///
/// Does not panic in practice: `serde_json::to_value` is infallible for
/// `GenerateSkillResult` because all fields are standard Rust types with
/// derived `Serialize` implementations.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::{build_skill_context, render_generation_prompt};
///
/// let context = build_skill_context("github", &[], None, None);
/// let prompt = render_generation_prompt(&context).unwrap();
/// ```
pub fn render_generation_prompt(context: &GenerateSkillResult) -> Result<String, TemplateError> {
    // PANIC: `GenerateSkillResult` derives `Serialize` with only primitive and
    // standard-library types — `to_value` cannot fail for this type.
    let mut value =
        serde_json::to_value(context).expect("GenerateSkillResult serialization is infallible");
    value["skill_name"] = serde_json::Value::String(sanitize_untrusted_text(
        &context.skill_name,
        MAX_UNTRUSTED_FIELD_LEN,
    ));

    let rendered = HANDLEBARS.render("skill", &value)?;
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
/// The YAML frontmatter (`name`, `description`) is rendered separately from the rest
/// of the template as one `Frontmatter` block via `Frontmatter::to_yaml_block`, so
/// that special characters in either field — both are attacker-controlled MCP server
/// metadata (`skill_name` from the caller, `server_description` inferred from the
/// server) — cannot corrupt the frontmatter or inject additional YAML keys (S3).
///
/// `skill_name` is rendered a second time, as the body's `# {{{skill_name}}}` heading
/// (triple-stash, matching tool descriptions — no HTML-escaping needed/wanted). Being
/// YAML-safe for the frontmatter above does not make a value safe as a single Markdown
/// heading line: a value containing a newline could still open a new heading, fenced
/// code block, or list item in the body. So the copy used for that heading is separately
/// flattened with [`sanitize_untrusted_text`], the same defense already applied to tool
/// names/descriptions/categories in [`crate::build_skill_context`] (issue #410).
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
/// derived `Serialize` implementations, and `Frontmatter::to_yaml_block` is
/// likewise infallible for the same reason.
///
/// # Errors
///
/// Returns [`TemplateError`] if Handlebars rendering fails, including when
/// `context` is missing a field the template references — Handlebars strict
/// mode is enabled, so a missing variable hard-fails instead of silently
/// rendering an empty string.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::{build_skill_context, render_skill_md};
///
/// let context = build_skill_context("github", &[], None, None);
/// let md = render_skill_md(&context).unwrap();
/// assert!(md.starts_with("---\n"));
/// ```
pub fn render_skill_md(context: &GenerateSkillResult) -> Result<String, TemplateError> {
    // PANIC: `GenerateSkillResult` derives `Serialize` with only primitive and
    // standard-library types — `to_value` cannot fail for this type.
    let mut value =
        serde_json::to_value(context).expect("GenerateSkillResult serialization is infallible");

    let default_description = format!(
        "{} MCP server tools ({} tools)",
        context.server_id, context.tool_count
    );
    let frontmatter = Frontmatter {
        name: &context.skill_name,
        description: context
            .server_description
            .as_deref()
            .unwrap_or(&default_description),
    };
    value["frontmatter_yaml"] = serde_json::Value::String(frontmatter.to_yaml_block());

    // The frontmatter `name:` above is YAML-safe via `to_yaml_block`, but the body's
    // `# {{{skill_name}}}` heading is a separate splice point with a separate safety
    // requirement: a newline that's perfectly fine inside a YAML block-literal scalar
    // would still open a new heading/fenced-code-block/list-item line in the body
    // (issue #410). Overriding just this key in `value` leaves the frontmatter's own
    // `context.skill_name` reference (built above from the original, unflattened string)
    // unaffected.
    value["skill_name"] = serde_json::Value::String(sanitize_untrusted_text(
        &context.skill_name,
        MAX_UNTRUSTED_FIELD_LEN,
    ));

    let rendered = HANDLEBARS.render("skill-md", &value)?;
    // Normalize CRLF → LF so output is consistent across platforms (Windows CI).
    Ok(rendered.replace("\r\n", "\n"))
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
            warnings: vec![],
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

    /// Issue #411: `render_generation_prompt` (the "skill" `skill-generation.hbs` template)
    /// must sanitize `skill_name` the same way `render_skill_md` does for its body heading —
    /// it splices the value in twice (the example `name:` line and the `# {{{skill_name}}}`
    /// heading), and previously did so via plain double-stash with no flattening at all.
    #[test]
    fn test_render_generation_prompt_sanitizes_hostile_skill_name() {
        let mut context = create_test_context();
        context.skill_name = "evil\n### Injected Heading\n```\ninjected block\n```".to_string();

        let prompt = render_generation_prompt(&context).unwrap();

        assert!(
            !prompt.contains("\n### Injected Heading"),
            "hostile skill_name must not introduce a new heading line: {prompt}"
        );
        assert!(
            prompt.contains("Injected Heading"),
            "sanitized content must still render, just inert"
        );
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

    /// Counts frontmatter `name:` lines and cross-checks against the project's own
    /// [`crate::parser::extract_skill_metadata`] parser (not a hand-rolled
    /// `strip_prefix`/`find` split) — belt-and-suspenders: the line count catches key
    /// injection even if it happened to still parse, and the real-parser round-trip
    /// catches any other structural corruption the line count would miss.
    fn assert_frontmatter_safe(md: &str, expected_name: &str, expected_description: &str) {
        let after_open = md.strip_prefix("---\n").expect("must start with ---");
        let fm_end = after_open.find("\n---").expect("must have closing ---");
        let frontmatter = &after_open[..fm_end];
        let name_count = frontmatter
            .lines()
            .filter(|l| l.starts_with("name:"))
            .count();
        assert_eq!(
            name_count, 1,
            "YAML key injection detected in: {frontmatter}"
        );

        let metadata = crate::parser::extract_skill_metadata(md)
            .unwrap_or_else(|e| panic!("SKILL.md must have valid frontmatter: {e}\n{md}"));
        assert_eq!(metadata.name, expected_name);
        assert_eq!(metadata.description, expected_description);
    }

    #[test]
    fn test_render_skill_md_yaml_frontmatter_safe() {
        // S3: malicious server_description must not inject YAML keys or corrupt frontmatter.
        let mut context = create_test_context();
        context.server_description = Some("GitHub: issues & CI\nname: injected".to_string());

        let md = render_skill_md(&context).unwrap();

        assert_frontmatter_safe(
            &md,
            "test-progressive",
            "GitHub: issues & CI\nname: injected",
        );
    }

    #[test]
    fn test_render_skill_md_yaml_frontmatter_control_chars_safe() {
        // Issue #398: the old hand-rolled escaper only covered `\`, `"`, `\n`, and `\r` —
        // it left other C0 control characters unescaped, which a spec-compliant YAML
        // double-quoted scalar must escape. Delegating to `serde-saphyr` must close
        // that gap.
        let mut context = create_test_context();
        context.server_description = Some("NUL:\u{0} BEL:\u{7} ESC:\u{1b} desc".to_string());

        let md = render_skill_md(&context).unwrap();

        assert_frontmatter_safe(
            &md,
            "test-progressive",
            "NUL:\u{0} BEL:\u{7} ESC:\u{1b} desc",
        );
    }

    #[test]
    fn test_render_skill_md_yaml_frontmatter_trailing_newline_preserved() {
        // S2 regression: a description ending in a semantically significant '\n' must
        // round-trip exactly. `serde-saphyr` renders it as a YAML block literal whose
        // own trailing '\n' is real content (clip chomping), not just a document
        // terminator; naively stripping "the" trailing newline from the emitter's
        // output silently drops it instead.
        let mut context = create_test_context();
        context.server_description = Some("ends with newline\n".to_string());
        let md = render_skill_md(&context).unwrap();
        assert_frontmatter_safe(&md, "test-progressive", "ends with newline\n");

        // Also covers "keep" chomping, where multiple trailing newlines are all
        // semantically significant.
        let mut context = create_test_context();
        context.server_description = Some("multiple trailing\n\n\n".to_string());
        let md = render_skill_md(&context).unwrap();
        assert_frontmatter_safe(&md, "test-progressive", "multiple trailing\n\n\n");
    }

    #[test]
    fn test_render_skill_md_yaml_frontmatter_skill_name_injection_safe() {
        // S1: `skill_name` is exactly as attacker-controlled as `server_description`
        // (it comes from the CLI's `--skill-name` flag / an MCP tool call argument) but
        // was not encoded at all before this fix — the old `yaml_quote` covered only
        // `server_description`, leaving `name:` fully injectable.
        let attacks = [
            "evil\ninjected: true",
            "evil: value",
            "x\n---\nbody",
            "-leading dash",
        ];

        for attack in attacks {
            let mut context = create_test_context();
            context.skill_name = attack.to_string();
            let md = render_skill_md(&context).unwrap();
            assert_frontmatter_safe(&md, attack, "Test server");
        }
    }

    /// Issue #298 (end-to-end): a tool description containing embedded line breaks
    /// that mimic Markdown structure must not be able to inject a heading, a fenced
    /// code block, or an extra list item into the rendered SKILL.md body. The
    /// sanitization happens in `build_skill_context` (`context::group_by_category`),
    /// so this exercises the real production pipeline rather than a hand-built
    /// `GenerateSkillResult`.
    #[test]
    fn test_render_skill_md_end_to_end_flattens_injected_markdown_structure() {
        use crate::build_skill_context;
        use crate::parser::ParsedToolFile;

        let hostile = ParsedToolFile {
            name: "evil_tool".to_string(),
            typescript_name: "evilTool".to_string(),
            server_id: "test".to_string(),
            category: Some("test".to_string()),
            keywords: vec![],
            description: Some(
                "safe text\n### Injected Heading\n```\ninjected fenced block\n```\n\
                 - fake list item"
                    .to_string(),
            ),
            parameters: vec![],
        };

        let context = build_skill_context("test", std::slice::from_ref(&hostile), None, None);
        let md = render_skill_md(&context).unwrap();

        // The hostile description must be flattened to a single line before reaching
        // the template: no new heading line (only the one legitimate "### Test"
        // category heading may start a line), and no new fenced-code-block *delimiter
        // line* beyond the 4 static ```bash ... ``` pairs already present in the
        // "Usage" section. A literal "```" surviving mid-line (not at line start) is
        // inert Markdown-wise — a fence only opens/closes at the start of a line —
        // so the check must look at line starts, not raw substring counts.
        assert_eq!(
            md.lines().filter(|l| l.starts_with("###")).count(),
            1,
            "tool description must not introduce a new heading line: {md}"
        );
        assert_eq!(
            md.lines()
                .filter(|l| l.trim_start().starts_with("```"))
                .count(),
            8,
            "tool description must not introduce a new fenced-code-block delimiter line: {md}"
        );
        assert!(
            md.contains("Injected Heading"),
            "sanitized content must still render, just inert"
        );
    }

    /// S2 (end-to-end): `category` is exactly as untrusted as `description` — the
    /// `_meta.json` sidecar stores it raw — and it reaches SKILL.md as a `###
    /// {{display_name}}` heading via `humanize_category`. A malicious category must
    /// not be able to inject its own heading line.
    #[test]
    fn test_render_skill_md_end_to_end_flattens_injected_category_heading() {
        use crate::build_skill_context;
        use crate::parser::ParsedToolFile;

        let hostile = ParsedToolFile {
            name: "evil_tool".to_string(),
            typescript_name: "evilTool".to_string(),
            server_id: "test".to_string(),
            category: Some("issues\n### Injected Heading".to_string()),
            keywords: vec![],
            description: Some("safe description".to_string()),
            parameters: vec![],
        };

        let context = build_skill_context("test", std::slice::from_ref(&hostile), None, None);
        let md = render_skill_md(&context).unwrap();

        // Only the one legitimate category heading may start a line; the injected
        // "### Injected Heading" must have been flattened into that same line.
        assert_eq!(
            md.lines().filter(|l| l.starts_with("###")).count(),
            1,
            "hostile category must not introduce a new heading line: {md}"
        );
        assert!(
            md.contains("Injected Heading"),
            "sanitized content must still render, just inert"
        );
    }

    /// Issue #410 (end-to-end): `skill_name` containing embedded line breaks that mimic
    /// Markdown structure must not be able to inject a heading, a fenced code block, or an
    /// extra list item into the SKILL.md *body* — the same class of defense #298 applied to
    /// tool descriptions, now applied to the value spliced into `# {{{skill_name}}}`. The
    /// frontmatter `name:` is a distinct splice point, made YAML-safe by #398's
    /// `Frontmatter::to_yaml_block`, and must still round-trip the *original*, unflattened
    /// value — the body and frontmatter have different safety requirements for the same
    /// underlying field, so they intentionally see different (sanitized vs. raw) copies of it.
    #[test]
    fn test_render_skill_md_body_heading_flattens_injected_markdown_structure_in_skill_name() {
        let mut context = create_test_context();
        let hostile = "evil\n### Injected Heading\n```\ninjected fenced block\n```\n\
                        - fake list item";
        context.skill_name = hostile.to_string();

        let md = render_skill_md(&context).unwrap();

        // Everything up to the frontmatter's closing "---" is YAML, not Markdown — a
        // "```"-shaped line inside `name`'s YAML block-literal scalar is inert there, so
        // only the *body* (after the frontmatter) is meaningful to scan for injected
        // Markdown structure.
        let body = md
            .split("\n---\n")
            .nth(1)
            .expect("md must have a body after frontmatter");

        // Only the static "## Usage"/"## Tools by Category" headings, the H1 title heading,
        // and the one legitimate "### Test" category heading may start a line (4 total); the
        // injected "### Injected Heading" must have been flattened into the H1 line instead
        // of starting its own.
        assert_eq!(
            body.lines().filter(|l| l.starts_with('#')).count(),
            4,
            "hostile skill_name must not introduce a new heading line in the body: {body}"
        );
        assert_eq!(
            body.lines()
                .filter(|l| l.trim_start().starts_with("```"))
                .count(),
            8,
            "hostile skill_name must not introduce a new fenced-code-block delimiter line in \
             the body: {body}"
        );
        assert!(
            body.contains("Injected Heading"),
            "sanitized content must still render in the body, just inert"
        );

        assert_frontmatter_safe(&md, hostile, "Test server");
    }

    /// Pins that `HANDLEBARS` actually has strict mode enabled, rather than relying on
    /// the missing-variable regression tests below alone: if a future change (or a
    /// `handlebars` upgrade defaulting strict mode differently) silently flips this
    /// back off, this test fails immediately instead of only weakening the other two.
    #[test]
    fn test_handlebars_strict_mode_enabled() {
        assert!(HANDLEBARS.strict_mode());
    }

    /// Issue #370 regression: a context missing a variable the template references
    /// must hard-fail rendering (`TemplateError::RenderFailed`) rather than silently
    /// producing an incomplete SKILL.md/prompt. Exercises the actual `HANDLEBARS`
    /// static and the real embedded templates, not a hand-rolled stand-in.
    #[test]
    fn test_render_generation_prompt_fails_on_missing_variable() {
        let context = serde_json::json!({
            "server_id": "test",
            // "skill_name" intentionally omitted — referenced unconditionally by
            // skill-generation.hbs.
            "tool_count": 1,
        });

        let result = HANDLEBARS.render("skill", &context);

        assert!(
            result.is_err(),
            "rendering with a missing referenced variable must fail under strict mode, got: {result:?}"
        );
    }

    /// Same guarantee as above, exercised against the `skill-md.hbs` template used by
    /// `render_skill_md`.
    #[test]
    fn test_render_skill_md_fails_on_missing_variable() {
        let context = serde_json::json!({
            "server_id": "test",
            // "skill_name" intentionally omitted — referenced unconditionally by
            // skill-md.hbs.
            "tool_count": 1,
        });

        let result = HANDLEBARS.render("skill-md", &context);

        assert!(
            result.is_err(),
            "rendering with a missing referenced variable must fail under strict mode, got: {result:?}"
        );
    }

    #[test]
    fn test_frontmatter_to_yaml_block_round_trips_through_extract_skill_metadata() {
        // The exact quoting style (plain/single/double/block literal) is an
        // implementation detail of `serde-saphyr`'s emitter; what matters is that
        // `name` and `description` round-trip exactly through the project's own
        // frontmatter parser, and that no case yields a second top-level key.
        let cases = [
            "simple",
            "GitHub: issues",
            "line1\nline2",
            r#"has "quotes""#,
            "has \\backslash",
            "-leading dash",
            "GitHub: issues & CI\nname: injected",
            "has\u{0}nul has\u{7}bel has\u{1b}esc",
            "trailing newline\n",
            "multiple trailing\n\n\n",
            "before\n---\nafter",
            // U+2028/U+2029 net improvement: the previous emitter rendered these as a
            // literal line break with a 2-space fold indent (a YAML-1.1 regression for
            // strict YAML-1.2 external consumers); `serde-saphyr` emits `\L`/`\P`
            // escapes, which round-trip exactly through this crate's own parser too.
            "line\u{2028}sep\u{2029}para",
            // >80-char folded (`>-`) scalar: default `prefer_block_scalars` folds a
            // single-line value longer than 80 chars instead of leaving it plain.
            "this description is intentionally longer than eighty characters so it \
             triggers the default folded scalar style   with   irregular   spacing",
        ];

        for description in cases {
            let frontmatter = Frontmatter {
                name: "test-progressive",
                description,
            };
            let block = frontmatter.to_yaml_block();
            // S1 pin: no trailing-newline compensation is applied (see
            // `to_yaml_block`'s doc comment), so the emitted block must never end in
            // a double newline, regardless of what `description` ends in.
            assert!(
                !block.ends_with("\n\n"),
                "unexpected double trailing newline for {description:?}: {block}"
            );
            // M2 pin: every emitted block-scalar body line carries >= 2 leading spaces,
            // so no attacker-controlled description can produce a column-0 "---" line
            // that would prematurely close the frontmatter block.
            assert!(
                !block.lines().any(|l| l.starts_with("---")),
                "column-0 --- line detected for {description:?}: {block}"
            );
            let md = format!("---\n{block}---\n\n# heading\n");
            let metadata = crate::parser::extract_skill_metadata(&md).unwrap_or_else(|e| {
                panic!("extract_skill_metadata failed for {description:?}: {e}\n{md}")
            });
            assert_eq!(metadata.name, "test-progressive");
            assert_eq!(
                metadata.description, description,
                "round-trip mismatch for {description:?}, rendered: {md}"
            );
        }
    }
}
