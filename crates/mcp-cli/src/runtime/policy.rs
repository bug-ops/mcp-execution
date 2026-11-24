//! Dry-run policy: which tools a `--dry-run` may actually execute.
//!
//! Only exact `reads` entries in the policy file execute; tool annotations never classify a
//! call. The default policy is empty, so every call is a write. An invalid policy file, or a
//! missing explicit `--policy` path, is an input error that never echoes file contents.
//!
//! The file is JSON: `{"version": 1, "reads": ["<server>/<tool>", ...]}`. `version` is required
//! and must be 1, unknown fields are rejected, and each entry is split at the first `/` and
//! validated as a [`ServerId`] and a [`ToolName`] (a tool name cannot contain `/`, so such an
//! entry is invalid).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use mcp_execution_core::annotations::Hint;
use mcp_execution_core::metadata::ServerMetadata;
use mcp_execution_core::{Error as CoreError, ServerId, ToolName};
use serde::Deserialize;

use super::dirs::RuntimeDirs;
use super::journal::ArgsDigest;
use crate::commands::drift::read_recorded;

/// Largest policy file, in bytes.
pub const MAX_POLICY_FILE_BYTES: u64 = 64 * 1024;

/// Most skipped writes listed in the dry-run report before "and N more".
pub const MAX_REPORTED_WRITES: usize = 50;

/// How the policy classifies a call.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::policy::CallClass;
///
/// assert_ne!(CallClass::Read, CallClass::Write);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallClass {
    /// The call may run in a dry run.
    Read,
    /// The call is skipped in a dry run. The default for every call.
    Write,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    version: u32,
    #[serde(default)]
    reads: Vec<String>,
}

/// The one policy file version this build understands.
const POLICY_VERSION: u32 = 1;

/// The loaded dry-run policy.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::policy::{CallClass, Policy};
/// use mcp_execution_core::{ServerId, ToolName};
///
/// let class = Policy::default()
///     .classify(&ServerId::new("demo").unwrap(), &ToolName::new("echo").unwrap());
/// assert_eq!(class, CallClass::Write);
/// ```
#[derive(Debug, Clone, Default)]
pub struct Policy {
    reads: HashSet<(ServerId, ToolName)>,
}

impl Policy {
    /// Loads the policy from `explicit`, or from the default location when `None`.
    ///
    /// An absent default file is the empty policy; a missing explicit path is an error.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error, without echoing file content, when the file is missing
    /// (explicit path), too large, unreadable or invalid.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::policy::Policy;
    ///
    /// # async fn example() {
    /// assert!(Policy::load(None).await.is_ok());
    /// # }
    /// ```
    pub async fn load(explicit: Option<&Path>) -> Result<Self> {
        let (path, required) = match explicit {
            Some(path) => (path.to_path_buf(), true),
            None => (RuntimeDirs::policy_path()?, false),
        };
        let invalid = |reason: &str| {
            anyhow::Error::new(CoreError::InvalidArgument(format!(
                "policy file {}: {reason}",
                path.display()
            )))
        };
        let meta = match tokio::fs::metadata(&path).await {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => {
                return Ok(Self::default());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(invalid("not found"));
            }
            Err(_) => return Err(invalid("cannot be read")),
        };
        if !meta.is_file() || meta.len() > MAX_POLICY_FILE_BYTES {
            return Err(invalid("is not a regular file within the size limit"));
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| invalid("cannot be read"))?;
        Self::parse(&bytes).map_err(invalid)
    }

    fn parse(bytes: &[u8]) -> std::result::Result<Self, &'static str> {
        let file: PolicyFile = serde_json::from_slice(bytes).map_err(
            |_| "is not valid JSON of the form {\"version\": 1, \"reads\": [\"server/tool\", ...]}",
        )?;
        if file.version != POLICY_VERSION {
            return Err("has an unsupported version (expected 1)");
        }
        let reads = file
            .reads
            .iter()
            .map(|entry| parse_entry(entry))
            .collect::<std::result::Result<HashSet<_>, _>>()?;
        Ok(Self { reads })
    }

    /// Classifies a call; every call not listed under `reads` is a [`CallClass::Write`].
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::policy::{CallClass, Policy};
    /// use mcp_execution_core::{ServerId, ToolName};
    ///
    /// let class = Policy::default()
    ///     .classify(&ServerId::new("demo").unwrap(), &ToolName::new("echo").unwrap());
    /// assert_eq!(class, CallClass::Write);
    /// ```
    #[must_use]
    pub fn classify(&self, server: &ServerId, tool: &ToolName) -> CallClass {
        if self.reads.contains(&(server.clone(), tool.clone())) {
            CallClass::Read
        } else {
            CallClass::Write
        }
    }
}

