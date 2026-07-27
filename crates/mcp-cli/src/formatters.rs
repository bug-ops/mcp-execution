//! Output formatters for CLI commands.
//!
//! Provides consistent formatting across all CLI commands for JSON, text, and pretty output modes.

use anyhow::Result;
use colored::Colorize;
use mcp_execution_core::cli::OutputFormat;
use serde::Serialize;

/// Format data according to the specified output format.
///
/// # Arguments
///
/// * `data` - The data to format (must be serializable)
/// * `format` - The output format (Json, Text, Pretty)
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::formatters::format_output;
/// use mcp_execution_core::cli::OutputFormat;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct ServerInfo {
///     name: String,
///     version: String,
/// }
///
/// let info = ServerInfo {
///     name: "test-server".to_string(),
///     version: "1.0.0".to_string(),
/// };
///
/// let output = format_output(&info, OutputFormat::Json)?;
/// assert!(output.contains("\"name\""));
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn format_output<T: Serialize>(data: &T, format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Json => json::format(data),
        OutputFormat::Text => text::format(data),
        OutputFormat::Pretty => pretty::format(data),
    }
}

/// Escapes a string for safe interpolation into hand-crafted `Text`/`Pretty` output lines.
///
/// Commands that render whole structs through [`format_output`] get escaping for free, since
/// every string value is serialized through `serde_json` before printing. Commands that instead
/// build freeform lines (e.g. `"Server: {name} ({id})"`) must escape server-supplied strings
/// themselves, or a malicious MCP server could inject raw ANSI/control escape sequences into the
/// user's terminal via handshake or tool metadata fields. `pretty`'s internal value formatter
/// delegates to this same function for its `String` values, so control characters (including
/// ESC) are backslash-escaped instead of passed through verbatim, and both call sites share one
/// implementation. The returned string is always JSON-quoted, even for input with no control
/// characters, since callers need one consistent (and unambiguous) rendering rather than
/// conditionally-quoted output.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::formatters::escape_display;
///
/// assert_eq!(escape_display("hello"), "\"hello\"");
/// assert_eq!(escape_display("esc\u{1b}[2J"), "\"esc\\u001b[2J\"");
/// ```
#[must_use]
pub fn escape_display(s: &str) -> String {
    // `Value::String`'s `Display` impl serializes through the same JSON string writer as
    // `serde_json::to_string`, but is infallible (no `Result` to unwrap): formatting a `String`
    // as a JSON string literal cannot fail.
    serde_json::Value::String(s.to_owned()).to_string()
}

/// Cap, in `char`s, on a single value sanitized by [`escape_error_text`] — e.g. one
/// `err.chain()` link's rendered text, or one `warn!` log argument, never a whole assembled
/// multi-part report.
///
/// 4000 is generous for any one link/argument this crate actually passes through
/// [`escape_error_text`] (`runner::sanitized_error_report`'s own chain-link text runs well under
/// this in practice), while still bounding how much a single hostile MCP server response can
/// force onto the terminal or into a log line. Does not bound a report's total length when it has
/// several causes (each is capped independently) or a backtrace (never passed through
/// [`escape_error_text`] at all — see `runner::sanitized_error_report`'s doc comment).
const MAX_ERROR_TEXT_LEN: usize = 4000;

