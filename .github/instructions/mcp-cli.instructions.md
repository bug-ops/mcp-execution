---
applyTo: "crates/mcp-cli/**/*.rs"
---

# Copilot Instructions: mcp-cli

This crate is the **command-line interface** for MCP Execution. It is the **ONLY crate** in the workspace that can use `anyhow` for error handling.

## Error Handling - CRITICAL

**Use `anyhow::Result` and `anyhow::Context`**:

```rust
use anyhow::{Context, Result, bail};

fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .context("failed to read config file")?;

    let config: Config = toml::from_str(&content)
        .context("failed to parse config")?;

    if config.port == 0 {
        bail!("port cannot be zero");
    }

    Ok(config)
}
```

**DO NOT use `thiserror` in this crate** - it's for libraries only.

## CLI Patterns

### Command Structure

The CLI uses `clap` with derive macros:

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mcp-cli")]
#[command(about = "MCP Code Execution CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Introspect an MCP server
    Introspect {
        /// Server command to execute
        server: String,

        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Generate code from server tools
    Generate {
        /// Server to generate from
        server: String,

        #[arg(long, short)]
        output: PathBuf,
    },
}
```

### Output Formatting

Use the formatters module for consistent output:

```rust
use crate::formatters::{format_server_info, format_table};
use colored::Colorize;

println!("{}", "Success!".green().bold());
println!("{}", format_server_info(&info));
```

### Error Display

Application errors should be user-friendly:

```rust
fn main() -> Result<()> {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red().bold(), e);

        // Show cause chain
        let mut source = e.source();
        while let Some(cause) = source {
            eprintln!("  {} {}", "Caused by:".yellow(), cause);
            source = cause.source();
        }

        std::process::exit(1);
    }
    Ok(())
}
```

## Library Dependencies

When using library crates from this workspace:

```rust
// ✅ GOOD: Library crates return their own Result types
use mcp_bridge::Bridge;
use mcp_core::ServerId;

async fn connect_server(id: ServerId) -> Result<Bridge> {
    let bridge = Bridge::new(1000);
    bridge.connect(id, "server-cmd")
        .await
        .context("failed to connect to MCP server")?;
    Ok(bridge)
}
```

The `.context()` method converts library errors into `anyhow::Error` automatically.

## Async Main

Use Tokio for async main:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // Setup tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Introspect { server, output } => {
            commands::introspect(&server, output).await?;
        }
        Commands::Generate { server, output } => {
            commands::generate(&server, output).await?;
        }
    }

    Ok(())
}
```

## Testing

Tests in CLI crate can also use `anyhow::Result`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_parse_args() -> Result<()> {
        let cli = Cli::parse_from(&["mcp-cli", "introspect", "server"]);
        match cli.command {
            Commands::Introspect { server, .. } => {
                assert_eq!(server, "server");
            }
            _ => bail!("expected Introspect command"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_async_operation() -> Result<()> {
        // Async test with anyhow::Result
        Ok(())
    }
}
```

## Key Differences from Library Crates

| Aspect | mcp-cli (Application) | Other Crates (Libraries) |
|--------|----------------------|--------------------------|
| Error type | `anyhow::Error` | `thiserror::Error` |
| Result alias | `anyhow::Result<T>` | `crate::Result<T>` |
| Error creation | `bail!()`, `.context()` | Custom enum variants |
| Dependencies | Can depend on all other crates | Follow dependency graph |
| Panics | Acceptable in main.rs | Never in library code |

## Summary

- **Always use `anyhow`** for error handling
- Focus on **user experience** in output formatting
- Use **`.context()`** to add context when calling library functions
- Make errors **descriptive and actionable** for end users
- This is the **entry point** - it orchestrates all other crates
