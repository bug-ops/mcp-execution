//! Optional `--typecheck`: a `tsc --noEmit` pass over the script before node starts.
//!
//! The runner builds a tsconfig in the run directory from the generated [`TSCONFIG_JSON`]
//! compiler options plus `typeRoots` (the `@types` directories next to the generated servers),
//! `paths` (`mcp:*` to the generated servers, `mcp:helpers` to a written declaration file) and
//! `include`. It never reads the script itself. The check is skipped, with a notice, when `tsc`
//! is missing, when `tsc` runs too long, or when every diagnostic is only about missing Node
//! type definitions; anything else is a type error, and diagnostics that are only a consequence
//! of missing Node types are left out of what is shown.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use mcp_execution_codegen::TSCONFIG_JSON;
use mcp_execution_core::write_confined_file;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::runtime::exec::{DRAIN_GRACE, Drained, PipeCapture, ProcessTree, drain};
use crate::runtime::path_lookup::find_on_path;
use crate::runtime::spill::MAX_SPILL_FILE_BYTES;

/// Most server directories listed when building `typeRoots`.
pub const MAX_TYPECHECK_SERVER_DIRS: usize = 256;

/// How long `tsc` may run before the check is skipped.
pub const TYPECHECK_TIMEOUT: Duration = Duration::from_secs(60);

/// Most diagnostic lines shown to the caller.
pub const MAX_DIAGNOSTIC_LINES: usize = 20;

/// Most diagnostic bytes shown to the caller.
pub const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;

const HELPERS_DECLARATIONS: &str = include_str!("helpers.d.ts");

/// Diagnostics that only say a Node type definition is missing.
const MISSING_NODE_TYPES_CODES: [&str; 3] = ["TS2580", "TS2591", "TS2503"];

const MODULE_NOT_FOUND_CODE: &str = "TS2307";

/// Implicit-any diagnostics, which unresolved types make appear.
const IMPLICIT_ANY_CODES: [&str; 2] = ["TS7006", "TS7031"];

/// "Cannot find type definition file for 'node'", raised by `types: ["node"]`.
const MISSING_TYPE_FILE_CODE: &str = "TS2688";

/// Names Node resolves without the `node:` prefix.
const NODE_BUILTINS: [&str; 24] = [
    "assert",
    "buffer",
    "child_process",
    "crypto",
    "dns",
    "events",
    "fs",
    "fs/promises",
    "http",
    "https",
    "module",
    "net",
    "os",
    "path",
    "process",
    "readline",
    "stream",
    "timers",
    "tls",
    "url",
    "util",
    "worker_threads",
    "zlib",
    "test",
];

/// What the type check concluded.
///
/// # Examples
///
/// ```
/// use std::assert_matches;
/// use mcp_execution_cli::runtime::typecheck::TypecheckOutcome;
///
/// let outcome = TypecheckOutcome::Skipped { reason: "tsc not found".to_string() };
/// assert_matches!(outcome, TypecheckOutcome::Skipped { .. });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypecheckOutcome {
    /// `tsc` found no errors.
    Passed,
    /// The check could not run meaningfully; the run proceeds with a notice.
    Skipped {
        /// Why the check was skipped.
        reason: String,
    },
    /// A real type error; the run must not start.
    Failed {
        /// Sanitized diagnostics, within [`MAX_DIAGNOSTIC_LINES`] and [`MAX_DIAGNOSTIC_BYTES`].
        diagnostics: String,
    },
}

fn skipped(reason: &str) -> TypecheckOutcome {
    TypecheckOutcome::Skipped {
        reason: reason.to_string(),
    }
}

