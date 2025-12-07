//! JSON Schema to Zod type conversion utilities.
//!
//! Provides functions to convert JSON Schema to Zod schema definitions
//! for the Claude Agent SDK.
//!
//! # Examples
//!
//! ```
//! use mcp_codegen::claude_agent::zod;
//! use serde_json::json;
//!
//! let schema = json!({
//!     "type": "string",
//!     "format": "email"
//! });
//!
//! let (zod_type, modifiers) = zod::json_type_to_zod(&schema);
//! assert_eq!(zod_type, "string");
//! assert!(modifiers.contains(&".email()".to_string()));
//! ```

use serde_json::Value;

/// Converts JSON Schema type to Zod type with optional modifiers.
///
/// Returns a tuple of (base_type, modifiers) where modifiers are
/// additional Zod chain methods like `.int()`, `.email()`, etc.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::zod::json_type_to_zod;
/// use serde_json::json;
///
/// let (zod_type, mods) = json_type_to_zod(&json!({"type": "string"}));
/// assert_eq!(zod_type, "string");
/// assert!(mods.is_empty());
///
/// let (zod_type, mods) = json_type_to_zod(&json!({"type": "integer"}));
/// assert_eq!(zod_type, "number");
/// assert!(mods.contains(&".int()".to_string()));
/// ```
#[must_use]
pub fn json_type_to_zod(schema: &Value) -> (String, Vec<String>) {
    let mut modifiers = Vec::new();

    let base_type = match schema {
        Value::Object(obj) => {
            let type_str = obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            // Check for format modifiers (string types)
            if type_str == "string" {
                if let Some(format) = obj.get("format").and_then(|v| v.as_str()) {
                    match format {
                        "email" => modifiers.push(".email()".to_string()),
                        "uri" | "url" => modifiers.push(".url()".to_string()),
                        "uuid" => modifiers.push(".uuid()".to_string()),
                        "date" => modifiers.push(".date()".to_string()),
                        "date-time" => modifiers.push(".datetime()".to_string()),
                        "ipv4" => modifiers.push(".ip({ version: 'v4' })".to_string()),
                        "ipv6" => modifiers.push(".ip({ version: 'v6' })".to_string()),
                        _ => {}
                    }
                }

                // Check for string constraints
                if let Some(min_length) = obj.get("minLength").and_then(serde_json::Value::as_u64) {
                    modifiers.push(format!(".min({min_length})"));
                }
                if let Some(max_length) = obj.get("maxLength").and_then(serde_json::Value::as_u64) {
                    modifiers.push(format!(".max({max_length})"));
                }
                if let Some(pattern) = obj.get("pattern").and_then(|v| v.as_str()) {
                    // Escape forward slashes in the pattern for JavaScript regex
                    let escaped_pattern = pattern.replace('/', "\\/");
                    modifiers.push(format!(".regex(/{escaped_pattern}/)"));
                }
            }

            // Check for enum
            if let Some(enum_values) = obj.get("enum").and_then(|v| v.as_array()) {
                let values: Vec<String> = enum_values
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| format!("'{}'", s))
                    .collect();

                if !values.is_empty() {
                    return ("enum".to_string(), vec![format!("[{}]", values.join(", "))]);
                }
            }

            match type_str {
                "string" => "string".to_string(),
                "number" => {
                    // Check for number constraints
                    if let Some(minimum) = obj.get("minimum").and_then(serde_json::Value::as_f64) {
                        modifiers.push(format!(".min({minimum})"));
                    }
                    if let Some(maximum) = obj.get("maximum").and_then(serde_json::Value::as_f64) {
                        modifiers.push(format!(".max({maximum})"));
                    }
                    "number".to_string()
                }
                "integer" => {
                    // Collect constraints first, then prepend .int()
                    let mut int_modifiers = vec![".int()".to_string()];
                    if let Some(minimum) = obj.get("minimum").and_then(serde_json::Value::as_i64) {
                        int_modifiers.push(format!(".min({minimum})"));
                    }
                    if let Some(maximum) = obj.get("maximum").and_then(serde_json::Value::as_i64) {
                        int_modifiers.push(format!(".max({maximum})"));
                    }
                    modifiers = int_modifiers;
                    "number".to_string()
                }
                "boolean" => "boolean".to_string(),
                "null" => "null".to_string(),
                "array" => {
                    if let Some(items) = obj.get("items") {
                        let (item_type, item_mods) = json_type_to_zod(items);
                        let item_zod = format_zod_type(&item_type, &item_mods);
                        return ("array".to_string(), vec![format!("(z.{})", item_zod)]);
                    }
                    "array".to_string()
                }
                "object" => {
                    // For nested objects, we'll use z.object({}) or z.record()
                    if obj.get("properties").is_some() {
                        // Complex object - will be handled separately
                        "object".to_string()
                    } else if let Some(additional) = obj.get("additionalProperties") {
                        let (value_type, value_mods) = json_type_to_zod(additional);
                        let value_zod = format_zod_type(&value_type, &value_mods);
                        return (
                            "record".to_string(),
                            vec![format!("(z.string(), z.{})", value_zod)],
                        );
                    } else {
                        "record".to_string()
                    }
                }
                _ => "unknown".to_string(),
            }
        }
        Value::String(s) => match s.as_str() {
            "string" => "string".to_string(),
            "number" => "number".to_string(),
            "integer" => {
                modifiers.push(".int()".to_string());
                "number".to_string()
            }
            "boolean" => "boolean".to_string(),
            "null" => "null".to_string(),
            "array" => "array".to_string(),
            "object" => "object".to_string(),
            _ => "unknown".to_string(),
        },
        _ => "unknown".to_string(),
    };

    (base_type, modifiers)
}

