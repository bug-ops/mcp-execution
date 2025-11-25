//! LLM-based intelligent categorization for MCP tools.
//!
//! Uses Claude API to analyze tool names, descriptions, and schemas
//! to generate semantically meaningful categories.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::skills::LlmCategorizer;
//! use mcp_introspector::ToolInfo;
//!
//! # async fn example(tools: Vec<ToolInfo>) -> Result<(), mcp_core::Error> {
//! let categorizer = LlmCategorizer::new(
//!     "claude-sonnet-4".to_string(),
//!     std::env::var("ANTHROPIC_API_KEY").unwrap(),
//!     10, // max categories
//! );
//!
//! let manifest = categorizer.categorize(&tools).await?;
//! println!("Created {} categories", manifest.category_count());
//! # Ok(())
//! # }
//! ```

use mcp_core::{CategoryManifest, Error, Result, SkillCategory};
use mcp_introspector::ToolInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// LLM categorization request to Claude API.
#[derive(Debug, Serialize)]
struct CategorizationRequest {
    /// Model to use (e.g., "claude-sonnet-4")
    model: String,
    /// Maximum tokens in response
    max_tokens: usize,
    /// Messages to send to Claude
    messages: Vec<Message>,
}

/// Message in Claude API format.
#[derive(Debug, Serialize)]
struct Message {
    /// Role: "user" or "assistant"
    role: String,
    /// Message content
    content: String,
}

/// Claude API response wrapper.
#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    /// Response content blocks
    content: Vec<ContentBlock>,
}

/// Content block in Claude response.
#[derive(Debug, Deserialize)]
struct ContentBlock {
    /// Content type (e.g., "text")
    #[serde(rename = "type")]
    content_type: String,
    /// Text content
    text: String,
}

/// LLM categorization response structure.
#[derive(Debug, Deserialize)]
struct CategorizationResponse {
    /// Categories with descriptions
    #[allow(dead_code)]
    categories: HashMap<String, CategoryInfo>,
    /// Tool assignments: tool_name -> category_name
    assignments: HashMap<String, String>,
}

/// Information about a category.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CategoryInfo {
    /// Category name
    name: String,
    /// Category description
    description: String,
    /// Rationale for grouping
    rationale: String,
}

/// LLM-based categorizer using Claude API.
///
/// Provides intelligent, semantic categorization of MCP tools
/// by analyzing tool names and descriptions with Claude.
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::skills::LlmCategorizer;
///
/// let categorizer = LlmCategorizer::new(
///     "claude-sonnet-4".to_string(),
///     "sk-ant-api-key".to_string(),
///     10,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct LlmCategorizer {
    model: String,
    api_key: String,
    max_categories: usize,
}

impl LlmCategorizer {
    /// Create new LLM categorizer.
    ///
    /// # Arguments
    ///
    /// * `model` - Claude model to use (e.g., "claude-sonnet-4")
    /// * `api_key` - Anthropic API key
    /// * `max_categories` - Maximum number of categories to create
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::skills::LlmCategorizer;
    ///
    /// let categorizer = LlmCategorizer::new(
    ///     "claude-sonnet-4".to_string(),
    ///     "api-key".to_string(),
    ///     10,
    /// );
    /// ```
    #[must_use]
    pub fn new(model: String, api_key: String, max_categories: usize) -> Self {
        Self {
            model,
            api_key,
            max_categories,
        }
    }

    /// Categorize tools using LLM intelligence.
    ///
    /// Sends tool information to Claude API and receives semantic categories.
    ///
    /// # Arguments
    ///
    /// * `tools` - Tools to categorize
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - API request fails
    /// - Response is malformed
    /// - Categories cannot be created
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::LlmCategorizer;
    /// use mcp_introspector::ToolInfo;
    ///
    /// # async fn example(tools: Vec<ToolInfo>) -> Result<(), mcp_core::Error> {
    /// let categorizer = LlmCategorizer::new(
    ///     "claude-sonnet-4".to_string(),
    ///     std::env::var("ANTHROPIC_API_KEY").unwrap(),
    ///     10,
    /// );
    ///
    /// let manifest = categorizer.categorize(&tools).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn categorize(&self, tools: &[ToolInfo]) -> Result<CategoryManifest> {
        // Build prompt with tool information
        let prompt = self.build_categorization_prompt(tools);

        // Call Claude API
        let response = self.call_claude_api(&prompt).await?;

