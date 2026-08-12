//! Shared path-confinement algorithm for caller-supplied output paths.
//!
//! `mcp-execution-skill`'s `save_skill` and `mcp-execution-server`'s `introspect_server` both
//! accept an optional caller-supplied relative path (`output_path`, `output_dir`) that must be
//! confined to a per-server subdirectory of a trusted base directory: without confinement, an
//! absolute path, a `..`-relative path, or a path that walks through a symlink planted inside
//! the base directory lets a caller redirect a write anywhere the process can reach (issues
//! #184, #216, #217). Both crates walked an identical component-by-component resolve-and-confine
//! loop with their own error types; [`resolve_confined_path`] is that walk, extracted once so
//! the two copies cannot silently drift apart.
//!
//! The absolute-path, `..`-component, and file-name pre-checks stay in each crate's own
//! `relative_subpath`/`relative_target` helper, since callers disagree on what an *absent* path
//! means (no subdirectory override, vs. the default `SKILL.md`) and on whether an empty result
//! is legitimate. Only the shared filesystem walk — segment resolution, the lenient intermediate
//! walk, and the terminal-component check — lives here.

use std::ffi::OsStr;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

use crate::path::{sanitize_path_for_error, validate_path_segment};
use crate::untrusted::sanitize_untrusted_inline;

/// The terminal path component a [`resolve_confined_path`] walk resolves and confinement-checks
/// but deliberately does not create.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ConfinementTarget;
/// use std::ffi::OsStr;
///
/// let target = ConfinementTarget::File(OsStr::new("SKILL.md"));
/// assert!(matches!(target, ConfinementTarget::File(_)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfinementTarget<'a> {
    /// The caller publishes the directory itself (e.g. via an atomic staged rename), so the
    /// resolved directory is confinement-checked and canonicalized but not created.
    Directory(&'a OsStr),
    /// The caller writes the file itself, so the resolved path is confinement-checked but
    /// neither created nor canonicalized. [`write_confined_file`] closes the symlink-planting
    /// race at this exact terminal path between that check and the write itself (issue #496); a
    /// plain [`tokio::fs::write`] does not. It does not defend against a symlink swapped in for a
    /// parent directory after the check, nor a hardlink at the target — see its own doc comment
    /// for the residual.
    File(&'a OsStr),
}

/// Errors from walking and confining a path to a base directory.
///
/// Every variant is publicly constructible only by [`resolve_confined_path`] itself; callers map
/// this enum into their own crate-specific error type with a total `From` implementation rather
/// than matching on it directly, since two callers give the same failure different names (e.g.
/// [`ConfinementError::WrongTargetKind`] means "not a directory" for one caller and "not a file"
/// for the other).
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ConfinementError, resolve_confined_path};
/// use std::path::Path;
///
/// // Segment validation runs before any filesystem access, so this fails synchronously.
/// let err = tokio::runtime::Builder::new_current_thread()
///     .build()
///     .unwrap()
///     .block_on(resolve_confined_path(Path::new("/base"), "..", Path::new(""), None))
///     .unwrap_err();
/// assert!(matches!(err, ConfinementError::InvalidSegment { .. }));
/// ```
#[derive(Debug, Error)]
pub enum ConfinementError {
    /// The path segment pushed onto the base directory (e.g. a `server_id`) is empty or is not
    /// a single plain path component.
    #[error("segment must be a single non-empty path segment: {segment:?}")]
    InvalidSegment {
        /// Sanitized display form of the rejected segment (see
        /// [`sanitize_untrusted_inline`](crate::untrusted::sanitize_untrusted_inline)): control
        /// characters, bidi-reordering characters, and other invisible/structural characters are
        /// neutralized, and `&`/`<`/`>` are entity-escaped, since this value is
        /// attacker-controlled and reaches LLM-facing error text. The `{segment:?}` (`Debug`)
        /// formatting above is a required second layer of defense on top of that sanitization,
        /// not incidental — see [`ServerIdError`](crate::ServerIdError)'s doc comment for why it
        /// must not be "simplified" to `{segment}` (`Display`).
        ///
        /// This field is `pub` only because the enum itself is; [`resolve_confined_path`] is the
        /// sole constructor of this variant, and it always passes an already-sanitized string
        /// here. A caller building this variant directly (there are none in this workspace) must
        /// sanitize the value the same way, or a raw value reaches every downstream consumer that
        /// embeds this field (see `mcp-execution-server`'s `OutputDirError::InvalidServerId` and
        /// `mcp-execution-skill`'s `OutputPathError::InvalidServerId`, both of which move this
        /// field verbatim into their own `server_id` without re-sanitizing).
        segment: String,
    },

