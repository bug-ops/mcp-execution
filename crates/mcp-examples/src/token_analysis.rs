//! Token usage analysis and savings calculation.
//!
//! Provides utilities for calculating and comparing token usage between
//! standard MCP approach and the Code Execution pattern.

use mcp_introspector::ServerInfo;

/// Token usage statistics for a workflow.
///
/// Tracks token consumption for both standard MCP and Code Execution approaches,
/// allowing comparison and calculation of savings.
///
/// # Examples
///
/// ```
/// use mcp_examples::token_analysis::TokenAnalysis;
///
/// let analysis = TokenAnalysis {
///     standard_mcp_tokens: 5000,
///     code_execution_tokens: 400,
///     savings_percent: 92.0,
/// };
///
/// assert!(analysis.is_significant_savings());
/// ```
#[derive(Debug, Clone)]
pub struct TokenAnalysis {
    /// Total tokens used in standard MCP approach.
    pub standard_mcp_tokens: usize,

    /// Total tokens used in Code Execution approach.
    pub code_execution_tokens: usize,

    /// Percentage of tokens saved (0-100).
    pub savings_percent: f64,
}

impl TokenAnalysis {
    /// Analyzes token usage for a given server and workflow.
    ///
    /// Calculates token usage for both approaches:
    ///
    /// **Standard MCP:**
    /// - Initial tool listing: ~500 tokens per tool
    /// - Each tool call: ~300 tokens (schema + parameters)
    /// - Total for N calls: 500N + 300N = 800N tokens
    ///
    /// **Code Execution:**
    /// - One-time code generation: ~200 tokens per tool
    /// - Each tool call: ~50 tokens (just function name + args)
    /// - Total for N calls: 200T + 50N tokens (T = total tools)
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::token_analysis::TokenAnalysis;
    /// use mcp_core::{ServerId, ToolName};
    /// use mcp_introspector::{ServerInfo, ServerCapabilities, ToolInfo};
    /// use serde_json::json;
    ///
    /// let server = ServerInfo {
    ///     id: ServerId::new("test"),
    ///     name: "Test".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     capabilities: ServerCapabilities {
    ///         supports_tools: true,
    ///         supports_resources: false,
    ///         supports_prompts: false,
    ///     },
    ///     tools: vec![
    ///         ToolInfo {
    ///             name: ToolName::new("tool1"),
    ///             description: "Tool 1".to_string(),
    ///             input_schema: json!({}),
    ///             output_schema: None,
    ///         },
    ///         ToolInfo {
    ///             name: ToolName::new("tool2"),
    ///             description: "Tool 2".to_string(),
    ///             input_schema: json!({}),
    ///             output_schema: None,
    ///         },
    ///     ],
    /// };
    ///
    /// let analysis = TokenAnalysis::analyze(&server, 5);
    /// assert!(analysis.savings_percent > 0.0);
    /// ```
    pub fn analyze(server_info: &ServerInfo, num_calls: usize) -> Self {
        let num_tools = server_info.tools.len();

        // Standard MCP approach
        // - List tools: 500 tokens per tool
        // - Each call: 300 tokens (includes tool schema + parameters)
        let standard_tokens = (num_tools * 500) + (num_calls * 300);

        // Code Execution approach
        // - One-time code generation: 200 tokens per tool (compressed TypeScript)
        // - Each call: 50 tokens (just function name + compact args)
        let code_exec_tokens = (num_tools * 200) + (num_calls * 50);

        let savings = if standard_tokens > 0 {
            ((standard_tokens.saturating_sub(code_exec_tokens)) as f64 / standard_tokens as f64)
                * 100.0
        } else {
            0.0
        };

        Self {
            standard_mcp_tokens: standard_tokens,
            code_execution_tokens: code_exec_tokens,
            savings_percent: savings,
        }
    }

