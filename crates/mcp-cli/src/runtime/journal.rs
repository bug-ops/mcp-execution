//! The per-run call journal: a bounded, value-free record of what a script called.
//!
//! Entries hold a start and an end per call, never argument values or server text; arguments
//! appear only as a keyed digest. The start is recorded before the call is sent, so a call in
//! flight when the run ends reads as `unknown`. A summary is produced for the runner to print
//! on failure only.

use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use mcp_execution_core::{ServerId, ToolName};
use serde::{Serialize, Serializer};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::dirs::{
    PrunePolicy, RunId, RuntimeDirs, create_private_file, ensure_private_dir, prune_dir,
};

/// Most entries written to one journal.
pub const MAX_JOURNAL_ENTRIES: usize = 2000;

/// Largest journal file, in bytes.
pub const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;

/// Most journal files kept after pruning.
pub const MAX_JOURNAL_FILES: usize = 50;

/// How long a journal file is kept.
pub const JOURNAL_RETENTION: Duration = Duration::from_hours(7 * 24);

/// Most entries listed in a failure summary before "and N more".
pub const MAX_SUMMARY_ENTRIES: usize = 20;

/// Journals younger than this are never removed by the count limit, so a concurrent live run
/// keeps its file.
const LIVE_JOURNAL_GRACE: Duration = Duration::from_secs(600);

/// Bytes kept free for the final truncation marker.
const MARKER_RESERVE: u64 = 256;

/// Domain separation of the argument digest.
const DIGEST_DOMAIN: &[u8] = b"mcp-execution/journal/args/v1\0";

/// Length of the per-user digest key.
const KEY_BYTES: usize = 32;

/// How a journaled call ended; a start entry without an end entry reads as `Unknown`.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::journal::CallStatus;
///
/// assert_ne!(CallStatus::Ok, CallStatus::Unknown);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStatus {
    /// The call returned a value.
    Ok,
    /// The call succeeded with no result.
    Empty,
    /// The tool reported an error.
    ToolError,
    /// The server answered with a JSON-RPC error.
    RpcError,
    /// The call failed before or while talking to the server.
    Failed,
    /// The call exceeded its timeout.
    TimedOut,
    /// Dry run: the call was not executed.
    Skipped,
    /// The call started and the run ended before it did.
    Unknown,
}

impl CallStatus {
    /// The lowercase word used in the journal file and in summaries.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::journal::CallStatus;
    ///
    /// assert_eq!(CallStatus::TimedOut.as_str(), "timed_out");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Empty => "empty",
            Self::ToolError => "tool_error",
            Self::RpcError => "rpc_error",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Skipped => "skipped",
            Self::Unknown => "unknown",
        }
    }
}

/// SHA-256 over a domain tag, the per-user key and the canonical arguments.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::journal::ArgsDigest;
///
/// let digest = ArgsDigest::from_bytes([0xab; 32]);
/// assert_eq!(digest.prefix(), "abababababab");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArgsDigest([u8; 32]);

impl ArgsDigest {
    /// Wraps raw digest bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::journal::ArgsDigest;
    ///
    /// let _digest = ArgsDigest::from_bytes([0; 32]);
    /// ```
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The 12-hex-character prefix shown in summaries.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::journal::ArgsDigest;
    ///
    /// assert_eq!(ArgsDigest::from_bytes([1; 32]).prefix(), "010101010101");
    /// ```
    #[must_use]
    pub fn prefix(&self) -> String {
        self.hex()[..12].to_string()
    }

    fn hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        self.0
            .iter()
            .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]])
            .map(char::from)
            .collect()
    }
}

/// Serializes `arguments` canonically: object keys sorted recursively, so the digest does not
/// depend on key order or on `serde_json`'s `preserve_order` feature.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::journal::canonical_args;
/// use serde_json::json;
///
/// assert_eq!(canonical_args(&json!({"b": 1, "a": 2})), canonical_args(&json!({"a": 2, "b": 1})));
/// ```
#[must_use]
pub fn canonical_args(arguments: &Value) -> Vec<u8> {
    serde_json::to_vec(&Sorted(arguments)).unwrap_or_default()
}

