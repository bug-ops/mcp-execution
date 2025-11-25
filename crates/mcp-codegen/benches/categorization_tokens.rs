//! Token consumption benchmarks for categorized skills.
//!
//! Validates the 85-93% token reduction claims by comparing:
//! - Monolithic SKILL.md (all tools in one file)
//! - Categorized structure (minimal SKILL.md + category files)
//!
//! Measures:
//! 1. Token consumption for various usage scenarios
//! 2. Generation performance overhead
//! 3. Real-world workflow simulations
//!
//! Run with: `cargo bench --package mcp-codegen --bench categorization_tokens`

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mcp_codegen::skills::SkillOrchestrator;
use mcp_core::{ServerId, SkillDescription, SkillName, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::hint::black_box;

// ============================================================================
// Token Counting
// ============================================================================

/// Approximates token count using simple heuristic: words * 1.3
///
/// This matches the rough approximation used in design docs.
/// For more accuracy, could use tiktoken, but this is sufficient for benchmarking.
fn count_tokens(text: &str) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let tokens = (text.split_whitespace().count() as f64 * 1.3) as usize;
    tokens
}

/// Counts tokens in a skill bundle's SKILL.md
fn count_skill_md_tokens(skill_md: &str) -> usize {
    count_tokens(skill_md)
}

/// Counts tokens in REFERENCE.md
fn count_reference_md_tokens(reference_md: &str) -> usize {
    count_tokens(reference_md)
}

// ============================================================================
// Test Data: Realistic GitHub Server
// ============================================================================

/// Creates a realistic GitHub-like server with 40 tools categorized by domain.
///
/// Categories (matching design docs):
/// - User operations: 5 tools (`get_me`, `get_teams`, `search_users`, etc.)
/// - Repository operations: 12 tools (`create_branch`, `list_commits`, `get_file_contents`, etc.)
/// - Issue operations: 8 tools (`issue_read`, `issue_write`, `list_issues`, etc.)
/// - Pull request operations: 10 tools (`pull_request_read`, `create_pull_request`, etc.)
/// - Search operations: 5 tools (`search_code`, `search_repositories`, etc.)
fn create_github_server() -> ServerInfo {
    let mut tools = Vec::new();

    // User operations (5 tools)
    tools.extend(create_user_tools());

    // Repository operations (12 tools)
    tools.extend(create_repo_tools());

    // Issue operations (8 tools)
    tools.extend(create_issue_tools());

    // Pull request operations (10 tools)
    tools.extend(create_pr_tools());

    // Search operations (5 tools)
    tools.extend(create_search_tools());

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

fn create_user_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: ToolName::new("get_me"),
            description: "Get details of the authenticated GitHub user. Returns user profile with login, name, email, and public repos count.".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("get_teams"),
            description: "Get teams the user is a member of. Returns a list of team names and their repositories.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "user": {"type": "string", "description": "Username. Defaults to authenticated user."}
                }
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("search_users"),
            description: "Find GitHub users by username or profile information. Supports location, language, and other filters.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query (e.g., 'location:seattle')"},
                    "page": {"type": "number", "description": "Page number"},
                    "perPage": {"type": "number", "description": "Results per page (max 100)"}
                },
                "required": ["query"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("get_team_members"),
            description: "List members of a specific team. Returns usernames and roles.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "org": {"type": "string", "description": "Organization name"},
                    "team": {"type": "string", "description": "Team slug"}
                },
                "required": ["org", "team"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("get_user_profile"),
            description: "Get detailed profile for any GitHub user. Returns bio, company, location, blog, and social accounts.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "username": {"type": "string", "description": "GitHub username"}
                },
                "required": ["username"]
            }),
            output_schema: None,
        },
    ]
}

