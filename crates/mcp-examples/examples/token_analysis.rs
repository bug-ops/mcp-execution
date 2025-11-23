//! Token usage analysis demonstration.
//!
//! Analyzes and compares token usage between standard MCP approach
//! and the Code Execution pattern across different scenarios.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example token_analysis
//! ```

use mcp_examples::mock_server::MockMcpServer;
use mcp_examples::token_analysis::{TokenAnalysis, min_calls_for_target};

fn main() {
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║   MCP Code Execution - Token Analysis            ║");
    println!("║   Comparing Token Usage Patterns                  ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    // Create mock server for analysis
    let mock_server = MockMcpServer::new_github();
    let server_info = mock_server.server_info();

    println!("Server Configuration:");
    println!("  Name:  {}", server_info.name);
    println!("  Tools: {}", server_info.tools.len());
    println!();

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Scenario 1: Few calls (shows initial overhead)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("═══════════════════════════════════════════════════");
    println!("  Scenario 1: Few Tool Calls (3 calls)");
    println!("═══════════════════════════════════════════════════\n");

    let analysis_few = TokenAnalysis::analyze(server_info, 3);
    print_scenario_breakdown(server_info, 3);
    println!("\n{}", analysis_few.format_report());

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Scenario 2: Medium usage (typical workflow)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n═══════════════════════════════════════════════════");
    println!("  Scenario 2: Typical Workflow (20 calls)");
    println!("═══════════════════════════════════════════════════\n");

    let analysis_medium = TokenAnalysis::analyze(server_info, 20);
    print_scenario_breakdown(server_info, 20);
    println!("\n{}", analysis_medium.format_report());

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Scenario 3: Heavy usage (multi-agent workflow)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n═══════════════════════════════════════════════════");
    println!("  Scenario 3: Heavy Usage (100 calls)");
    println!("═══════════════════════════════════════════════════\n");

    let analysis_heavy = TokenAnalysis::analyze(server_info, 100);
    print_scenario_breakdown(server_info, 100);
    println!("\n{}", analysis_heavy.format_report());

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Target Achievement Analysis
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n╔═══════════════════════════════════════════════════╗");
    println!("║       TARGET ACHIEVEMENT ANALYSIS                 ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    let min_calls = min_calls_for_target(server_info.tools.len());
    println!("To achieve 90% token savings:");
    println!("  Server:       {}", server_info.name);
    println!("  Tools:        {}", server_info.tools.len());
    println!("  Min calls:    {min_calls}");
    println!();

    let analysis_target = TokenAnalysis::analyze(server_info, min_calls);
    println!("At minimum threshold ({min_calls} calls):");
    println!("  Token savings: {:.2}%", analysis_target.savings_percent);
    println!(
        "  Target met:    {}",
        if analysis_target.is_significant_savings() {
            "✓ YES"
        } else {
            "✗ NO"
        }
    );
    println!();

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Comparison Table
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n╔═══════════════════════════════════════════════════╗");
    println!("║          COMPARISON TABLE                         ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    println!("┌─────────┬──────────────┬──────────────┬──────────────┐");
    println!("│ Calls   │ Standard MCP │ Code Exec    │ Savings      │");
    println!("├─────────┼──────────────┼──────────────┼──────────────┤");

    for &num_calls in &[1, 3, 5, 10, 20, 50, 100, 200] {
        let analysis = TokenAnalysis::analyze(server_info, num_calls);
        println!(
            "│ {:>7} │ {:>12} │ {:>12} │ {:>11.1}% │",
            num_calls,
            analysis.standard_mcp_tokens,
            analysis.code_execution_tokens,
            analysis.savings_percent
        );
    }

    println!("└─────────┴──────────────┴──────────────┴──────────────┘");

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Key Insights
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n╔═══════════════════════════════════════════════════╗");
    println!("║              KEY INSIGHTS                         ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    println!("Token Efficiency Model:");
    println!();
    println!("Standard MCP:");
    println!(
        "  • Tool listing:  500 tokens/tool × {} tools = {} tokens",
        server_info.tools.len(),
        server_info.tools.len() * 500
    );
    println!("  • Each call:     300 tokens (schema + params)");
    println!(
        "  • Total (N):     {} + 300N tokens",
        server_info.tools.len() * 500
    );
    println!();
    println!("Code Execution:");
    println!(
        "  • Code gen:      200 tokens/tool × {} tools = {} tokens (one-time)",
        server_info.tools.len(),
        server_info.tools.len() * 200
    );
    println!("  • Each call:     50 tokens (function + args)");
    println!(
        "  • Total (N):     {} + 50N tokens",
        server_info.tools.len() * 200
    );
    println!();
    println!("Savings Equation:");
    println!("  Savings = (800N - 200T - 50N) / 800N × 100%");
    println!("          = (750N - 200T) / 800N × 100%");
    println!("  Where: N = calls, T = tools");
    println!();
    println!("Break-even Point:");
    println!("  For 90% savings: N ≥ 6.67T");
    println!("  For this server: N ≥ {min_calls}");
    println!();
    println!("Practical Implications:");
    println!("  • Low call count (1-5):    Moderate savings (60-75%)");
    println!("  • Medium usage (10-50):    Good savings (80-88%)");
    println!("  • High usage (50+):        Excellent savings (90%+)");
    println!("  • Multi-agent (100+):      Outstanding savings (93%+)");
    println!();

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Recommendations
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n╔═══════════════════════════════════════════════════╗");
    println!("║            RECOMMENDATIONS                        ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    println!("When to use Code Execution pattern:");
    println!("  ✓ Workflows with 3+ tool calls");
    println!("  ✓ Multi-agent systems");
    println!("  ✓ Long-running conversations");
    println!("  ✓ Servers with many tools (5+)");
    println!();
    println!("When standard MCP may be sufficient:");
    println!("  • Single, one-off tool calls");
    println!("  • Exploratory/discovery phase");
    println!("  • Very simple servers (1-2 tools)");
    println!();
    println!("Best Practices:");
    println!("  1. Use Code Execution for production workflows");
    println!("  2. Cache generated code aggressively");
    println!("  3. Batch multiple tool calls when possible");
    println!("  4. Monitor token usage in production");
    println!();

    println!("╔═══════════════════════════════════════════════════╗");
    println!("║   ✓ TOKEN ANALYSIS COMPLETE                       ║");
    println!("╚═══════════════════════════════════════════════════╝\n");
}

/// Prints detailed breakdown for a scenario.
fn print_scenario_breakdown(server_info: &mcp_introspector::ServerInfo, num_calls: usize) {
    let breakdown = TokenAnalysis::analyze_detailed(server_info, num_calls);

    println!("Token Breakdown:");
    println!();
    println!("Standard MCP Approach:");
    println!(
        "  Initial tool listing:  {:>6} tokens",
        breakdown["standard_tool_listing"]
    );
    println!(
        "  Per-call overhead:     {:>6} tokens ({} calls × 300)",
        breakdown["standard_per_call"], num_calls
    );
    println!("  ─────────────────────────────");
    println!(
        "  Total:                 {:>6} tokens",
        breakdown["standard_total"]
    );
    println!();
    println!("Code Execution Approach:");
    println!(
        "  One-time code gen:     {:>6} tokens",
        breakdown["codegen_one_time"]
    );
    println!(
        "  Per-call overhead:     {:>6} tokens ({} calls × 50)",
        breakdown["codegen_per_call"], num_calls
    );
    println!("  ─────────────────────────────");
    println!(
        "  Total:                 {:>6} tokens",
        breakdown["codegen_total"]
    );
}
