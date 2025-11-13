//! TypeScript code generation utilities.
//!
//! Provides functions to convert JSON Schema to TypeScript types
//! and generate type-safe TypeScript code.
//!
//! # Examples
//!
//! ```
//! use mcp_codegen::common::typescript;
//! use serde_json::json;
//!
//! let schema = json!({
//!     "type": "object",
//!     "properties": {
//!         "name": {"type": "string"},
//!         "age": {"type": "number"}
//!     }
//! });
//!
//! let ts_type = typescript::json_schema_to_typescript(&schema);
//! ```

use serde_json::Value;

/// Converts a snake_case name to camelCase for TypeScript.
///
/// # Examples
///
/// ```
/// use mcp_codegen::common::typescript::to_camel_case;
///
/// assert_eq!(to_camel_case("send_message"), "sendMessage");
/// assert_eq!(to_camel_case("get_user_data"), "getUserData");
/// assert_eq!(to_camel_case("hello"), "hello");
/// ```
#[must_use]
pub fn to_camel_case(snake_case: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for ch in snake_case.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

/// Converts a snake_case name to PascalCase for TypeScript types.
///
/// # Examples
///
/// ```
/// use mcp_codegen::common::typescript::to_pascal_case;
///
/// assert_eq!(to_pascal_case("send_message"), "SendMessage");
/// assert_eq!(to_pascal_case("get_user_data"), "GetUserData");
/// assert_eq!(to_pascal_case("hello"), "Hello");
/// ```
#[must_use]
pub fn to_pascal_case(snake_case: &str) -> String {
    let camel = to_camel_case(snake_case);
    let mut chars = camel.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Converts JSON Schema type to TypeScript type.
///
/// Maps JSON Schema primitive types to their TypeScript equivalents.
///
/// # Examples
///
/// ```
/// use mcp_codegen::common::typescript::json_type_to_typescript;
///
/// assert_eq!(json_type_to_typescript("string"), "string");
/// assert_eq!(json_type_to_typescript("number"), "number");
/// assert_eq!(json_type_to_typescript("integer"), "number");
/// assert_eq!(json_type_to_typescript("boolean"), "boolean");
/// assert_eq!(json_type_to_typescript("unknown_type"), "unknown");
/// ```
#[must_use]
pub fn json_type_to_typescript(json_type: &str) -> &'static str {
    match json_type {
        "string" => "string",
        "number" | "integer" => "number",
        "boolean" => "boolean",
        "array" => "unknown[]",
        "object" => "Record<string, unknown>",
        "null" => "null",
        _ => "unknown",
    }
}

/// Converts a JSON Schema to TypeScript type definition.
///
/// Handles complex schemas including objects, arrays, and nested types.
///
/// # Examples
///
/// ```
/// use mcp_codegen::common::typescript::json_schema_to_typescript;
/// use serde_json::json;
///
/// let schema = json!({
///     "type": "object",
///     "properties": {
///         "name": {"type": "string"},
///         "age": {"type": "number"}
///     },
///     "required": ["name"]
/// });
///
/// let ts = json_schema_to_typescript(&schema);
/// assert!(ts.contains("name: string"));
/// ```
#[must_use]
pub fn json_schema_to_typescript(schema: &Value) -> String {
    match schema {
        Value::Object(obj) => {
            // Get type field
            let schema_type = obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            match schema_type {
                "object" => {
                    // Extract properties
                    let properties = obj.get("properties").and_then(|v| v.as_object());
                    let required = obj
                        .get("required")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                        .unwrap_or_default();

                    if let Some(props) = properties {
                        let mut fields = Vec::new();
                        for (key, value) in props {
                            let is_required = required.contains(&key.as_str());
                            let optional_marker = if is_required { "" } else { "?" };
                            let ts_type = json_schema_to_typescript(value);
                            fields.push(format!("  {}{}: {};", key, optional_marker, ts_type));
                        }

                        if fields.is_empty() {
                            "Record<string, unknown>".to_string()
                        } else {
                            format!("{{\n{}\n}}", fields.join("\n"))
                        }
                    } else {
                        "Record<string, unknown>".to_string()
                    }
                }
                "array" => {
                    let items = obj.get("items");
                    if let Some(item_schema) = items {
                        format!("{}[]", json_schema_to_typescript(item_schema))
                    } else {
                        "unknown[]".to_string()
                    }
                }
                other => json_type_to_typescript(other).to_string(),
            }
        }
        Value::String(s) => json_type_to_typescript(s).to_string(),
        _ => "unknown".to_string(),
    }
}

/// Extracts property definitions from JSON Schema for template rendering.
///
/// Returns a vector of property information suitable for Handlebars templates.
///
/// # Examples
///
/// ```
/// use mcp_codegen::common::typescript::extract_properties;
/// use serde_json::json;
///
/// let schema = json!({
///     "type": "object",
///     "properties": {
///         "name": {"type": "string"},
///         "age": {"type": "number"}
///     },
///     "required": ["name"]
/// });
///
/// let props = extract_properties(&schema);
/// assert_eq!(props.len(), 2);
/// ```
#[must_use]
pub fn extract_properties(schema: &Value) -> Vec<serde_json::Value> {
    let mut properties = Vec::new();

    if let Some(obj) = schema.as_object() {
        if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
            let required = obj
                .get("required")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            for (name, prop_schema) in props {
                let ts_type = json_schema_to_typescript(prop_schema);
                let is_required = required.contains(name);

                properties.push(serde_json::json!({
                    "name": name,
                    "type": ts_type,
                    "required": is_required,
                }));
            }
        }
    }

    properties
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("send_message"), "sendMessage");
        assert_eq!(to_camel_case("get_user_data"), "getUserData");
        assert_eq!(to_camel_case("hello"), "hello");
        assert_eq!(to_camel_case("a_b_c"), "aBC");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("send_message"), "SendMessage");
        assert_eq!(to_pascal_case("get_user_data"), "GetUserData");
        assert_eq!(to_pascal_case("hello"), "Hello");
    }

    #[test]
    fn test_json_type_to_typescript() {
        assert_eq!(json_type_to_typescript("string"), "string");
        assert_eq!(json_type_to_typescript("number"), "number");
        assert_eq!(json_type_to_typescript("integer"), "number");
        assert_eq!(json_type_to_typescript("boolean"), "boolean");
        assert_eq!(json_type_to_typescript("array"), "unknown[]");
        assert_eq!(json_type_to_typescript("object"), "Record<string, unknown>");
        assert_eq!(json_type_to_typescript("null"), "null");
        assert_eq!(json_type_to_typescript("unknown_type"), "unknown");
    }

    #[test]
    fn test_json_schema_to_typescript_primitive() {
        assert_eq!(
            json_schema_to_typescript(&json!({"type": "string"})),
            "string"
        );
        assert_eq!(
            json_schema_to_typescript(&json!({"type": "number"})),
            "number"
        );
    }

    #[test]
    fn test_json_schema_to_typescript_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "number"}
            },
            "required": ["name"]
        });

        let result = json_schema_to_typescript(&schema);
        assert!(result.contains("name: string"));
        assert!(result.contains("age?: number"));
    }

    #[test]
    fn test_json_schema_to_typescript_array() {
        let schema = json!({
            "type": "array",
            "items": {"type": "string"}
        });

        assert_eq!(json_schema_to_typescript(&schema), "string[]");
    }

    #[test]
    fn test_extract_properties() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "number"}
            },
            "required": ["name"]
        });

        let props = extract_properties(&schema);
        assert_eq!(props.len(), 2);

        // Find the "name" property (HashMap order is not guaranteed)
        let name_prop = props
            .iter()
            .find(|p| p["name"] == "name")
            .expect("name property not found");

        assert_eq!(name_prop["type"], "string");
        assert_eq!(name_prop["required"], true);

        // Check age property
        let age_prop = props
            .iter()
            .find(|p| p["name"] == "age")
            .expect("age property not found");

        assert_eq!(age_prop["type"], "number");
        assert_eq!(age_prop["required"], false);
    }

    #[test]
    fn test_extract_properties_empty() {
        let schema = json!({"type": "string"});
        let props = extract_properties(&schema);
        assert_eq!(props.len(), 0);
    }
}