/// Type-checks `script` against the generated servers under `servers_dir`.
///
/// `run_dir` receives the generated tsconfig and `helpers.d.ts`.
///
/// # Errors
///
/// Returns an error when the scratch files cannot be written or `tsc` cannot be started.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::typecheck::{TypecheckOutcome, typecheck};
/// use std::path::Path;
///
/// # async fn example() -> anyhow::Result<()> {
/// let outcome = typecheck(
///     Path::new("script.ts"),
///     Path::new("/home/user/.claude/servers"),
///     Path::new("/state/runs/abc"),
/// )
/// .await?;
/// assert_eq!(outcome, TypecheckOutcome::Passed);
/// # Ok(())
/// # }
/// ```
pub async fn typecheck(
    script: &Path,
    servers_dir: &Path,
    run_dir: &Path,
) -> Result<TypecheckOutcome> {
    let Some(tsc) = find_on_path("tsc", &[".cmd", ".exe"]) else {
        return Ok(skipped("tsc was not found on PATH"));
    };
    let type_roots = list_type_roots(servers_dir).await;
    let node_types = has_node_types(&type_roots).await;

    let helpers = run_dir.join("helpers.d.ts");
    let tsconfig = run_dir.join("tsconfig.json");
    write_confined_file(&helpers, HELPERS_DECLARATIONS.as_bytes())
        .await
        .context("cannot write the helper declarations")?;
    let config = build_tsconfig(script, servers_dir, &helpers, &type_roots, node_types)?;
    write_confined_file(&tsconfig, config.as_bytes())
        .await
        .context("cannot write the type-check configuration")?;

    let mut command = Command::new(tsc);
    command
        .arg("-p")
        .arg(&tsconfig)
        .args(["--noEmit", "--pretty", "false"])
        .current_dir(run_dir);
    let (success, drained) = match run_tsc(command, TYPECHECK_TIMEOUT).await? {
        TscRun::Finished { success, drained } => (success, drained),
        TscRun::TimedOut => return Ok(skipped("tsc did not finish within 60 seconds")),
    };
    let origin = Origin::new(servers_dir, run_dir).await;
    Ok(conclude(success, &drained, &origin).await)
}

/// How a `tsc` process ended.
#[derive(Debug)]
enum TscRun {
    Finished { success: bool, drained: Drained },
    TimedOut,
}

/// Runs `command` as `tsc` within `timeout`. On timeout the whole process tree is killed and
/// the output readers are aborted; dropping this future does the same.
async fn run_tsc(mut command: Command, timeout: Duration) -> Result<TscRun> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tree = ProcessTree::spawn(&mut command).context("cannot run tsc")?;
    let child = tree.child_mut();
    let stdout = child.stdout.take().context("tsc stdout was not captured")?;
    let stderr = child.stderr.take().context("tsc stderr was not captured")?;
    let out = PipeCapture::start(stdout);
    let err = PipeCapture::start(stderr);

    let Ok(status) = tokio::time::timeout(timeout, tree.child_mut().wait()).await else {
        tree.kill();
        return Ok(TscRun::TimedOut);
    };
    let status = status.context("cannot wait for tsc")?;
    let drained = drain(out, err, DRAIN_GRACE).await;
    Ok(TscRun::Finished {
        success: status.success(),
        drained,
    })
}

/// Turns a finished `tsc` run into an outcome. Exit 0 is a pass even when a stream was cut; a
/// failure whose output was cut says so after the diagnostics.
async fn conclude(success: bool, drained: &Drained, origin: &Origin) -> TypecheckOutcome {
    if success {
        return TypecheckOutcome::Passed;
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&drained.stdout.bytes),
        String::from_utf8_lossy(&drained.stderr.bytes)
    );
    match classify_diagnostics(&text, origin).await {
        TypecheckOutcome::Failed { diagnostics }
            if drained.stdout.is_cut() || drained.stderr.is_cut() =>
        {
            TypecheckOutcome::Failed {
                diagnostics: format!(
                    "{diagnostics}tsc output exceeded {MAX_SPILL_FILE_BYTES} bytes; remaining diagnostics were not read\n"
                ),
            }
        }
        other => other,
    }
}

