//! Documentation drift check for #563 and #609: user-facing docs must not reintroduce commands,
//! imports and names that do not work against freshly generated output.
//!
//! Two layers. A denylist guards prose. An executable-docs harness runs the fenced examples: the
//! imports of `typescript` fences go through `run` against a generated `github` package, and every
//! `mcp-execution-cli` command of a `bash` fence is parsed with the real `Cli` parser.
#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

#[cfg(feature = "test-fixtures")]
mod common;
mod package;

use std::assert_matches;
use std::fs;
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::str::Chars;

use clap::Parser as _;
use serde_json::{Map, Value};

/// Tools of the generated `github` fixture: the names the documents call or import.
const GITHUB_TOOLS: &[&str] = &[
    "create_issue",
    "update_issue",
    "list_issues",
    "create_pull_request",
    "get_authenticated_user",
    "list_pull_requests",
    "search_code",
];

/// How the documents spell the generated `github` package directory.
const GITHUB_DIR_IN_DOCS: &str = "~/.claude/servers/github/";

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

    let names: Vec<String> = mcp_execution_cli::Cli::command()
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

/// Writes the generated package of a `github` server with [`GITHUB_TOOLS`] below `servers`,
/// without contacting any server.
fn write_github_package(servers: &Path) {
    package::write_package(servers, "github", "GitHub", GITHUB_TOOLS);
}

/// A fenced code block of a document.
#[derive(Debug)]
struct Fence {
    lang: String,
    /// 1-based line number of the first body line.
    line: usize,
    body: Vec<String>,
}

fn fences(text: &str) -> Vec<Fence> {
    let mut found = Vec::new();
    let mut open: Option<Fence> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        match (&mut open, line.strip_prefix("```")) {
            (None, Some(info)) => {
                open = Some(Fence {
                    lang: info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    line: index + 2,
                    body: Vec::new(),
                });
            }
            (Some(_), Some("")) => found.extend(open.take()),
            (Some(fence), _) => fence.body.push(raw.to_owned()),
            (None, None) => {}
        }
    }
    found
}

/// One shell word and whether it was written as a single `'...'` literal.
#[derive(Debug, PartialEq, Eq)]
struct Word {
    text: String,
    single_quoted: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Word(Word),
    /// `|`, `||`, `&&`, `;` or `&`.
    Separator,
    /// `>`, `>>`, `<`, `&>`; the word that follows is the target.
    Redirect,
}

