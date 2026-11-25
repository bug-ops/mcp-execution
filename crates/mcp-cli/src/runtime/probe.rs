//! Detection of a runtime other than Node.js that runs generated `.ts` tool files.
//!
//! `setup` asks here only when no suitable Node.js exists. A runtime counts when it executes a
//! typed one-line script, so the answer is about capability and not about a version number. The
//! script runs from a private directory under the runner state root, because Bun and Deno read
//! configuration (and Bun runs preload files) from the working directory.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use mcp_execution_core::untrusted::sanitize_untrusted_text;
use serde::Serialize;
use tokio::io::AsyncReadExt as _;
use tokio::process::Command;

use super::dirs::{RunId, RuntimeDirs, create_private_file, ensure_private_dir};
use super::exec::ProcessTree;
use super::path_lookup::find_on_path;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_MARKER: &str = "mcp-execution-probe ";
const MAX_PROBE_OUTPUT_BYTES: u64 = 4096;
const MAX_VERSION_CHARS: usize = 64;

/// A runtime that can execute generated tool files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeName {
    /// Node.js, the only runtime `run`, `--typecheck` and `--restrict` use.
    Node,
    /// Bun.
    Bun,
    /// Deno.
    Deno,
}

impl RuntimeName {
    /// Human-readable product name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Node => "Node.js",
            Self::Bun => "Bun",
            Self::Deno => "Deno",
        }
    }

    /// The command that runs a generated tool file with this runtime.
    pub const fn invocation(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Bun => "bun",
            Self::Deno => "deno run -A",
        }
    }
}

/// The runtime `setup` found and the version it reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedRuntime {
    /// Which runtime it is.
    pub name: RuntimeName,
    /// Its version, without a leading `v`.
    pub version: String,
}

/// The runtimes probed when Node.js is unusable, in order of preference.
#[derive(Debug, Clone, Copy)]
enum Alternative {
    Bun,
    Deno,
}

impl Alternative {
    const ALL: [Self; 2] = [Self::Bun, Self::Deno];

    const fn name(self) -> RuntimeName {
        match self {
            Self::Bun => RuntimeName::Bun,
            Self::Deno => RuntimeName::Deno,
        }
    }

    const fn program(self) -> &'static str {
        match self {
            Self::Bun => "bun",
            Self::Deno => "deno",
        }
    }

    /// The type annotation makes a runtime that cannot run TypeScript fail the probe.
    const fn script(self) -> &'static str {
        match self {
            Self::Bun => {
                "const v: string = Bun.version; console.log(\"mcp-execution-probe \" + v);\n"
            }
            Self::Deno => {
                "const v: string = Deno.version.deno; console.log(\"mcp-execution-probe \" + v);\n"
            }
        }
    }

    const fn leading_args(self) -> &'static [&'static str] {
        match self {
            Self::Bun => &[],
            Self::Deno => &["run", "--no-config", "--quiet"],
        }
    }
}

/// Removes the probe script when dropped, so no exit path leaves it behind.
#[derive(Debug)]
struct ProbeFile(PathBuf);

impl ProbeFile {
    /// Synchronous on purpose: a cancelled blocking task could create the file after the guard
    /// was dropped.
    fn create(dir: &Path, script: &str) -> Result<Self> {
        let probe = Self(dir.join(format!("probe-{}.ts", RunId::new().as_str())));
        create_private_file(&probe.0)?
            .write_all(script.as_bytes())
            .with_context(|| format!("cannot write {}", probe.0.display()))?;
        Ok(probe)
    }
}

impl Drop for ProbeFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::debug!(%error, "cannot remove the runtime probe script");
        }
    }
}

/// Probes Bun, then Deno, and returns the first that runs a TypeScript file.
///
/// A runtime that is absent from the absolute `PATH` entries is skipped silently; one that is
/// present but fails, prints something else or exceeds 10 seconds is logged and skipped.
pub async fn detect_alternative_runtime() -> Option<DetectedRuntime> {
    for candidate in Alternative::ALL {
        let Some(program) = find_on_path(candidate.program(), &[".exe", ".cmd"]) else {
            continue;
        };
        match probe(candidate, &program).await {
            Ok(version) => {
                return Some(DetectedRuntime {
                    name: candidate.name(),
                    version,
                });
            }
            Err(error) => tracing::warn!(
                runtime = candidate.program(),
                error = %format_args!("{error:#}"),
                "runtime probe failed"
            ),
        }
    }
    None
}

async fn probe(candidate: Alternative, program: &Path) -> Result<String> {
    let dir = RuntimeDirs::resolve()?.root().join("probe");
    ensure_private_dir(&dir).await?;
    let script = ProbeFile::create(&dir, candidate.script())?;

    let mut command = Command::new(program);
    command
        .args(candidate.leading_args())
        .arg(&script.0)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    tokio::time::timeout(PROBE_TIMEOUT, read_version(command))
        .await
        .with_context(|| format!("no answer within {} s", PROBE_TIMEOUT.as_secs()))?
}

/// Runs `command` as a process tree, so a shell shim cannot leave the real runtime running when
/// the caller drops this future.
async fn read_version(mut command: Command) -> Result<String> {
    let mut tree = ProcessTree::spawn(&mut command).context("cannot start")?;
    let stdout = tree
        .child_mut()
        .stdout
        .take()
        .context("stdout is not piped")?;
    let mut output = Vec::new();
    // Concurrently: `wait` stops what the leader left behind, which may hold the pipe open, and
    // dropping the pipe at the cap stops a writer from blocking the exit.
    let (read, status) = tokio::join!(
        async {
            stdout
                .take(MAX_PROBE_OUTPUT_BYTES)
                .read_to_end(&mut output)
                .await
        },
        tree.wait()
    );
    read.context("cannot read its output")?;
    let status = status.context("cannot wait for it")?;
    anyhow::ensure!(status.success(), "exited with {status}");
    parse_version(&String::from_utf8_lossy(&output)).context("did not print the probe line")
}

fn parse_version(output: &str) -> Option<String> {
    let raw = output
        .lines()
        .find_map(|line| line.strip_prefix(PROBE_MARKER))?;
    let version = sanitize_untrusted_text(raw.trim(), MAX_VERSION_CHARS);
    (!version.is_empty()).then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_takes_the_marked_line_sanitized_and_bounded() {
        assert_eq!(
            parse_version("noise\nmcp-execution-probe 1.3.0\n").as_deref(),
            Some("1.3.0")
        );
        assert_eq!(
            parse_version("mcp-execution-probe 1\u{1b}[31m").as_deref(),
            Some("1 [31m")
        );
        let long = format!("mcp-execution-probe {}", "9".repeat(300));
        assert_eq!(
            parse_version(&long).map(|v| v.len()),
            Some(MAX_VERSION_CHARS)
        );
        assert_eq!(parse_version("mcp-execution-probe   "), None);
        assert_eq!(parse_version("1.3.0"), None);
    }

    #[test]
    fn every_probe_script_prints_the_parsed_marker() {
        for candidate in Alternative::ALL {
            assert!(candidate.script().contains(PROBE_MARKER));
        }
    }

    #[test]
    fn probe_file_is_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProbeFile::create(dir.path(), "x").unwrap();
        let path = file.0.clone();
        assert!(path.is_file());
        drop(file);
        assert!(!path.exists());
    }
}
