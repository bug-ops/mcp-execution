//! TypeScript code generation utilities.
//!
//! Provides functions to convert JSON Schema to TypeScript types
//! and generate type-safe TypeScript code.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_codegen::common::typescript;
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
use std::collections::HashSet;

/// Maximum nesting depth [`json_schema_to_typescript`] will descend into before treating the
/// remainder of a branch as opaque, rather than recursing further.
///
/// The `JSDoc` description sanitizer in `progressive::generator` uses the same cap. Both
/// functions recurse into every nested `object`/`array` schema with no depth limit of
/// their own, and `mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES` bounds a schema's
/// *serialized byte size*, not its nesting depth — so nothing in this crate stops a caller from
/// handing either function an arbitrarily deep, directly-constructed `serde_json::Value` (e.g.
/// built by hand via `Value::Object`/`Map`, as in this module's own tests). That is the actual
/// threat this constant defends against: **not** a schema arriving over the wire from a target
/// MCP server. `input_schema` there is deserialized by `serde_json` from a `tools/list`
/// response, and this workspace never raises or disables that deserializer's own default
/// recursion limit (128 — no `disable_recursion_limit`/`unbounded_depth` anywhere in the
/// dependency tree). Measured through a real `tools/list` envelope, that limit caps *reachable*
/// nesting at 122 levels for an array-shaped schema and ~61 levels for `properties`-nested
/// objects (each object level costs two parser recursion levels: the wrapper and its
/// `properties` map) — a server that sends anything deeper gets its response line dropped as a
/// parse error before `mcp-execution-introspector` ever builds a `ToolInfo` from it, let alone
/// hands it to this crate. So on the wire path, this cap can never fire, let alone protect
/// against a stack overflow.
///
/// This cap is therefore defense-in-depth for direct callers of these two functions — bounding
/// a `Value` handed straight to `json_schema_to_typescript` or the `JSDoc` sanitizer, regardless
/// of provenance — not a fix for a reachable wire-path denial of service, and **not** a
/// guarantee that covers the rest of the `ProgressiveGenerator::generate`/
/// `generate_with_categories` pipeline built on top of them. That pipeline has other
/// unconditionally-recursive touches on the same schema this cap does not reach: e.g.
/// `progressive::generator`'s `create_tool_context` clones `tool.input_schema` (an
/// unguarded, recursive `Value::Clone`) before ever calling the capped sanitizer, and the
/// schema is later re-serialized (`serde_json::to_vec`) and rendered through Handlebars, and
/// eventually dropped (`Value`'s `Drop` impl is itself recursive) — none of which this constant
/// bounds. 128 is set at the wire path's own ceiling (comfortably above the 122 levels a real
/// `tools/list` response can ever produce, so no schema that actually arrives via introspection
/// is ever clipped) while still bounding the unconditionally-recursive descent for a `Value`
/// passed directly to either of these two functions.
///
/// That ceiling is coupled to `serde_json`'s own default recursion limit staying at 128: this
/// value isn't an arbitrary round number, it's chosen to sit at the wire path's current ceiling
/// specifically because that upstream default is 128. If a future `serde_json` release changes
/// its own default, or this workspace ever configures a different limit, this constant's "never
/// clips a real wire schema" property needs re-verifying against the new ceiling.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::typescript::MAX_SCHEMA_RECURSION_DEPTH;
///
/// assert!(MAX_SCHEMA_RECURSION_DEPTH > 0);
/// ```
pub const MAX_SCHEMA_RECURSION_DEPTH: usize = 128;

/// Converts a `snake_case` name to camelCase for TypeScript.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::typescript::to_camel_case;
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