    /// The segment's own directory already exists as a symlink, which is rejected outright
    /// regardless of where it points - including at a sibling directory that still resolves
    /// inside the base, which would otherwise pass a resolve-and-confine check (issue #217).
    #[error("segment directory must not be a symlink: {path}")]
    SegmentIsSymlink {
        /// Sanitized display form of the offending path.
        path: String,
    },

    /// The resolved path escapes the segment directory, typically because a path component
    /// resolved through (or is itself) a symlink that points outside it.
    #[error("resolved path escapes the confined directory: {path}")]
    Escape {
        /// Sanitized display form of the path that escaped confinement.
        path: String,
    },

    /// A path component that must be a directory already exists as something else (e.g. a
    /// regular file).
    #[error("path component is not a directory: {path}")]
    NotADirectory {
        /// Sanitized display form of the offending component.
        path: String,
    },

    /// The terminal component already exists as the kind of entry [`ConfinementTarget`] says it
    /// isn't (a file where [`ConfinementTarget::Directory`] was expected, or a directory where
    /// [`ConfinementTarget::File`] was expected).
    #[error("path exists as the wrong kind of entry: {path}")]
    WrongTargetKind {
        /// Sanitized display form of the offending path.
        path: String,
    },

    /// Creating a directory needed along the path failed.
    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        /// Sanitized display form of the directory that could not be created.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// I/O error resolving the base directory or a path component.
    #[error("failed to resolve confined path: {0}")]
    Io(#[from] std::io::Error),
}

/// Resolves `segment`, then `relative_dirs`, then `target` (if any), confining every step to
/// `base_dir/segment`.
///
/// `segment` is validated as a single plain path component (see [`validate_path_segment`]) and
/// pushed onto `base_dir` (canonicalized once, since `base_dir` itself is trusted, first-party
/// configuration rather than caller input). It is rejected outright if it already exists as a
/// symlink, regardless of where it points, since a resolve-and-confine check alone would accept
/// a symlink from a sibling directory that still resolves under `base_dir` (issue #217).
///
/// `relative_dirs` must already be `..`-free and non-absolute — callers reject those shapes in
/// their own pre-check before calling this function, since what an absent or empty path means
/// differs by caller. Each of its components is confined to the resolved segment directory (not
/// merely to `base_dir` as a whole) and created if missing; an existing symlink is followed only
/// if it still resolves inside the segment directory.
///
/// `target`, when supplied, names the walk's terminal component. It is confinement-checked but
/// deliberately never created: a [`ConfinementTarget::Directory`] is resolved and canonicalized
/// (the caller typically publishes it itself via an atomic staged rename), while a
/// [`ConfinementTarget::File`] is checked but left exactly as constructed, uncanonicalized, since
/// the caller is about to create it. `target: None` returns the walked `relative_dirs` chain
/// itself. Either way the terminal component is rejected outright if it already exists as a
/// symlink, dangling or not: a dangling symlink can't be resolved by `canonicalize`, but would
/// still be followed by a subsequent write, so it is checked with `symlink_metadata` instead.
///
/// Each component is created and confinement-checked one at a time (rather than via a single
/// recursive create-and-canonicalize), so a symlink already present under `base_dir` when this
/// call starts — whether at `segment` or at any deeper component — is resolved and rejected
/// *before* this function creates anything under it or descends into it. This is a check against
/// pre-existing state, not a concurrency guarantee: it does not defend against a symlink planted
/// by a racing process between this function's checks and the caller's subsequent write (see
/// [`write_confined_file`], which closes that gap for the write itself at the terminal path —
/// though not against a parent directory swapped for a symlink after this function returns, nor
/// a hardlink at the target; see its own doc comment for the residual).
///
/// Directory creation along the walk *is* safe against concurrent callers resolving the same
/// not-yet-existing path: `ErrorKind::AlreadyExists` from creating `segment`'s own directory or
/// any `relative_dirs` component is tolerated, and the entry left behind by the caller that won
/// the race is then validated exactly as if it had already existed when this call started -
/// rejected under the same rules (symlink, wrong kind) rather than trusted just because someone
/// else created it (issue #491). This is the only race this function defends against; the
/// terminal `target` component is still never created, per the paragraph above.
///
/// # Errors
///
/// Returns [`ConfinementError`] if `segment` is empty or not a single plain path segment, if
/// `segment`'s own directory already exists as a symlink, if the resolved path escapes
/// `base_dir/segment` at any step, if a required directory could not be created, if a path
/// component that must be a directory already exists as something else, or if `target`'s
/// terminal component already exists as the wrong kind of entry.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_core::{ConfinementTarget, resolve_confined_path};
/// use std::ffi::OsStr;
/// use std::path::Path;
///
/// # async fn example() -> Result<(), mcp_execution_core::ConfinementError> {
/// let path = resolve_confined_path(
///     Path::new("/home/user/.claude/skills"),
///     "github",
///     Path::new(""),
///     Some(ConfinementTarget::File(OsStr::new("SKILL.md"))),
/// )
/// .await?;
/// println!("Resolved to {}", path.display());
/// # Ok(())
/// # }
/// ```
pub async fn resolve_confined_path(
    base_dir: &Path,
    segment: &str,
    relative_dirs: &Path,
    target: Option<ConfinementTarget<'_>>,
) -> Result<PathBuf, ConfinementError> {
    let component =
        validate_path_segment(segment).ok_or_else(|| ConfinementError::InvalidSegment {
            segment: sanitize_untrusted_inline(segment),
        })?;

    tokio::fs::create_dir_all(base_dir)
        .await
        .map_err(|source| ConfinementError::CreateDir {
            path: sanitize_path_for_error(base_dir),
            source,
        })?;
    let canonical_root = tokio::fs::canonicalize(base_dir).await?;

    let segment_dir = resolve_segment_dir(&canonical_root, component).await?;

    let mut current = segment_dir.clone();
    for dir_component in relative_dirs.components() {
        current.push(dir_component);
        resolve_lenient_component(&mut current, &segment_dir).await?;
    }

    match target {
        None => Ok(current),
        Some(target) => resolve_terminal(current, &segment_dir, target).await,
    }
}

/// Validates `segment_dir`, which is already known to exist (`meta` is its `symlink_metadata`),
/// under [`resolve_segment_dir`]'s strict policy: rejected outright if it's a symlink (regardless
/// of where it points), or if it exists as anything other than a directory.
fn validate_existing_segment_dir(
    segment_dir: &Path,
    meta: &std::fs::Metadata,
) -> Result<(), ConfinementError> {
    if meta.file_type().is_symlink() {
        return Err(ConfinementError::SegmentIsSymlink {
            path: sanitize_path_for_error(segment_dir),
        });
    }
    if !meta.is_dir() {
        return Err(ConfinementError::NotADirectory {
            path: sanitize_path_for_error(segment_dir),
        });
    }
    Ok(())
}

/// Resolves and confines `segment`'s own directory under `canonical_root`, rejecting it outright
/// if it already exists as a symlink rather than resolving and re-checking it (see
/// [`resolve_confined_path`]'s doc comment for why). Creates the directory if it doesn't exist
/// yet; if a concurrent caller creates it first, `ErrorKind::AlreadyExists` is tolerated and the
/// winner's directory is validated exactly as if it had already existed (issue #491).
async fn resolve_segment_dir(
    canonical_root: &Path,
    component: Component<'_>,
) -> Result<PathBuf, ConfinementError> {
    let mut segment_dir = canonical_root.to_path_buf();
    segment_dir.push(component);
    if !segment_dir.starts_with(canonical_root) {
        return Err(ConfinementError::Escape {
            path: sanitize_path_for_error(&segment_dir),
        });
    }
    if let Ok(meta) = tokio::fs::symlink_metadata(&segment_dir).await {
        validate_existing_segment_dir(&segment_dir, &meta)?;
    } else if let Err(source) = tokio::fs::create_dir(&segment_dir).await {
        if source.kind() != ErrorKind::AlreadyExists {
            return Err(ConfinementError::CreateDir {
                path: sanitize_path_for_error(&segment_dir),
                source,
            });
        }
        let meta = tokio::fs::symlink_metadata(&segment_dir).await?;
        validate_existing_segment_dir(&segment_dir, &meta)?;
    }
    Ok(segment_dir)
}

/// Confirms `current`, which is already known to exist, resolves (as a symlink or otherwise) to a
/// directory still confined to `segment_dir` - [`resolve_lenient_component`]'s lenient policy,
/// which resolves and re-checks an existing symlink rather than rejecting it outright.
async fn validate_existing_lenient_component(
    current: &mut PathBuf,
    segment_dir: &Path,
) -> Result<(), ConfinementError> {
    let resolved = tokio::fs::canonicalize(&current).await?;
    if !resolved.starts_with(segment_dir) {
        return Err(ConfinementError::Escape {
            path: sanitize_path_for_error(current),
        });
    }
    if !tokio::fs::metadata(&resolved).await?.is_dir() {
        return Err(ConfinementError::NotADirectory {
            path: sanitize_path_for_error(current),
        });
    }
    *current = resolved;
    Ok(())
}

/// Confines `current` to `segment_dir`, resolving (and confirming) an existing symlink rather
/// than rejecting it outright, or creating the directory if it's missing. If a concurrent caller
/// creates it first, `ErrorKind::AlreadyExists` is tolerated and the winner's directory is
/// validated exactly as if it had already existed (issue #491).
async fn resolve_lenient_component(
    current: &mut PathBuf,
    segment_dir: &Path,
) -> Result<(), ConfinementError> {
    if !current.starts_with(segment_dir) {
        return Err(ConfinementError::Escape {
            path: sanitize_path_for_error(current),
        });
    }
    match tokio::fs::symlink_metadata(&current).await {
        Ok(_) => validate_existing_lenient_component(current, segment_dir).await,
        Err(_) => match tokio::fs::create_dir(&current).await {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                validate_existing_lenient_component(current, segment_dir).await
            }
            Err(source) => Err(ConfinementError::CreateDir {
                path: sanitize_path_for_error(current),
                source,
            }),
        },
    }
}

