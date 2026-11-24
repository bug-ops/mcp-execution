//! Observed result shapes: the bounds the runtime bridge's recorder obeys, and the reader of
//! what it recorded.
//!
//! The bridge's opt-in recorder and the Rust reader of its output must apply identical limits.
//! Every limit is defined once here and rendered into the bridge through
//! [`BridgeContext`](crate::progressive::BridgeContext), so the TypeScript side copies no
//! number by hand. The reader treats the file as untrusted: a line that breaks any rule is
//! skipped whole.

use crate::common::typescript::is_bare_identifier;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Maximum nesting depth of a recorded shape.
pub const OBSERVED_SHAPE_MAX_DEPTH: usize = 4;

/// Maximum number of keys an object may have to be recorded key by key.
pub const OBSERVED_SHAPE_MAX_KEYS: usize = 16;

/// Maximum length in characters of a recorded object key.
pub const OBSERVED_SHAPE_MAX_KEY_LEN: usize = 32;

/// Maximum size in bytes of one recorded line.
pub const OBSERVED_SHAPE_MAX_LINE_BYTES: usize = 8192;

/// The only keywords a recorded shape may use.
const ALLOWED_KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "items",
    "additionalProperties",
];

/// The JSON types a recorded shape may name.
const JSON_TYPES: &[&str] = &[
    "string", "number", "integer", "boolean", "null", "object", "array",
];

#[derive(Debug, Deserialize)]
struct Line {
    server: String,
    tool: String,
    shape: Value,
}

/// Result shapes parsed from the lines a past run of the generated bridge recorded.
///
/// Only validated shapes are kept, and the most recent valid line wins per server and raw tool
/// name.
#[derive(Debug, Default, Clone)]
pub struct ObservedShapes {
    shapes: HashMap<(String, String), Value>,
}

impl ObservedShapes {
    /// Parses `jsonl`, one recorded line per line, skipping every line that is not valid JSON,
    /// exceeds [`OBSERVED_SHAPE_MAX_LINE_BYTES`], or breaks the shape rules, with one warning
    /// naming how many were skipped.
    pub fn parse(jsonl: &str) -> Self {
        let mut shapes = HashMap::new();
        let mut skipped = 0usize;
        for line in jsonl.lines().filter(|line| !line.trim().is_empty()) {
            match parse_line(line) {
                Some((key, shape)) => {
                    shapes.insert(key, shape);
                }
                None => skipped += 1,
            }
        }
        if skipped > 0 {
            tracing::warn!(skipped, "skipped invalid observed-shape lines");
        }
        Self { shapes }
    }

    /// The most recent valid shape for the raw tool name of `server`.
    pub fn shape_for(&self, server: &str, tool: &str) -> Option<&Value> {
        self.shapes.get(&(server.to_string(), tool.to_string()))
    }
}

fn parse_line(line: &str) -> Option<((String, String), Value)> {
    if line.len() > OBSERVED_SHAPE_MAX_LINE_BYTES {
        return None;
    }
    let parsed: Line = serde_json::from_str(line).ok()?;
    if !is_valid_shape(&parsed.shape, 0) {
        return None;
    }
    let mut shape = parsed.shape;
    close_observed_objects(&mut shape);
    Some(((parsed.server, parsed.tool), shape))
}

fn is_recordable_key(key: &str) -> bool {
    is_bare_identifier(key) && key.chars().count() <= OBSERVED_SHAPE_MAX_KEY_LEN
}

fn is_valid_type(value: &Value) -> bool {
    match value {
        Value::String(name) => JSON_TYPES.contains(&name.as_str()),
        Value::Array(names) => {
            names.len() <= JSON_TYPES.len()
                && names
                    .iter()
                    .all(|n| n.as_str().is_some_and(|n| JSON_TYPES.contains(&n)))
        }
        _ => false,
    }
}

fn is_valid_properties(value: &Value, depth: usize) -> bool {
    value.as_object().is_some_and(|properties| {
        properties.len() <= OBSERVED_SHAPE_MAX_KEYS
            && properties
                .iter()
                .all(|(key, shape)| is_recordable_key(key) && is_valid_shape(shape, depth + 1))
    })
}

fn is_valid_required(value: &Value) -> bool {
    value.as_array().is_some_and(|names| {
        names.len() <= OBSERVED_SHAPE_MAX_KEYS
            && names
                .iter()
                .all(|name| name.as_str().is_some_and(is_recordable_key))
    })
}

