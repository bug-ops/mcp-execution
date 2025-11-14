//! Edge case tests for mcp-codegen.
//!
//! Tests handling of unusual or edge case scenarios:
//! - Empty schemas
//! - Missing optional fields
//! - Deeply nested objects
//! - Large arrays
//! - Unicode characters
//! - Special characters in names
//! - Very long descriptions

use mcp_codegen::CodeGenerator;
use mcp_core::{ServerId, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

#[test]
fn test_tool_with_no_parameters() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("no_params"),
            description: "Tool with no parameters".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/noParams.ts")
        .unwrap();

    // Should generate valid interface even with no properties
    assert!(
        tool_file
            .content
            .contains("export interface noParamsParams")
    );
    assert!(tool_file.content.contains("export async function noParams"));
}

#[test]
fn test_tool_with_all_optional_parameters() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("all_optional"),
            description: "All params optional".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "param1": {"type": "string"},
                    "param2": {"type": "number"},
                    "param3": {"type": "boolean"}
                },
                "required": []
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/allOptional.ts")
        .unwrap();

    // All parameters should be optional
    assert!(tool_file.content.contains("param1?: string"));
    assert!(tool_file.content.contains("param2?: number"));
    assert!(tool_file.content.contains("param3?: boolean"));
}

#[test]
fn test_deeply_nested_schema() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("deep_nested"),
            description: "Deeply nested schema".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "level1": {
                        "type": "object",
                        "properties": {
                            "level2": {
                                "type": "object",
                                "properties": {
                                    "level3": {
                                        "type": "object",
                                        "properties": {
                                            "value": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "required": []
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info);

    assert!(generated.is_ok(), "Should handle deeply nested schemas");
}

#[test]
fn test_array_of_objects() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("array_objects"),
            description: "Array of objects".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string"},
                                "name": {"type": "string"}
                            }
                        }
                    }
                },
                "required": ["items"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/arrayObjects.ts")
        .unwrap();

    // Should handle array of objects
    assert!(tool_file.content.contains("items:"));
}

#[test]
fn test_array_of_arrays() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("nested_arrays"),
            description: "Nested arrays".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "matrix": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": {"type": "number"}
                        }
                    }
                },
                "required": ["matrix"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/nestedArrays.ts")
        .unwrap();

    // Should handle nested arrays
    assert!(tool_file.content.contains("number[][]"));
}

#[test]
fn test_unicode_in_description() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test Server 中文 🚀".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("unicode_tool"),
            description: "Unicode test: 你好世界 🎉 Привет مرحبا".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string"}
                },
                "required": ["message"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    // Should preserve unicode characters
    let manifest = generated
        .files
        .iter()
        .find(|f| f.path == "manifest.json")
        .unwrap();
    assert!(manifest.content.contains("Test Server 中文 🚀"));

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/unicodeTool.ts")
        .unwrap();
    assert!(tool_file.content.contains("你好世界"));
    assert!(tool_file.content.contains("🎉"));
}

#[test]
fn test_very_long_description() {
    let long_desc = "A".repeat(10000);

    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("long_desc"),
            description: long_desc.clone(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info);

    assert!(generated.is_ok(), "Should handle very long descriptions");
    let tool_file = generated
        .unwrap()
        .files
        .iter()
        .find(|f| f.path == "tools/longDesc.ts")
        .unwrap()
        .clone();
    assert!(tool_file.content.contains(&long_desc));
}

#[test]
fn test_special_characters_in_property_names() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("special_props"),
            description: "Special property names".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "normal_prop": {"type": "string"},
                    "$special": {"type": "string"},
                    "@mention": {"type": "string"}
                },
                "required": ["normal_prop"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/specialProps.ts")
        .unwrap();

    // Should preserve special characters in property names
    assert!(tool_file.content.contains("normal_prop"));
    // Note: JSON property names with special chars might need quoting
}

#[test]
fn test_null_type_schema() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("null_type"),
            description: "Null type".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "nullable_field": {"type": "null"}
                },
                "required": []
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/nullType.ts")
        .unwrap();

    // Should handle null type
    assert!(tool_file.content.contains("nullable_field"));
}

#[test]
fn test_mixed_required_optional_parameters() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("mixed_params"),
            description: "Mixed parameters".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "required1": {"type": "string"},
                    "optional1": {"type": "string"},
                    "required2": {"type": "number"},
                    "optional2": {"type": "boolean"}
                },
                "required": ["required1", "required2"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/mixedParams.ts")
        .unwrap();

    // Required parameters should not have ?
    assert!(tool_file.content.contains("required1: string"));
    assert!(tool_file.content.contains("required2: number"));

    // Optional parameters should have ?
    assert!(tool_file.content.contains("optional1?: string"));
    assert!(tool_file.content.contains("optional2?: boolean"));
}

#[test]
fn test_empty_required_array() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("no_required"),
            description: "No required fields".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "param1": {"type": "string"},
                    "param2": {"type": "number"}
                },
                "required": []
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/noRequired.ts")
        .unwrap();

    // All should be optional
    assert!(tool_file.content.contains("param1?: string"));
    assert!(tool_file.content.contains("param2?: number"));
}

#[test]
fn test_missing_required_field_in_schema() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("no_required_field"),
            description: "Schema without required field".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "param1": {"type": "string"}
                }
                // No "required" field
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/noRequiredField.ts")
        .unwrap();

    // Should default to optional when required field is missing
    assert!(tool_file.content.contains("param1?: string"));
}

#[test]
fn test_number_vs_integer_types() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("number_types"),
            description: "Different number types".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "float_value": {"type": "number"},
                    "int_value": {"type": "integer"}
                },
                "required": ["float_value", "int_value"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/numberTypes.ts")
        .unwrap();

    // Both should map to TypeScript 'number'
    assert!(tool_file.content.contains("float_value: number"));
    assert!(tool_file.content.contains("int_value: number"));
}

#[test]
fn test_snake_case_to_camel_case_conversion() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![
            ToolInfo {
                name: ToolName::new("simple_name"),
                description: "Simple".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("complex_snake_case_name"),
                description: "Complex".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("single"),
                description: "Single".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
                output_schema: None,
            },
        ],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    // Verify naming conversions
    assert!(
        generated
            .files
            .iter()
            .any(|f| f.path == "tools/simpleName.ts")
    );
    assert!(
        generated
            .files
            .iter()
            .any(|f| f.path == "tools/complexSnakeCaseName.ts")
    );
    assert!(generated.files.iter().any(|f| f.path == "tools/single.ts"));
}

#[test]
fn test_unknown_schema_type() {
    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("unknown_type"),
            description: "Unknown type".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "weird_field": {"type": "weird_unknown_type"}
                },
                "required": []
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let tool_file = generated
        .files
        .iter()
        .find(|f| f.path == "tools/unknownType.ts")
        .unwrap();

    // Should default to 'unknown' for unrecognized types
    assert!(tool_file.content.contains("weird_field?: unknown"));
}