pub(crate) fn canonical_map(arguments: &Map<String, Value>) -> Vec<u8> {
    serde_json::to_vec(&SortedMap(arguments)).unwrap_or_default()
}

struct Sorted<'a>(&'a Value);

struct SortedMap<'a>(&'a Map<String, Value>);

impl Serialize for Sorted<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(map) => SortedMap(map).serialize(serializer),
            Value::Array(items) => serializer.collect_seq(items.iter().map(Sorted)),
            other => other.serialize(serializer),
        }
    }
}

impl Serialize for SortedMap<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<(&String, &Value)> = self.0.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        serializer.collect_map(entries.into_iter().map(|(key, value)| (key, Sorted(value))))
    }
}

/// Handle of a started call, returned by [`Journal::start`] and given back to
/// [`Journal::end`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::journal::CallToken;
///
/// assert_eq!(CallToken::default(), CallToken::default());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallToken(u64);

#[derive(Debug)]
struct CallRecord {
    server: ServerId,
    tool: ToolName,
    digest: ArgsDigest,
    status: CallStatus,
    duration: Option<Duration>,
}

#[derive(Debug)]
struct State {
    sender: Option<mpsc::UnboundedSender<String>>,
    records: Vec<CallRecord>,
    calls: u64,
    lines: usize,
    bytes: u64,
    dropped: u64,
}

impl State {
    fn emit(&mut self, line: &Value) {
        let mut text = line.to_string();
        text.push('\n');
        let len = text.len() as u64;
        let fits = self.lines < MAX_JOURNAL_ENTRIES
            && self.bytes + len + MARKER_RESERVE <= MAX_JOURNAL_BYTES;
        let sent = fits
            && self
                .sender
                .as_ref()
                .is_some_and(|sender| sender.send(text).is_ok());
        if sent {
            self.lines += 1;
            self.bytes += len;
        } else {
            self.dropped += 1;
        }
    }
}

/// The journal of one run.
///
/// Created by [`Journal::create`], fed by the helper for every call and closed with
/// [`Journal::finish`], which returns the failure summary text.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::dirs::{RunId, RuntimeDirs};
/// use mcp_execution_cli::runtime::journal::Journal;
///
/// # async fn example() -> anyhow::Result<()> {
/// let journal = Journal::create(&RuntimeDirs::resolve()?, &RunId::new()).await?;
/// let summary = journal.finish().await;
/// assert!(summary.is_none());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Journal {
    key: [u8; KEY_BYTES],
    state: Mutex<State>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

impl Journal {
    /// Prunes old journals, loads or creates the per-user digest key and opens this run's
    /// file (mode `0600` on Unix) under the private state root.
    ///
    /// # Errors
    ///
    /// Returns an error when the state directory, the key or the file cannot be prepared.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::{RunId, RuntimeDirs};
    /// use mcp_execution_cli::runtime::journal::Journal;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let journal = Journal::create(&RuntimeDirs::resolve()?, &RunId::new()).await?;
    /// # drop(journal);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(dirs: &RuntimeDirs, run: &RunId) -> Result<Self> {
        let dir = dirs.journals_dir();
        ensure_private_dir(&dir).await?;
        prune_dir(
            &dir,
            PrunePolicy {
                max_age: JOURNAL_RETENTION,
                max_entries: MAX_JOURNAL_FILES - 1,
                max_total_bytes: None,
                count_prune_min_age: LIVE_JOURNAL_GRACE,
            },
            SystemTime::now(),
        )
        .await?;

        let key_path = dirs.journal_key_path();
        let path = dir.join(format!("journal-{}.jsonl", run.as_str()));
        let (key, file) = tokio::task::spawn_blocking(move || {
            Ok::<_, anyhow::Error>((load_or_create_key(&key_path)?, create_private_file(&path)?))
        })
        .await
        .context("journal setup task failed")??;

