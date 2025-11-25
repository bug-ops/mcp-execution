//! Token consumption report generator for categorized skills.
//!
//! Generates a detailed report comparing monolithic vs categorized skill
//! token consumption across various usage scenarios.
//!
//! Run with: `cargo run --example token_report --features skills`

use mcp_codegen::skills::SkillOrchestrator;
use mcp_core::{ServerId, SkillDescription, SkillName, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

/// Approximates token count using simple heuristic: words * 1.3
fn count_tokens(text: &str) -> usize {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let tokens = (text.split_whitespace().count() as f64 * 1.3) as usize;
    tokens
}

/// Creates a realistic GitHub-like server with 40 tools.
fn create_github_server() -> ServerInfo {
    let mut tools = Vec::new();

    // User operations (5 tools)
    tools.push(ToolInfo {
        name: ToolName::new("get_me"),
        description: "Get details of the authenticated GitHub user.".to_string(),
        input_schema: json!({"type": "object", "properties": {}}),
        output_schema: None,
    });
    tools.push(ToolInfo {
        name: ToolName::new("get_teams"),
        description: "Get teams the user is a member of.".to_string(),
        input_schema: json!({"type": "object", "properties": {"user": {"type": "string"}}}),
        output_schema: None,
    });
    tools.push(ToolInfo {
        name: ToolName::new("search_users"),
        description: "Find GitHub users by username or profile information.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"]
        }),
        output_schema: None,
    });
    tools.push(ToolInfo {
        name: ToolName::new("get_team_members"),
        description: "List members of a specific team.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {"org": {"type": "string"}, "team": {"type": "string"}},
            "required": ["org", "team"]
        }),
        output_schema: None,
    });
    tools.push(ToolInfo {
        name: ToolName::new("get_user_profile"),
        description: "Get detailed profile for any GitHub user.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {"username": {"type": "string"}},
            "required": ["username"]
        }),
        output_schema: None,
    });

    // Repository operations (12 tools)
    for tool_name in [
        "create_branch",
        "list_commits",
        "get_file_contents",
        "create_repository",
        "fork_repository",
        "list_branches",
        "get_repository",
        "list_tags",
        "get_commit",
        "compare_commits",
        "list_contributors",
        "update_file",
    ] {
        tools.push(ToolInfo {
            name: ToolName::new(tool_name),
            description: format!("Repository operation: {tool_name}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        });
    }

    // Issue operations (8 tools)
    for tool_name in [
        "issue_read",
        "issue_write",
        "list_issues",
        "add_issue_comment",
        "update_issue",
        "add_label_to_issue",
        "assign_issue",
        "close_issue",
    ] {
        tools.push(ToolInfo {
            name: ToolName::new(tool_name),
            description: format!("Issue operation: {tool_name}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        });
    }

    // Pull request operations (10 tools)
    for tool_name in [
        "pull_request_read",
        "create_pull_request",
        "list_pull_requests",
        "add_pr_comment",
        "merge_pull_request",
        "add_review_comment",
        "request_reviewers",
        "get_pr_reviews",
        "update_pull_request",
        "close_pull_request",
    ] {
        tools.push(ToolInfo {
            name: ToolName::new(tool_name),
            description: format!("Pull request operation: {tool_name}"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        });
    }

    // Search operations (5 tools)
    for tool_name in [
        "search_code",
        "search_repositories",
        "search_issues",
        "search_commits",
        "search_topics",
    ] {
        tools.push(ToolInfo {
            name: ToolName::new(tool_name),
            description: format!("Search operation: {tool_name}"),
            input_schema: json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"]
            }),
            output_schema: None,
        });
    }

    ServerInfo {
        id: ServerId::new("github"),
        name: "GitHub MCP Server".to_string(),
        version: "1.0.0".to_string(),
        tools,
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Token Consumption Report ===\n");
    println!("Comparing monolithic vs categorized skill generation");
    println!("for a GitHub-like MCP server with 40 tools.\n");

    let server_info = create_github_server();
    let orchestrator = SkillOrchestrator::new()?;
    let skill_name = SkillName::new("github")?;
    let skill_desc = SkillDescription::new("GitHub MCP Server integration for Claude Code")?;

    // Generate monolithic bundle
    println!("Generating monolithic skill...");
    let mono = orchestrator.generate_bundle(&server_info, &skill_name, &skill_desc)?;

    // Calculate token counts
    let mono_skill_tokens = count_tokens(mono.skill_md());
    let mono_ref_tokens = count_tokens(mono.reference_md().unwrap_or_default());
    let mono_total_tokens = mono_skill_tokens + mono_ref_tokens;

    println!("\n## Monolithic Implementation (Current)\n");
    println!("SKILL.md: {mono_skill_tokens} tokens");
    println!("REFERENCE.md: {mono_ref_tokens} tokens");
    println!("Total per invocation: {mono_total_tokens} tokens\n");

    // TODO: Enable when categorized implementation is ready
    /*
    println!("Generating categorized skill...");
    let cat = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc)?;

    let cat_entry_tokens = count_tokens(cat.skill_md());

    println!("## Categorized Implementation (Proposed)\n");
    println!("SKILL.md (entry point): {} tokens", cat_entry_tokens);
    println!("Entry point reduction: {:.1}%\n",
        100.0 * (1.0 - cat_entry_tokens as f64 / mono_total_tokens as f64));

    println!("Categories:");
    let mut category_tokens = Vec::new();
    for (cat, content) in cat.categories() {
        let tokens = count_tokens(content);
        category_tokens.push((cat.as_str().to_string(), tokens));
        println!("  - {}: {} tokens", cat.as_str(), tokens);
    }

    let total_cat_tokens: usize = category_tokens.iter().map(|(_, t)| t).sum();
    let avg_cat_tokens = total_cat_tokens / category_tokens.len();

    println!("\n## Usage Scenarios\n");

    // Scenario 1: Entry point only (discovery)
    println!("1. Discovery (entry point only):");
    println!("   Tokens: {} ({:.1}% reduction)",
        cat_entry_tokens,
        100.0 * (1.0 - cat_entry_tokens as f64 / mono_total_tokens as f64));

    // Scenario 2: Single tool call (1 category)
    let single_tool_tokens = cat_entry_tokens + avg_cat_tokens;
    println!("\n2. Single tool call (1 category):");
    println!("   Tokens: {} ({:.1}% reduction)",
        single_tool_tokens,
        100.0 * (1.0 - single_tool_tokens as f64 / mono_total_tokens as f64));

    // Scenario 3: Multi-tool workflow (2 categories)
    let multi_tool_tokens = cat_entry_tokens + 2 * avg_cat_tokens;
    println!("\n3. Multi-tool workflow (2 categories):");
    println!("   Tokens: {} ({:.1}% reduction)",
        multi_tool_tokens,
        100.0 * (1.0 - multi_tool_tokens as f64 / mono_total_tokens as f64));

    // Scenario 4: Complex workflow (5 categories)
    let complex_tokens = cat_entry_tokens + 5 * avg_cat_tokens;
    println!("\n4. Complex workflow (5 categories):");
    println!("   Tokens: {} ({:.1}% reduction)",
        complex_tokens,
        100.0 * (1.0 - complex_tokens as f64 / mono_total_tokens as f64));

    // Scenario 5: Worst case (all categories)
    let worst_case_tokens = cat_entry_tokens + total_cat_tokens;
    println!("\n5. Worst case (all categories):");
    println!("   Tokens: {} ({:.1}% reduction)",
        worst_case_tokens,
        100.0 * (1.0 - worst_case_tokens as f64 / mono_total_tokens as f64));

    println!("\n## Validation Against Targets\n");
    let single_reduction = 100.0 * (1.0 - single_tool_tokens as f64 / mono_total_tokens as f64);
    println!("Target: ≥85% reduction for single tool calls");
    println!("Actual: {:.1}%", single_reduction);
    if single_reduction >= 85.0 {
        println!("✅ PASS - Target exceeded");
    } else {
        println!("❌ FAIL - Target not met");
    }

    // Check category sizes
    println!("\nTarget: Category files <1000 tokens each");
    let max_cat_tokens = category_tokens.iter().map(|(_, t)| t).max().unwrap();
    println!("Actual: Max {} tokens", max_cat_tokens);
    if *max_cat_tokens < 1000 {
        println!("✅ PASS - All categories under limit");
    } else {
        println!("❌ FAIL - Some categories exceed limit");
    }
    */

    println!("\n## Note\n");
    println!("Categorized implementation is not yet complete.");
    println!("Enable the commented sections once implementation is ready.\n");

    Ok(())
}
