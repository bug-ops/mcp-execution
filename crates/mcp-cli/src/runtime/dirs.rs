//! Per-OS runner state directories and bounded pruning of what they hold.
//!
//! All runner-owned state (spill files, journals, the journal key, per-run scratch) lives under
//! one root:
//!
//! | OS | Root |
//! |----|------|
//! | Linux | `dirs::state_dir()/mcp-execution` |
//! | macOS | `dirs::data_local_dir()/mcp-execution` |
//! | Windows | `%LOCALAPPDATA%/mcp-execution` (absolute), else `dirs::data_local_dir()/mcp-execution` |
//!
//! The policy file lives under the config directory instead; on Windows that is an absolute
//! `%APPDATA%`, else `dirs::config_dir()`.
//!
//! A missing base directory is an error, never a silent temp-dir fallback. On Unix the
//! directories are `0700`; on Windows the profile ACL applies and no mode bits are set.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

/// How long a per-run scratch directory is kept before pruning removes it.
pub const RUN_DIR_RETENTION: Duration = Duration::from_hours(24);

/// Most per-run scratch directories kept after pruning.
pub const MAX_RUN_DIRS: usize = 32;

/// A per-run directory younger than this is never pruned by count, so a concurrent live run
/// keeps its scratch.
pub const RUN_DIR_COUNT_PRUNE_MIN_AGE: Duration = Duration::from_secs(600);

const STATE_DIR_NAME: &str = "mcp-execution";
const POLICY_FILE_NAME: &str = "policy.json";

/// Identifier of one `run` invocation; names its scratch directory.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::dirs::RunId;
///
/// let id = RunId::new();
/// assert_eq!(id.as_str().len(), 32);
/// assert_ne!(id, RunId::new());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunId(String);

impl RunId {
    /// Generates a fresh random identifier (32 lowercase hex characters).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::dirs::RunId;
    ///
    /// assert!(RunId::new().as_str().chars().all(|c| c.is_ascii_hexdigit()));
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    /// Returns the identifier as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::dirs::RunId;
    ///
    /// assert!(!RunId::new().as_str().is_empty());
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

/// Limits a directory is pruned down to.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::dirs::PrunePolicy;
/// use std::time::Duration;
///
/// let policy = PrunePolicy {
///     max_age: Duration::from_secs(60),
///     max_entries: 10,
///     max_total_bytes: None,
///     count_prune_min_age: Duration::ZERO,
/// };
/// assert_eq!(policy.max_entries, 10);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrunePolicy {
    /// Entries older than this are removed.
    pub max_age: Duration,
    /// Entries beyond this count are removed, oldest first.
    pub max_entries: usize,
    /// Regular-file bytes beyond this total are removed, oldest first.
    pub max_total_bytes: Option<u64>,
    /// Entries younger than this are never removed by the count or size limits.
    pub count_prune_min_age: Duration,
}

/// The runner's state directories.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
///
/// let dirs = RuntimeDirs::resolve().unwrap();
/// println!("{}", dirs.spill_dir().display());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDirs {
    root: PathBuf,
}

impl RuntimeDirs {
    /// Resolves the per-OS state root.
    ///
    /// # Errors
    ///
    /// Returns an error when the platform base directory cannot be determined.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// let dirs = RuntimeDirs::resolve().unwrap();
    /// assert!(dirs.root().ends_with("mcp-execution"));
    /// ```
    pub fn resolve() -> Result<Self> {
        let base = absolute_base(
            platform_state_base()
                .context("cannot determine the state directory for this platform")?,
            "state",
        )?;
        Ok(Self::from_root(base.join(STATE_DIR_NAME)))
    }

    pub(crate) const fn from_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// The state root.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// let _root = RuntimeDirs::resolve().unwrap().root().to_path_buf();
    /// ```
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory holding spilled output files.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// assert!(RuntimeDirs::resolve().unwrap().spill_dir().ends_with("spill"));
    /// ```
    #[must_use]
    pub fn spill_dir(&self) -> PathBuf {
        self.root.join("spill")
    }