/// Formats a Zod type with its modifiers into a complete expression.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::zod::format_zod_type;
///
/// assert_eq!(format_zod_type("string", &[]), "string()");
/// assert_eq!(
///     format_zod_type("string", &[".email()".to_string()]),
///     "string().email()"
/// );
/// assert_eq!(
///     format_zod_type("number", &[".int()".to_string(), ".min(0)".to_string()]),
///     "number().int().min(0)"
/// );
/// ```
#[must_use]
pub fn format_zod_type(base_type: &str, modifiers: &[String]) -> String {
    if base_type == "enum" && !modifiers.is_empty() {
        // Special case for enum: z.enum(['a', 'b', 'c'])
        return format!("enum({})", modifiers[0]);
    }

    if base_type == "array" && !modifiers.is_empty() {
        // Special case for array: z.array(z.string())
        return format!("array{}", modifiers[0]);
    }

    if base_type == "record" && !modifiers.is_empty() {
        // Special case for record: z.record(z.string(), z.number())
        return format!("record{}", modifiers[0]);
    }

    let mut result = format!("{}()", base_type);
    for modifier in modifiers {
        result.push_str(modifier);
    }
    result
}

/// Extracts property information from JSON Schema for Zod generation.
///
/// Returns property details including Zod type and modifiers.
/// This is an internal function used by [`ClaudeAgentGenerator`](crate::claude_agent::ClaudeAgentGenerator).
#[must_use]
pub(crate) fn extract_zod_properties(schema: &Value) -> Vec<ZodPropertyInfo> {
    // Pre-calculate capacity if possible
    let capacity = schema
        .as_object()
        .and_then(|obj| obj.get("properties"))
        .and_then(|v| v.as_object())
        .map_or(0, serde_json::Map::len);

    let mut properties = Vec::with_capacity(capacity);

    if let Some(obj) = schema.as_object()
        && let Some(props) = obj.get("properties").and_then(|v| v.as_object())
    {
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
            let (zod_type, zod_modifiers) = json_type_to_zod(prop_schema);
            let is_required = required.contains(name);

            let description = prop_schema
                .as_object()
                .and_then(|obj| obj.get("description"))
                .and_then(|v| v.as_str())
                .map(String::from);

            properties.push(ZodPropertyInfo {
                name: name.clone(),
                zod_type,
                zod_modifiers,
                description,
                required: is_required,
            });
        }
    }

    // Sort properties by name for consistent output
    properties.sort_by(|a, b| a.name.cmp(&b.name));

    properties
}

/// Information about a property with Zod type details.
///
/// This is an internal type used during JSON Schema extraction.
/// It gets converted to [`PropertyInfo`](crate::claude_agent::PropertyInfo)
/// for template rendering.
#[derive(Debug, Clone)]
pub(crate) struct ZodPropertyInfo {
    /// Property name
    pub name: String,
    /// Base Zod type (e.g., "string", "number")
    pub zod_type: String,
    /// Zod modifiers (e.g., ".int()", ".email()")
    pub zod_modifiers: Vec<String>,
    /// Optional description from schema
    pub description: Option<String>,
    /// Whether the property is required
    pub required: bool,
}

