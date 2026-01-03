//! Context builder for skill generation.
//!
//! Transforms parsed tool files into structured context
//! that the LLM uses to generate SKILL.md content.

use crate::parser::ParsedToolFile;
use crate::types::{GenerateSkillResult, SkillCategory, SkillTool, ToolExample};
use std::collections::HashMap;

/// Build skill generation context from parsed tools.
///
/// # Arguments
///
/// * `server_id` - Server identifier (e.g., "github")
/// * `tools` - Parsed tool files from `scan_tools_directory`
/// * `use_case_hints` - Optional hints about intended use cases
///
/// # Returns
///
/// `GenerateSkillResult` with all context needed for skill generation.
///
/// # Examples
///
/// ```
/// use mcp_server::skill::{build_skill_context, ParsedToolFile, ParsedParameter};
///
/// let tools: Vec<ParsedToolFile> = vec![]; // Parsed from scan_tools_directory
/// let context = build_skill_context("github", &tools, None);
///
/// assert_eq!(context.server_id, "github");
/// ```
#[must_use]
pub fn build_skill_context(
    server_id: &str,
    tools: &[ParsedToolFile],
    use_case_hints: Option<&[String]>,
) -> GenerateSkillResult {
    let tool_count = tools.len();

    // Group tools by category
    let categories = group_by_category(tools);

    // Select representative examples
    let example_tools = select_example_tools(tools, 5);

    // Generate skill name
    let skill_name = format!("{server_id}-progressive");

    // Build output path
    let output_path = format!("~/.claude/skills/{server_id}/SKILL.md");

    // Render generation prompt
    let generation_prompt = build_generation_prompt(
        server_id,
        &skill_name,
        &categories,
        &example_tools,
        use_case_hints,
    );

    GenerateSkillResult {
        server_id: server_id.to_string(),
        skill_name,
        server_description: infer_server_description(tools),
        categories,
        tool_count,
        example_tools,
        generation_prompt,
        output_path,
    }
}

/// Group tools by category.
///
/// Tools without a category are placed in "uncategorized".
fn group_by_category(tools: &[ParsedToolFile]) -> Vec<SkillCategory> {
    let mut category_map: HashMap<String, Vec<SkillTool>> = HashMap::new();

    for tool in tools {
        let category = tool
            .category
            .clone()
            .unwrap_or_else(|| "uncategorized".to_string());

        let skill_tool = SkillTool {
            name: tool.name.clone(),
            typescript_name: tool.typescript_name.clone(),
            description: tool
                .description
                .clone()
                .unwrap_or_else(|| format!("{} tool", tool.name)),
            keywords: tool.keywords.clone(),
            required_params: tool
                .parameters
                .iter()
                .filter(|p| p.required)
                .map(|p| p.name.clone())
                .collect(),
            optional_params: tool
                .parameters
                .iter()
                .filter(|p| !p.required)
                .map(|p| p.name.clone())
                .collect(),
        };

        category_map.entry(category).or_default().push(skill_tool);
    }

    // Convert to sorted vector
    let mut categories: Vec<SkillCategory> = category_map
        .into_iter()
        .map(|(name, tools)| {
            let display_name = humanize_category(&name);
            SkillCategory {
                name,
                display_name,
                tools,
            }
        })
        .collect();

    // Sort categories alphabetically, but put "uncategorized" last
    categories.sort_by(|a, b| {
        if a.name == "uncategorized" {
            std::cmp::Ordering::Greater
        } else if b.name == "uncategorized" {
            std::cmp::Ordering::Less
        } else {
            a.name.cmp(&b.name)
        }
    });

    categories
}