        let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
        let mut file = tokio::fs::File::from_std(file);
        let writer = tokio::spawn(async move {
            while let Some(line) = receiver.recv().await {
                if let Err(error) = file.write_all(line.as_bytes()).await {
                    tracing::warn!(%error, "journal write failed; later entries are lost");
                    return;
                }
                if let Err(error) = file.flush().await {
                    tracing::warn!(%error, "journal flush failed; later entries are lost");
                    return;
                }
            }
        });
        Ok(Self {
            key,
            state: Mutex::new(State {
                sender: Some(sender),
                records: Vec::new(),
                calls: 0,
                lines: 0,
                bytes: 0,
                dropped: 0,
            }),
            writer: Mutex::new(Some(writer)),
        })
    }

    /// Keyed digest and canonical size of `arguments`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::journal::Journal;
    /// # fn example(journal: &Journal) {
    /// let (digest, bytes) = journal.digest(&serde_json::Map::new());
    /// println!("{} ({bytes} bytes)", digest.prefix());
    /// # }
    /// ```
    #[must_use]
    pub fn digest(&self, arguments: &Map<String, Value>) -> (ArgsDigest, usize) {
        let canonical = canonical_map(arguments);
        let mut hasher = Sha256::new();
        hasher.update(DIGEST_DOMAIN);
        hasher.update(self.key);
        hasher.update(&canonical);
        (
            ArgsDigest::from_bytes(hasher.finalize().into()),
            canonical.len(),
        )
    }

    /// Records that a call is about to be sent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::journal::Journal;
    /// # use mcp_execution_core::{ServerId, ToolName};
    /// # fn example(journal: &Journal) -> anyhow::Result<()> {
    /// let (digest, size) = journal.digest(&serde_json::Map::new());
    /// let token = journal.start(&ServerId::new("demo")?, &ToolName::new("echo")?, digest, size);
    /// # drop(token);
    /// # Ok(())
    /// # }
    /// ```
    pub fn start(
        &self,
        server: &ServerId,
        tool: &ToolName,
        digest: ArgsDigest,
        args_bytes: usize,
    ) -> CallToken {
        let mut state = self.lock();
        state.calls += 1;
        let seq = state.calls;
        if state.records.len() < MAX_JOURNAL_ENTRIES {
            state.records.push(CallRecord {
                server: server.clone(),
                tool: tool.clone(),
                digest,
                status: CallStatus::Unknown,
                duration: None,
            });
        }
        state.emit(&json!({
            "event": "start", "seq": seq, "server": server.as_str(), "tool": tool.as_str(),
            "digest": digest.hex(), "args_bytes": args_bytes,
        }));
        drop(state);
        CallToken(seq)
    }

    /// Records how a started call ended.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::journal::{CallStatus, CallToken, Journal};
    /// # use std::time::Duration;
    /// # fn example(journal: &Journal, token: CallToken) {
    /// journal.end(token, CallStatus::Ok, Duration::from_millis(12), 0);
    /// # }
    /// ```
    pub fn end(
        &self,
        token: CallToken,
        status: CallStatus,
        duration: Duration,
        result_bytes: usize,
    ) {
        let mut state = self.lock();
        let index = usize::try_from(token.0.saturating_sub(1)).unwrap_or(usize::MAX);
        if let Some(record) = state.records.get_mut(index) {
            record.status = status;
            record.duration = Some(duration);
        }
        state.emit(&json!({
            "event": "end", "seq": token.0, "status": status.as_str(),
            "duration_ms": u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            "result_bytes": result_bytes,
        }));
        drop(state);
    }

    /// Closes the file (marking it truncated when entries were dropped) and returns the call
    /// summary, or `None` when no call was made.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::journal::Journal;
    /// # async fn example(journal: &Journal) {
    /// if let Some(summary) = journal.finish().await {
    ///     println!("{summary}");
    /// }
    /// # }
    /// ```
    pub async fn finish(&self) -> Option<String> {
        let sender = {
            let mut state = self.lock();
            if state.dropped > 0
                && let Some(sender) = &state.sender
            {
                let marker = json!({"event": "truncated", "dropped": state.dropped});
                if sender.send(format!("{marker}\n")).is_err() {
                    tracing::debug!("journal writer already stopped");
                }
            }
            state.sender.take()
        };
        drop(sender);
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(writer) = writer
            && let Err(error) = writer.await
        {
            tracing::warn!(%error, "journal writer ended abnormally");
        }
        self.summary()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Bounded summary: every unfinished or failed call first (up to the cap), then completed
    /// calls in order, listed in call order.
    fn summary(&self) -> Option<String> {
        let state = self.lock();
        if state.calls == 0 {
            return None;
        }
        let notable = |record: &CallRecord| {
            !matches!(
                record.status,
                CallStatus::Ok | CallStatus::Empty | CallStatus::Skipped
            )
        };
        let mut chosen: Vec<usize> = (0..state.records.len())
            .filter(|&index| notable(&state.records[index]))
            .take(MAX_SUMMARY_ENTRIES)
            .collect();
        chosen.extend(
            (0..state.records.len())
                .filter(|&index| !notable(&state.records[index]))
                .take(MAX_SUMMARY_ENTRIES - chosen.len()),
        );
        chosen.sort_unstable();

        let mut lines = vec![format!(
            "call summary: {} call(s); completed calls may have changed state",
            state.calls
        )];
        for index in &chosen {
            let record = &state.records[*index];
            let outcome = match (record.status, record.duration) {
                (CallStatus::Unknown, _) => "unknown (in flight when the run ended)".to_string(),
                (status, Some(duration)) => {
                    format!("{} {}ms", status.as_str(), duration.as_millis())
                }
                (status, None) => status.as_str().to_string(),
            };
            lines.push(format!(
                "  #{} {}/{} args={} {outcome}",
                index + 1,
                record.server,
                record.tool,
                record.digest.prefix(),
            ));
        }
        let more = state.calls - chosen.len() as u64;
        if more > 0 {
            lines.push(format!("  and {more} more"));
        }
        drop(state);
        let text = lines.join("\n");
        Some(text)
    }
}