    /// Directory holding call journals.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// assert!(RuntimeDirs::resolve().unwrap().journals_dir().ends_with("journals"));
    /// ```
    #[must_use]
    pub fn journals_dir(&self) -> PathBuf {
        self.root.join("journals")
    }

    /// Path of the per-user journal digest key.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// assert!(RuntimeDirs::resolve().unwrap().journal_key_path().ends_with("journal.key"));
    /// ```
    #[must_use]
    pub fn journal_key_path(&self) -> PathBuf {
        self.root.join("journal.key")
    }

    /// Directory holding per-run scratch directories.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// assert!(RuntimeDirs::resolve().unwrap().runs_dir().ends_with("runs"));
    /// ```
    #[must_use]
    pub fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    /// Scratch directory of one run.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::{RunId, RuntimeDirs};
    ///
    /// let id = RunId::new();
    /// let dir = RuntimeDirs::resolve().unwrap().run_dir(&id);
    /// assert!(dir.ends_with(id.as_str()));
    /// ```
    #[must_use]
    pub fn run_dir(&self, id: &RunId) -> PathBuf {
        self.runs_dir().join(id.as_str())
    }

    /// Path of the dry-run policy file (`<config dir>/mcp-execution/policy.json`).
    ///
    /// # Errors
    ///
    /// Returns an error when the platform config directory cannot be determined.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// let path = RuntimeDirs::policy_path().unwrap();
    /// assert!(path.ends_with("policy.json"));
    /// ```
    pub fn policy_path() -> Result<PathBuf> {
        let base = absolute_base(
            platform_config_base()
                .context("cannot determine the config directory for this platform")?,
            "config",
        )?;
        Ok(base.join(STATE_DIR_NAME).join(POLICY_FILE_NAME))
    }

    /// Prunes per-run scratch directories under [`RUN_DIR_RETENTION`], [`MAX_RUN_DIRS`] and
    /// [`RUN_DIR_COUNT_PRUNE_MIN_AGE`].
    ///
    /// # Errors
    ///
    /// Returns an error when the runs directory exists but cannot be read.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// RuntimeDirs::resolve()?.prune_runs().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn prune_runs(&self) -> Result<()> {
        prune_dir(&self.runs_dir(), RUN_DIRS_POLICY, SystemTime::now()).await?;
        Ok(())
    }
}

const RUN_DIRS_POLICY: PrunePolicy = PrunePolicy {
    max_age: RUN_DIR_RETENTION,
    max_entries: MAX_RUN_DIRS,
    max_total_bytes: None,
    count_prune_min_age: RUN_DIR_COUNT_PRUNE_MIN_AGE,
};

/// Refuses a relative platform base, which a relative `HOME` or XDG variable would make depend on
/// the working directory.
fn absolute_base(base: PathBuf, kind: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        base.is_absolute(),
        "the {kind} directory '{}' is not an absolute path; check HOME and the XDG base directory variables",
        base.display()
    );
    Ok(base)
}

#[cfg(target_os = "linux")]
fn platform_state_base() -> Option<PathBuf> {
    dirs::state_dir()
}

#[cfg(windows)]
fn platform_state_base() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(dirs::data_local_dir)
}

#[cfg(windows)]
fn platform_config_base() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(dirs::config_dir)
}

#[cfg(not(windows))]
fn platform_config_base() -> Option<PathBuf> {
    dirs::config_dir()
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_state_base() -> Option<PathBuf> {
    dirs::data_local_dir()
}

/// Creates `path` (and parents) as a private directory and refuses a symlink there.
///
/// On Unix the leaf is forced to `0700`, also when it pre-existed with looser bits.
///
/// # Errors
///
/// Returns an error when the directory cannot be created, is a symlink, or is not a directory.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::dirs::ensure_private_dir;
///
/// # async fn example() -> anyhow::Result<()> {
/// let temp = tempfile::tempdir()?;
/// ensure_private_dir(&temp.path().join("state")).await?;
/// # Ok(())
/// # }
/// ```
pub async fn ensure_private_dir(path: &Path) -> Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || ensure_private_dir_blocking(&path))
        .await
        .context("directory creation task failed")?
}