fn create_repo_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: ToolName::new("create_branch"),
            description: "Create a new branch in a repository from a specified ref or the default branch.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "branch": {"type": "string"},
                    "from_branch": {"type": "string"}
                },
                "required": ["owner", "repo", "branch"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("list_commits"),
            description: "List commits in a repository. Supports filtering by branch, author, and date range.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "branch": {"type": "string"},
                    "perPage": {"type": "number"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("get_file_contents"),
            description: "Get contents of a file from a repository. Supports specific commits or branches.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "path": {"type": "string"},
                    "ref": {"type": "string"}
                },
                "required": ["owner", "repo", "path"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("create_repository"),
            description: "Create a new GitHub repository. Can be public or private with optional initialization.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "description": {"type": "string"},
                    "private": {"type": "boolean"},
                    "auto_init": {"type": "boolean"}
                },
                "required": ["name"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("fork_repository"),
            description: "Fork an existing repository to your account or organization.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "organization": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("list_branches"),
            description: "List all branches in a repository with their latest commit info.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("get_repository"),
            description: "Get detailed information about a repository including stars, forks, and topics.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("list_tags"),
            description: "List tags in a repository with commit SHA and tagger info.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("get_commit"),
            description: "Get detailed information about a specific commit including diff stats and files changed.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "sha": {"type": "string"}
                },
                "required": ["owner", "repo", "sha"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("compare_commits"),
            description: "Compare two commits and get the diff summary.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "base": {"type": "string"},
                    "head": {"type": "string"}
                },
                "required": ["owner", "repo", "base", "head"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("list_contributors"),
            description: "List contributors to a repository with their contribution counts.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("update_file"),
            description: "Update or create a file in a repository with a commit message.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "message": {"type": "string"},
                    "branch": {"type": "string"}
                },
                "required": ["owner", "repo", "path", "content", "message"]
            }),
            output_schema: None,
        },
    ]
}

fn create_issue_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: ToolName::new("issue_read"),
            description: "Get details of a specific issue including comments, labels, and assignees.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "issue_number": {"type": "number"}
                },
                "required": ["owner", "repo", "issue_number"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("issue_write"),
            description: "Create a new issue with title, body, labels, and assignees.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "title": {"type": "string"},
                    "body": {"type": "string"},
                    "labels": {"type": "array", "items": {"type": "string"}},
                    "assignees": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["owner", "repo", "title"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("list_issues"),
            description: "List issues in a repository with filtering by state, labels, assignees, and milestone.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "state": {"type": "string", "enum": ["open", "closed", "all"]},
                    "labels": {"type": "string"},
                    "page": {"type": "number"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("add_issue_comment"),
            description: "Add a comment to an existing issue.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "issue_number": {"type": "number"},
                    "body": {"type": "string"}
                },
                "required": ["owner", "repo", "issue_number", "body"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("update_issue"),
            description: "Update an issue's title, body, state, labels, or assignees.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "issue_number": {"type": "number"},
                    "title": {"type": "string"},
                    "body": {"type": "string"},
                    "state": {"type": "string"}
                },
                "required": ["owner", "repo", "issue_number"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("add_label_to_issue"),
            description: "Add labels to an issue.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "issue_number": {"type": "number"},
                    "labels": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["owner", "repo", "issue_number", "labels"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("assign_issue"),
            description: "Assign users to an issue.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "issue_number": {"type": "number"},
                    "assignees": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["owner", "repo", "issue_number", "assignees"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("close_issue"),
            description: "Close an issue with an optional comment.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "issue_number": {"type": "number"},
                    "comment": {"type": "string"}
                },
                "required": ["owner", "repo", "issue_number"]
            }),
            output_schema: None,
        },
    ]
}

fn create_pr_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: ToolName::new("pull_request_read"),
            description:
                "Get details of a pull request including diff, reviews, and status checks."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"}
                },
                "required": ["owner", "repo", "pull_number"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("create_pull_request"),
            description: "Create a new pull request from a branch.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "title": {"type": "string"},
                    "body": {"type": "string"},
                    "head": {"type": "string"},
                    "base": {"type": "string"}
                },
                "required": ["owner", "repo", "title", "head", "base"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("list_pull_requests"),
            description: "List pull requests in a repository with filtering options.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "state": {"type": "string"},
                    "page": {"type": "number"}
                },
                "required": ["owner", "repo"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("add_pr_comment"),
            description: "Add a comment to a pull request discussion.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"},
                    "body": {"type": "string"}
                },
                "required": ["owner", "repo", "pull_number", "body"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("merge_pull_request"),
            description: "Merge a pull request using specified merge method.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"},
                    "merge_method": {"type": "string", "enum": ["merge", "squash", "rebase"]}
                },
                "required": ["owner", "repo", "pull_number"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("add_review_comment"),
            description: "Add a review comment on a specific line of a pull request diff."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"},
                    "body": {"type": "string"},
                    "path": {"type": "string"},
                    "line": {"type": "number"}
                },
                "required": ["owner", "repo", "pull_number", "body", "path", "line"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("request_reviewers"),
            description: "Request reviews from specific users or teams.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"},
                    "reviewers": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["owner", "repo", "pull_number", "reviewers"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("get_pr_reviews"),
            description: "Get all reviews submitted for a pull request.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"}
                },
                "required": ["owner", "repo", "pull_number"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("update_pull_request"),
            description: "Update a pull request's title, body, or base branch.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"},
                    "title": {"type": "string"},
                    "body": {"type": "string"}
                },
                "required": ["owner", "repo", "pull_number"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("close_pull_request"),
            description: "Close a pull request without merging.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "repo": {"type": "string"},
                    "pull_number": {"type": "number"}
                },
                "required": ["owner", "repo", "pull_number"]
            }),
            output_schema: None,
        },
    ]
}