/// Makes `s` safe to print to a terminal or log sink that does not itself escape untrusted content.
///
/// Neutralizes control characters (including line breaks), redacts any embedded URL's
/// credentials/query string, and bounds the result to 4000 `char`s.
///
/// Redaction runs *before* truncation deliberately — truncating first could cut a redacted URL's
/// marker off and leave a bare secret prefix as the last thing printed.
///
/// Delegates to two single-source-of-truth helpers rather than maintaining parallel logic here:
/// [`mcp_execution_core::redact_urls_in_text`] finds and redacts every `scheme://…` token in `s`
/// (a `reqwest`/`rmcp` transport error's `Display` routinely embeds the full request URL,
/// including a `?token=…`-style query string, inline in prose — see `runner::sanitized_error_report`,
/// whose whole reason for calling this function per-cause is to catch exactly that), and
/// [`mcp_execution_core::untrusted::sanitize_untrusted_text`] then neutralizes every character
/// `char::is_control` reports (the full C0 and C1 ranges, covering `\r`, `\n`, ESC, BEL, and
/// friends) plus the Markdown/ECMAScript line separators U+2028/U+2029, replacing each with a
/// space, and caps the result to 4000 `char`s (`MAX_ERROR_TEXT_LEN`, not itself public — its value
/// is documented here since a reader of this function's public docs cannot otherwise resolve it).
/// Used to sanitize command errors and log messages that may embed content from an untrusted MCP
/// server, or a URL the user themselves supplied with a secret in its query string, before they
/// reach the terminal.
///
/// `s` is treated as a single unit of untrusted text with no internal structure worth preserving
/// — including any newline it contains, which this collapses like every other control character.
/// This is why the name is `escape_error_text`, not e.g. `escape_report`: this function must
/// never be called on an already-assembled multi-cause report (that would flatten anyhow's own
/// trusted `Caused by:` structure along with the untrusted content it carries — see
/// `runner::sanitized_error_report`'s doc comment for how that structure is instead rebuilt by
/// calling this function once per cause and rejoining with separators the caller controls, rather
/// than sanitizing the whole rendered report as one blob). Only ever call this on one piece of
/// untrusted text at a time.
///
/// Sanitized here, at each `mcp-execution-cli` print/log call site — `runner::report_and_classify`
/// (indirectly, via `sanitized_error_report`) and the `warn!` logging in `commands::server` —
/// rather than where a server-supplied message first enters a [`mcp_execution_core::Error`] (e.g.
/// `ConnectionFailed`'s boxed `source`) in
/// `mcp-execution-core`/`mcp-execution-introspector`. `source` there is a generic `Box<dyn
/// std::error::Error + Send + Sync>`, not specifically MCP-server text, and `mcp_execution_core::Error`
/// is consumed by every crate in this workspace (this one, `mcp-execution-server`, and any future
/// one), not just terminal/log output; sanitizing at that shared boundary would force one
/// escaping policy — lossy, space-collapsing, terminal-oriented — onto every consumer, including
/// ones that legitimately want the raw text (e.g. `mcp-execution-server`'s own untrusted-metadata
/// handling in `mcp_execution_core::untrusted`, which has different escaping needs for an
/// LLM-facing prompt than this crate has for a terminal). Scoping the fix to where untrusted text
/// actually reaches a terminal/log sink — while still delegating the escaping logic itself to the
/// one authoritative sanitizer — keeps the policy decision local to the output boundary that
/// needs it, without widening this bug-fix change beyond `mcp-execution-cli`.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::formatters::escape_error_text;
///
/// let cause = "boom\u{1b}[2Jname\nfake extra line";
/// let escaped = escape_error_text(cause);
/// assert!(!escaped.contains('\u{1b}'));
/// assert!(!escaped.contains('\n'));
/// assert!(escaped.contains("boom"));
/// ```
///
/// A URL embedded in the text has its credentials/query string redacted too:
///
/// ```
/// use mcp_execution_cli::formatters::escape_error_text;
///
/// let cause = "error sending request for url (https://api.example.com/mcp?token=hunter2secret)";
/// let escaped = escape_error_text(cause);
/// assert!(!escaped.contains("hunter2secret"));
/// assert!(escaped.contains("https://api.example.com/mcp?<redacted>"));
/// ```
#[must_use]
pub fn escape_error_text(s: &str) -> String {
    let redacted = mcp_execution_core::redact_urls_in_text(s);
    mcp_execution_core::untrusted::sanitize_untrusted_text(&redacted, MAX_ERROR_TEXT_LEN)
}

/// JSON output formatting.
pub mod json {
    use super::{Result, Serialize};

    /// Format data as JSON.
    ///
    /// Uses pretty-printing with 2-space indentation.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails (e.g., if the data
    /// contains non-serializable types or custom serialization fails).
    ///
    /// # Examples
    ///
    /// ```
    /// use serde::Serialize;
    /// use mcp_execution_cli::formatters::json;
    ///
    /// #[derive(Serialize)]
    /// struct Data { value: i32 }
    ///
    /// let data = Data { value: 42 };
    /// let json = json::format(&data).unwrap();
    /// assert!(json.contains("42"));
    /// ```
    pub fn format<T: Serialize>(data: &T) -> Result<String> {
        let json = serde_json::to_string_pretty(data)?;
        Ok(json)
    }