/// Creates (truncating) a regular file at `path` without following a symlink, mode `0600` on
/// Unix. Shared by the spill and journal writers.
///
/// # Errors
///
/// Returns an error when the file cannot be created or restricted.
pub(crate) fn create_private_file(path: &Path) -> Result<std::fs::File> {
    let file = mcp_execution_core::open_confined_write(path)
        .with_context(|| format!("cannot create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("cannot restrict {}", path.display()))?;
    }
    Ok(file)
}

fn ensure_private_dir_blocking(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("cannot create {}", path.display()))?;

    let meta = std::fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect {}", path.display()))?;
    anyhow::ensure!(
        meta.is_dir(),
        "{} is not a directory (symlinks are refused)",
        path.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("cannot restrict {}", path.display()))?;
        }
    }
    Ok(())
}

/// Removes entries of `dir` until it satisfies `policy`; returns how many were removed.
///
/// A missing `dir` is not an error. Symlinks are removed as links, never followed. Removal
/// failures of single entries are skipped, so one stuck entry cannot block a run.
///
/// # Errors
///
/// Returns an error when `dir` exists but cannot be listed.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::dirs::{PrunePolicy, prune_dir};
/// use std::time::{Duration, SystemTime};
///
/// # async fn example() -> anyhow::Result<()> {
/// let temp = tempfile::tempdir()?;
/// let policy = PrunePolicy {
///     max_age: Duration::from_secs(60),
///     max_entries: 8,
///     max_total_bytes: None,
///     count_prune_min_age: Duration::ZERO,
/// };
/// assert_eq!(prune_dir(temp.path(), policy, SystemTime::now()).await?, 0);
/// # Ok(())
/// # }
/// ```
pub async fn prune_dir(dir: &Path, policy: PrunePolicy, now: SystemTime) -> Result<usize> {
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || prune_dir_blocking(&dir, policy, now))
        .await
        .context("prune task failed")?
}

struct Entry {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
    is_dir: bool,
}

fn list_entries(dir: &Path) -> Result<Vec<Entry>> {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("cannot list {}", dir.display())),
    };
    let entries = read
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let meta = std::fs::symlink_metadata(entry.path()).ok()?;
            Some(Entry {
                path: entry.path(),
                modified: meta.modified().ok()?,
                bytes: if meta.is_file() { meta.len() } else { 0 },
                is_dir: meta.is_dir(),
            })
        })
        .collect();
    Ok(entries)
}

fn remove_entry(entry: &Entry) -> bool {
    let result = if entry.is_dir {
        std::fs::remove_dir_all(&entry.path)
    } else {
        std::fs::remove_file(&entry.path)
    };
    result.is_ok()
}