fn create_search_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: ToolName::new("search_code"),
            description: "Search for code across GitHub repositories with advanced filters."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "page": {"type": "number"},
                    "perPage": {"type": "number"}
                },
                "required": ["query"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("search_repositories"),
            description: "Search for repositories by name, description, or topics.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "sort": {"type": "string"},
                    "order": {"type": "string"},
                    "page": {"type": "number"}
                },
                "required": ["query"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("search_issues"),
            description: "Search for issues and pull requests across repositories.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "sort": {"type": "string"},
                    "page": {"type": "number"}
                },
                "required": ["query"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("search_commits"),
            description: "Search for commits by message, author, or date.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "sort": {"type": "string"},
                    "page": {"type": "number"}
                },
                "required": ["query"]
            }),
            output_schema: None,
        },
        ToolInfo {
            name: ToolName::new("search_topics"),
            description: "Search for repository topics across GitHub.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "page": {"type": "number"}
                },
                "required": ["query"]
            }),
            output_schema: None,
        },
    ]
}

// ============================================================================
// Benchmark 1: Token Consumption Comparison
// ============================================================================

/// Benchmarks token consumption: monolithic vs categorized.
///
/// This is the PRIMARY validation benchmark for the 85%+ reduction claim.
fn bench_token_consumption_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_consumption");

    let server_info = create_github_server();
    let orchestrator = SkillOrchestrator::new().unwrap();
    let skill_name = SkillName::new("github").unwrap();
    let skill_desc =
        SkillDescription::new("GitHub MCP Server integration for Claude Code").unwrap();

    // Benchmark 1: Monolithic skill generation (current implementation)
    group.bench_function("monolithic_full_load", |b| {
        b.iter(|| {
            let bundle = orchestrator
                .generate_bundle(
                    black_box(&server_info),
                    black_box(&skill_name),
                    black_box(&skill_desc),
                )
                .unwrap();

            // Count tokens in SKILL.md + REFERENCE.md (both loaded together)
            let skill_tokens = count_skill_md_tokens(bundle.skill_md());
            let reference_tokens =
                count_reference_md_tokens(bundle.reference_md().unwrap_or_default());
            let total = skill_tokens + reference_tokens;

            black_box(total)
        });
    });

    // TODO: Enable when categorized implementation is ready
    // Benchmark 2: Categorized skill - minimal SKILL.md only (entry point)
    // group.bench_function("categorized_entry_point", |b| {
    //     b.iter(|| {
    //         let bundle = orchestrator
    //             .generate_categorized_bundle(
    //                 black_box(&server_info),
    //                 black_box(&skill_name),
    //                 black_box(&skill_desc),
    //             )
    //             .unwrap();
    //
    //         // Count tokens in minimal SKILL.md only
    //         let tokens = count_skill_md_tokens(bundle.skill_md());
    //         black_box(tokens)
    //     });
    // });

    // TODO: Enable when categorized implementation is ready
    // Benchmark 3: Categorized skill - single category load (typical use case)
    // group.bench_function("categorized_single_category", |b| {
    //     b.iter(|| {
    //         let bundle = orchestrator
    //             .generate_categorized_bundle(
    //                 black_box(&server_info),
    //                 black_box(&skill_name),
    //                 black_box(&skill_desc),
    //             )
    //             .unwrap();
    //
    //         // Count tokens: SKILL.md + one category
    //         let skill_tokens = count_skill_md_tokens(bundle.skill_md());
    //         let category = bundle.categories().iter().next().unwrap().1;
    //         let category_tokens = count_tokens(category);
    //         let total = skill_tokens + category_tokens;
    //
    //         black_box(total)
    //     });
    // });

    // TODO: Enable when categorized implementation is ready
    // Benchmark 4: Categorized skill - all categories (worst case)
    // group.bench_function("categorized_all_categories", |b| {
    //     b.iter(|| {
    //         let bundle = orchestrator
    //             .generate_categorized_bundle(
    //                 black_box(&server_info),
    //                 black_box(&skill_name),
    //                 black_box(&skill_desc),
    //             )
    //             .unwrap();
    //
    //         // Count tokens: SKILL.md + all categories
    //         let mut total = count_skill_md_tokens(bundle.skill_md());
    //         for content in bundle.categories().values() {
    //             total += count_tokens(content);
    //         }
    //
    //         black_box(total)
    //     });
    // });

    group.finish();
}