impl From<ZodPropertyInfo> for crate::claude_agent::types::PropertyInfo {
    fn from(zod: ZodPropertyInfo) -> Self {
        Self {
            name: zod.name,
            zod_type: zod.zod_type,
            zod_modifiers: zod.zod_modifiers,
            description: zod.description,
            required: zod.required,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_type_to_zod_string() {
        let (zod_type, mods) = json_type_to_zod(&json!({"type": "string"}));
        assert_eq!(zod_type, "string");
        assert!(mods.is_empty());
    }

    #[test]
    fn test_json_type_to_zod_email() {
        let (zod_type, mods) = json_type_to_zod(&json!({"type": "string", "format": "email"}));
        assert_eq!(zod_type, "string");
        assert!(mods.contains(&".email()".to_string()));
    }

    #[test]
    fn test_json_type_to_zod_url() {
        let (zod_type, mods) = json_type_to_zod(&json!({"type": "string", "format": "uri"}));
        assert_eq!(zod_type, "string");
        assert!(mods.contains(&".url()".to_string()));
    }

    #[test]
    fn test_json_type_to_zod_integer() {
        let (zod_type, mods) = json_type_to_zod(&json!({"type": "integer"}));
        assert_eq!(zod_type, "number");
        assert!(mods.contains(&".int()".to_string()));
    }

    #[test]
    fn test_json_type_to_zod_integer_with_constraints() {
        let (zod_type, mods) = json_type_to_zod(&json!({
            "type": "integer",
            "minimum": 0,
            "maximum": 100
        }));
        assert_eq!(zod_type, "number");
        assert!(mods.contains(&".int()".to_string()));
        assert!(mods.contains(&".min(0)".to_string()));
        assert!(mods.contains(&".max(100)".to_string()));
    }

    #[test]
    fn test_json_type_to_zod_string_with_length() {
        let (zod_type, mods) = json_type_to_zod(&json!({
            "type": "string",
            "minLength": 1,
            "maxLength": 100
        }));
        assert_eq!(zod_type, "string");
        assert!(mods.contains(&".min(1)".to_string()));
        assert!(mods.contains(&".max(100)".to_string()));
    }

    #[test]
    fn test_json_type_to_zod_enum() {
        let (zod_type, mods) = json_type_to_zod(&json!({
            "type": "string",
            "enum": ["a", "b", "c"]
        }));
        assert_eq!(zod_type, "enum");
        assert_eq!(mods, vec!["['a', 'b', 'c']"]);
    }

    #[test]
    fn test_json_type_to_zod_array() {
        let (zod_type, mods) = json_type_to_zod(&json!({
            "type": "array",
            "items": {"type": "string"}
        }));
        assert_eq!(zod_type, "array");
        assert_eq!(mods, vec!["(z.string())"]);
    }

    #[test]
    fn test_format_zod_type_simple() {
        assert_eq!(format_zod_type("string", &[]), "string()");
        assert_eq!(format_zod_type("number", &[]), "number()");
        assert_eq!(format_zod_type("boolean", &[]), "boolean()");
    }

    #[test]
    fn test_format_zod_type_with_modifiers() {
        assert_eq!(
            format_zod_type("string", &[".email()".to_string()]),
            "string().email()"
        );
        assert_eq!(
            format_zod_type("number", &[".int()".to_string(), ".min(0)".to_string()]),
            "number().int().min(0)"
        );
    }

    #[test]
    fn test_format_zod_type_enum() {
        assert_eq!(
            format_zod_type("enum", &["['a', 'b']".to_string()]),
            "enum(['a', 'b'])"
        );
    }

    #[test]
    fn test_format_zod_type_array() {
        assert_eq!(
            format_zod_type("array", &["(z.string())".to_string()]),
            "array(z.string())"
        );
    }

    #[test]
    fn test_extract_zod_properties() {
        let schema = json!({
            "type": "object",
            "properties": {
                "email": {"type": "string", "format": "email", "description": "User email"},
                "age": {"type": "integer", "minimum": 0}
            },
            "required": ["email"]
        });

        let props = extract_zod_properties(&schema);
        assert_eq!(props.len(), 2);

        let email_prop = props.iter().find(|p| p.name == "email").unwrap();
        assert_eq!(email_prop.zod_type, "string");
        assert!(email_prop.zod_modifiers.contains(&".email()".to_string()));
        assert!(email_prop.required);
        assert_eq!(email_prop.description, Some("User email".to_string()));

        let age_prop = props.iter().find(|p| p.name == "age").unwrap();
        assert_eq!(age_prop.zod_type, "number");
        assert!(age_prop.zod_modifiers.contains(&".int()".to_string()));
        assert!(!age_prop.required);
    }

    #[test]
    fn test_zod_property_info_to_property_info() {
        use crate::claude_agent::types::PropertyInfo;

        let zod_prop = ZodPropertyInfo {
            name: "email".to_string(),
            zod_type: "string".to_string(),
            zod_modifiers: vec![".email()".to_string()],
            description: Some("User email".to_string()),
            required: true,
        };

        let prop: PropertyInfo = zod_prop.into();

        assert_eq!(prop.name, "email");
        assert_eq!(prop.zod_type, "string");
        assert_eq!(prop.zod_modifiers, vec![".email()".to_string()]);
        assert_eq!(prop.description, Some("User email".to_string()));
        assert!(prop.required);
    }

    #[test]
    fn test_regex_pattern_escaping() {
        let (_, mods) = json_type_to_zod(&json!({
            "type": "string",
            "pattern": "^https?://[a-z]+.com/path$"
        }));
        // Forward slashes in pattern should be escaped
        assert!(mods.iter().any(|m| m.contains("\\/") || !m.contains('/')));
    }
}