/// Whether any of `roots` holds the `node` type definitions.
async fn has_node_types(roots: &[PathBuf]) -> bool {
    for root in roots {
        if tokio::fs::metadata(root.join("node"))
            .await
            .is_ok_and(|meta| meta.is_dir())
        {
            return true;
        }
    }
    false
}

/// The `@types` directories of the generated servers, listing at most
/// [`MAX_TYPECHECK_SERVER_DIRS`] entries.
async fn list_type_roots(servers_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(servers_dir).await else {
        return roots;
    };
    let mut listed = 0;
    while listed < MAX_TYPECHECK_SERVER_DIRS {
        let Ok(Some(entry)) = entries.next_entry().await else {
            break;
        };
        listed += 1;
        let types = entry.path().join("node_modules").join("@types");
        if tokio::fs::metadata(&types)
            .await
            .is_ok_and(|meta| meta.is_dir())
        {
            roots.push(types);
        }
    }
    roots
}

fn build_tsconfig(
    script: &Path,
    servers_dir: &Path,
    helpers: &Path,
    type_roots: &[PathBuf],
    node_types: bool,
) -> Result<String> {
    let mut base: Value =
        serde_json::from_str(TSCONFIG_JSON).context("the generated tsconfig is not valid JSON")?;
    let options = base
        .get_mut("compilerOptions")
        .and_then(Value::as_object_mut)
        .context("the generated tsconfig has no compilerOptions")?;
    let servers = servers_dir.display().to_string();
    // Naming a missing `node` type library is a global error that stops tsc before it reports
    // anything about the script, so it is only requested when it can be found.
    if !node_types {
        options.insert("types".to_string(), json!([]));
    }
    if !type_roots.is_empty() {
        options.insert(
            "typeRoots".to_string(),
            json!(
                type_roots
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect::<Vec<_>>()
            ),
        );
    }
    options.insert(
        "paths".to_string(),
        json!({
            "mcp:helpers": [helpers.display().to_string()],
            "mcp:*": [format!("{servers}/*/index.ts")],
        }),
    );
    // Node runs the script as ESM when it uses `import`/top-level `await`, whatever the nearest
    // package.json says; `NodeNext` would judge a `.ts` file by that package.json and reject
    // top-level `await` (TS1309).
    options.insert("module".to_string(), json!("ESNext"));
    options.insert("moduleResolution".to_string(), json!("Bundler"));
    // Node also runs an import-free script that uses top-level `await` as a module.
    options.insert("moduleDetection".to_string(), json!("force"));
    let compiler_options = base["compilerOptions"].clone();
    serde_json::to_string_pretty(&json!({
        "compilerOptions": compiler_options,
        "include": [script.display().to_string(), helpers.display().to_string()],
    }))
    .context("cannot encode the type-check configuration")
}

/// One `tsc --pretty false` diagnostic line: `file(l,c): error TSnnnn: message`, or without the
/// file prefix for global diagnostics.
fn parse_diagnostic(line: &str) -> Option<(&str, &str)> {
    let rest = line
        .split_once(": error ")
        .map(|(_, rest)| rest)
        .or_else(|| line.strip_prefix("error "))?;
    let (code, message) = rest.split_once(": ")?;
    (code.starts_with("TS") && code[2..].bytes().all(|b| b.is_ascii_digit()))
        .then_some((code, message))
}

fn is_node_specifier(message: &str) -> bool {
    let Some(specifier) = message.split('\'').nth(1) else {
        return false;
    };
    specifier.starts_with("node:") || NODE_BUILTINS.contains(&specifier)
}

fn is_missing_node_types(code: &str, message: &str) -> bool {
    MISSING_NODE_TYPES_CODES.contains(&code)
        || (code == MODULE_NOT_FOUND_CODE && is_node_specifier(message))
        || (code == MISSING_TYPE_FILE_CODE && message.contains("'node'"))
}

