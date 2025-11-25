//! Denial of Service security tests for categorized skills.
//!
//! Validates that malicious servers cannot cause excessive resource consumption.

use mcp_codegen::skills::SkillOrchestrator;
use mcp_core::{SkillDescription, SkillName};
use mcp_introspector::{ServerInfo, ToolInfo};

/// Creates a mock server info with specified number of tools.
fn create_server_with_n_tools(n: usize) -> ServerInfo {
    let tools: Vec<ToolInfo> = (0..n)
        .map(|i| ToolInfo {
            name: format!("tool_{}", i),
            description: format!("Tool number {}", i),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "param": {"type": "string"}
                }
            }),
        })
        .collect();

    ServerInfo {
        name: "test-server".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
        capabilities: mcp_introspector::ServerCapabilities {
            tools: Some(mcp_introspector::ToolsCapability {}),
            ..Default::default()
        },
        tools,
    }
}

/// Test that category count is limited.
#[test]
fn test_category_count_limit() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");
    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    // Create server with many tools (should create many categories)
    let server_info = create_server_with_n_tools(200);

    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(bundle) => {
            // Should not create excessive categories
            assert!(
                bundle.categories().len() <= 50,
                "Should not create more than 50 categories, but created {}",
                bundle.categories().len()
            );

            // Each category should have reasonable tool count
            for (category, tools) in bundle.categories() {
                assert!(
                    tools.len() <= 30,
                    "Category '{}' should not have more than 30 tools, but has {}",
                    category.as_str(),
                    tools.len()
                );
            }
        }
        Err(e) => {
            // Acceptable to reject excessive tool counts
            assert!(
                e.to_string().contains("too many")
                    || e.to_string().contains("limit")
                    || e.to_string().contains("exceeded"),
                "Error should indicate limit exceeded: {}",
                e
            );
        }
    }
}

/// Test that tools per category is limited.
#[test]
fn test_tools_per_category_limit() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    // Create many tools with similar names (should go in same category)
    let tools: Vec<ToolInfo> = (0..100)
        .map(|i| ToolInfo {
            name: format!("repo_operation_{}", i),
            description: "Repository operation".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        })
        .collect();

    let server_info = ServerInfo {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
        capabilities: mcp_introspector::ServerCapabilities {
            tools: Some(mcp_introspector::ToolsCapability {}),
            ..Default::default()
        },
        tools,
    };

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(bundle) => {
            // Each category should have limited tools
            for (category, tools) in bundle.categories() {
                assert!(
                    tools.len() <= 20,
                    "Category '{}' should not have more than 20 tools, but has {}",
                    category.as_str(),
                    tools.len()
                );
            }
        }
        Err(e) => {
            // Acceptable to reject
            assert!(
                e.to_string().contains("limit") || e.to_string().contains("too many"),
                "Error should indicate limit: {}",
                e
            );
        }
    }
}

/// Test that category file size is limited.
#[test]
fn test_category_file_size_limit() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    // Create tools with very long descriptions
    let tools: Vec<ToolInfo> = (0..20)
        .map(|i| ToolInfo {
            name: format!("tool_{}", i),
            description: "A".repeat(5000), // 5KB description each
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "param1": {"type": "string", "description": "B".repeat(1000)},
                    "param2": {"type": "string", "description": "C".repeat(1000)},
                    "param3": {"type": "string", "description": "D".repeat(1000)},
                }
            }),
        })
        .collect();

    let server_info = ServerInfo {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
        capabilities: mcp_introspector::ServerCapabilities {
            tools: Some(mcp_introspector::ToolsCapability {}),
            ..Default::default()
        },
        tools,
    };

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(bundle) => {
            // Check each category file size
            for (category, content) in bundle.category_contents() {
                assert!(
                    content.len() < 100_000,
                    "Category '{}' file should be under 100KB, but is {} bytes",
                    category.as_str(),
                    content.len()
                );

                // Token estimate: ~4 bytes per token
                let estimated_tokens = content.len() / 4;
                assert!(
                    estimated_tokens < 25_000,
                    "Category '{}' should be under 25k tokens, but is ~{} tokens",
                    category.as_str(),
                    estimated_tokens
                );
            }
        }
        Err(e) => {
            // Acceptable to reject
            assert!(
                e.to_string().contains("size") || e.to_string().contains("large"),
                "Error should indicate size issue: {}",
                e
            );
        }
    }
}

/// Test that manifest size is limited.
#[test]
fn test_manifest_size_limit() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    // Create server with many tools
    let server_info = create_server_with_n_tools(500);

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(bundle) => {
            let manifest_yaml = serde_yaml::to_string(bundle.manifest())
                .expect("Manifest should serialize");

            assert!(
                manifest_yaml.len() < 50_000,
                "Manifest should be under 50KB, but is {} bytes",
                manifest_yaml.len()
            );

            // Token estimate: ~4 bytes per token
            let estimated_tokens = manifest_yaml.len() / 4;
            assert!(
                estimated_tokens < 12_500,
                "Manifest should be under 12.5k tokens, but is ~{} tokens",
                estimated_tokens
            );
        }
        Err(_) => {
            // Acceptable to reject excessive tools
        }
    }
}

