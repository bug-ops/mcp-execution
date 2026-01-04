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

/// JSON output formatting.
pub mod json {
    use super::{Result, Serialize};

    /// Format data as JSON.
    ///
    /// Uses pretty-printing with 2-space indentation.
    pub fn format<T: Serialize>(data: &T) -> Result<String> {
        let json = serde_json::to_string_pretty(data)?;
        Ok(json)
    }

    /// Format data as compact JSON (no formatting).
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
    pub fn format<T: Serialize>(data: &T) -> Result<String> {
        // For text mode, use JSON without pretty printing
        json::format_compact(data)
    }
}

/// Pretty (human-readable) output formatting.
pub mod pretty {
    use super::{Colorize, Result, Serialize};

    /// Format data as colorized, human-readable output.
    ///
    /// Uses colors and formatting for better terminal readability.
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
            Value::String(s) => Ok(format!("\"{}\"", s.green())),
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
                    result.push_str(&format!("\"{}\": ", key.blue().bold()));
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