/// Resolves and confinement-checks the walk's terminal component per `target`, without creating
/// it. See [`ConfinementTarget`] and [`resolve_confined_path`]'s doc comment for the deliberate
/// asymmetry between the two variants.
async fn resolve_terminal(
    mut current: PathBuf,
    segment_dir: &Path,
    target: ConfinementTarget<'_>,
) -> Result<PathBuf, ConfinementError> {
    match target {
        ConfinementTarget::Directory(name) => {
            current.push(name);
            if !current.starts_with(segment_dir) {
                return Err(ConfinementError::Escape {
                    path: sanitize_path_for_error(&current),
                });
            }
            if let Ok(meta) = tokio::fs::symlink_metadata(&current).await {
                if meta.file_type().is_symlink() {
                    return Err(ConfinementError::Escape {
                        path: sanitize_path_for_error(&current),
                    });
                }
                let resolved = tokio::fs::canonicalize(&current).await?;
                if !resolved.starts_with(segment_dir) {
                    return Err(ConfinementError::Escape {
                        path: sanitize_path_for_error(&current),
                    });
                }
                if !meta.is_dir() {
                    return Err(ConfinementError::WrongTargetKind {
                        path: sanitize_path_for_error(&current),
                    });
                }
                current = resolved;
            }
            Ok(current)
        }
        ConfinementTarget::File(name) => {
            let final_path = current.join(name);
            if let Ok(meta) = tokio::fs::symlink_metadata(&final_path).await {
                if meta.file_type().is_symlink() {
                    return Err(ConfinementError::Escape {
                        path: sanitize_path_for_error(&final_path),
                    });
                }
                if meta.is_dir() {
                    return Err(ConfinementError::WrongTargetKind {
                        path: sanitize_path_for_error(&final_path),
                    });
                }
            }
            Ok(final_path)
        }
    }
}