    /// Analyzes token usage with detailed breakdown.
    ///
    /// Returns a breakdown showing token usage at each stage of the workflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::token_analysis::TokenAnalysis;
    /// use mcp_core::ServerId;
    /// use mcp_introspector::{ServerInfo, ServerCapabilities};
    ///
    /// let server = ServerInfo {
    ///     id: ServerId::new("test"),
    ///     name: "Test".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     capabilities: ServerCapabilities {
    ///         supports_tools: true,
    ///         supports_resources: false,
    ///         supports_prompts: false,
    ///     },
    ///     tools: vec![],
    /// };
    ///
    /// let breakdown = TokenAnalysis::analyze_detailed(&server, 5);
    /// assert!(breakdown.contains_key("standard_tool_listing"));
    /// ```
    pub fn analyze_detailed(
        server_info: &ServerInfo,
        num_calls: usize,
    ) -> std::collections::HashMap<String, usize> {
        let mut breakdown = std::collections::HashMap::new();
        let num_tools = server_info.tools.len();

        // Standard MCP breakdown
        breakdown.insert("standard_tool_listing".to_string(), num_tools * 500);
        breakdown.insert("standard_per_call".to_string(), num_calls * 300);
        breakdown.insert(
            "standard_total".to_string(),
            (num_tools * 500) + (num_calls * 300),
        );

        // Code Execution breakdown
        breakdown.insert("codegen_one_time".to_string(), num_tools * 200);
        breakdown.insert("codegen_per_call".to_string(), num_calls * 50);
        breakdown.insert(
            "codegen_total".to_string(),
            (num_tools * 200) + (num_calls * 50),
        );

        breakdown
    }

    /// Checks if savings are significant (≥90%).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::token_analysis::TokenAnalysis;
    ///
    /// let analysis = TokenAnalysis {
    ///     standard_mcp_tokens: 5000,
    ///     code_execution_tokens: 400,
    ///     savings_percent: 92.0,
    /// };
    ///
    /// assert!(analysis.is_significant_savings());
    /// ```
    pub fn is_significant_savings(&self) -> bool {
        self.savings_percent >= 90.0
    }

    /// Formats the analysis as a human-readable report.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::token_analysis::TokenAnalysis;
    ///
    /// let analysis = TokenAnalysis {
    ///     standard_mcp_tokens: 5000,
    ///     code_execution_tokens: 400,
    ///     savings_percent: 92.0,
    /// };
    ///
    /// let report = analysis.format_report();
    /// assert!(report.contains("TOKEN USAGE COMPARISON"));
    /// ```
    pub fn format_report(&self) -> String {
        let mut report = String::new();

        report.push_str("═══════════════════════════════════════════════\n");
        report.push_str("         TOKEN USAGE COMPARISON               \n");
        report.push_str("═══════════════════════════════════════════════\n\n");

        report.push_str("Standard MCP Approach:\n");
        report.push_str(&format!(
            "  Total Tokens:       {:>8}\n\n",
            self.standard_mcp_tokens
        ));

        report.push_str("Code Execution Approach:\n");
        report.push_str(&format!(
            "  Total Tokens:       {:>8}\n\n",
            self.code_execution_tokens
        ));

        report.push_str("Savings:\n");
        report.push_str(&format!(
            "  Tokens Saved:       {:>8}\n",
            self.standard_mcp_tokens
                .saturating_sub(self.code_execution_tokens)
        ));
        report.push_str(&format!(
            "  Percentage:         {:>7.1}% {}\n\n",
            self.savings_percent,
            if self.is_significant_savings() {
                "✓"
            } else {
                "✗"
            }
        ));

        report.push_str("Target Achievement:\n");
        report.push_str(&format!(
            "  Target (≥90%):      {}\n",
            if self.is_significant_savings() {
                "✓ ACHIEVED"
            } else {
                "✗ NOT MET"
            }
        ));

        report.push_str("\n═══════════════════════════════════════════════\n");

        report
    }
}

