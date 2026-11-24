//! Bounded spill files: where output that exceeds its budget goes.
//!
//! A spill file keeps the full text (up to a cap) so the caller can read it on demand instead
//! of having it enter their context. Files live in a private directory, are written without
//! following a planted symlink, and are pruned by age, count and total size before each new
//! one is created.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use super::dirs::{PrunePolicy, RuntimeDirs, create_private_file, ensure_private_dir, prune_dir};

/// Largest number of bytes stored in one spill file.
pub const MAX_SPILL_FILE_BYTES: usize = 8 * 1024 * 1024;

/// Most spill files kept after pruning (one more is about to be written).
pub const MAX_SPILL_FILES: usize = 32;

/// Most total bytes kept across all spill files.
pub const MAX_SPILL_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// How long a spill file is kept.
pub const SPILL_RETENTION: Duration = Duration::from_hours(7 * 24);

/// A spill file that was written.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
/// use mcp_execution_cli::runtime::spill::Spill;
///
/// # async fn example() -> anyhow::Result<()> {
/// let spill = Spill::new(&RuntimeDirs::resolve()?);
/// let file = spill.write(b"full output").await?;
/// assert_eq!(file.original_bytes, 11);
/// assert!(!file.truncated);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpilledFile {
    /// Where the text was stored.
    pub path: PathBuf,
    /// Bytes of the original text.
    pub original_bytes: usize,
    /// Whether the original was longer than [`MAX_SPILL_FILE_BYTES`] and was cut.
    pub truncated: bool,
}

/// The spill directory and its bounds.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
/// use mcp_execution_cli::runtime::spill::Spill;
///
/// let spill = Spill::new(&RuntimeDirs::resolve().unwrap());
/// # let _ = spill;
/// ```
#[derive(Debug, Clone)]
pub struct Spill {
    dir: PathBuf,
}

impl Spill {
    /// Uses the spill directory of `dirs`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    /// use mcp_execution_cli::runtime::spill::Spill;
    ///
    /// let _spill = Spill::new(&RuntimeDirs::resolve().unwrap());
    /// ```
    #[must_use]
    pub fn new(dirs: &RuntimeDirs) -> Self {
        Self {
            dir: dirs.spill_dir(),
        }
    }

    /// Prunes old files, then stores `content` (cut at [`MAX_SPILL_FILE_BYTES`]) in a new file
    /// with mode `0600` on Unix.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be prepared or the file cannot be written.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::dirs::RuntimeDirs;
    /// use mcp_execution_cli::runtime::spill::Spill;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let file = Spill::new(&RuntimeDirs::resolve()?).write(b"output").await?;
    /// println!("{}", file.path.display());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write(&self, content: &[u8]) -> Result<SpilledFile> {
        ensure_private_dir(&self.dir).await?;
        prune_dir(&self.dir, spill_policy(), SystemTime::now()).await?;

        let path = self
            .dir
            .join(format!("spill-{}.txt", uuid::Uuid::new_v4().simple()));
        let stored = content[..content.len().min(MAX_SPILL_FILE_BYTES)].to_vec();
        let original_bytes = content.len();
        let truncated = stored.len() < original_bytes;
        let target = path.clone();
        tokio::task::spawn_blocking(move || write_private(&target, &stored))
            .await
            .context("spill write task failed")??;
        Ok(SpilledFile {
            path,
            original_bytes,
            truncated,
        })
    }
}

const fn spill_policy() -> PrunePolicy {
    PrunePolicy {
        max_age: SPILL_RETENTION,
        max_entries: MAX_SPILL_FILES - 1,
        max_total_bytes: Some(MAX_SPILL_TOTAL_BYTES - MAX_SPILL_FILE_BYTES as u64),
        count_prune_min_age: Duration::ZERO,
    }
}

fn write_private(path: &Path, content: &[u8]) -> Result<()> {
    create_private_file(path)?
        .write_all(content)
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spill_in(temp: &tempfile::TempDir) -> Spill {
        Spill::new(&RuntimeDirs::from_root(temp.path().to_path_buf()))
    }

    #[tokio::test]
    async fn write_stores_content_privately() {
        let temp = tempfile::tempdir().unwrap();
        let file = spill_in(&temp).write(b"hello").await.unwrap();

        assert_eq!(std::fs::read(&file.path).unwrap(), b"hello");
        assert_eq!(file.original_bytes, 5);
        assert!(!file.truncated);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn write_cuts_content_at_the_file_cap() {
        let temp = tempfile::tempdir().unwrap();
        let content = vec![b'a'; MAX_SPILL_FILE_BYTES + 10];
        let file = spill_in(&temp).write(&content).await.unwrap();

        assert!(file.truncated);
        assert_eq!(file.original_bytes, MAX_SPILL_FILE_BYTES + 10);
        assert_eq!(
            std::fs::metadata(&file.path).unwrap().len(),
            MAX_SPILL_FILE_BYTES as u64
        );
    }

    #[tokio::test]
    async fn write_keeps_the_directory_within_the_file_count_bound() {
        let temp = tempfile::tempdir().unwrap();
        let spill = spill_in(&temp);
        for _ in 0..MAX_SPILL_FILES + 3 {
            spill.write(b"x").await.unwrap();
        }
        let count = std::fs::read_dir(temp.path().join("spill"))
            .unwrap()
            .count();
        assert!(count <= MAX_SPILL_FILES, "{count} files kept");
    }
}
