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
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

use crate::path::{sanitize_path_for_error, validate_path_segment};

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
    /// neither created nor canonicalized.
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
        /// The rejected segment.
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
/// by a racing process between this function's checks and the caller's subsequent write.
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
            segment: segment.to_string(),
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

/// Resolves and confines `segment`'s own directory under `canonical_root`, rejecting it outright
/// if it already exists as a symlink rather than resolving and re-checking it (see
/// [`resolve_confined_path`]'s doc comment for why). Creates the directory if it doesn't exist
/// yet.
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
        if meta.file_type().is_symlink() {
            return Err(ConfinementError::SegmentIsSymlink {
                path: sanitize_path_for_error(&segment_dir),
            });
        }
        if !meta.is_dir() {
            return Err(ConfinementError::NotADirectory {
                path: sanitize_path_for_error(&segment_dir),
            });
        }
    } else {
        tokio::fs::create_dir(&segment_dir)
            .await
            .map_err(|source| ConfinementError::CreateDir {
                path: sanitize_path_for_error(&segment_dir),
                source,
            })?;
    }
    Ok(segment_dir)
}

/// Confines `current` to `segment_dir`, resolving (and confirming) an existing symlink rather
/// than rejecting it outright, or creating the directory if it's missing.
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
        Ok(_) => {
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
        Err(_) => {
            tokio::fs::create_dir(&current)
                .await
                .map_err(|source| ConfinementError::CreateDir {
                    path: sanitize_path_for_error(current),
                    source,
                })
        }
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