/// Loads the per-user digest key, creating it atomically (private temp file, then rename) when
/// it is missing or not a valid key, so a reader never sees a partial key and one corrupt file
/// never makes later runs fail.
fn load_or_create_key(path: &Path) -> Result<[u8; KEY_BYTES]> {
    if let Some(key) = read_valid_key(path) {
        return Ok(key);
    }
    let mut key = [0_u8; KEY_BYTES];
    key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());

    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&temp).and_then(|mut file| {
        use std::io::Write as _;
        file.write_all(&key)?;
        file.sync_all()
    });
    // Publish with a hard link, which fails when another run published first: concurrent first
    // runs then all end up with the winner's key. A key that exists but is invalid (or a file
    // system without hard links) is replaced by renaming over it.
    let published = written.and_then(|()| match std::fs::hard_link(&temp, path) {
        Ok(()) => Ok(key),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            read_valid_key(path).map_or_else(|| std::fs::rename(&temp, path).map(|()| key), Ok)
        }
        Err(_) => std::fs::rename(&temp, path).map(|()| key),
    });
    if let Err(cleanup) = std::fs::remove_file(&temp)
        && cleanup.kind() != std::io::ErrorKind::NotFound
    {
        tracing::debug!(%cleanup, "could not remove the staged journal key");
    }
    published.with_context(|| format!("cannot write {}", path.display()))
}

