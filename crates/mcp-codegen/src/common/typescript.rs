//! TypeScript code generation utilities.
//!
//! Provides name-casing and identifier helpers and the exact string-literal encoder used when
//! generating TypeScript. Schema conversion lives in [`ts_types`](crate::common::ts_types).
//!
//! # Examples
//!
//! ```
//! use mcp_execution_codegen::common::typescript;
//!
//! assert_eq!(typescript::to_camel_case("send_message"), "sendMessage");
//! ```

use mcp_execution_core::untrusted::{MAX_UNTRUSTED_FIELD_LEN, sanitize_untrusted_text};

/// Maximum nesting depth [`convert_schema`](crate::common::ts_types::convert_schema) descends
/// into before it renders the remainder of a branch as `unknown` and records a depth
/// diagnostic.
///
/// Every descent counts: nested objects, array items, union and intersection members, and each
/// `$ref` hop. `mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES` bounds a schema's serialized
/// size, not its nesting depth, so this bound is what keeps the converter's recursion, and the
/// rendering of its result, from overflowing the stack on a deeply nested `Value`.
///
/// A schema arriving over the wire from an MCP server is already limited by `serde_json`'s own
/// default recursion limit (128, never raised in this workspace): a response nested deeper than
/// about 122 array levels, or about 61 `properties` levels, is dropped as a parse error before a
/// `ToolInfo` is built. The value 128 sits at that ceiling, so a schema that arrives through
/// introspection is never clipped. The ceiling is coupled to `serde_json`'s default; if that
/// changes, re-check that this bound never clips a real wire schema.
///
/// The bound does not cover the rest of the generation pipeline: cloning, serializing and
/// dropping a `serde_json::Value` stay recursive without a limit.
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

/// Sanitizes a server-controlled string for safe interpolation into `JSDoc` block comments.
///
/// Neutralizes control characters (C0, DEL, C1 — everything `char::is_control` reports —
/// plus U+2028 LINE SEPARATOR/U+2029 PARAGRAPH SEPARATOR, which ECMAScript treats as line
/// terminators even though they aren't in the control-character category) by delegating to
/// the shared [`mcp_execution_core::untrusted::sanitize_untrusted_text`], which *replaces*
/// each with a space rather than deleting it — deleting would glue adjacent words together
/// (`"tab\tseparated"` -> `"tabseparated"`) and, more importantly, would let a control
/// character sitting between `*` and `/` collapse into a live `JSDoc` comment terminator once
/// removed. This neutralization runs *before* the `*/` escape step for exactly that reason:
/// escaping first would see no `*/` match (the control character still separates them), then
/// deleting/collapsing that character afterward would reopen the comment. Truncation to
/// `max_len` runs last, after escaping, so a `*/` straddling the truncation boundary is
/// already widened to `*\/` and cannot be split back into a bare `*/` by the cut.
pub(crate) fn sanitize_jsdoc(s: &str, max_len: usize) -> String {
    let neutralized = mcp_execution_core::untrusted::sanitize_untrusted_text(s, usize::MAX);
    let sanitized = neutralized.replace("*/", "*\\/");
    if sanitized.chars().count() > max_len {
        sanitized.chars().take(max_len).collect()
    } else {
        sanitized
    }
}

/// The `\uXXXX` escape of a character in the Basic Multilingual Plane.
fn unicode_escape(c: char) -> String {
    format!("\\u{:04x}", u32::from(c))
}

/// Returns `s` as the body of a single-quoted TypeScript string literal that evaluates back to
/// exactly `s`, or `None` when that cannot be guaranteed.
///
/// Control characters, quotes, backslashes and the line separators U+2028/U+2029 are escaped.
/// `None` above [`MAX_UNTRUSTED_FIELD_LEN`] characters, or when the text contains a character
/// the shared sanitizer would neutralize (bidi controls, the Unicode Tags block, zero-width
/// operators, variation-selector runs): such text cannot be kept both exact and safe.
pub(crate) fn ts_string_literal_exact(s: &str) -> Option<String> {
    if s.chars().count() > MAX_UNTRUSTED_FIELD_LEN {
        return None;
    }
    let mut body = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => body.push_str("\\\\"),
            '\'' => body.push_str("\\'"),
            '\n' => body.push_str("\\n"),
            '\r' => body.push_str("\\r"),
            '\t' => body.push_str("\\t"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') => {
                body.push_str(&unicode_escape(c));
            }
            c => body.push(c),
        }
    }
    (sanitize_untrusted_text(&body, usize::MAX) == body).then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ts_string_literal_exact_escapes_control_characters_and_rejects_invisible_ones() {
        assert_eq!(
            ts_string_literal_exact("a\u{1}b").as_deref(),
            Some("a\\u0001b")
        );
        assert_eq!(
            ts_string_literal_exact("a\u{2028}b").as_deref(),
            Some("a\\u2028b")
        );
        assert_eq!(ts_string_literal_exact("a\u{e0041}b"), None);
        assert_eq!(ts_string_literal_exact("a\u{feff}b"), None);
    }
}