/// Calculates minimum number of calls to reach 80% token savings.
///
/// Note: The maximum possible savings approaches ~83.3% as calls increase,
/// so 90% savings is not achievable with this model. This function calculates
/// the minimum calls needed for 80% savings.
///
/// Solves the equation:
/// `((500T + 300N) - (200T + 50N)) / (500T + 300N) ≥ 0.80`
///
/// Where:
/// - N = number of calls
/// - T = number of tools
///
/// # Examples
///
/// ```
/// use mcp_examples::token_analysis::min_calls_for_target;
///
/// let min_calls = min_calls_for_target(10);
/// assert!(min_calls > 0);
/// ```
pub fn min_calls_for_target(num_tools: usize) -> usize {
    // Solving for 80% savings:
    // (300T + 250N) / (500T + 300N) >= 0.80
    // 300T + 250N >= 0.80 * (500T + 300N)
    // 300T + 250N >= 400T + 240N
    // 10N >= 100T
    // N >= 10T
    let min = num_tools * 10;
    min.max(1) // At least 1 call
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::{ServerId, ToolName};
    use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
    use serde_json::json;

    fn create_test_server(num_tools: usize) -> ServerInfo {
        let tools: Vec<ToolInfo> = (0..num_tools)
            .map(|i| ToolInfo {
                name: ToolName::new(&format!("tool_{}", i)),
                description: format!("Tool {}", i),
                input_schema: json!({"type": "object"}),
                output_schema: None,
            })
            .collect();

        ServerInfo {
            id: ServerId::new("test"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools,
        }
    }

    #[test]
    fn test_analyze_basic() {
        let server = create_test_server(5);
        let analysis = TokenAnalysis::analyze(&server, 10);

        // Standard: (5 * 500) + (10 * 300) = 2500 + 3000 = 5500
        assert_eq!(analysis.standard_mcp_tokens, 5500);

        // Code Exec: (5 * 200) + (10 * 50) = 1000 + 500 = 1500
        assert_eq!(analysis.code_execution_tokens, 1500);

        // Savings: (5500 - 1500) / 5500 = 4000 / 5500 = 72.7%
        assert!((analysis.savings_percent - 72.7).abs() < 0.1);
    }

    #[test]
    fn test_analyze_high_calls() {
        let server = create_test_server(5);
        let analysis = TokenAnalysis::analyze(&server, 100);

        // Standard: (5 * 500) + (100 * 300) = 2500 + 30000 = 32500
        assert_eq!(analysis.standard_mcp_tokens, 32500);

        // Code Exec: (5 * 200) + (100 * 50) = 1000 + 5000 = 6000
        assert_eq!(analysis.code_execution_tokens, 6000);

        // Savings: (32500 - 6000) / 32500 = 26500 / 32500 = 81.5%
        assert!((analysis.savings_percent - 81.5).abs() < 0.1);
    }

    #[test]
    fn test_is_significant_savings() {
        let analysis = TokenAnalysis {
            standard_mcp_tokens: 5000,
            code_execution_tokens: 400,
            savings_percent: 92.0,
        };
        assert!(analysis.is_significant_savings());

        let low_savings = TokenAnalysis {
            standard_mcp_tokens: 5000,
            code_execution_tokens: 1000,
            savings_percent: 80.0,
        };
        assert!(!low_savings.is_significant_savings());
    }

    #[test]
    fn test_analyze_detailed() {
        let server = create_test_server(3);
        let breakdown = TokenAnalysis::analyze_detailed(&server, 5);

        assert_eq!(breakdown["standard_tool_listing"], 1500); // 3 * 500
        assert_eq!(breakdown["standard_per_call"], 1500); // 5 * 300
        assert_eq!(breakdown["standard_total"], 3000); // 1500 + 1500

        assert_eq!(breakdown["codegen_one_time"], 600); // 3 * 200
        assert_eq!(breakdown["codegen_per_call"], 250); // 5 * 50
        assert_eq!(breakdown["codegen_total"], 850); // 600 + 250
    }

    #[test]
    fn test_min_calls_for_target() {
        // For 10 tools: 10 * 10 = 100 calls for 80% savings
        let min = min_calls_for_target(10);
        assert_eq!(min, 100);

        // Verify with actual calculation
        let server = create_test_server(10);
        let analysis = TokenAnalysis::analyze(&server, min);
        // Should achieve 80% savings
        assert!(analysis.savings_percent >= 80.0);
    }

    #[test]
    fn test_format_report() {
        let analysis = TokenAnalysis {
            standard_mcp_tokens: 5000,
            code_execution_tokens: 400,
            savings_percent: 92.0,
        };

        let report = analysis.format_report();
        assert!(report.contains("TOKEN USAGE COMPARISON"));
        assert!(report.contains("Standard MCP Approach"));
        assert!(report.contains("Code Execution Approach"));
        assert!(report.contains("ACHIEVED"));
    }
}