/// Converts a `snake_case` name to `PascalCase` for TypeScript types.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::typescript::to_pascal_case;
///
/// assert_eq!(to_pascal_case("send_message"), "SendMessage");
/// assert_eq!(to_pascal_case("get_user_data"), "GetUserData");
/// assert_eq!(to_pascal_case("hello"), "Hello");
/// ```
#[must_use]
pub fn to_pascal_case(snake_case: &str) -> String {
    let camel = to_camel_case(snake_case);
    let mut chars = camel.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// Sanitizes a string for safe use as a TypeScript identifier (e.g. a function, export,
/// or object property name).
///
/// Characters in `[A-Za-z0-9$]` pass through unchanged; every other character — including a
/// literal `_` already in the input — is treated as a separator and replaces its whole
/// consecutive run with a single `_`, and the result is prefixed with `_` if it would
/// otherwise start with a digit or be empty. This prevents identifier-position injection of
/// arbitrary TypeScript syntax from untrusted schema data (property keys, tool names, etc.
/// sourced from an MCP server are not guaranteed to be valid identifiers).
///
/// Collapsing an entire separator run — rather than emitting one `_` per invalid character —
/// means e.g. a multi-byte non-ASCII run doesn't balloon into a wall of underscores that
/// discards more information than a single separator already would. It also means a literal
/// run like `"a__b"` collapses to `"a_b"`: once a separator has been emitted, further
/// separator-producing characters are redundant regardless of why each one would have
/// produced a `_`.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::typescript::sanitize_ts_identifier;
///
/// assert_eq!(sanitize_ts_identifier("valid_name"), "valid_name");
/// assert_eq!(sanitize_ts_identifier("123abc"), "_123abc");
/// assert_eq!(sanitize_ts_identifier("a-b c"), "a_b_c");
/// assert_eq!(sanitize_ts_identifier("café_menu_日本語"), "caf_menu_");
/// ```
#[must_use]
pub fn sanitize_ts_identifier(s: &str) -> String {
    let mut result = String::new();
    let mut prev_was_underscore = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '$' {
            result.push(c);
            prev_was_underscore = false;
        } else if !prev_was_underscore {
            result.push('_');
            prev_was_underscore = true;
        }
    }

    if result.is_empty() || result.starts_with(|c: char| c.is_ascii_digit()) {
        result.insert(0, '_');
    }

    result
}

/// Disambiguates `base` against the `used` set by appending a numeric suffix
/// (`_2`, `_3`, ...) until the candidate is not already present, then reserves the
/// winning candidate by inserting it into `used`.
///
/// Shared by every call site that maps untrusted, sanitized names into a namespace where
/// distinct source names can collide after sanitization (e.g. sibling schema keys or tool
/// names) — collisions must be disambiguated deterministically rather than silently
/// overwriting one another.
pub(crate) fn disambiguate_identifier(base: &str, used: &mut HashSet<String>) -> String {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }
    candidate
}

/// Converts JSON Schema type to TypeScript type.
///
/// Maps JSON Schema primitive types to their TypeScript equivalents.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::typescript::json_type_to_typescript;
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
/// use mcp_execution_codegen::common::typescript::json_schema_to_typescript;
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
    let mut cap_hit = false;
    let ts_type = json_schema_to_typescript_at_depth(schema, 0, &mut cap_hit);
    if cap_hit {
        // Known limitation: `extract_properties` calls this function once per top-level
        // property, so a schema whose properties each nest deeply logs once per affected
        // property rather than once per tool, and this warning carries no tool/server
        // identifier to correlate it back to the originating `generate_with_categories` call.
        tracing::warn!(
            max_depth = MAX_SCHEMA_RECURSION_DEPTH,
            "schema nesting exceeded MAX_SCHEMA_RECURSION_DEPTH; branches beyond that depth were \
             rendered as an opaque `unknown` type"
        );
    }
    ts_type
}