fn read_valid_key(path: &Path) -> Option<[u8; KEY_BYTES]> {
    let bytes = std::fs::read(path).ok()?;
    <[u8; KEY_BYTES]>::try_from(bytes.as_slice()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(temp: &tempfile::TempDir) -> RuntimeDirs {
        RuntimeDirs::from_root(temp.path().to_path_buf())
    }

    fn ids() -> (ServerId, ToolName) {
        (
            ServerId::new("demo").unwrap(),
            ToolName::new("echo").unwrap(),
        )
    }

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(map) = value else {
            panic!("expected an object");
        };
        map
    }

    #[test]
    fn canonical_args_golden_bytes() {
        let bytes = canonical_args(&json!({"b": [{"y": 1, "x": null}, "s"], "a": 2.5}));
        assert_eq!(bytes, br#"{"a":2.5,"b":[{"x":null,"y":1},"s"]}"#);
        let map = object(json!({"b": 1, "a": {"d": true, "c": false}}));
        assert_eq!(canonical_map(&map), br#"{"a":{"c":false,"d":true},"b":1}"#);
    }

    #[test]
    fn args_digest_prefix_is_the_first_twelve_hex_characters() {
        let mut bytes = [0_u8; 32];
        bytes[..7].copy_from_slice(&[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd]);
        assert_eq!(ArgsDigest::from_bytes(bytes).prefix(), "0123456789ab");
    }

    #[test]
    fn canonical_args_sorts_every_level() {
        let one = canonical_args(&json!({"b": 2, "a": {"y": [1, {"q": 1, "p": 2}], "x": 2}}));
        let two = canonical_args(&json!({"a": {"x": 2, "y": [1, {"p": 2, "q": 1}]}, "b": 2}));
        assert_eq!(one, two);
        assert_eq!(
            String::from_utf8(one).unwrap(),
            r#"{"a":{"x":2,"y":[1,{"p":2,"q":1}]},"b":2}"#
        );
    }

    #[tokio::test]
    async fn the_digest_is_stable_for_one_user_and_differs_between_keys() {
        let (first, second) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let run = RunId::new();
        let a = Journal::create(&dirs(&first), &run).await.unwrap();
        let again = Journal::create(&dirs(&first), &RunId::new()).await.unwrap();
        let other = Journal::create(&dirs(&second), &run).await.unwrap();
        let args = object(json!({"k": 1}));

        assert_eq!(a.digest(&args).0, again.digest(&args).0);
        assert_ne!(a.digest(&args).0, other.digest(&args).0);
        assert_ne!(a.digest(&args).0, a.digest(&object(json!({"k": 2}))).0);
    }

    #[tokio::test]
    async fn an_unfinished_call_reads_as_unknown_and_values_never_reach_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let journal = Journal::create(&dirs(&temp), &RunId::new()).await.unwrap();
        let (server, tool) = ids();
        let args = object(json!({"secret": "SECRET-VALUE"}));
        let (digest, size) = journal.digest(&args);

        let done = journal.start(&server, &tool, digest, size);
        journal.end(done, CallStatus::Ok, Duration::from_millis(5), 3);
        journal.start(&server, &tool, digest, size);
        let summary = journal.finish().await.unwrap();

        assert!(summary.contains("unknown"), "{summary}");
        assert!(summary.contains("ok 5ms"), "{summary}");
        let file = std::fs::read_dir(temp.path().join("journals"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let text = std::fs::read_to_string(file).unwrap();
        assert!(!text.contains("SECRET-VALUE"));
        assert_eq!(text.lines().count(), 3);
    }

    #[tokio::test]
    async fn entries_beyond_the_cap_are_dropped_and_the_file_says_so() {
        let temp = tempfile::tempdir().unwrap();
        let journal = Journal::create(&dirs(&temp), &RunId::new()).await.unwrap();
        let (server, tool) = ids();
        let (digest, size) = journal.digest(&Map::new());

        for _ in 0..MAX_JOURNAL_ENTRIES {
            let token = journal.start(&server, &tool, digest, size);
            journal.end(token, CallStatus::Ok, Duration::ZERO, 0);
        }
        let summary = journal.finish().await.unwrap();

        assert!(summary.contains("and "), "{summary}");
        let file = std::fs::read_dir(temp.path().join("journals"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains(r#""event":"truncated""#));
        assert!(std::fs::metadata(file).unwrap().len() <= MAX_JOURNAL_BYTES);
    }

    #[tokio::test]
    async fn no_calls_means_no_summary() {
        let temp = tempfile::tempdir().unwrap();
        let journal = Journal::create(&dirs(&temp), &RunId::new()).await.unwrap();
        assert_eq!(journal.finish().await, None);
    }

    #[test]
    fn a_corrupt_key_file_is_replaced_and_a_valid_one_is_kept() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("journal.key");
        std::fs::write(&path, b"short").unwrap();

        let fresh = load_or_create_key(&path).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), fresh);
        assert_eq!(load_or_create_key(&path).unwrap(), fresh);
        assert_eq!(
            std::fs::read_dir(temp.path()).unwrap().count(),
            1,
            "no temp file left"
        );
    }

    #[test]
    fn concurrent_first_runs_agree_on_one_key() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("journal.key");

        let keys: Vec<[u8; KEY_BYTES]> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..16)
                .map(|_| scope.spawn(|| load_or_create_key(&path).unwrap()))
                .collect();
            workers.into_iter().map(|w| w.join().unwrap()).collect()
        });

        assert!(keys.iter().all(|key| *key == keys[0]));
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}