/// Writes `content` to `path`, refusing to follow a symlink planted at `path`'s exact location
/// after a caller's [`ConfinementTarget::File`] confinement check but before this call.
///
/// [`ConfinementTarget::File`]'s own doc comment calls out the gap this closes: resolving and
/// confinement-checking a file target deliberately does not create it, so nothing stops a
/// symlink from being planted at the resolved path between that check and the caller's write -
/// and a plain [`tokio::fs::write`] would follow it, redirecting the write outside the confined
/// directory (issue #496).
///
/// The entire check-then-write sequence runs inside a single [`tokio::task::spawn_blocking`]
/// call, using `std::fs` rather than `tokio::fs`: this preserves the same atomicity a plain
/// `tokio::fs::write` already had - once the blocking task is queued, dropping the returned
/// future (e.g. because a client disconnected and `rmcp` drops the handler future) does not stop
/// or partially undo a write that already started running on the blocking pool. An async-native
/// version built from `tokio::fs::OpenOptions` plus `AsyncWriteExt::write_all` would not have
/// this property: each `.await` point is a place a dropped future can land between `O_TRUNC`
/// truncating the file and the content actually being written, leaving a 0-byte file behind
/// instead of either the old or the new content — worse than not attempting the write at all.
///
/// On Unix, the open that creates or truncates `path` also carries `O_NOFOLLOW`, so the kernel
/// rejects a symlinked terminal component (dangling or not) as part of that same syscall - there
/// is no separate check-then-write step left for a racing process to land in between *for that
/// exact path*. A pre-existing regular file is still opened and truncated normally: `O_NOFOLLOW`
/// only rejects a symlink at the final path component, so this does not change overwrite
/// semantics for a caller that already decided (via its own `exists()`/`overwrite` check) that
/// clobbering an existing file is fine.
///
/// **Residual gaps, even on Unix**: `O_NOFOLLOW` only guards the *terminal* component. Something
/// with write access further up the confined path (e.g. `{server_id}/` itself) could rename that
/// directory aside and drop a symlink in its place after the caller's confinement check but
/// before this call — the open then traverses the symlinked *parent* and still escapes, one
/// directory level up from what a flag on the final `open` call can see. A hardlink planted at
/// `path` (same filesystem) is also not a symlink, so `O_NOFOLLOW` does not reject it, and the
/// write clobbers whatever the hardlink points at. Neither is defended against here; closing them
/// would need a directory file descriptor captured during the confinement walk itself
/// (`openat`/`openat2` with `RESOLVE_NO_SYMLINKS` on Linux) rather than a flag on the terminal
/// open call alone.
///
/// Windows has no usable equivalent for this specific open, even though `custom_flags` is exposed
/// there too (`std::fs::OpenOptions::custom_flags` via `std::os::windows::fs::OpenOptionsExt`):
/// `FILE_FLAG_OPEN_REPARSE_POINT`, the flag that opens a reparse point (Windows's symlink
/// mechanism) itself, is documented by `CreateFileW` as unusable together with `CREATE_ALWAYS` —
/// which is exactly what a `create(true).truncate(true)` open maps to — and even where it can be
/// used, it does not reject on open the way `O_NOFOLLOW` does; it hands back a handle to the link
/// itself, which a plain write would still land on. So Windows relies solely on an
/// immediately-preceding [`std::fs::symlink_metadata`] check, still inside the same blocking
/// closure and so still with no yield point in between - this narrows the window against a
/// symlink already present when the check runs, but remains genuinely open (not just narrowed) to
/// a symlink a racing *process* plants between that check and the `open` call, since there is no
/// single-syscall check-and-open on this platform. This crate's test suite has no Windows symlink
/// coverage — creating a symlink there requires a privilege most CI runners don't grant.
///
/// # Errors
///
/// Returns [`ConfinementError::Io`] if the symlink check (Windows only), the open, or the write
/// failed - including the platform's "too many levels of symbolic links" error on Unix when
/// `path`'s terminal component is a symlink - or if the blocking task itself panicked.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::write_confined_file;
/// use tempfile::TempDir;
///
/// let dir = TempDir::new().unwrap();
/// let path = dir.path().join("SKILL.md");
/// tokio::runtime::Builder::new_current_thread()
///     .build()
///     .unwrap()
///     .block_on(write_confined_file(&path, b"---\nname: demo\n---\n"))
///     .unwrap();
/// assert_eq!(std::fs::read(&path).unwrap(), b"---\nname: demo\n---\n");
/// ```
pub async fn write_confined_file(path: &Path, content: &[u8]) -> Result<(), ConfinementError> {
    let path = path.to_path_buf();
    let content = content.to_vec();
    // The first `?` propagates a `JoinError` (the blocking task panicked or was cancelled,
    // wrapped via `Error::other`); the second propagates `write_confined_file_blocking`'s own
    // `io::Result`. Both convert to `ConfinementError` via its `#[from] std::io::Error` variant.
    tokio::task::spawn_blocking(move || write_confined_file_blocking(&path, &content))
        .await
        .map_err(std::io::Error::other)??;
    Ok(())
}