/// Splits `entry` at the first `/` and validates both sides.
fn parse_entry(entry: &str) -> std::result::Result<(ServerId, ToolName), &'static str> {
    const INVALID: &str = "has an entry that is not a valid \"server/tool\"";
    let (server, tool) = entry.split_once('/').ok_or(INVALID)?;
    let server = ServerId::new(server).map_err(|_| INVALID)?;
    let tool = ToolName::new(tool).map_err(|_| INVALID)?;
    Ok((server, tool))
}

#[derive(Debug)]
struct SkippedWrite {
    server: ServerId,
    tool: ToolName,
    digest: Option<ArgsDigest>,
    args_bytes: usize,
}

/// A dry run in progress: the policy and the writes it skipped.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::policy::{DryRun, Policy};
///
/// let dry_run = DryRun::new(Policy::default(), "/home/user/.claude/servers".into());
/// assert!(dry_run.report().is_none());
/// ```
#[derive(Debug)]
pub struct DryRun {
    policy: Policy,
    servers_dir: PathBuf,
    skipped: Mutex<Skipped>,
}

/// The skipped writes: the first [`MAX_REPORTED_WRITES`] in full, the rest only counted.
#[derive(Debug, Default)]
struct Skipped {
    listed: Vec<SkippedWrite>,
    total: usize,
}

impl DryRun {
    /// Starts a dry run under `policy`; `servers_dir` is where the recorded `_meta.json` of each
    /// server is read for read-only suggestions in the report.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::policy::{DryRun, Policy};
    ///
    /// let dry_run = DryRun::new(Policy::default(), "/home/user/.claude/servers".into());
    /// # drop(dry_run);
    /// ```
    #[must_use]
    pub fn new(policy: Policy, servers_dir: PathBuf) -> Self {
        Self {
            policy,
            servers_dir,
            skipped: Mutex::new(Skipped::default()),
        }
    }

    /// Decides one call: `true` means it must be skipped (and was recorded as skipped).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::policy::DryRun;
    /// # use mcp_execution_core::{ServerId, ToolName};
    /// # fn example(dry_run: &DryRun) -> anyhow::Result<()> {
    /// let skipped = dry_run.skips(&ServerId::new("demo")?, &ToolName::new("create")?, None, 2);
    /// assert!(skipped, "an empty policy treats every call as a write");
    /// # Ok(())
    /// # }
    /// ```
    pub fn skips(
        &self,
        server: &ServerId,
        tool: &ToolName,
        digest: Option<ArgsDigest>,
        args_bytes: usize,
    ) -> bool {
        if self.policy.classify(server, tool) == CallClass::Read {
            return false;
        }
        let mut skipped = self.skipped.lock().unwrap_or_else(PoisonError::into_inner);
        skipped.total += 1;
        if skipped.listed.len() < MAX_REPORTED_WRITES {
            skipped.listed.push(SkippedWrite {
                server: server.clone(),
                tool: tool.clone(),
                digest,
                args_bytes,
            });
        }
        drop(skipped);
        true
    }

    /// The bounded report of skipped writes, or `None` when nothing was skipped.
    ///
    /// Reads the recorded `_meta.json` of each reported server with blocking I/O; async callers
    /// run it on a blocking thread.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::policy::DryRun;
    /// # fn example(dry_run: &DryRun) {
    /// if let Some(report) = dry_run.report() {
    ///     println!("{report}");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn report(&self) -> Option<String> {
        let skipped = self.skipped.lock().unwrap_or_else(PoisonError::into_inner);
        if skipped.total == 0 {
            return None;
        }
        let mut lines = vec![format!(
            "dry run: {} write call(s) skipped; the result is incomplete where later steps depend on them",
            skipped.total
        )];
        let mut recorded: HashMap<ServerId, Option<ServerMetadata>> = HashMap::new();
        for (index, write) in skipped.listed.iter().enumerate() {
            let digest = write
                .digest
                .map_or_else(String::new, |d| format!(" args={}", d.prefix()));
            let meta = recorded.entry(write.server.clone()).or_insert_with(|| {
                read_recorded(&self.servers_dir.join(write.server.as_str())).ok()
            });
            let suggestion = if marked_read_only(meta.as_ref(), &write.tool) {
                format!(
                    "; the server marks it read-only (a hint, not trusted): list \"{}/{}\" under reads to run it",
                    write.server, write.tool
                )
            } else {
                String::new()
            };
            lines.push(format!(
                "  #{} {}/{}{digest} ({} bytes of arguments){suggestion}",
                index + 1,
                write.server,
                write.tool,
                write.args_bytes
            ));
        }
        if skipped.total > skipped.listed.len() {
            lines.push(format!(
                "  and {} more",
                skipped.total - skipped.listed.len()
            ));
        }
        drop(skipped);
        Some(lines.join("\n"))
    }
}