/// Depth-tracked implementation backing [`json_schema_to_typescript`].
///
/// Once `depth` reaches [`MAX_SCHEMA_RECURSION_DEPTH`], the current branch is treated as
/// opaque (`unknown`/`unknown[]`) instead of recursing further — see that constant's docs for
/// what this cap actually defends against. `cap_hit` is set (never cleared) the first time any
/// branch trips the cap, so the public wrapper can log once per call rather than once per
/// clipped branch.
fn json_schema_to_typescript_at_depth(schema: &Value, depth: usize, cap_hit: &mut bool) -> String {
    if depth >= MAX_SCHEMA_RECURSION_DEPTH {
        *cap_hit = true;
        return "unknown".to_string();
    }

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

                    properties.map_or_else(
                        || "Record<string, unknown>".to_string(),
                        |props| {
                            let mut fields = Vec::new();
                            let mut used_keys = HashSet::new();
                            for (key, value) in props {
                                let is_required = required.contains(&key.as_str());
                                let optional_marker = if is_required { "" } else { "?" };
                                let ts_type =
                                    json_schema_to_typescript_at_depth(value, depth + 1, cap_hit);
                                let base_key = sanitize_ts_identifier(key);
                                let safe_key = disambiguate_identifier(&base_key, &mut used_keys);
                                fields.push(format!("  {safe_key}{optional_marker}: {ts_type};"));
                            }

                            if fields.is_empty() {
                                "Record<string, unknown>".to_string()
                            } else {
                                format!("{{\n{}\n}}", fields.join("\n"))
                            }
                        },
                    )
                }
                "array" => obj.get("items").map_or_else(
                    || "unknown[]".to_string(),
                    |item_schema| {
                        format!(
                            "{}[]",
                            json_schema_to_typescript_at_depth(item_schema, depth + 1, cap_hit)
                        )
                    },
                ),
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
/// Calls [`json_schema_to_typescript`] once per top-level property, starting each call fresh
/// at depth 0 — the `schema`'s own `type: "object"` wrapper and `properties` map are peeled off
/// by this function rather than counted by [`MAX_SCHEMA_RECURSION_DEPTH`]. So the real
/// per-tool call path (`ProgressiveGenerator::extract_property_infos` /
/// `extract_property_data`, both built on this function) tolerates one level more of true JSON
/// nesting in a tool's raw `input_schema` than the constant's value alone would suggest: a
/// property value schema can nest `MAX_SCHEMA_RECURSION_DEPTH` levels *below* the property
/// itself before clipping, for `MAX_SCHEMA_RECURSION_DEPTH + 1` raw levels overall.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::typescript::extract_properties;
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
            let ts_type = json_schema_to_typescript(prop_schema);
            let is_required = required.contains(name);

            properties.push(serde_json::json!({
                "name": name,
                "type": ts_type,
                "required": is_required,
            }));
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
    fn test_json_schema_to_typescript_object_sanitizes_nested_keys() {
        let malicious_key = "x }; export const pwned = evil(); interface J {";
        let schema = json!({
            "type": "object",
            "properties": {
                malicious_key: {"type": "string"}
            },
            "required": []
        });

        let result = json_schema_to_typescript(&schema);
        assert!(!result.contains("export const pwned"));
        assert!(!result.contains(malicious_key));
        assert!(result.contains(&sanitize_ts_identifier(malicious_key)));
    }

    #[test]
    fn test_json_schema_to_typescript_dedups_colliding_sibling_keys() {
        // "a-b" and "a.b" both sanitize to "a_b"; the second must be disambiguated
        // rather than producing a duplicate field name in the generated interface.
        let schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a.b": {"type": "number"}
            },
            "required": []
        });

        let result = json_schema_to_typescript(&schema);
        // Exact field-line matches: a substring count on "a_b" would also match "a_b_2",
        // so assert on the full `key?: type` field lines instead.
        assert_eq!(result.matches("a_b?: string").count(), 1, "{result}");
        assert_eq!(result.matches("a_b_2?: number").count(), 1, "{result}");
    }

    #[test]
    fn test_json_schema_to_typescript_dedups_three_way_colliding_sibling_keys() {
        let schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a.b": {"type": "number"},
                "a b": {"type": "boolean"}
            },
            "required": []
        });

        let result = json_schema_to_typescript(&schema);
        // The `preserve_order` feature is enabled transitively (via schemars/rmcp), so
        // `serde_json::Map` iterates in insertion order: "a-b" claims the base "a_b" first.
        assert_eq!(result.matches("a_b?: string").count(), 1, "{result}");
        assert_eq!(result.matches("a_b_2?: number").count(), 1, "{result}");
        assert_eq!(result.matches("a_b_3?: boolean").count(), 1, "{result}");
    }

    #[test]
    fn test_json_schema_to_typescript_dedups_colliding_keys_in_nested_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "outer": {
                    "type": "object",
                    "properties": {
                        "a-b": {"type": "string"},
                        "a.b": {"type": "number"}
                    },
                    "required": []
                }
            },
            "required": []
        });

        let result = json_schema_to_typescript(&schema);
        assert!(result.contains("a_b?: string"), "{result}");
        assert!(result.contains("a_b_2?: number"), "{result}");
    }

    #[test]
    fn test_disambiguate_identifier_reuses_base_across_independent_scopes() {
        // Disambiguation state must not leak between unrelated objects: two sibling nested
        // objects each independently reusing "a_b" as a field name is not a collision.
        let schema = json!({
            "type": "object",
            "properties": {
                "first": {
                    "type": "object",
                    "properties": {"a_b": {"type": "string"}},
                    "required": []
                },
                "second": {
                    "type": "object",
                    "properties": {"a_b": {"type": "number"}},
                    "required": []
                }
            },
            "required": []
        });

        let result = json_schema_to_typescript(&schema);
        assert!(!result.contains("a_b_2"), "{result}");
    }

    #[test]
    fn test_sanitize_ts_identifier_replaces_invalid_characters() {
        assert_eq!(sanitize_ts_identifier("a-b c"), "a_b_c");
    }

    #[test]
    fn test_sanitize_ts_identifier_prefixes_leading_digit() {
        assert_eq!(sanitize_ts_identifier("123name"), "_123name");
    }

    #[test]
    fn test_sanitize_ts_identifier_prefixes_empty_string() {
        assert_eq!(sanitize_ts_identifier(""), "_");
    }

    #[test]
    fn test_sanitize_ts_identifier_collapses_consecutive_non_ascii_run() {
        // Issue #192: each invalid char used to become its own `_`, so one accented
        // letter plus a run of CJK characters produced four separate underscores. The
        // literal `_`s between words must also collapse into the run rather than
        // surviving as a second, adjacent separator (otherwise `sanitize_ts_identifier`
        // alone — as used for property names, without `to_camel_case` first — still
        // produces `caf__menu__`, the exact artifact the issue was filed about).
        assert_eq!(sanitize_ts_identifier("café_menu_日本語"), "caf_menu_");
    }

    #[test]
    fn test_sanitize_ts_identifier_collapses_mixed_invalid_run() {
        assert_eq!(sanitize_ts_identifier("a---b"), "a_b");
        assert_eq!(sanitize_ts_identifier("a- .b"), "a_b");
    }

    #[test]
    fn test_sanitize_ts_identifier_collapses_literal_underscore_runs() {
        // A run of literal `_`s in the input collapses too: once a separator has been
        // emitted, further separator-producing characters (whether literal `_` or a
        // substituted invalid character) are redundant regardless of which kind they are.
        assert_eq!(sanitize_ts_identifier("a__b"), "a_b");
        assert_eq!(sanitize_ts_identifier("日_本"), "_");
        assert_eq!(sanitize_ts_identifier("_日_"), "_");
    }

    #[test]
    fn test_sanitize_ts_identifier_preserves_isolated_invalid_chars() {
        // A single invalid character separated from the next by a valid character must still
        // become its own `_` — only *consecutive* separator-producing runs collapse.
        assert_eq!(sanitize_ts_identifier("a-b-c"), "a_b_c");
    }

    #[test]
    fn test_sanitize_ts_identifier_all_invalid_chars_collapses_to_bare_underscore() {
        // The most extreme case of issue #192: an identifier made entirely of invalid
        // characters must collapse to a single `_`, not one `_` per character. Distinct
        // from the empty-string case (`""` never enters the loop at all).
        assert_eq!(sanitize_ts_identifier("日本語"), "_");
        assert_eq!(sanitize_ts_identifier("---"), "_");
    }

    #[test]
    fn test_json_schema_to_typescript_array() {
        let schema = json!({
            "type": "array",
            "items": {"type": "string"}
        });

        assert_eq!(json_schema_to_typescript(&schema), "string[]");
    }

    /// Builds a schema with `depth` nested `type: "array"` levels wrapping a `string` leaf.
    ///
    /// Assembles each level directly via `serde_json::Map`/`Value` (rather than the `json!`
    /// macro, which round-trips embedded values through `to_value`'s own recursive
    /// serializer) so building this pathological fixture can't itself overflow the stack.
    fn nested_array_schema(depth: usize) -> Value {
        let mut schema = json!({"type": "string"});
        for _ in 0..depth {
            let mut map = serde_json::Map::new();
            map.insert("type".to_string(), Value::String("array".to_string()));
            map.insert("items".to_string(), schema);
            schema = Value::Object(map);
        }
        schema
    }

    /// Builds a schema with `depth` nested `type: "object"` levels (each with a single
    /// property `"a"`) wrapping a `string` leaf, for the same construction reason as
    /// [`nested_array_schema`].
    fn nested_object_schema(depth: usize) -> Value {
        let mut schema = json!({"type": "string"});
        for _ in 0..depth {
            let mut properties = serde_json::Map::new();
            properties.insert("a".to_string(), schema);
            let mut map = serde_json::Map::new();
            map.insert("type".to_string(), Value::String("object".to_string()));
            map.insert("properties".to_string(), Value::Object(properties));
            map.insert("required".to_string(), Value::Array(Vec::new()));
            schema = Value::Object(map);
        }
        schema
    }

    /// Runs `f` on a dedicated thread with a generous stack, and returns its result.
    ///
    /// `serde_json::Value`'s own `Drop` impl is recursive and unrelated to this module's fix:
    /// a `Value` nested 5,000+ levels deep overflows the *default* thread stack merely by
    /// going out of scope, regardless of how it was traversed beforehand. That's not
    /// reachable in production — `serde_json`'s deserializer already rejects JSON nested
    /// beyond its own default recursion limit (well under 5,000) before a `Value` this deep
    /// can ever be constructed from the wire — but a test fixture built directly via
    /// `Value`/`Map` (bypassing deserialization, see [`nested_array_schema`]) still has to
    /// survive being dropped at the end of the test. A larger stack lets construction, the
    /// (now depth-capped) function under test, and teardown all complete without that
    /// unrelated limitation masking a pass.
    fn run_on_large_stack<F: FnOnce() + Send + 'static>(f: F) {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(f)
            .expect("spawn test thread")
            .join()
            .expect("test thread panicked");
    }

    #[test]
    fn test_json_schema_to_typescript_bounds_deeply_nested_array() {
        // Issue #303: verifies the recursion-depth cap actually bounds this function's
        // descent for a `Value` nested far beyond MAX_SCHEMA_RECURSION_DEPTH (see that
        // constant's docs — this is defense-in-depth for the `pub` API, not a reachable
        // wire-path scenario). Past the cap, the type must degrade to a well-formed
        // `unknown[]...[]` string rather than continuing to recurse.
        run_on_large_stack(|| {
            let schema = nested_array_schema(5_000);
            let result = json_schema_to_typescript(&schema);
            assert!(result.starts_with("unknown"), "{result}");
            assert!(result.ends_with("[]"), "{result}");
        });
    }

    #[test]
    fn test_json_schema_to_typescript_bounds_deeply_nested_object() {
        // Same cap-behavior check as the array case above, for object nesting.
        run_on_large_stack(|| {
            let schema = nested_object_schema(5_000);
            let result = json_schema_to_typescript(&schema);
            assert!(result.contains("unknown"), "{result}");
        });
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
