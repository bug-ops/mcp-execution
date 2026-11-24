//! Documentation drift check for #563: user-facing docs must not reintroduce commands, imports and
//! names that do not work against freshly generated output.
#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Figures and names no user-facing text (documents and rustdoc front matter) may contain.
const STALE_TEXT: &[(&str, &str)] = &[
    ("98%", "unmeasured figure; no command reproduces it"),
    ("30,000", "unmeasured baseline; no command reproduces it"),
    ("WASM", "the project no longer has a WebAssembly runtime"),
    (
        "Node.js 18",
        "generated `.ts` files need Node.js 22.18.0 or newer",
    ),
];

/// Substrings no user-facing document may contain, each with the reason.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "github-mcp-execution-server",
        "no such server exists; it is `github/github-mcp-server`",
    ),
    (
        "npm install @modelcontextprotocol/sdk",
        "the generated bridge imports only Node built-ins",
    ),
    (
        "from '~",
        "`~` is not expanded in an import specifier; import by relative path",
    ),
    (
        "next to the generated directory",
        "`generate` replaces the server directory; user scripts must live elsewhere",
    ),
    (
        "sit in the generated server directory",
        "`generate` replaces the server directory; user scripts must live elsewhere",
    ),
    (
        ".js';",
        "generated files are `.ts` and are imported with the extension",
    ),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the workspace root")
        .to_path_buf()
}

fn markdown_in(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect()
}

fn documents() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut docs = vec![root.join("README.md")];
    docs.extend(markdown_in(&root.join("examples")));
    let mut crates: Vec<PathBuf> = fs::read_dir(root.join("crates"))
        .expect("crates directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("README.md"))
        .filter(|path| path.is_file())
        .collect();
    crates.sort();
    docs.extend(crates);
    docs.push(root.join("CONTRIBUTING.md"));
    docs
}

fn rust_sources_in(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_dir() {
            rust_sources_in(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn rustdoc_front_matter_lines() -> Vec<(PathBuf, usize, String)> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(workspace_root().join("crates"))
        .expect("crates directory")
        .filter_map(Result::ok)
    {
        let src = entry.path().join("src");
        if src.is_dir() {
            rust_sources_in(&src, &mut sources);
        }
    }
    sources.sort();
    let mut lines = Vec::new();
    for path in sources {
        let text = fs::read_to_string(&path).expect("readable source");
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//!") {
                lines.push((path.clone(), number + 1, line.to_owned()));
            }
        }
    }
    lines
}

#[test]
fn docs_contain_no_known_broken_examples() {
    let mut violations = Vec::new();
    for doc in documents() {
        let text = fs::read_to_string(&doc).expect("readable document");
        for (number, line) in text.lines().enumerate() {
            for (needle, reason) in FORBIDDEN.iter().chain(STALE_TEXT) {
                if line.contains(needle) {
                    violations.push(format!(
                        "{}:{}: `{needle}`: {reason}",
                        doc.display(),
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn node_invocations_of_generated_tools_pass_one_json_argument() {
    let mut violations = Vec::new();
    for doc in documents() {
        let text = fs::read_to_string(&doc).expect("readable document");
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let Some(args) = trimmed
                .strip_prefix("node ")
                .and_then(|rest| rest.split_once(".ts"))
                .map(|(_, args)| args.trim())
            else {
                continue;
            };
            if !(args.is_empty() || args.starts_with('\'')) {
                violations.push(format!(
                    "{}:{}: expected one single-quoted JSON argument, found `{args}`",
                    doc.display(),
                    number + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn rustdoc_front_matter_contains_no_stale_figures() {
    let mut violations = Vec::new();
    for (path, number, line) in rustdoc_front_matter_lines() {
        for (needle, reason) in STALE_TEXT {
            if line.contains(needle) {
                violations.push(format!("{}:{number}: `{needle}`: {reason}", path.display()));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn user_scripts_are_not_shown_inside_the_generated_server_directory() {
    let mut violations = Vec::new();
    for doc in documents() {
        let text = fs::read_to_string(&doc).expect("readable document");
        for (number, line) in text.lines().enumerate() {
            let in_servers_dir = line.contains(".claude/servers/");
            let names_a_script = line.contains("script.ts") || line.contains("script.mts");
            if in_servers_dir && names_a_script {
                violations.push(format!(
                    "{}:{}: a user script must not live under `~/.claude/servers/`, `generate` replaces that directory",
                    doc.display(),
                    number + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn docs_never_call_a_measurement_savings() {
    let mut violations = Vec::new();
    for doc in documents() {
        let text = fs::read_to_string(&doc).expect("readable document");
        for (number, line) in text.lines().enumerate() {
            if line.to_lowercase().contains("saving") {
                violations.push(format!(
                    "{}:{}: figures are sizes and estimates, never labelled savings (ADR-562)",
                    doc.display(),
                    number + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn documented_commands_are_real_subcommands() {
    use clap::CommandFactory as _;

    let names: Vec<String> = mcp_execution_cli::cli::Cli::command()
        .get_subcommands()
        .map(|c| c.get_name().to_owned())
        .collect();
    let mut violations = Vec::new();
    for doc in documents() {
        let text = fs::read_to_string(&doc).expect("readable document");
        for (number, line) in text.lines().enumerate() {
            let Some(rest) = line.trim_start().strip_prefix("mcp-execution-cli ") else {
                continue;
            };
            let word = rest.split_whitespace().next().unwrap_or_default();
            let is_command_word = word.starts_with(|c: char| c.is_ascii_lowercase());
            if is_command_word && !names.iter().any(|name| name == word) {
                violations.push(format!(
                    "{}:{}: `{word}` is not a subcommand",
                    doc.display(),
                    number + 1
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}