/// Whether the metadata recorded at generation marks `tool` as read-only.
fn marked_read_only(meta: Option<&ServerMetadata>, tool: &ToolName) -> bool {
    meta.is_some_and(|meta| {
        meta.tools.iter().any(|recorded| {
            recorded.name == *tool
                && recorded
                    .annotations
                    .as_ref()
                    .is_some_and(|a| a.read_only == Hint::StatedTrue)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(server: &str, tool: &str) -> (ServerId, ToolName) {
        (ServerId::new(server).unwrap(), ToolName::new(tool).unwrap())
    }

    #[test]
    fn only_exact_reads_entries_classify_as_reads() {
        let policy = Policy::parse(br#"{"version":1,"reads":["demo/list","other/get"]}"#).unwrap();
        let (server, list) = ids("demo", "list");
        let (_, listing) = ids("demo", "listing");

        assert_eq!(policy.classify(&server, &list), CallClass::Read);
        assert_eq!(policy.classify(&server, &listing), CallClass::Write);
        let (other, get) = ids("elsewhere", "get");
        assert_eq!(policy.classify(&other, &get), CallClass::Write);
    }

    #[test]
    fn invalid_content_is_rejected_without_echoing_it() {
        let cases: [&[u8]; 11] = [
            br#"{"version":1,"reads": ["SECRET-VALUE"#,
            br#"{"version":1,"reads": "SECRET-VALUE"}"#,
            br#"{"version":1,"reads": [], "SECRET-VALUE": 1}"#,
            br#"["SECRET-VALUE"]"#,
            br#"{"reads": ["demo/SECRET-VALUE"]}"#,
            br#"{"version":2,"reads": ["demo/SECRET-VALUE"]}"#,
            br#"{"version":"1","reads": []}"#,
            br#"{"version":1,"reads": ["SECRET-VALUE"]}"#,
            br#"{"version":1,"reads": ["Bad Server!/SECRET-VALUE"]}"#,
            br#"{"version":1,"reads": ["demo/SECRET-VALUE bad!"]}"#,
            br#"{"version":1,"reads": ["demo/SECRET/VALUE"]}"#,
        ];
        for bytes in cases {
            let reason = Policy::parse(bytes).unwrap_err();
            assert!(!reason.contains("SECRET"), "{reason}");
        }
    }

    #[test]
    fn version_one_is_required_and_the_empty_policy_is_valid() {
        assert!(Policy::parse(br#"{"version":1}"#).is_ok());
        assert!(Policy::parse(br#"{"version":1,"reads":[]}"#).is_ok());
        assert!(Policy::parse(br#"{"reads":[]}"#).is_err());
        assert!(Policy::parse(br#"{"version":2,"reads":[]}"#).is_err());
        assert!(Policy::parse(br#"{"version":1,"unknown":true}"#).is_err());
    }

    #[test]
    fn an_entry_is_split_at_the_first_slash_and_a_slash_in_the_tool_is_invalid() {
        assert!(parse_entry("demo/list").is_ok());
        assert!(
            parse_entry("demo/a/b").is_err(),
            "a tool name cannot contain a slash"
        );
        assert!(parse_entry("demo").is_err());
        assert!(parse_entry("/list").is_err());
    }

    #[tokio::test]
    async fn load_reports_a_missing_explicit_path_and_an_oversized_file() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("none.json");
        let error = Policy::load(Some(&missing)).await.unwrap_err();
        assert!(matches!(
            error.downcast_ref::<CoreError>(),
            Some(CoreError::InvalidArgument(_))
        ));

        let big = temp.path().join("big.json");
        std::fs::write(
            &big,
            vec![b' '; usize::try_from(MAX_POLICY_FILE_BYTES).unwrap() + 1],
        )
        .unwrap();
        assert!(Policy::load(Some(&big)).await.is_err());
    }

    #[test]
    fn a_dry_run_skips_writes_and_reports_them_in_order() {
        let policy = Policy::parse(br#"{"version":1,"reads":["demo/list"]}"#).unwrap();
        let dry_run = DryRun::new(policy, PathBuf::from("/nonexistent"));
        let (server, list) = ids("demo", "list");
        let (_, create) = ids("demo", "create");

        assert!(!dry_run.skips(&server, &list, None, 2));
        assert!(dry_run.report().is_none());
        assert!(dry_run.skips(&server, &create, None, 2));

        let report = dry_run.report().unwrap();
        assert!(report.contains("incomplete") && report.contains("#1 demo/create"));
    }

    #[test]
    fn the_report_is_bounded() {
        let dry_run = DryRun::new(Policy::default(), PathBuf::from("/nonexistent"));
        let (server, tool) = ids("demo", "create");
        for _ in 0..MAX_REPORTED_WRITES + 5 {
            dry_run.skips(&server, &tool, None, 0);
        }
        let report = dry_run.report().unwrap();
        assert!(report.ends_with("and 5 more"), "{report}");
    }
}