// ============================================================================
// Benchmark 2: Generation Performance
// ============================================================================

/// Benchmarks generation overhead: categorized vs monolithic.
///
/// Validates that categorization doesn't add significant generation time.
/// Target: <100ms additional overhead.
fn bench_generation_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("generation_performance");

    let server_info = create_github_server();
    let orchestrator = SkillOrchestrator::new().unwrap();
    let skill_name = SkillName::new("github").unwrap();
    let skill_desc =
        SkillDescription::new("GitHub MCP Server integration for Claude Code").unwrap();

    // Monolithic generation (baseline)
    group.bench_function("generate_monolithic", |b| {
        b.iter(|| {
            let bundle = orchestrator
                .generate_bundle(
                    black_box(&server_info),
                    black_box(&skill_name),
                    black_box(&skill_desc),
                )
                .unwrap();
            black_box(bundle)
        });
    });

    // TODO: Enable when categorized implementation is ready
    // Categorized generation
    // group.bench_function("generate_categorized", |b| {
    //     b.iter(|| {
    //         let bundle = orchestrator
    //             .generate_categorized_bundle(
    //                 black_box(&server_info),
    //                 black_box(&skill_name),
    //                 black_box(&skill_desc),
    //             )
    //             .unwrap();
    //         black_box(bundle)
    //     });
    // });

    // TODO: Enable when manifest generation is ready
    // Manifest generation only
    // group.bench_function("generate_manifest", |b| {
    //     b.iter(|| {
    //         let manifest_gen = ManifestGenerator::new();
    //         let manifest = manifest_gen.generate(black_box(&server_info.tools)).unwrap();
    //         black_box(manifest)
    //     });
    // });

    group.finish();
}

// ============================================================================
// Benchmark 3: Category Loading Simulation
// ============================================================================

/// Simulates real-world usage scenarios and measures token consumption.
///
/// Scenarios based on typical GitHub workflows:
/// - User info lookup (1 tool from user category)
/// - Create issue (1 tool from issues category)
/// - PR workflow (3 tools from prs category)
/// - Repo analysis (3 tools from repos + search categories)
fn bench_category_loading_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("category_loading");

    // Real-world scenarios
    let scenarios = vec![
        ("user_info", vec!["get_me"]),
        ("create_issue", vec!["issue_write"]),
        (
            "pr_workflow",
            vec!["pull_request_read", "add_pr_comment", "get_pr_reviews"],
        ),
        (
            "repo_analysis",
            vec!["list_commits", "get_file_contents", "search_code"],
        ),
    ];

    let server_info = create_github_server();
    let orchestrator = SkillOrchestrator::new().unwrap();
    let skill_name = SkillName::new("github").unwrap();
    let skill_desc =
        SkillDescription::new("GitHub MCP Server integration for Claude Code").unwrap();

    // Generate monolithic bundle once for all scenarios
    let mono_bundle = orchestrator
        .generate_bundle(&server_info, &skill_name, &skill_desc)
        .unwrap();
    let mono_tokens = count_skill_md_tokens(mono_bundle.skill_md())
        + count_reference_md_tokens(mono_bundle.reference_md().unwrap_or_default());

    for (scenario_name, _tools_needed) in scenarios {
        group.bench_with_input(
            BenchmarkId::new("monolithic", scenario_name),
            &mono_tokens,
            |b, tokens| {
                b.iter(|| {
                    // Monolithic always loads everything
                    black_box(*tokens)
                });
            },
        );

        // TODO: Enable when categorized implementation is ready
        // group.bench_with_input(
        //     BenchmarkId::new("categorized", scenario_name),
        //     &tools_needed,
        //     |b, tools| {
        //         b.iter(|| {
        //             // Simulate: load SKILL.md + identify category + load category
        //             let bundle = create_categorized_bundle();
        //             let categories = identify_categories_for_tools(&bundle.manifest(), tools);
        //
        //             let mut total = count_skill_md_tokens(bundle.skill_md());
        //             for category in categories {
        //                 let content = bundle.categories().get(&category).unwrap();
        //                 total += count_tokens(content);
        //             }
        //
        //             black_box(total)
        //         });
        //     },
        // );
    }

    group.finish();
}

// ============================================================================
// Benchmark Configuration
// ============================================================================

criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(std::time::Duration::from_secs(10))
        .warm_up_time(std::time::Duration::from_secs(3));
    targets =
        bench_token_consumption_comparison,
        bench_generation_performance,
        bench_category_loading_simulation,
);

criterion_main!(benches);