fn read_word(chars: &mut Peekable<Chars<'_>>) -> Result<Word, String> {
    let mut text = String::new();
    let mut single_segments = 0;
    let mut other = false;
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || matches!(c, '|' | '&' | ';' | '<' | '>') {
            break;
        }
        chars.next();
        match c {
            '\'' => {
                single_segments += 1;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => text.push(ch),
                        None => return Err("unterminated single quote".to_owned()),
                    }
                }
            }
            '"' => {
                other = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\' | '$' | '`')) => text.push(ch),
                            Some(ch) => {
                                text.push('\\');
                                text.push(ch);
                            }
                            None => return Err("unterminated double quote".to_owned()),
                        },
                        Some(ch) => text.push(ch),
                        None => return Err("unterminated double quote".to_owned()),
                    }
                }
            }
            '\\' => {
                other = true;
                text.push(chars.next().ok_or("dangling backslash")?);
            }
            _ => {
                other = true;
                text.push(c);
            }
        }
    }
    Ok(Word {
        text,
        single_quoted: single_segments == 1 && !other,
    })
}

fn tokenize(line: &str) -> Result<Vec<Token>, String> {
    let mut chars = line.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '#' => break,
            '|' | ';' => {
                chars.next();
                if c == '|' && chars.peek() == Some(&'|') {
                    chars.next();
                }
                tokens.push(Token::Separator);
            }
            '&' => {
                chars.next();
                match chars.peek() {
                    Some('&') => {
                        chars.next();
                        tokens.push(Token::Separator);
                    }
                    Some('>') => {
                        chars.next();
                        if chars.peek() == Some(&'>') {
                            chars.next();
                        }
                        tokens.push(Token::Redirect);
                    }
                    _ => tokens.push(Token::Separator),
                }
            }
            '<' | '>' => {
                chars.next();
                if c == '>' && chars.peek() == Some(&'>') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    chars.next();
                }
                tokens.push(Token::Redirect);
            }
            _ => {
                let word = read_word(&mut chars)?;
                let is_fd = !word.text.is_empty() && word.text.bytes().all(|b| b.is_ascii_digit());
                if is_fd && matches!(chars.peek(), Some('<' | '>')) {
                    continue;
                }
                tokens.push(Token::Word(word));
            }
        }
    }
    Ok(tokens)
}

/// Splits one shell line into its simple commands: separators end a command, a redirect drops
/// its target word.
fn split_commands(line: &str) -> Result<Vec<Vec<Word>>, String> {
    let mut commands = Vec::new();
    let mut current = Vec::new();
    let mut drop_next_word = false;
    for token in tokenize(line)? {
        match token {
            Token::Word(_) if drop_next_word => drop_next_word = false,
            Token::Word(word) => current.push(word),
            Token::Redirect => drop_next_word = true,
            Token::Separator => {
                drop_next_word = false;
                if !current.is_empty() {
                    commands.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if !current.is_empty() {
        commands.push(current);
    }
    Ok(commands)
}

/// Shell lines of a fence with `\` continuations joined, each with its 1-based line number.
fn logical_lines(fence: &Fence) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (index, raw) in fence.body.iter().enumerate() {
        let (start, mut text) = pending
            .take()
            .unwrap_or_else(|| (fence.line + index, String::new()));
        let trimmed = raw.trim();
        let (piece, continues) = trimmed
            .strip_suffix('\\')
            .map_or((trimmed, false), |head| (head.trim_end(), true));
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(piece);
        if continues {
            pending = Some((start, text));
        } else if !text.trim().is_empty() {
            lines.push((start, text));
        }
    }
    lines.extend(pending);
    lines
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// A usage synopsis (`generate <server> [OPTIONS]`) is not a runnable command.
fn is_synopsis(line: &str) -> bool {
    line.split_whitespace().any(|piece| {
        (piece.starts_with('<') && piece.ends_with('>') && piece.len() > 2)
            || piece.starts_with('[')
    })
}

/// The single optional JSON-object argument that generated tools and `call` take.
fn check_json_argument(args: &[Word]) -> Result<(), String> {
    match args {
        [] => Ok(()),
        [arg] => {
            if !arg.single_quoted {
                return Err(format!("argument `{}` is not single-quoted", arg.text));
            }
            serde_json::from_str::<Map<String, Value>>(&arg.text)
                .map(drop)
                .map_err(|e| format!("argument `{}` is not a JSON object: {e}", arg.text))
        }
        _ => Err(format!(
            "expected at most one single-quoted JSON argument, found {}",
            args.len()
        )),
    }
}

fn check_call(words: &[Word]) -> Result<(), String> {
    let Some(separator) = words.iter().position(|w| w.text == "--") else {
        return Ok(());
    };
    let [tool, args @ ..] = &words[separator + 1..] else {
        return Err("`call` has no tool name after `--`".to_owned());
    };
    let targets_github = words[..separator]
        .windows(2)
        .any(|pair| pair[0].text == "--from-config" && pair[1].text == "github");
    if targets_github && !GITHUB_TOOLS.contains(&tool.text.as_str()) {
        return Err(format!(
            "`{}` is not a tool of the generated server",
            tool.text
        ));
    }
    check_json_argument(args)
}

fn check_node(words: &[Word], generated: &Path) -> Result<(), String> {
    let Some(script) = words
        .iter()
        .skip(1)
        .position(|w| {
            Path::new(&w.text)
                .extension()
                .is_some_and(|ext| ext == "ts" || ext == "mts")
        })
        .map(|i| i + 1)
    else {
        return Ok(());
    };
    if !words[script].text.starts_with(GITHUB_DIR_IN_DOCS) {
        return Err(format!(
            "`{}` is not a file of the generated server",
            words[script].text
        ));
    }
    check_generated_path(&words[script].text, generated)?;
    check_json_argument(&words[script + 1..])
}

fn check_generated_path(word: &str, generated: &Path) -> Result<(), String> {
    let Some(rest) = word.strip_prefix(GITHUB_DIR_IN_DOCS) else {
        return Ok(());
    };
    if generated.join("github").join(rest).exists() {
        Ok(())
    } else {
        Err(format!("`{word}` is not in the generated output"))
    }
}

/// Everything wrong with one simple command, judged against the generated package below
/// `generated`.
fn command_problems(words: &[Word], generated: &Path) -> Vec<String> {
    let mut words = words;
    while words.first().is_some_and(|w| is_assignment(&w.text)) {
        words = &words[1..];
    }
    let Some(program) = words.first() else {
        return Vec::new();
    };
    let mut problems: Vec<String> = words
        .iter()
        .filter_map(|w| check_generated_path(&w.text, generated).err())
        .collect();
    match program.text.as_str() {
        "mcp-execution-cli" => {
            if let Err(e) = mcp_execution_cli::Cli::try_parse_from(words.iter().map(|w| &w.text))
                && e.use_stderr()
            {
                problems.push(format!("does not parse: {}", e.render()));
            }
            if words.get(1).is_some_and(|w| w.text == "call") {
                problems.extend(check_call(words).err());
            }
        }
        "node" => problems.extend(check_node(words, generated).err()),
        _ => {}
    }
    problems
}

/// Every problem of the `bash` fences of `text`, as `doc:line: message`.
fn bash_problems(doc: &Path, text: &str, generated: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for fence in fences(text)
        .iter()
        .filter(|f| matches!(f.lang.as_str(), "bash" | "sh" | "shell"))
    {
        for (line, raw) in logical_lines(fence) {
            let command_line = raw.strip_prefix("$ ").unwrap_or(&raw);
            let located = |message: String| format!("{}:{line}: {message}", doc.display());
            if command_line.starts_with("mcp-execution-cli ") && is_synopsis(command_line) {
                continue;
            }
            match split_commands(command_line) {
                Err(e) => problems.push(located(format!("cannot split `{command_line}`: {e}"))),
                Ok(commands) => problems.extend(
                    commands
                        .iter()
                        .flat_map(|words| command_problems(words, generated))
                        .map(located),
                ),
            }
        }
    }
    problems
}

/// The import statements of a fence as one module: static imports verbatim, dynamic `import(..)`
/// expressions awaited. `None` when the fence imports nothing.
fn import_module(fence: &Fence) -> Option<String> {
    let mut statements = Vec::new();
    let mut open_static: Option<String> = None;
    for raw in &fence.body {
        let line = raw.trim();
        if let Some(statement) = &mut open_static {
            statement.push('\n');
            statement.push_str(line);
            if line.ends_with(';') {
                statements.extend(open_static.take());
            }
        } else if line.starts_with("import ") || line.starts_with("import{") {
            if line.ends_with(';') {
                statements.push(line.to_owned());
            } else {
                open_static = Some(line.to_owned());
            }
        } else if !line.starts_with("//")
            && let Some(start) = line.find("import(")
        {
            let mut depth = 0_usize;
            let end = line[start..].char_indices().find_map(|(i, c)| {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                (depth == 0 && c == ')').then_some(start + i + 1)
            })?;
            statements.push(format!("await {};", &line[start..end]));
        }
    }
    (!statements.is_empty()).then(|| statements.join("\n"))
}

/// Every problem of the `typescript` fences of `text`, as `doc:line: message`: the imports of
/// each fence are run through `run` against the package generated below the sandbox.
#[cfg(feature = "test-fixtures")]
fn typescript_problems(sandbox: &common::Sandbox, doc: &Path, text: &str) -> Vec<String> {
    let scripts = sandbox.home.join("scripts");
    fs::create_dir_all(&scripts).expect("create scripts dir");
    let mut problems = Vec::new();
    for (n, fence) in fences(text)
        .iter()
        .filter(|f| matches!(f.lang.as_str(), "typescript" | "ts" | "mts"))
        .enumerate()
    {
        let Some(module) = import_module(fence) else {
            continue;
        };
        let script = scripts.join(format!("fence-{n}.mts"));
        fs::write(&script, module).expect("write script");
        let out = sandbox.run_script(&script, &["--timeout", "60"]);
        if out.code != Some(0) {
            let detail: Vec<&str> = out.stderr_lines().into_iter().take(4).collect();
            problems.push(format!(
                "{}:{}: imports do not run (exit {:?}): {}",
                doc.display(),
                fence.line,
                out.code,
                detail.join(" | ")
            ));
        }
    }
    problems
}

#[cfg(feature = "test-fixtures")]
fn github_sandbox(test: &str) -> Option<common::Sandbox> {
    common::require_node(test).then(|| {
        let sandbox = common::Sandbox::new();
        write_github_package(&sandbox.servers);
        sandbox
    })
}

#[test]
fn bash_fences_of_the_docs_parse_and_name_generated_files() {
    let generated = tempfile::tempdir().expect("temp dir");
    write_github_package(generated.path());
    let mut problems = Vec::new();
    for doc in documents() {
        let text = fs::read_to_string(&doc).expect("readable document");
        problems.extend(bash_problems(&doc, &text, generated.path()));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
#[cfg(feature = "test-fixtures")]
fn typescript_fences_of_the_docs_import_from_generated_output() {
    let Some(sandbox) = github_sandbox("typescript_fences_of_the_docs") else {
        return;
    };
    let mut problems = Vec::new();
    for doc in documents() {
        let text = fs::read_to_string(&doc).expect("readable document");
        problems.extend(typescript_problems(&sandbox, &doc, &text));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn bash_harness_rejects_broken_commands() {
    let generated = tempfile::tempdir().expect("temp dir");
    write_github_package(generated.path());
    let bad = [
        "$ node ~/.claude/servers/github/createIssue.ts --flag",
        "node ~/.claude/servers/github/createIssue.ts '{bad json}'",
        "node ~/.claude/servers/github/createIssue.ts \"{}\"",
        "node ~/.claude/servers/github/missing.ts '{}'",
        "mcp-execution-cli nonsense github",
        "mcp-execution-cli call --from-config github -- create_issue '[1]'",
        "mcp-execution-cli call --from-config github -- no_such_tool '{}'",
        "FOO=1 mcp-execution-cli nonsense | head",
        "cat ~/.claude/servers/github/missing.ts > /dev/null",
        "mcp-execution-cli run x.mts 2>&1 && mcp-execution-cli nonsense",
        "mcp-execution-cli run \\\n  --no-such-flag x.mts",
    ];
    for command in bad {
        let text = format!("```bash\n{command}\n```\n");
        let problems = bash_problems(Path::new("synthetic.md"), &text, generated.path());
        assert!(!problems.is_empty(), "accepted `{command}`");
    }
}

#[test]
fn bash_harness_accepts_valid_commands() {
    let generated = tempfile::tempdir().expect("temp dir");
    write_github_package(generated.path());
    let good = "```bash\n\
        # a comment\n\
        $ node ~/.claude/servers/github/createIssue.ts '{\"owner\":\"o\"}'\n\
        node ~/.claude/servers/github/createIssue.ts\n\
        cat ~/.claude/servers/github/catalog.ts | head -n 5 > /dev/null 2>&1\n\
        FOO=1 BAR=2 mcp-execution-cli run x.mts --restrict   # Unix only\n\
        mcp-execution-cli call --from-config github -- list_issues '{\"state\":\"open\"}'\n\
        mcp-execution-cli generate docker \\\n  --arg=run --arg=-i \\\n  --name=github\n\
        mcp-execution-cli generate <server> [OPTIONS]\n\
        ```\n";
    let problems = bash_problems(Path::new("synthetic.md"), good, generated.path());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn shell_splitter_handles_prefixes_pipes_redirects_and_quotes() {
    let commands = split_commands("FOO=1 cat 'a b' | grep \"x y\" 2>&1 > out.txt && echo done")
        .expect("splits");
    let texts: Vec<Vec<&str>> = commands
        .iter()
        .map(|words| words.iter().map(|w| w.text.as_str()).collect())
        .collect();
    assert_eq!(
        texts,
        [
            vec!["FOO=1", "cat", "a b"],
            vec!["grep", "x y"],
            vec!["echo", "done"]
        ]
    );
    assert!(commands[0][2].single_quoted);
    assert!(!commands[1][1].single_quoted);
    assert_matches!(split_commands("echo 'open"), Err(e) if e.contains("single quote"));
    assert_matches!(split_commands("# only a comment"), Ok(c) if c.is_empty());
}

#[test]
#[cfg(feature = "test-fixtures")]
fn typescript_harness_runs_imports_and_rejects_broken_ones() {
    let Some(sandbox) = github_sandbox("typescript_harness_runs_imports") else {
        return;
    };
    let wrap = |code: &str| format!("```typescript\n{code}\n```\n");
    let good = [
        "import { createIssue } from 'mcp:github';",
        "import {\n  listIssues,\n  searchCode\n} from 'mcp:github';",
        "import { callMCPTool } from '../.claude/servers/github/_runtime/mcp-bridge.ts';",
    ];
    for code in good {
        let problems = typescript_problems(&sandbox, Path::new("good.md"), &wrap(code));
        assert!(problems.is_empty(), "rejected `{code}`: {problems:?}");
    }
    let bad = [
        "import { x } from \"~/.claude/servers/github/x.ts\";",
        "import { x } from '~/.claude/servers/github/x.ts';",
        "const m = await import('./x.js');",
        "import { createIssue } from '../.claude/servers/github/createIssue.js';",
        "import { noSuchTool } from 'mcp:github';",
        "import { createIssue } from 'mcp:no-such-server';",
    ];
    for code in bad {
        let problems = typescript_problems(&sandbox, Path::new("bad.md"), &wrap(code));
        assert!(!problems.is_empty(), "accepted `{code}`");
    }
}