    /// Format data as compact JSON (no formatting).
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails (e.g., if the data
    /// contains non-serializable types or custom serialization fails).
    ///
    /// # Examples
    ///
    /// ```
    /// use serde::Serialize;
    /// use mcp_execution_cli::formatters::json;
    ///
    /// #[derive(Serialize)]
    /// struct Data { value: i32 }
    ///
    /// let data = Data { value: 42 };
    /// let json = json::format_compact(&data).unwrap();
    /// assert!(!json.contains('\n'));
    /// ```
    pub fn format_compact<T: Serialize>(data: &T) -> Result<String> {
        let json = serde_json::to_string(data)?;
        Ok(json)
    }
}

/// Plain text output formatting.
pub mod text {
    use super::{Result, Serialize, json};

    /// Format data as plain text.
    ///
    /// Uses JSON representation but without colors or fancy formatting.
    /// Suitable for piping to other commands or scripts.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails (propagated from the
    /// underlying `json::format_compact` call).
    ///
    /// # Examples
    ///
    /// ```
    /// use serde::Serialize;
    /// use mcp_execution_cli::formatters::text;
    ///
    /// #[derive(Serialize)]
    /// struct Data { value: i32 }
    ///
    /// let data = Data { value: 42 };
    /// let text = text::format(&data).unwrap();
    /// assert!(text.contains("42"));
    /// ```
    pub fn format<T: Serialize>(data: &T) -> Result<String> {
        // For text mode, use JSON without pretty printing
        json::format_compact(data)
    }
}

/// Pretty (human-readable) output formatting.
pub mod pretty {
    use super::{Colorize, Result, Serialize, escape_display};

    /// Format data as colorized, human-readable output.
    ///
    /// Uses colors and formatting for better terminal readability.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails (e.g., if the data
    /// contains non-serializable types). Value formatting itself cannot fail.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde::Serialize;
    /// use mcp_execution_cli::formatters::pretty;
    ///
    /// #[derive(Serialize)]
    /// struct Data { value: i32 }
    ///
    /// let data = Data { value: 42 };
    /// let output = pretty::format(&data).unwrap();
    /// assert!(output.contains("42"));
    /// ```
    pub fn format<T: Serialize>(data: &T) -> Result<String> {
        // Convert to JSON value first for inspection
        let value = serde_json::to_value(data)?;

        // Format with colors
        format_value(&value, 0)
    }