/// Test that total bundle size is reasonable.
#[test]
fn test_total_bundle_size_limit() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    let server_info = create_server_with_n_tools(100);

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(bundle) => {
            // Calculate total size
            let skill_md_size = bundle.skill_md().len();
            let manifest_size = serde_yaml::to_string(bundle.manifest())
                .expect("Should serialize")
                .len();
            let categories_size: usize = bundle
                .category_contents()
                .map(|(_, content)| content.len())
                .sum();
            let scripts_size: usize = bundle
                .scripts()
                .iter()
                .map(|s| s.content().len())
                .sum();

            let total_size = skill_md_size + manifest_size + categories_size + scripts_size;

            // Total bundle should be under 1MB
            assert!(
                total_size < 1_000_000,
                "Total bundle size should be under 1MB, but is {} bytes",
                total_size
            );
        }
        Err(_) => {
            // Acceptable
        }
    }
}

/// Test that generation time is bounded.
#[test]
fn test_generation_time_limit() {
    use std::time::Instant;

    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    let server_info = create_server_with_n_tools(100);

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    let start = Instant::now();
    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);
    let elapsed = start.elapsed();

    // Should complete within 5 seconds for 100 tools
    assert!(
        elapsed.as_secs() < 5,
        "Generation should complete within 5 seconds, but took {:?}",
        elapsed
    );

    if let Ok(_bundle) = result {
        // Success is ok
    }
}

/// Test that deeply nested input schemas don't cause issues.
#[test]
fn test_nested_schema_depth_limit() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    // Create tool with deeply nested schema
    fn create_nested_schema(depth: usize) -> serde_json::Value {
        if depth == 0 {
            return serde_json::json!({"type": "string"});
        }

        serde_json::json!({
            "type": "object",
            "properties": {
                "nested": create_nested_schema(depth - 1)
            }
        })
    }

    let tools = vec![ToolInfo {
        name: "nested_tool".to_string(),
        description: "Tool with nested schema".to_string(),
        input_schema: create_nested_schema(50), // 50 levels deep
    }];

    let server_info = ServerInfo {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
        capabilities: mcp_introspector::ServerCapabilities {
            tools: Some(mcp_introspector::ToolsCapability {}),
            ..Default::default()
        },
        tools,
    };

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    // Should handle gracefully (either process or reject)
    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(bundle) => {
            // If accepted, output should be reasonable
            for (_, content) in bundle.category_contents() {
                assert!(
                    content.len() < 100_000,
                    "Deep nesting should not cause excessive output"
                );
            }
        }
        Err(e) => {
            // Rejection is acceptable
            assert!(
                e.to_string().contains("depth")
                    || e.to_string().contains("nested")
                    || e.to_string().contains("complex"),
                "Error should indicate complexity issue: {}",
                e
            );
        }
    }
}

/// Test that circular schema references don't cause infinite loops.
#[test]
fn test_circular_schema_handling() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    // Create tool with self-referencing schema
    let tools = vec![ToolInfo {
        name: "circular_tool".to_string(),
        description: "Tool with circular schema".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "child": {"$ref": "#"}  // Self-reference
            }
        }),
    }];

    let server_info = ServerInfo {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
        capabilities: mcp_introspector::ServerCapabilities {
            tools: Some(mcp_introspector::ToolsCapability {}),
            ..Default::default()
        },
        tools,
    };

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    use std::time::Instant;
    let start = Instant::now();

    // Should not hang or cause stack overflow
    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 10,
        "Should not hang on circular schemas"
    );

    // Result doesn't matter as long as it completes
    let _ = result;
}

/// Test that memory usage is bounded.
#[test]
fn test_memory_usage_bounded() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    // Create many small tools
    let server_info = create_server_with_n_tools(1000);

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    // This test mainly ensures we don't panic or crash
    // In a real system, you'd use a memory profiler
    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(_bundle) => {
            // If we got here without OOM, memory usage was acceptable
        }
        Err(e) => {
            // Rejection is acceptable
            assert!(
                e.to_string().contains("limit") || e.to_string().contains("too many"),
                "Error should indicate resource limit: {}",
                e
            );
        }
    }
}

/// Test that empty tool lists are handled gracefully.
#[test]
fn test_empty_tool_list() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    let server_info = ServerInfo {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
        capabilities: mcp_introspector::ServerCapabilities::default(),
        tools: vec![],
    };

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    match result {
        Ok(bundle) => {
            // Empty bundle is acceptable
            assert_eq!(
                bundle.categories().len(),
                0,
                "Empty tool list should produce empty categories"
            );
        }
        Err(e) => {
            // Rejection is also acceptable
            assert!(
                e.to_string().contains("empty") || e.to_string().contains("no tools"),
                "Error should indicate empty tools: {}",
                e
            );
        }
    }
}

/// Test that duplicate tool names are handled.
#[test]
fn test_duplicate_tool_names() {
    let orchestrator = SkillOrchestrator::new().expect("Failed to create orchestrator");

    // Create tools with duplicate names
    let tools = vec![
        ToolInfo {
            name: "duplicate".to_string(),
            description: "First".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        },
        ToolInfo {
            name: "duplicate".to_string(),
            description: "Second".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        },
    ];

    let server_info = ServerInfo {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
        capabilities: mcp_introspector::ServerCapabilities {
            tools: Some(mcp_introspector::ToolsCapability {}),
            ..Default::default()
        },
        tools,
    };

    let skill_name = SkillName::new("test").expect("Valid name");
    let skill_desc = SkillDescription::new("Test skill").expect("Valid description");

    // Should handle gracefully (dedupe, error, or keep both)
    let result = orchestrator.generate_categorized_bundle(&server_info, &skill_name, &skill_desc);

    // Either result is acceptable as long as it doesn't crash
    let _ = result;
}