fn prune_dir_blocking(dir: &Path, policy: PrunePolicy, now: SystemTime) -> Result<usize> {
    let mut entries = list_entries(dir)?;
    let age = |entry: &Entry| now.duration_since(entry.modified).unwrap_or(Duration::ZERO);
    let mut removed = 0;

    entries.retain(|entry| {
        if age(entry) > policy.max_age && remove_entry(entry) {
            removed += 1;
            false
        } else {
            true
        }
    });

    entries.sort_by_key(|entry| entry.modified);
    let mut total: u64 = entries.iter().map(|entry| entry.bytes).sum();
    let mut count = entries.len();
    for entry in &entries {
        let over_count = count > policy.max_entries;
        let over_size = policy.max_total_bytes.is_some_and(|max| total > max);
        if !(over_count || over_size) {
            break;
        }
        if age(entry) < policy.count_prune_min_age {
            continue;
        }
        if remove_entry(entry) {
            removed += 1;
            count -= 1;
            total = total.saturating_sub(entry.bytes);
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_base_refuses_a_relative_directory_and_names_it() {
        let error = absolute_base(PathBuf::from("relative/home/.local/state"), "state")
            .expect_err("a relative base must be refused");
        let message = error.to_string();
        assert!(message.contains("relative/home/.local/state"), "{message}");
        assert!(message.contains("not an absolute path"), "{message}");
    }

    // Unix only: on Windows the base comes from LOCALAPPDATA/APPDATA or the known-folder API, not
    // from HOME, so a relative HOME cannot reach it.
    #[cfg(unix)]
    #[test]
    fn a_relative_home_is_refused_by_resolve_and_policy_path() {
        let _home = crate::commands::common::home_env::HomeEnv::set(Path::new("relative/home"));

        for (name, error) in [
            ("resolve", RuntimeDirs::resolve().map(|_| ()).unwrap_err()),
            ("policy_path", RuntimeDirs::policy_path().unwrap_err()),
        ] {
            let message = error.to_string();
            assert!(
                message.contains("not an absolute path"),
                "{name}: {message}"
            );
            assert!(message.contains("relative/home"), "{name}: {message}");
        }
    }

    #[test]
    fn absolute_base_keeps_an_absolute_directory() {
        let base = std::env::current_dir().unwrap();
        assert_eq!(absolute_base(base.clone(), "config").unwrap(), base);
    }

    fn touch(path: &Path, bytes: usize, age: Duration) {
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    const POLICY: PrunePolicy = PrunePolicy {
        max_age: Duration::from_secs(1000),
        max_entries: 2,
        max_total_bytes: Some(10),
        count_prune_min_age: Duration::from_secs(5),
    };

    #[tokio::test]
    async fn prune_removes_old_then_excess_oldest_first() {
        let temp = tempfile::tempdir().unwrap();
        touch(&temp.path().join("ancient"), 1, Duration::from_secs(5000));
        touch(&temp.path().join("a"), 1, Duration::from_secs(300));
        touch(&temp.path().join("b"), 1, Duration::from_secs(200));
        touch(&temp.path().join("c"), 1, Duration::from_secs(100));

        let removed = prune_dir(temp.path(), POLICY, SystemTime::now())
            .await
            .unwrap();

        assert_eq!(removed, 2);
        assert!(!temp.path().join("ancient").exists());
        assert!(!temp.path().join("a").exists());
        assert!(temp.path().join("b").exists() && temp.path().join("c").exists());
    }

    #[tokio::test]
    async fn prune_enforces_total_bytes_and_spares_young_entries() {
        let temp = tempfile::tempdir().unwrap();
        touch(&temp.path().join("big"), 8, Duration::from_secs(100));
        touch(&temp.path().join("young"), 8, Duration::from_secs(1));

        prune_dir(temp.path(), POLICY, SystemTime::now())
            .await
            .unwrap();

        assert!(!temp.path().join("big").exists());
        assert!(temp.path().join("young").exists());
    }

    /// Sets a directory's modification time; Windows needs backup semantics to open a directory.
    fn set_dir_modified(path: &Path, when: SystemTime) {
        #[cfg(windows)]
        let dir = {
            use std::os::windows::fs::OpenOptionsExt as _;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)
                .unwrap()
        };
        #[cfg(not(windows))]
        let dir = std::fs::File::open(path).unwrap();
        dir.set_modified(when).unwrap();
    }

    #[tokio::test]
    async fn prune_removes_directories_and_ignores_a_missing_dir() {
        let temp = tempfile::tempdir().unwrap();
        let run = temp.path().join("run");
        std::fs::create_dir(&run).unwrap();
        std::fs::write(run.join("script.ts"), "x").unwrap();
        set_dir_modified(&run, SystemTime::now() - Duration::from_secs(5000));

        assert_eq!(
            prune_dir(temp.path(), POLICY, SystemTime::now())
                .await
                .unwrap(),
            1
        );
        assert!(!run.exists());
        assert_eq!(
            prune_dir(&temp.path().join("missing"), POLICY, SystemTime::now())
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn ensure_private_dir_creates_and_rejects_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("a").join("b");
        ensure_private_dir(&dir).await.unwrap();
        assert!(dir.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }

        let file = temp.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(ensure_private_dir(&file).await.is_err());
    }

    #[test]
    fn layout_hangs_off_one_root() {
        let dirs = RuntimeDirs::from_root(PathBuf::from("/state/mcp-execution"));
        let id = RunId::new();
        assert_eq!(dirs.spill_dir(), Path::new("/state/mcp-execution/spill"));
        assert_eq!(
            dirs.journals_dir(),
            Path::new("/state/mcp-execution/journals")
        );
        assert!(dirs.run_dir(&id).starts_with(dirs.runs_dir()));
    }
}
