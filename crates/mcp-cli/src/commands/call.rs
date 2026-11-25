//! `call`: invoke one tool of one server and print its result within the output budget.
//!
//! The command is a client of [`Introspector::connect`]: it owns no connection code. The
//! result is printed as one compact JSON value, strings included (quoted, like the generated
//! wrapper), and an empty success prints nothing. Output past the stdout budget is cut and the full text
//! is spilled to a private file named in a trailing notice.

use std::fmt;

use anyhow::{Context, Result};
use clap::Args;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_core::helper_protocol::ResultSource;
use mcp_execution_core::{Error as CoreError, ServerId, ToolName};
use mcp_execution_introspector::{
    CallTimeout, Introspector, ServerText, ToolCall, ToolCallOutcome,
};
use serde_json::{Map, Value};

use super::common::{ServerSource, refuse_inside_run, resolve_server_config};
use crate::cli::ServerFlags;
use crate::formatters::escape_display;
use crate::runtime::budget::OutputBudget;
use crate::runtime::dirs::RuntimeDirs;
use crate::runtime::helper::DEFAULT_CALL_TIMEOUT;
use crate::runtime::spill::{MAX_SPILL_FILE_BYTES, Spill};

/// Options of the `call` command.
///
/// The tool name and the optional JSON object of arguments follow the server flags after `--`.
/// A non-object or malformed JSON argument exits 2 without echoing the text.
#[derive(Args)]
pub struct CallArgs {
    /// Server selection, transport, and timeout flags (shared with `introspect`)
    #[command(flatten)]
    pub flags: ServerFlags,

    /// Tool name, then an optional JSON object of arguments (after `--`)
    #[arg(last = true, required = true, num_args = 1..=2, value_names = ["TOOL", "JSON"])]
    pub call: Vec<String>,
}

// Hand-written so the JSON argument, which may carry secrets, never reaches a debug print.
impl fmt::Debug for CallArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallArgs")
            .field("flags", &self.flags)
            .field("tool", &self.call.first())
            .field("arguments", &"<redacted>")
            .finish()
    }
}

/// Runs the `call` command.
///
/// Exit codes: a tool-reported or JSON-RPC error is [`ExitCode::SERVER_ERROR`] with a one-line
/// message on stderr; connection failures are 3, a timeout 4 and invalid input 2.
///
/// # Errors
///
/// Returns an error for invalid input (including a call made inside `mcp-execution-cli run`),
/// connection failure or a timeout.
pub async fn run(args: CallArgs, _format: OutputFormat) -> Result<ExitCode> {
    refuse_inside_run()?;
    let CallArgs { flags, call } = args;
    let (tool, arguments) = parse_call(&call)?;
    let (server_id, config) = resolve_server_config(ServerSource::try_from(flags)?)?;

    let session = Introspector::new()
        .connect(server_id.clone(), &config)
        .await
        .with_context(|| format!("failed to connect to server '{server_id}'"))?;
    let outcome = session
        .call_tool(
            ToolCall::new(&tool, arguments, ResultSource::Structured),
            CallTimeout::new(DEFAULT_CALL_TIMEOUT)?,
        )
        .await;
    session.close().await;
    let outcome = outcome.with_context(|| format!("call to '{server_id}' failed"))?;

    report(&server_id, &tool, outcome).await
}

fn parse_call(call: &[String]) -> Result<(ToolName, Map<String, Value>)> {
    let [tool, rest @ ..] = call else {
        return Err(CoreError::InvalidArgument("a tool name is required".to_string()).into());
    };
    let tool = ToolName::new(tool.as_str())
        .map_err(|e| CoreError::InvalidArgument(format!("invalid tool name: {e}")))?;
    let arguments = match rest.first() {
        None => Map::new(),
        Some(raw) => parse_arguments(raw)?,
    };
    Ok((tool, arguments))
}

/// Parses the JSON argument; no error text carries any part of `raw`.
fn parse_arguments(raw: &str) -> Result<Map<String, Value>> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(CoreError::InvalidArgument(
            "the JSON argument must be a single JSON object".to_string(),
        )
        .into()),
        Err(error) => Err(CoreError::InvalidArgument(format!(
            "the JSON argument is not valid JSON ({} at line {}, column {})",
            error_class(&error),
            error.line(),
            error.column()
        ))
        .into()),
    }
}