/// Splits tsc output into diagnostic groups: a diagnostic line plus its indented continuation
/// lines. Text before the first diagnostic forms a group of its own.
fn group_diagnostics(text: &str) -> Vec<Vec<&str>> {
    let mut groups: Vec<Vec<&str>> = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match groups.last_mut() {
            Some(group) if parse_diagnostic(line).is_none() => group.push(line),
            _ => groups.push(vec![line]),
        }
    }
    groups
}

/// Where diagnostics are located: files under the generated servers belong to the SDK, not to
/// the user's script.
struct Origin {
    sdk_root: PathBuf,
    cwd: PathBuf,
}

impl Origin {
    async fn new(servers_dir: &Path, cwd: &Path) -> Self {
        Self {
            sdk_root: canonical_or_normalized(servers_dir).await,
            cwd: cwd.to_path_buf(),
        }
    }

    async fn is_generated(&self, file: &str) -> bool {
        let located = canonical_or_normalized(&self.cwd.join(file)).await;
        located.starts_with(&self.sdk_root)
    }
}

async fn canonical_or_normalized(path: &Path) -> PathBuf {
    tokio::fs::canonicalize(path)
        .await
        .unwrap_or_else(|_| normalize(path))
}

/// Resolves `.` and `..` lexically, for paths that do not exist.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

/// The file of a `file(line,col): error ...` diagnostic line.
fn diagnostic_file(line: &str) -> Option<&str> {
    let (location, _) = line.split_once(": error ")?;
    Some(location.rfind('(').map_or(location, |at| &location[..at]))
}

enum Kind {
    /// Caused by Node type definitions not being installed.
    NodeTypes,
    /// Located in a generated SDK file.
    Sdk,
    /// An implicit `any`, which missing types can cause.
    ImplicitAny,
    /// Anything else: a real problem in the user's script.
    Genuine,
}

fn kind_of(group: &[&str], generated: bool) -> Kind {
    match parse_diagnostic(group[0]) {
        Some((code, message)) if is_missing_node_types(code, message) => Kind::NodeTypes,
        _ if generated => Kind::Sdk,
        Some((code, _)) if IMPLICIT_ANY_CODES.contains(&code) => Kind::ImplicitAny,
        _ => Kind::Genuine,
    }
}

/// The files named by the diagnostic groups, each once, in order of first appearance.
fn distinct_files<'a>(groups: &[Vec<&'a str>]) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    groups
        .iter()
        .filter_map(|group| diagnostic_file(group[0]))
        .filter(|file| seen.insert(*file))
        .collect()
}

async fn classify_diagnostics(text: &str, origin: &Origin) -> TypecheckOutcome {
    let groups = group_diagnostics(text);
    let mut generated_files: HashMap<&str, bool> = HashMap::new();
    for file in distinct_files(&groups) {
        generated_files.insert(file, origin.is_generated(file).await);
    }
    let kinds: Vec<Kind> = groups
        .iter()
        .map(|group| {
            let generated = diagnostic_file(group[0])
                .and_then(|file| generated_files.get(file).copied())
                .unwrap_or(false);
            kind_of(group, generated)
        })
        .collect();
    let node_types_missing = kinds.iter().any(|kind| matches!(kind, Kind::NodeTypes));
    let sdk_noise = kinds.iter().any(|kind| matches!(kind, Kind::Sdk));
    let environment_unresolved = node_types_missing || sdk_noise;
    let genuine: Vec<&str> = groups
        .iter()
        .zip(&kinds)
        .filter(|(_, kind)| match kind {
            Kind::Genuine => true,
            Kind::ImplicitAny => !environment_unresolved,
            Kind::NodeTypes | Kind::Sdk => false,
        })
        .flat_map(|(group, _)| group.iter().copied())
        .collect();
    if !genuine.is_empty() || groups.is_empty() {
        return TypecheckOutcome::Failed {
            diagnostics: bound_diagnostics(&genuine),
        };
    }
    if node_types_missing {
        skipped("environment unresolved (Node type definitions not installed)")
    } else {
        skipped(
            "environment unresolved (the generated SDK or the compiler reported problems of its own; run npm install in the server directory)",
        )
    }
}