    /// Recursively format a JSON value with colors and indentation.
    fn format_value(value: &serde_json::Value, indent: usize) -> Result<String> {
        use serde_json::Value;

        let indent_str = "  ".repeat(indent);
        let next_indent_str = "  ".repeat(indent + 1);

        match value {
            Value::Null => Ok("null".dimmed().to_string()),
            Value::Bool(b) => Ok(b.to_string().yellow().to_string()),
            Value::Number(n) => Ok(n.to_string().cyan().to_string()),
            Value::String(s) => Ok(escape_display(s).green().to_string()),
            Value::Array(arr) => {
                if arr.is_empty() {
                    return Ok("[]".to_string());
                }

                let mut result = "[\n".to_string();
                for (i, item) in arr.iter().enumerate() {
                    result.push_str(&next_indent_str);
                    result.push_str(&format_value(item, indent + 1)?);
                    if i < arr.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&indent_str);
                result.push(']');
                Ok(result)
            }
            Value::Object(obj) => {
                if obj.is_empty() {
                    return Ok("{}".to_string());
                }

                let mut result = "{\n".to_string();
                let entries: Vec<_> = obj.iter().collect();
                for (i, (key, val)) in entries.iter().enumerate() {
                    result.push_str(&next_indent_str);
                    let quoted_key = serde_json::to_string(key)?;
                    result.push_str(&quoted_key.blue().bold().to_string());
                    result.push_str(": ");
                    result.push_str(&format_value(val, indent + 1)?);
                    if i < entries.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&indent_str);
                result.push('}');
                Ok(result)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestData {
        name: String,
        count: i32,
        enabled: bool,
    }

    #[test]
    fn test_json_format() {
        let data = TestData {
            name: "test".to_string(),
            count: 42,
            enabled: true,
        };

        let output = json::format(&data).unwrap();
        assert!(output.contains("\"name\""));
        assert!(output.contains("\"test\""));
        assert!(output.contains("\"count\""));
        assert!(output.contains("42"));
        assert!(output.contains("\"enabled\""));
        assert!(output.contains("true"));
    }

    #[test]
    fn test_json_format_compact() {
        let data = TestData {
            name: "test".to_string(),
            count: 42,
            enabled: true,
        };

        let output = json::format_compact(&data).unwrap();
        // Compact format should not have newlines
        assert!(!output.contains('\n'));
        assert!(output.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_text_format() {
        let data = TestData {
            name: "test".to_string(),
            count: 42,
            enabled: true,
        };

        let output = text::format(&data).unwrap();
        // Text format uses compact JSON
        assert!(!output.contains('\n'));
        assert!(output.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_pretty_format() {
        let data = TestData {
            name: "test".to_string(),
            count: 42,
            enabled: true,
        };

        let output = pretty::format(&data).unwrap();
        // Pretty format should have structure
        assert!(output.contains("name"));
        assert!(output.contains("test"));
        assert!(output.contains("count"));
        assert!(output.contains("42"));
    }

    #[test]
    fn test_format_output_json() {
        let data = TestData {
            name: "test".to_string(),
            count: 42,
            enabled: true,
        };

        let output = format_output(&data, OutputFormat::Json).unwrap();
        assert!(output.contains("\"name\""));
    }

    #[test]
    fn test_format_output_text() {
        let data = TestData {
            name: "test".to_string(),
            count: 42,
            enabled: true,
        };

        let output = format_output(&data, OutputFormat::Text).unwrap();
        assert!(output.contains("\"name\""));
    }

    #[test]
    fn test_pretty_format_escapes_quotes_and_newlines() {
        // Regression test: strings containing embedded quotes, backslashes,
        // or newlines must round-trip through valid JSON once ANSI color
        // codes are stripped, not just be wrapped in literal quotes.
        #[derive(Serialize)]
        struct Message {
            text: String,
        }

        let data = Message {
            text: "line one\nline \"two\" with \\backslash\\".to_string(),
        };

        let output = pretty::format(&data).unwrap();
        let stripped = strip_ansi(&output);

        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed["text"], "line one\nline \"two\" with \\backslash\\");
    }

    #[test]
    fn test_pretty_format_escapes_object_keys() {
        // Regression test: object keys containing embedded quotes, backslashes,
        // or newlines must also be escaped, not just values (the schema-derived
        // property names rendered by `introspect --detailed` are attacker-controlled
        // by the remote MCP server).
        let mut data = std::collections::BTreeMap::new();
        data.insert("line one\nline \"two\" with \\backslash\\".to_string(), 1);

        let output = pretty::format(&data).unwrap();
        let stripped = strip_ansi(&output);

        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(
            parsed["line one\nline \"two\" with \\backslash\\"],
            serde_json::json!(1)
        );
    }

    /// Strips ANSI color escape sequences emitted by the `colored` crate.
    fn strip_ansi(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    #[test]
    fn test_escape_display_neutralizes_control_chars() {
        let escaped = escape_display("evil\u{1b}[2Jname");
        assert!(!escaped.contains('\u{1b}'));
        assert!(escaped.contains("\\u001b"));
    }

    #[test]
    fn test_escape_display_plain_string() {
        assert_eq!(escape_display("hello"), "\"hello\"");
    }

    #[test]
    fn test_escape_error_text_neutralizes_control_chars() {
        let cause = "boom\u{1b}[2Jname, connection refused";
        let escaped = escape_error_text(cause);
        assert!(!escaped.contains('\u{1b}'));
        assert!(escaped.contains("connection refused"));
    }

    #[test]
    fn test_escape_error_text_plain_text_unaffected() {
        let cause = "plain error, no control characters at all";
        assert_eq!(escape_error_text(cause), cause);
    }

    /// Regression test for #308/S1 (impl-critic): a single cause's own text embedding a raw
    /// newline must not survive as a real line break — the reason callers must never call this on
    /// an already-assembled multi-cause report (see this function's doc comment), only on one
    /// cause's text at a time, then rejoin with separators the caller controls (see
    /// `runner::sanitized_error_report`).
    #[test]
    fn test_escape_error_text_newlines_do_not_survive() {
        let hostile_cause = "boom\n\nCaused by:\n    0: Error: forged System component compromised";
        let escaped = escape_error_text(hostile_cause);
        assert!(!escaped.contains('\n'), "raw newline survived: {escaped}");
    }

    /// Regression test for #308/M3 (impl-critic): a lone `\r` (not part of a `\r\n` pair) must
    /// also be neutralized — `str::lines()`-based splitting handled this inconsistently, which
    /// is exactly why this delegates to `sanitize_untrusted_text`'s uniform char-by-char pass
    /// instead.
    #[test]
    fn test_escape_error_text_lone_carriage_return_neutralized() {
        let hostile = "before\rafter";
        let escaped = escape_error_text(hostile);
        assert!(!escaped.contains('\r'));
    }

    /// Leak B regression (see the security audit behind this fix): a
    /// `reqwest`/`rmcp` transport error's `Display` text embeds the full
    /// request URL, query string included, inline in prose. This must be
    /// redacted, not merely control-char-escaped.
    #[test]
    fn test_escape_error_text_redacts_embedded_url_secret() {
        let cause = "Client error: error sending request for url (http://127.0.0.1:1/mcp?token=REFUSEDSECRET), when send initialize request";
        let escaped = escape_error_text(cause);
        assert!(!escaped.contains("REFUSEDSECRET"));
        assert!(escaped.contains("http://127.0.0.1:1/mcp?<redacted>"));
        assert!(escaped.contains("when send initialize request"));
    }

    /// Redaction must run on the full text before the length cap is applied, so a secret
    /// straddling the truncation boundary is still fully redacted rather than surviving as a
    /// chopped-off prefix. `secret` is positioned so the `MAX_ERROR_TEXT_LEN`-char cut lands 5
    /// characters into it: a truncate-first implementation would keep exactly `secret[..5]` (a
    /// real prefix of the secret, not an unrelated substring) in its output.
    #[test]
    fn test_escape_error_text_redacts_secret_straddling_truncation_boundary() {
        let secret = "verysecretvalue";
        let url_prefix = "https://host.example.com/p?token=";
        let chars_before_secret = MAX_ERROR_TEXT_LEN - 5;
        let padding_len = chars_before_secret - 1 - url_prefix.chars().count();
        let padding = "x".repeat(padding_len);
        let cause = format!("{padding} {url_prefix}{secret}");
        assert_eq!(
            cause.chars().count(),
            MAX_ERROR_TEXT_LEN - 5 + secret.chars().count()
        );

        let escaped = escape_error_text(&cause);
        assert!(!escaped.contains(secret));
        assert!(!escaped.contains(&secret[..5]));
    }

    #[test]
    fn test_escape_error_text_caps_length() {
        let long = "a".repeat(MAX_ERROR_TEXT_LEN + 500);
        assert_eq!(escape_error_text(&long).chars().count(), MAX_ERROR_TEXT_LEN);
    }

    /// Pins the "4000" literal `escape_error_text`'s (necessarily public-facing, since the
    /// constant itself is private) doc comment states inline — a drift-detector, not a design
    /// constraint.
    #[test]
    fn test_max_error_text_len_matches_documented_value() {
        assert_eq!(MAX_ERROR_TEXT_LEN, 4000);
    }

    #[test]
    fn test_format_output_pretty() {
        let data = TestData {
            name: "test".to_string(),
            count: 42,
            enabled: true,
        };

        let output = format_output(&data, OutputFormat::Pretty).unwrap();
        assert!(output.contains("name"));
    }
}