fn error_class(error: &serde_json::Error) -> &'static str {
    use serde_json::error::Category;
    match error.classify() {
        Category::Io => "i/o error",
        Category::Syntax => "syntax error",
        Category::Data => "data error",
        Category::Eof => "unexpected end",
    }
}

async fn report(
    server_id: &ServerId,
    tool: &ToolName,
    outcome: ToolCallOutcome,
) -> Result<ExitCode> {
    match outcome {
        ToolCallOutcome::Value(value) => {
            print_within_budget(&serde_json::to_string(&value)?).await?;
            Ok(ExitCode::SUCCESS)
        }
        ToolCallOutcome::Empty => Ok(ExitCode::SUCCESS),
        ToolCallOutcome::ToolError { server_text } => {
            eprintln!(
                "Error: tool '{tool}' on server '{server_id}' reported an error{}",
                server_message(server_text.as_ref())
            );
            Ok(ExitCode::SERVER_ERROR)
        }
        ToolCallOutcome::RpcError { code, server_text } => {
            let code = code.map_or_else(String::new, |c| format!(" (code {c})"));
            eprintln!(
                "Error: server '{server_id}' answered the call to '{tool}' with a JSON-RPC error{code}{}",
                server_message(server_text.as_ref())
            );
            Ok(ExitCode::SERVER_ERROR)
        }
        _ => {
            eprintln!(
                "Error: server '{server_id}' answered the call to '{tool}' with an unrecognized outcome"
            );
            Ok(ExitCode::SERVER_ERROR)
        }
    }
}

/// Server text as a quoted, escaped suffix that marks it as coming from the server.
fn server_message(text: Option<&ServerText>) -> String {
    text.map_or_else(String::new, |t| {
        let cut = if t.truncated { ", truncated" } else { "" };
        format!("; server message{cut}: {}", escape_display(&t.text))
    })
}

/// Prints `text`, cutting it to the stdout budget and spilling the whole of it when it does
/// not fit.
async fn print_within_budget(text: &str) -> Result<()> {
    let cut = OutputBudget::stdout_default().apply(text);
    if !cut.is_truncated() {
        println!("{text}");
        return Ok(());
    }
    let spilled = Spill::new(&RuntimeDirs::resolve()?)
        .write(text.as_bytes())
        .await
        .context("cannot store the full output")?;
    let kept = if spilled.truncated {
        format!(" (the file holds its first {MAX_SPILL_FILE_BYTES} bytes)")
    } else {
        String::new()
    };
    println!(
        "{}\n[output truncated: showing {} of {} bytes; full output in {}{kept}]",
        cut.head,
        cut.head.len(),
        cut.total_bytes,
        spilled.path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    #[test]
    fn arguments_default_to_an_empty_object() {
        let (_, arguments) = parse_call(&["echo".to_string()]).unwrap();
        assert!(arguments.is_empty());
    }

    #[test]
    fn arguments_must_be_a_single_object_and_are_never_echoed() {
        for raw in [
            r#""secret-xyz""#,
            r#"[1,"secret-xyz"]"#,
            r#"{"k":"secret-xyz""#,
        ] {
            let error = parse_arguments(raw).unwrap_err().to_string();
            assert!(!error.contains("secret-xyz"), "{error}");
        }
    }

    #[test]
    fn an_invalid_tool_name_is_invalid_input() {
        let error = parse_call(&["bad name!".to_string()]).unwrap_err();
        assert_matches!(
            error.downcast_ref::<CoreError>(),
            Some(CoreError::InvalidArgument(_))
        );
    }

    #[test]
    fn server_message_is_escaped_and_marked() {
        let text = ServerText {
            text: "x\u{1b}[2J".to_string(),
            truncated: true,
        };
        let line = server_message(Some(&text));
        assert!(line.starts_with("; server message, truncated: \""));
        assert!(!line.contains('\u{1b}'));
        assert_eq!(server_message(None), "");
    }

    #[test]
    fn debug_hides_the_json_argument() {
        use crate::cli::{Cli, Commands};
        use clap::Parser;
        let cli = Cli::parse_from([
            "mcp-execution-cli",
            "call",
            "srv",
            "--",
            "t",
            r#"{"k":"s3cr3t"}"#,
        ]);
        let Commands::Call(args) = cli.command else {
            panic!("expected call")
        };
        assert!(!format!("{args:?}").contains("s3cr3t"));
    }
}