/// Convert category slug to human-readable name.
fn humanize_category(name: &str) -> String {
    name.split('-')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Select representative example tools.
///
/// Prioritizes common CRUD operations and picks one per category.
fn select_example_tools(tools: &[ParsedToolFile], max_examples: usize) -> Vec<ToolExample> {
    // Priority keywords for example selection
    let priority_prefixes = ["create", "list", "get", "search", "update"];

    let mut examples = Vec::new();
    let mut seen_categories = std::collections::HashSet::new();

    // First pass: pick priority operations from different categories
    for prefix in priority_prefixes {
        if examples.len() >= max_examples {
            break;
        }

        for tool in tools {
            if examples.len() >= max_examples {
                break;
            }

            let category = tool.category.as_deref().unwrap_or("uncategorized");

            if tool.name.starts_with(prefix) && !seen_categories.contains(category) {
                examples.push(build_tool_example(tool));
                seen_categories.insert(category.to_string());
            }
        }
    }

    // Second pass: fill remaining slots
    for tool in tools {
        if examples.len() >= max_examples {
            break;
        }

        let category = tool.category.as_deref().unwrap_or("uncategorized");

        if !seen_categories.contains(category) {
            examples.push(build_tool_example(tool));
            seen_categories.insert(category.to_string());
        }
    }

    examples
}

/// Build example for a single tool.
fn build_tool_example(tool: &ParsedToolFile) -> ToolExample {
    // Build example params
    let params: HashMap<&str, &str> = tool
        .parameters
        .iter()
        .filter(|p| p.required)
        .map(|p| (p.name.as_str(), get_example_value(&p.typescript_type)))
        .collect();

    let params_json = serde_json::to_string_pretty(&params).unwrap_or_else(|_| "{}".to_string());

    // Build CLI command
    let cli_command = format!(
        "node ~/.claude/servers/{}/{}.ts '{}'",
        tool.server_id,
        tool.typescript_name,
        params_json.replace('\n', " ").replace("  ", "")
    );

    ToolExample {
        tool_name: tool.name.clone(),
        description: tool
            .description
            .clone()
            .unwrap_or_else(|| format!("Execute {}", tool.name)),
        cli_command,
        params_json,
    }
}

/// Get example value for TypeScript type.
fn get_example_value(ts_type: &str) -> &'static str {
    match ts_type.trim() {
        "string" => "\"example\"",
        "number" => "42",
        "boolean" => "true",
        t if t.starts_with("string[]") => "[\"item1\", \"item2\"]",
        t if t.starts_with("number[]") => "[1, 2, 3]",
        _ => "\"...\"",
    }
}

/// Infer server description from tool metadata.
fn infer_server_description(tools: &[ParsedToolFile]) -> Option<String> {
    if tools.is_empty() {
        return None;
    }

    // Get unique categories
    let categories: std::collections::HashSet<_> =
        tools.iter().filter_map(|t| t.category.as_ref()).collect();

    if categories.is_empty() {
        return Some(format!("MCP server with {} tools", tools.len()));
    }

    let category_list: Vec<_> = categories.iter().map(|s| s.as_str()).collect();
    Some(format!(
        "MCP server for {} operations ({} tools)",
        category_list.join(", "),
        tools.len()
    ))
}

/// Build the generation prompt.
#[allow(clippy::format_push_string)]
fn build_generation_prompt(
    server_id: &str,
    skill_name: &str,
    categories: &[SkillCategory],
    examples: &[ToolExample],
    use_case_hints: Option<&[String]>,
) -> String {
    // Pre-allocate String capacity to reduce reallocations
    // Estimate: 500 base + 100/category + 200/example
    let estimated_size = 500 + (categories.len() * 100) + (examples.len() * 200);
    let mut prompt = String::with_capacity(estimated_size);

    prompt.push_str(&format!(
        r#"You are generating a Claude Code skill file (SKILL.md) for the "{server_id}" MCP server.

## Context

**Server ID**: {server_id}
**Skill Name**: {skill_name}
**Total Tools**: {}

### Categories and Tools

"#,
        categories.iter().map(|c| c.tools.len()).sum::<usize>()
    ));

    for category in categories {
        prompt.push_str(&format!(
            "#### {} ({} tools)\n",
            category.display_name,
            category.tools.len()
        ));

        for tool in &category.tools {
            prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));

            if !tool.keywords.is_empty() {
                prompt.push_str(&format!("  - Keywords: {}\n", tool.keywords.join(", ")));
            }

            if !tool.required_params.is_empty() {
                prompt.push_str(&format!(
                    "  - Required params: {}\n",
                    tool.required_params.join(", ")
                ));
            }
        }

        prompt.push('\n');
    }

    prompt.push_str("### Example Tool Usages\n\n");

    for example in examples {
        prompt.push_str(&format!(
            "**{}**\n```bash\n{}\n```\n\n",
            example.description, example.cli_command
        ));
    }

    if let Some(hints) = use_case_hints {
        prompt.push_str("### Use Case Hints\n\n");
        for hint in hints {
            prompt.push_str(&format!("- {hint}\n"));
        }
        prompt.push('\n');
    }

    prompt.push_str(GENERATION_INSTRUCTIONS);

    prompt
}