        // Parse response and build manifest
        self.build_manifest_from_response(&response, tools)
    }

    /// Build comprehensive prompt for LLM categorization.
    fn build_categorization_prompt(&self, tools: &[ToolInfo]) -> String {
        let tools_json = serde_json::to_string_pretty(
            &tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name.as_str(),
                        "description": t.description,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string());

        format!(
            r#"You are an expert at categorizing API tools for efficient organization and progressive loading.

Analyze the following MCP tools and create semantic categories:

Tools:
```json
{tools_json}
```

Requirements:
1. Create {max_categories} or fewer categories
2. Each category should contain 3-15 tools
3. Categories should be semantically meaningful
4. Use clear, concise category names (lowercase, underscore-separated)
5. Provide descriptions for each category

Output JSON format:
```json
{{
  "categories": {{
    "category_name": {{
      "name": "category_name",
      "description": "What this category contains",
      "rationale": "Why these tools belong together"
    }}
  }},
  "assignments": {{
    "tool_name": "category_name"
  }}
}}
```

Think step by step:
1. Analyze tool names and descriptions
2. Identify common patterns (CRUD, entities, workflows)
3. Group related tools
4. Create clear category names
5. Assign each tool to exactly one category"#,
            tools_json = tools_json,
            max_categories = self.max_categories,
        )
    }

    /// Call Claude API for categorization.
    async fn call_claude_api(&self, prompt: &str) -> Result<CategorizationResponse> {
        let client = reqwest::Client::new();

        let request = CategorizationRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::ExecutionError {
                message: format!("Failed to call Claude API: {e}"),
                source: Some(Box::new(e)),
            })?;

        // Check HTTP status
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::ExecutionError {
                message: format!("Claude API request failed: {status} - {error_text}"),
                source: None,
            });
        }

        // Parse Claude response wrapper
        let claude_response: ClaudeResponse =
            response.json().await.map_err(|e| Error::ExecutionError {
                message: format!("Failed to read API response: {e}"),
                source: Some(Box::new(e)),
            })?;

        // Extract text from content blocks
        let response_text = claude_response
            .content
            .iter()
            .find(|c| c.content_type == "text")
            .map(|c| c.text.clone())
            .ok_or_else(|| Error::SerializationError {
                message: "No text content in Claude response".to_string(),
                source: None,
            })?;

        // Extract JSON from response
        self.extract_json_from_response(&response_text)
    }

    /// Extract and parse JSON from Claude's response text.
    fn extract_json_from_response(&self, response_text: &str) -> Result<CategorizationResponse> {
        // Try to find JSON within markdown code blocks
        let json_str = if let Some(start) = response_text.find("```json") {
            let content_start = start + 7;
            if let Some(end) = response_text[content_start..].find("```") {
                response_text[content_start..content_start + end].trim()
            } else {
                response_text[content_start..].trim()
            }
        } else if let Some(start) = response_text.find('{') {
            // Try to find raw JSON
            if let Some(end) = response_text.rfind('}') {
                &response_text[start..=end]
            } else {
                response_text
            }
        } else {
            response_text
        };

        serde_json::from_str(json_str).map_err(|e| Error::SerializationError {
            message: format!("Failed to parse LLM response: {e}"),
            source: None,
        })
    }

    /// Build manifest from LLM response.
    fn build_manifest_from_response(
        &self,
        response: &CategorizationResponse,
        tools: &[ToolInfo],
    ) -> Result<CategoryManifest> {
        let mut builder = CategoryManifest::builder();

        for tool in tools {
            let tool_name = tool.name.as_str();
            if let Some(category_name) = response.assignments.get(tool_name) {
                let category = SkillCategory::new(category_name)?;
                builder = builder.add_tool(tool_name, &category)?;
            } else {
                // If LLM didn't assign a category, put in "other"
                let other = SkillCategory::new("other")?;
                builder = builder.add_tool(tool_name, &other)?;
            }
        }

        Ok(builder.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ToolName;

    fn create_test_tool(name: &str, description: &str) -> ToolInfo {
        ToolInfo {
            name: ToolName::new(name),
            description: description.to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: None,
        }
    }

    #[test]
    fn test_llm_categorizer_creation() {
        let categorizer =
            LlmCategorizer::new("claude-sonnet-4".to_string(), "test-key".to_string(), 10);

        assert_eq!(categorizer.model, "claude-sonnet-4");
        assert_eq!(categorizer.api_key, "test-key");
        assert_eq!(categorizer.max_categories, 10);
    }

    #[test]
    fn test_build_categorization_prompt() {
        let categorizer =
            LlmCategorizer::new("claude-sonnet-4".to_string(), "test-key".to_string(), 5);

        let tools = vec![
            create_test_tool("create_user", "Create a new user"),
            create_test_tool("get_user", "Get user by ID"),
        ];

        let prompt = categorizer.build_categorization_prompt(&tools);

        assert!(prompt.contains("create_user"));
        assert!(prompt.contains("get_user"));
        assert!(prompt.contains("Create a new user"));
        assert!(prompt.contains("5 or fewer categories"));
        assert!(prompt.contains("json"));
    }

    #[test]
    fn test_extract_json_from_markdown() {
        let categorizer =
            LlmCategorizer::new("claude-sonnet-4".to_string(), "test-key".to_string(), 10);

        let response = r#"Here are the categories:

```json
{
  "categories": {
    "users": {
      "name": "users",
      "description": "User management",
      "rationale": "All user-related operations"
    }
  },
  "assignments": {
    "create_user": "users",
    "get_user": "users"
  }
}
```

This should work well."#;

        let result = categorizer.extract_json_from_response(response);
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.categories.len(), 1);
        assert_eq!(parsed.assignments.len(), 2);
    }

    #[test]
    fn test_extract_json_from_raw() {
        let categorizer =
            LlmCategorizer::new("claude-sonnet-4".to_string(), "test-key".to_string(), 10);

        let response = r#"{
  "categories": {
    "files": {
      "name": "files",
      "description": "File operations",
      "rationale": "File management"
    }
  },
  "assignments": {
    "read_file": "files"
  }
}"#;

        let result = categorizer.extract_json_from_response(response);
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.categories.len(), 1);
        assert_eq!(parsed.assignments.len(), 1);
    }

    #[test]
    fn test_extract_json_invalid() {
        let categorizer =
            LlmCategorizer::new("claude-sonnet-4".to_string(), "test-key".to_string(), 10);

        let response = "This is not JSON at all";
        let result = categorizer.extract_json_from_response(response);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_manifest_from_response() {
        let categorizer =
            LlmCategorizer::new("claude-sonnet-4".to_string(), "test-key".to_string(), 10);

        let tools = vec![
            create_test_tool("create_user", "Create user"),
            create_test_tool("get_user", "Get user"),
        ];

        let response = CategorizationResponse {
            categories: {
                let mut map = HashMap::new();
                map.insert(
                    "users".to_string(),
                    CategoryInfo {
                        name: "users".to_string(),
                        description: "User management".to_string(),
                        rationale: "User operations".to_string(),
                    },
                );
                map
            },
            assignments: {
                let mut map = HashMap::new();
                map.insert("create_user".to_string(), "users".to_string());
                map.insert("get_user".to_string(), "users".to_string());
                map
            },
        };

        let manifest = categorizer
            .build_manifest_from_response(&response, &tools)
            .unwrap();

        assert_eq!(manifest.tool_count(), 2);
        assert_eq!(manifest.category_count(), 1);

        let users_category = SkillCategory::new("users").unwrap();
        assert_eq!(manifest.find_category("create_user"), Some(&users_category));
        assert_eq!(manifest.find_category("get_user"), Some(&users_category));
    }

    #[test]
    fn test_build_manifest_with_unassigned_tools() {
        let categorizer =
            LlmCategorizer::new("claude-sonnet-4".to_string(), "test-key".to_string(), 10);

        let tools = vec![
            create_test_tool("create_user", "Create user"),
            create_test_tool("unknown_tool", "Unknown operation"),
        ];

        let response = CategorizationResponse {
            categories: {
                let mut map = HashMap::new();
                map.insert(
                    "users".to_string(),
                    CategoryInfo {
                        name: "users".to_string(),
                        description: "User management".to_string(),
                        rationale: "User operations".to_string(),
                    },
                );
                map
            },
            assignments: {
                let mut map = HashMap::new();
                map.insert("create_user".to_string(), "users".to_string());
                // "unknown_tool" not assigned
                map
            },
        };

        let manifest = categorizer
            .build_manifest_from_response(&response, &tools)
            .unwrap();

        assert_eq!(manifest.tool_count(), 2);

        // Unassigned tool should go to "other"
        let other_category = SkillCategory::new("other").unwrap();
        assert_eq!(
            manifest.find_category("unknown_tool"),
            Some(&other_category)
        );
    }

    // Note: Real API tests are not included as they require:
    // 1. Valid API key
    // 2. Network access
    // 3. Cost money
    // Integration tests can be added separately with proper mocking
}