/// Keeps at most [`MAX_DIAGNOSTIC_LINES`] lines and [`MAX_DIAGNOSTIC_BYTES`] bytes, each line
/// escaped for terminal output.
fn bound_diagnostics(lines: &[&str]) -> String {
    let mut shown = String::new();
    let mut count = 0;
    for line in lines.iter().take(MAX_DIAGNOSTIC_LINES) {
        let escaped = crate::formatters::escape_error_text(line);
        if shown.len() + escaped.len() + 1 > MAX_DIAGNOSTIC_BYTES {
            break;
        }
        shown.push_str(&escaped);
        shown.push('\n');
        count += 1;
    }
    if count < lines.len() {
        shown.push_str("... and ");
        shown.push_str(&(lines.len() - count).to_string());
        shown.push_str(" more lines\n");
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    fn origin() -> Origin {
        Origin {
            sdk_root: PathBuf::from("/srv/servers"),
            cwd: PathBuf::from("/run/abc"),
        }
    }

    const SDK_IMPLICIT_ANY: &str = "../../srv/servers/fixture/_runtime/mcp-bridge.ts(10,5): error TS7006: Parameter 'data' implicitly has an 'any' type.";

    #[tokio::test]
    async fn diagnostics_in_the_generated_sdk_never_block_and_the_run_is_skipped() {
        let outcome = classify_diagnostics(SDK_IMPLICIT_ANY, &origin()).await;
        assert_matches!(outcome, TypecheckOutcome::Skipped { .. });
    }

    #[tokio::test]
    async fn a_location_less_error_other_than_missing_node_types_is_a_real_failure() {
        let config_error = "error TS6053: File '/x/missing.ts' not found.";
        let outcome = classify_diagnostics(config_error, &origin()).await;
        assert_matches!(outcome, TypecheckOutcome::Failed { .. });
        let node_types = "error TS2688: Cannot find type definition file for 'node'.";
        let outcome = classify_diagnostics(node_types, &origin()).await;
        assert_matches!(outcome, TypecheckOutcome::Skipped { .. });
    }

    #[tokio::test]
    async fn a_real_error_in_the_script_still_fails_beside_sdk_diagnostics() {
        let text = format!(
            "{SDK_IMPLICIT_ANY}\n../../work/s.ts(2,7): error TS2322: Type 'string' is not assignable to type 'number'."
        );
        let TypecheckOutcome::Failed { diagnostics } = classify_diagnostics(&text, &origin()).await
        else {
            panic!("expected a failure");
        };
        assert!(diagnostics.contains("TS2322") && !diagnostics.contains("mcp-bridge"));
    }

    #[tokio::test]
    async fn implicit_any_in_the_script_is_only_noise_while_types_are_unresolved() {
        let implicit = "s.ts(4,20): error TS7006: Parameter 'd' implicitly has an 'any' type.";
        let alone = classify_diagnostics(implicit, &origin()).await;
        assert_matches!(alone, TypecheckOutcome::Failed { .. });
        let with_noise =
            format!("s.ts(1,1): error TS2580: Cannot find name 'process'.\n{implicit}");
        let outcome = classify_diagnostics(&with_noise, &origin()).await;
        assert_matches!(outcome, TypecheckOutcome::Skipped { .. });
    }

    #[test]
    fn normalize_resolves_parent_components_lexically() {
        assert_eq!(
            normalize(Path::new("/run/abc/../../srv/./servers")),
            Path::new("/srv/servers")
        );
    }

    #[test]
    fn tsconfig_uses_esm_and_bundler_resolution_so_top_level_await_is_accepted() {
        let config = build_tsconfig(
            Path::new("/w/s.ts"),
            Path::new("/srv"),
            Path::new("/run/helpers.d.ts"),
            &[],
            false,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(value["compilerOptions"]["module"], "ESNext");
        assert_eq!(value["compilerOptions"]["moduleResolution"], "Bundler");
    }

    const NODE_FS_MISSING: &str = "s.ts(1,16): error TS2307: Cannot find module 'node:fs' or its corresponding type declarations.";

    #[tokio::test]
    async fn missing_node_types_alone_are_skipped() {
        let text = format!(
            "{NODE_FS_MISSING}\ns.ts(2,1): error TS2580: Cannot find name 'process'. Do you need to \
             install type definitions for node?\n"
        );
        assert_matches!(
            classify_diagnostics(&text, &origin()).await,
            TypecheckOutcome::Skipped { .. }
        );
    }

    #[tokio::test]
    async fn an_unknown_mcp_server_is_a_type_error_even_next_to_node_noise() {
        let text = format!(
            "{NODE_FS_MISSING}\ns.ts(3,1): error TS2307: Cannot find module 'mcp:githbu' or its \
             corresponding type declarations.\n"
        );
        let TypecheckOutcome::Failed { diagnostics } = classify_diagnostics(&text, &origin()).await
        else {
            panic!("expected a failure");
        };
        assert!(diagnostics.contains("mcp:githbu"));
    }

    #[tokio::test]
    async fn a_real_type_error_fails_and_unparsable_output_fails_too() {
        let text = "s.ts(2,7): error TS2322: Type 'string' is not assignable to type 'number'.";
        assert_matches!(
            classify_diagnostics(text, &origin()).await,
            TypecheckOutcome::Failed { .. }
        );
        assert_matches!(
            classify_diagnostics("tsc: crashed", &origin()).await,
            TypecheckOutcome::Failed { .. }
        );
    }

    #[test]
    fn diagnostics_are_bounded_in_lines_and_bytes() {
        let many: Vec<String> = (0..50)
            .map(|i| format!("s.ts({i},1): error TS2322: bad {i}"))
            .collect();
        let lines: Vec<&str> = many.iter().map(String::as_str).collect();
        let shown = bound_diagnostics(&lines);
        assert_eq!(shown.lines().count(), MAX_DIAGNOSTIC_LINES + 1);
        assert!(shown.ends_with("... and 30 more lines\n"));

        let long = "x".repeat(MAX_DIAGNOSTIC_BYTES);
        let shown = bound_diagnostics(&[long.as_str(), "second"]);
        assert!(shown.len() < MAX_DIAGNOSTIC_BYTES);
    }

    #[test]
    fn tsconfig_reuses_the_generated_options_and_maps_the_mcp_imports() {
        let config = build_tsconfig(
            Path::new("/w/s.ts"),
            Path::new("/home/servers"),
            Path::new("/run/helpers.d.ts"),
            &[PathBuf::from("/home/servers/a/node_modules/@types")],
            true,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&config).unwrap();
        assert_eq!(value["compilerOptions"]["noEmit"], true);

        assert_eq!(
            value["compilerOptions"]["paths"]["mcp:*"][0],
            "/home/servers/*/index.ts"
        );
        assert_eq!(
            value["compilerOptions"]["typeRoots"][0],
            "/home/servers/a/node_modules/@types"
        );
        assert_eq!(value["include"][0], "/w/s.ts");
        assert!(value.get("extends").is_none());
    }

    #[tokio::test]
    async fn node_types_are_found_only_when_a_root_holds_the_node_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("@types");
        std::fs::create_dir_all(&root).unwrap();
        assert!(!has_node_types(std::slice::from_ref(&root)).await);
        std::fs::create_dir(root.join("node")).unwrap();
        assert!(has_node_types(&[root]).await);
    }

    #[tokio::test]
    async fn type_roots_list_only_servers_with_an_at_types_directory() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("a/node_modules/@types/node")).unwrap();
        std::fs::create_dir_all(temp.path().join("b")).unwrap();
        let roots = list_type_roots(temp.path()).await;
        assert_eq!(roots, [temp.path().join("a/node_modules/@types")]);
    }

    #[tokio::test]
    async fn node_type_noise_is_hidden_next_to_a_real_error_and_skipped_alone() {
        let noise = "error TS2688: Cannot find type definition file for 'node'.\n\
                     g.ts(1,1): error TS2580: Cannot find name 'process'.";
        assert_matches!(
            classify_diagnostics(noise, &origin()).await,
            TypecheckOutcome::Skipped { .. }
        );
        let mixed = format!(
            "{noise}\ns.ts(2,7): error TS2322: Type 'string' is not assignable to type 'number'.\n  \
             detail line"
        );
        let TypecheckOutcome::Failed { diagnostics } =
            classify_diagnostics(&mixed, &origin()).await
        else {
            panic!("expected a failure");
        };
        assert!(diagnostics.contains("TS2322") && diagnostics.contains("detail line"));
        assert!(!diagnostics.contains("TS2580") && !diagnostics.contains("TS2688"));
    }

    #[test]
    fn distinct_files_lists_each_file_once_in_order() {
        let text = "a.ts(1,1): error TS2322: x\na.ts(2,1): error TS2322: y\n\
                    b.ts(1,1): error TS2322: z\nerror TS6053: no file\na.ts(3,1): error TS2322: w";
        let groups = group_diagnostics(text);
        assert_eq!(distinct_files(&groups), ["a.ts", "b.ts"]);
    }

    fn drained(stdout: &str, total: u64) -> Drained {
        Drained {
            stdout: crate::runtime::exec::Captured {
                bytes: stdout.as_bytes().to_vec(),
                total,
            },
            stderr: crate::runtime::exec::Captured::default(),
            abandoned: false,
        }
    }

    #[tokio::test]
    async fn exit_zero_passes_even_when_the_stream_was_cut() {
        let cut = drained("s.ts(1,1): error TS2322: bad", u64::MAX);
        assert_eq!(
            conclude(true, &cut, &origin()).await,
            TypecheckOutcome::Passed
        );
    }

    #[tokio::test]
    async fn a_failure_with_a_cut_stream_says_the_diagnostics_were_cut() {
        let line = "s.ts(1,1): error TS2322: bad";
        let cut = drained(line, line.len() as u64 + 1);
        let TypecheckOutcome::Failed { diagnostics } = conclude(false, &cut, &origin()).await
        else {
            panic!("expected a failure");
        };
        assert!(diagnostics.contains("TS2322"));
        assert!(
            diagnostics.ends_with(
                "tsc output exceeded 8388608 bytes; remaining diagnostics were not read\n"
            )
        );
        let whole = drained(line, line.len() as u64);
        let TypecheckOutcome::Failed { diagnostics } = conclude(false, &whole, &origin()).await
        else {
            panic!("expected a failure");
        };
        assert!(!diagnostics.contains("exceeded"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_timed_out_tsc_takes_its_descendants_with_it() {
        use crate::runtime::exec::tree_test_support::{
            assert_gone_soon, recorded_pid, sleeping_grandchild,
        };

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let run = run_tsc(sleeping_grandchild(&pid_file), Duration::from_secs(2))
            .await
            .unwrap();
        assert_matches!(run, TscRun::TimedOut);
        assert_gone_soon(recorded_pid(&pid_file).await).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_finished_tsc_reports_its_exit_and_output() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo diagnostics; exit 2"]);
        let run = run_tsc(command, Duration::from_secs(30)).await.unwrap();
        let TscRun::Finished { success, drained } = run else {
            panic!("tsc should have finished: {run:?}");
        };
        assert!(!success);
        assert_eq!(drained.stdout.bytes, b"diagnostics\n");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_timed_out_tsc_cmd_shim_takes_its_node_descendant_with_it() {
        use crate::runtime::exec::tree_test_support::{
            assert_gone_soon, node_grandchild_shim, recorded_pid,
        };

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let run = run_tsc(
            node_grandchild_shim(dir.path(), &pid_file),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_matches!(run, TscRun::TimedOut);
        assert_gone_soon(recorded_pid(&pid_file).await).await;
    }
}