const GENERATION_INSTRUCTIONS: &str = r#"
## Instructions

Generate a SKILL.md file with the following structure:

1. **YAML Frontmatter** (required):
   ```yaml
   ---
   name: {skill_name}
   description: [One-sentence description of what this skill enables]
   ---
   ```

2. **Introduction** (1-2 paragraphs):
   - What this server/skill does
   - Key capabilities in bullet points
   - When to use this skill

3. **Quick Start** (numbered steps):
   - How to discover available tools
   - How to execute a tool
   - Example with a common use case

4. **Common Tasks** (3-5 sections):
   - Organize by USE CASE, not by tool
   - Each section should solve a real problem
   - Include natural language examples that trigger tool usage
   - Show CLI commands where helpful

5. **Tool Reference** (organized by category):
   - List all tools by category
   - Brief description of each
   - Key parameters

6. **Troubleshooting** (3-5 items):
   - Common errors and solutions
   - Authentication issues
   - Connection problems

## Guidelines

- Write for AI agents (Claude), not humans
- Focus on WHEN to use tools, not just HOW
- Use natural language examples: "Create an issue about the login bug"
- Keep descriptions concise but informative
- Include path references: ~/.claude/servers/{server_id}/

## Output Format

Return ONLY the SKILL.md content, starting with the YAML frontmatter.
Do not include any explanation or commentary outside the file content.
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedParameter;

    fn create_test_tool(name: &str, category: Option<&str>) -> ParsedToolFile {
        ParsedToolFile {
            name: name.to_string(),
            typescript_name: name.to_string(),
            server_id: "test".to_string(),
            category: category.map(ToString::to_string),
            keywords: vec!["test".to_string()],
            description: Some(format!("{name} description")),
            parameters: vec![ParsedParameter {
                name: "param1".to_string(),
                typescript_type: "string".to_string(),
                required: true,
                description: None,
            }],
        }
    }

    #[test]
    fn test_build_skill_context() {
        let tools = vec![
            create_test_tool("create_issue", Some("issues")),
            create_test_tool("list_repos", Some("repos")),
        ];

        let context = build_skill_context("github", &tools, None);

        assert_eq!(context.server_id, "github");
        assert_eq!(context.skill_name, "github-progressive");
        assert_eq!(context.tool_count, 2);
        assert_eq!(context.categories.len(), 2);
        assert!(!context.generation_prompt.is_empty());
    }

    #[test]
    fn test_group_by_category() {
        let tools = vec![
            create_test_tool("tool1", Some("cat-a")),
            create_test_tool("tool2", Some("cat-b")),
            create_test_tool("tool3", Some("cat-a")),
            create_test_tool("tool4", None),
        ];

        let categories = group_by_category(&tools);

        assert_eq!(categories.len(), 3);

        // cat-a should have 2 tools
        let cat_a = categories.iter().find(|c| c.name == "cat-a").unwrap();
        assert_eq!(cat_a.tools.len(), 2);

        // uncategorized should be last
        assert_eq!(categories.last().unwrap().name, "uncategorized");
    }

    #[test]
    fn test_humanize_category() {
        assert_eq!(humanize_category("issues"), "Issues");
        assert_eq!(humanize_category("pull-requests"), "Pull Requests");
        assert_eq!(humanize_category("user-management"), "User Management");
    }

    #[test]
    fn test_select_example_tools() {
        let tools = vec![
            create_test_tool("create_issue", Some("issues")),
            create_test_tool("list_repos", Some("repos")),
            create_test_tool("get_user", Some("users")),
            create_test_tool("update_pr", Some("prs")),
            create_test_tool("delete_branch", Some("branches")),
        ];

        let examples = select_example_tools(&tools, 3);

        assert_eq!(examples.len(), 3);
        // Should prioritize create, list, get
        assert!(examples.iter().any(|e| e.tool_name == "create_issue"));
        assert!(examples.iter().any(|e| e.tool_name == "list_repos"));
        assert!(examples.iter().any(|e| e.tool_name == "get_user"));
    }

    #[test]
    fn test_get_example_value() {
        assert_eq!(get_example_value("string"), "\"example\"");
        assert_eq!(get_example_value("number"), "42");
        assert_eq!(get_example_value("boolean"), "true");
        assert_eq!(get_example_value("string[]"), "[\"item1\", \"item2\"]");
    }
}