fn is_valid_keyword(keyword: &str, value: &Value, depth: usize) -> bool {
    match keyword {
        "type" => is_valid_type(value),
        "properties" => is_valid_properties(value, depth),
        "required" => is_valid_required(value),
        "items" => is_valid_shape(value, depth + 1),
        "additionalProperties" => value.is_boolean() || is_valid_shape(value, depth + 1),
        _ => false,
    }
}

/// Whether `shape` is a recorder-produced fragment: a JSON-Schema object of at most
/// [`OBSERVED_SHAPE_MAX_DEPTH`] nesting levels that uses only [`ALLOWED_KEYWORDS`] and obeys
/// the key rule of the recorder.
fn is_valid_shape(shape: &Value, depth: usize) -> bool {
    depth <= OBSERVED_SHAPE_MAX_DEPTH
        && shape.as_object().is_some_and(|object| {
            object.iter().all(|(keyword, value)| {
                ALLOWED_KEYWORDS.contains(&keyword.as_str())
                    && is_valid_keyword(keyword, value, depth)
            })
        })
}

/// An observed object lists exactly the keys that were seen, so it renders closed rather than
/// with the open index signature an absent `additionalProperties` would add.
fn close_observed_objects(shape: &mut Value) {
    let Some(object) = shape.as_object_mut() else {
        return;
    };
    if object.contains_key("properties") && !object.contains_key("additionalProperties") {
        object.insert("additionalProperties".to_string(), Value::Bool(false));
    }
    for keyword in ["items", "additionalProperties"] {
        if let Some(child) = object.get_mut(keyword) {
            close_observed_objects(child);
        }
    }
    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        for child in properties.values_mut() {
            close_observed_objects(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(server: &str, tool: &str, shape: &Value) -> String {
        json!({"server": server, "tool": tool, "shape": shape}).to_string()
    }

    #[test]
    fn last_valid_line_wins_and_invalid_lines_are_skipped() {
        let first = json!({"type": "object", "properties": {"a": {"type": "string"}}});
        let second = json!({"type": "object", "properties": {"b": {"type": "string"}}});
        let jsonl = [
            line("s", "t", &first),
            line("s", "t", &second),
            "{not json".to_string(),
            line("s", "t", &json!({"type": "object", "oneOf": []})),
        ]
        .join("\n");

        let observed = ObservedShapes::parse(&jsonl);

        let shape = observed.shape_for("s", "t").unwrap();
        assert!(shape["properties"].get("b").is_some());
        assert!(observed.shape_for("other", "t").is_none());
    }

    #[test]
    fn observed_objects_are_closed() {
        let shape = json!({"type": "object", "properties": {"a": {"type": "array", "items": {"type": "object", "properties": {"b": {"type": "string"}}}}}});
        let observed = ObservedShapes::parse(&line("s", "t", &shape));

        let kept = observed.shape_for("s", "t").unwrap();
        assert_eq!(kept["additionalProperties"], json!(false));
        assert_eq!(
            kept["properties"]["a"]["items"]["additionalProperties"],
            json!(false)
        );
    }

    #[test]
    fn key_rule_boundaries() {
        let keys = |count: usize| -> Value {
            let properties: serde_json::Map<String, Value> = (0..count)
                .map(|i| (format!("k{i:02}"), json!({"type": "string"})))
                .collect();
            json!({"type": "object", "properties": properties})
        };
        assert!(is_valid_shape(&keys(OBSERVED_SHAPE_MAX_KEYS), 0));
        assert!(!is_valid_shape(&keys(OBSERVED_SHAPE_MAX_KEYS + 1), 0));
        assert!(is_recordable_key(&"k".repeat(OBSERVED_SHAPE_MAX_KEY_LEN)));
        assert!(!is_recordable_key(
            &"k".repeat(OBSERVED_SHAPE_MAX_KEY_LEN + 1)
        ));
        assert!(!is_recordable_key("a@b.com"));
    }

    #[test]
    fn depth_bound_counts_nesting_levels() {
        let mut shape = json!({"type": "string"});
        for _ in 0..OBSERVED_SHAPE_MAX_DEPTH {
            shape = json!({"type": "array", "items": shape});
        }
        assert!(is_valid_shape(&shape, 0));
        assert!(!is_valid_shape(
            &json!({"type": "array", "items": shape}),
            0
        ));
    }
}