/// The synchronous body of [`write_confined_file`], run inside a single `spawn_blocking` call so
/// the check-then-write sequence is atomic with respect to the calling future being dropped.
fn write_confined_file_blocking(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = open_confined_write(path)?;
    file.write_all(content)?;
    file.flush()
}

/// Opens `path` for writing, refusing to follow a pre-existing symlink planted at that exact
/// location.
///
/// The same guard [`write_confined_file`] applies, factored out as its own blocking, synchronous
/// primitive. [`write_confined_file`] stages content in memory and writes it to its *final* path
/// in one call; a caller that instead needs to stage into a separate `.tmp` path and `rename` it into
/// place (e.g. `mcp-execution-files`' `write_file_atomic`) cannot reuse that function directly,
/// since the symlink race it closes is specific to the exact path passed in — here, the `.tmp`
/// staging path rather than the final one (issue #504). Exposing this primitive lets both crates
/// share one guard instead of each hand-rolling its own.
///
/// On Unix, the open that creates or truncates `path` carries `O_NOFOLLOW`, so the kernel rejects
/// a symlinked terminal component (dangling or not) as part of that same syscall. Windows has no
/// equivalent flag usable together with `create(true).truncate(true)` (see
/// [`write_confined_file`]'s doc comment for why), so it relies solely on a
/// [`std::fs::symlink_metadata`] pre-check with no yield point before the open — this narrows but
/// does not close the window against a symlink planted by a racing process between the two.
///
/// A pre-existing regular file is still opened and truncated normally: this only rejects a
/// symlink at the final path component, not an ordinary overwrite.
///
/// # Errors
///
/// Returns an error if the symlink check (Windows only) or the open failed — including the
/// platform's "too many levels of symbolic links" error on Unix when `path`'s terminal component
/// is a symlink.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::open_confined_write;
/// use std::io::Write;
/// use tempfile::TempDir;
///
/// let dir = TempDir::new().unwrap();
/// let path = dir.path().join("staged.tmp");
/// let mut file = open_confined_write(&path).unwrap();
/// file.write_all(b"content").unwrap();
/// ```
pub fn open_confined_write(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            "refusing to write through a pre-existing symlink",
        ));
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    // No Windows equivalent: `FILE_FLAG_OPEN_REPARSE_POINT` is documented as unusable together
    // with `CREATE_ALWAYS` (what `create(true).truncate(true)` maps to), and even where usable it
    // doesn't reject on open the way `O_NOFOLLOW` does - see this function's doc comment. Windows
    // relies solely on the `symlink_metadata` pre-check above.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }

    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn target_none_returns_segment_dir_and_creates_nothing_beyond_it() {
        let base = TempDir::new().unwrap();
        let resolved = resolve_confined_path(base.path(), "my-server", Path::new(""), None)
            .await
            .unwrap();
        let canonical_base = base.path().canonicalize().unwrap();
        assert_eq!(resolved, canonical_base.join("my-server"));
    }

    #[tokio::test]
    async fn leading_cur_dir_resolves_like_its_normalized_form() {
        let base = TempDir::new().unwrap();
        let with_cur_dir = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new("./nested"),
            Some(ConfinementTarget::File(OsStr::new("out.txt"))),
        )
        .await
        .unwrap();
        let normalized = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new("nested"),
            Some(ConfinementTarget::File(OsStr::new("out.txt"))),
        )
        .await
        .unwrap();
        assert_eq!(with_cur_dir, normalized);
    }

    #[tokio::test]
    async fn segment_empty_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_confined_path(base.path(), "", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::InvalidSegment { .. }));
    }

    #[tokio::test]
    async fn segment_with_parent_traversal_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_confined_path(base.path(), "..", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::InvalidSegment { .. }));
    }

    /// Issue #452: a segment rejected for containing a path separator can still carry
    /// printable-but-disallowed characters (`&`, `<`, `>`, emoji) elsewhere in it; none of those
    /// may reach the error message unescaped.
    #[tokio::test]
    async fn segment_with_hostile_characters_is_escaped_in_error() {
        let base = TempDir::new().unwrap();
        for (candidate, escaped) in [
            ("a/b&c", "a/b&amp;c"),
            ("a/b<c", "a/b&lt;c"),
            ("a/b>c", "a/b&gt;c"),
        ] {
            let err = resolve_confined_path(base.path(), candidate, Path::new(""), None)
                .await
                .unwrap_err();
            let message = err.to_string();
            assert!(!message.contains(candidate), "{message:?}");
            assert!(message.contains(escaped), "{message:?}");
        }
    }

    /// S2: an emoji carries no structural or injection risk on its own, so it must appear in the
    /// error message unchanged rather than being mangled — unlike `&`/`<`/`>` above.
    #[tokio::test]
    async fn segment_with_emoji_is_left_unchanged_in_error() {
        let base = TempDir::new().unwrap();
        let err = resolve_confined_path(base.path(), "a/b\u{1F600}c", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("a/b\u{1F600}c"));
    }

    /// A legitimate non-ASCII segment must pass through `sanitize_untrusted_inline` unchanged;
    /// only the separator that triggers `InvalidSegment` is the actual problem here.
    #[tokio::test]
    async fn segment_with_legitimate_non_ascii_is_left_unchanged_in_error() {
        let base = TempDir::new().unwrap();
        let err = resolve_confined_path(base.path(), "café/menu_日本語", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("café/menu_日本語"));
    }

    /// `sanitize_untrusted_inline` deliberately leaves U+200D (ZERO WIDTH JOINER) untouched (see
    /// its own doc comment), so `InvalidSegment`'s stored `segment` field still carries a raw
    /// ZWJ. This is only safe because `{segment:?}` (`Debug`) formatting escapes it to
    /// `\u{200d}` rather than emitting it verbatim — this test pins that second layer of
    /// defense, mirroring `ServerIdError`'s equivalent regression test.
    #[tokio::test]
    async fn segment_with_zwj_is_debug_escaped_in_error() {
        let base = TempDir::new().unwrap();
        let err = resolve_confined_path(base.path(), "a/b\u{200D}c", Path::new(""), None)
            .await
            .unwrap_err();
        let message = err.to_string();
        assert!(
            !message.contains('\u{200D}'),
            "raw ZWJ leaked into: {message}"
        );
        assert!(message.contains("\\u{200d}"), "message was: {message}");
    }

    #[tokio::test]
    async fn segment_with_path_separator_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_confined_path(base.path(), "a/b", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::InvalidSegment { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn segment_dir_symlink_to_outside_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), base.path().join("my-server")).unwrap();

        let err = resolve_confined_path(base.path(), "my-server", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::SegmentIsSymlink { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn segment_dir_symlink_to_sibling_is_rejected() {
        let base = TempDir::new().unwrap();
        tokio::fs::create_dir_all(base.path().join("server-a"))
            .await
            .unwrap();
        std::os::unix::fs::symlink(base.path().join("server-a"), base.path().join("server-b"))
            .unwrap();

        let err = resolve_confined_path(base.path(), "server-b", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::SegmentIsSymlink { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn segment_dir_that_is_a_regular_file_is_rejected() {
        let base = TempDir::new().unwrap();
        tokio::fs::write(base.path().join("my-server"), "oops")
            .await
            .unwrap();

        let err = resolve_confined_path(base.path(), "my-server", Path::new(""), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::NotADirectory { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn lenient_walk_symlinked_intermediate_escape_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(outside.path(), server_dir.join("escape")).unwrap();

        let err = resolve_confined_path(base.path(), "my-server", Path::new("escape/custom"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::Escape { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn lenient_walk_regular_file_intermediate_is_rejected() {
        let base = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        tokio::fs::write(server_dir.join("not-a-dir"), "oops")
            .await
            .unwrap();

        let err = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new("not-a-dir/custom"),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ConfinementError::NotADirectory { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn lenient_walk_symlink_loop_surfaces_as_io() {
        let base = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink("a", server_dir.join("a")).unwrap();

        let err = resolve_confined_path(base.path(), "my-server", Path::new("a/custom"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::Io(_)));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn dangling_symlink_at_terminal_is_rejected_under_both_targets() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let dangling_target = outside.path().join("does-not-exist");

        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(&dangling_target, server_dir.join("custom")).unwrap();

        let dir_err = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new(""),
            Some(ConfinementTarget::Directory(OsStr::new("custom"))),
        )
        .await
        .unwrap_err();
        assert!(matches!(dir_err, ConfinementError::Escape { .. }));

        let file_err = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new(""),
            Some(ConfinementTarget::File(OsStr::new("custom"))),
        )
        .await
        .unwrap_err();
        assert!(matches!(file_err, ConfinementError::Escape { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlink_to_existing_outside_file_at_terminal_is_rejected_under_both_targets() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("real");
        tokio::fs::write(&outside_file, "outside").await.unwrap();

        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(&outside_file, server_dir.join("custom")).unwrap();

        let dir_err = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new(""),
            Some(ConfinementTarget::Directory(OsStr::new("custom"))),
        )
        .await
        .unwrap_err();
        assert!(matches!(dir_err, ConfinementError::Escape { .. }));

        let file_err = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new(""),
            Some(ConfinementTarget::File(OsStr::new("custom"))),
        )
        .await
        .unwrap_err();
        assert!(matches!(file_err, ConfinementError::Escape { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn terminal_exists_as_the_other_kind_under_both_targets() {
        let base = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        tokio::fs::write(server_dir.join("custom"), "oops")
            .await
            .unwrap();

        // A regular file where a directory was expected.
        let dir_err = resolve_confined_path(
            base.path(),
            "my-server",
            Path::new(""),
            Some(ConfinementTarget::Directory(OsStr::new("custom"))),
        )
        .await
        .unwrap_err();
        assert!(matches!(dir_err, ConfinementError::WrongTargetKind { .. }));

        // A directory where a file was expected.
        let other_server_dir = base.path().join("other-server");
        tokio::fs::create_dir_all(other_server_dir.join("custom"))
            .await
            .unwrap();
        let file_err = resolve_confined_path(
            base.path(),
            "other-server",
            Path::new(""),
            Some(ConfinementTarget::File(OsStr::new("custom"))),
        )
        .await
        .unwrap_err();
        assert!(matches!(file_err, ConfinementError::WrongTargetKind { .. }));
    }

    /// Issue #491: two callers racing to resolve the same not-yet-existing segment directory
    /// must both succeed - the loser's `create_dir` failing with `AlreadyExists` must be
    /// tolerated and the winner's directory re-validated, not surfaced as a hard error.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_first_time_segment_creation_both_succeed() {
        let base = TempDir::new().unwrap();
        let base_a = base.path().to_path_buf();
        let base_b = base_a.clone();

        let task_a = tokio::spawn(async move {
            resolve_confined_path(&base_a, "my-server", Path::new(""), None).await
        });
        let task_b = tokio::spawn(async move {
            resolve_confined_path(&base_b, "my-server", Path::new(""), None).await
        });

        let (a, b) = tokio::join!(task_a, task_b);
        assert_eq!(a.unwrap().unwrap(), b.unwrap().unwrap());
    }

    /// Same race as above, one level deeper in the lenient `relative_dirs` walk.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_first_time_relative_dir_creation_both_succeed() {
        let base = TempDir::new().unwrap();
        tokio::fs::create_dir_all(base.path().join("my-server"))
            .await
            .unwrap();
        let base_a = base.path().to_path_buf();
        let base_b = base_a.clone();

        let task_a = tokio::spawn(async move {
            resolve_confined_path(&base_a, "my-server", Path::new("nested"), None).await
        });
        let task_b = tokio::spawn(async move {
            resolve_confined_path(&base_b, "my-server", Path::new("nested"), None).await
        });

        let (a, b) = tokio::join!(task_a, task_b);
        assert_eq!(a.unwrap().unwrap(), b.unwrap().unwrap());
    }

    /// `write_confined_file`'s check-then-write sequence must run inside a single
    /// `spawn_blocking` call so it is atomic with respect to the calling future being dropped
    /// (e.g. `rmcp` dropping `save_skill`'s handler future on client disconnect) - an
    /// async-native version built from `tokio::fs::OpenOptions` plus `AsyncWriteExt::write_all`
    /// has multiple `.await` points, any of which a dropped future can land between `O_TRUNC`
    /// truncating the file and the content being written, leaving a 0-byte file behind. Aborting
    /// the spawned task immediately after starting it and re-reading the file must therefore
    /// always observe either the untouched pre-existing content or the fully written new content
    /// - never a partial or empty file.
    ///
    /// A single iteration only lands the abort inside the vulnerable window often enough to
    /// discriminate the fix from the pre-fix async-await implementation about 60% of the time, so
    /// this loops several times rather than relying on one attempt.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn write_confined_file_survives_future_drop_without_partial_write() {
        const OLD: &[u8] = b"old-content-should-survive-or-be-fully-replaced";
        const NEW: &[u8] = b"new-content-0123456789";

        for _ in 0..8 {
            let base = TempDir::new().unwrap();
            let path = base.path().join("SKILL.md");
            tokio::fs::write(&path, OLD).await.unwrap();

            let spawn_path = path.clone();
            let handle = tokio::spawn(async move { write_confined_file(&spawn_path, NEW).await });
            std::thread::sleep(std::time::Duration::from_micros(1));
            handle.abort();
            let _ = handle.await;

            // Poll briefly rather than a flat settle sleep: a blocking task that was already
            // running when `abort` fired cannot be interrupted mid-flight, but a write this
            // small finishes in well under a millisecond once scheduled, so most iterations
            // don't need to wait at all.
            let mut content = tokio::fs::read(&path).await.unwrap();
            for _ in 0..50 {
                if content.as_slice() == OLD || content.as_slice() == NEW {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
                content = tokio::fs::read(&path).await.unwrap();
            }

            assert!(
                content.as_slice() == OLD || content.as_slice() == NEW,
                "partial/corrupt content observed: {content:?}"
            );
        }
    }

    #[tokio::test]
    async fn write_confined_file_creates_new_file() {
        let base = TempDir::new().unwrap();
        let path = base.path().join("SKILL.md");
        write_confined_file(&path, b"content").await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"content");
    }

    /// `write_confined_file` must preserve the overwrite semantics of a plain `tokio::fs::write`
    /// for a pre-existing regular file - only a symlink at the terminal component is rejected.
    #[tokio::test]
    async fn write_confined_file_overwrites_existing_regular_file() {
        let base = TempDir::new().unwrap();
        let path = base.path().join("SKILL.md");
        tokio::fs::write(&path, b"old").await.unwrap();
        write_confined_file(&path, b"new").await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"new");
    }

    /// Issue #496: a symlink planted at the confined path after `resolve_confined_path`'s own
    /// `ConfinementTarget::File` check (which deliberately leaves the terminal component
    /// uncreated) must not be followed by the write that lands there.
    #[tokio::test]
    #[cfg(unix)]
    async fn write_confined_file_rejects_a_symlink_planted_at_the_target() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("real.md");

        let confined_path = base.path().join("SKILL.md");
        std::os::unix::fs::symlink(&outside_file, &confined_path).unwrap();

        let err = write_confined_file(&confined_path, b"attacker-controlled")
            .await
            .unwrap_err();
        // Not asserting the specific errno here: `O_NOFOLLOW` on a symlink is `ELOOP` on
        // Linux/macOS, but other Unix flavors (e.g. FreeBSD) surface `EMLINK` instead. The
        // load-bearing assertions are that it's an I/O failure at all, and that the write never
        // reached the symlink's target.
        assert!(matches!(err, ConfinementError::Io(_)));
        assert!(!outside_file.exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_root_relative_intermediate_cannot_escape_base() {
        let base = TempDir::new().unwrap();
        let err = resolve_confined_path(base.path(), "my-server", Path::new(r"\pwn\evil"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ConfinementError::Escape { .. }));
    }
}
