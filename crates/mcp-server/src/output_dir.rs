//! Confinement of caller-supplied `introspect_server` output directories to a base directory.
//!
//! `introspect_server` accepts an optional `output_dir` from the caller. Without confinement,
//! an absolute path, a `..`-relative path, or a path that walks through a symlink planted
//! inside the base directory let a caller redirect the entire generated file tree - written
//! later by `save_categorized_tools` via
//! [`mcp_execution_files::FileSystem::export_to_filesystem`] - anywhere the process can write
//! (see issue #216). [`resolve_output_dir`] mirrors the confinement model
//! `mcp_execution_skill::resolve_skill_output_path` uses for `save_skill`'s `output_path`,
//! adapted for a directory target that the caller's later export publishes atomically rather
//! than a file this module writes itself.
//!
//! [`resolve_output_dir`] does real filesystem work (creating directories, following and
//! rejecting symlinks), so it is called only once, from `save_categorized_tools`, immediately
//! before `export_to_filesystem` runs. `introspect_server` calls the cheaper, I/O-free
//! [`relative_subpath`] instead, purely to reject an obviously malformed `output_dir` (absolute,
//! or containing `..`) with fast feedback: it neither touches the filesystem nor commits to a
//! resolved path, so a caller cannot use `introspect_server` alone - without a corresponding
//! `save_categorized_tools` call - to populate `~/.claude/servers/` with directories, and the
//! confinement check that matters runs as close to the actual write as this two-step protocol
//! allows.

use mcp_execution_core::sanitize_path_for_error;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Errors from resolving and confining an `introspect_server` output directory.
#[derive(Debug, Error)]
pub enum OutputDirError {
    /// `server_id` is empty or is not a single plain path segment (e.g. it contains a `..`,
    /// `/`, or is otherwise not a bare directory name).
    #[error("server_id must be a single non-empty path segment: {server_id:?}")]
    InvalidServerId {
        /// The rejected server id.
        server_id: String,
    },

    /// The caller supplied an absolute `output_dir`, which would override the base directory
    /// entirely.
    #[error("output_dir must be relative to the servers directory, not absolute: {path}")]
    AbsolutePath {
        /// Sanitized display form of the rejected path.
        path: String,
    },

    /// The caller-supplied `output_dir` contains a `..` component.
    #[error("output_dir must not contain '..' components: {path}")]
    ParentTraversal {
        /// Sanitized display form of the rejected path.
        path: String,
    },

    /// `server_id`'s own directory already exists as a symlink, which is rejected outright
    /// regardless of where it points - including at a sibling server's own directory, which
    /// would otherwise pass a resolve-and-confine check (see issue #217's equivalent fix for
    /// `save_skill`).
    #[error("server_id directory must not be a symlink: {path}")]
    ServerDirIsSymlink {
        /// Sanitized display form of the offending path.
        path: String,
    },

    /// The resolved path escapes the base directory, typically because a path component
    /// resolved through (or is itself) a symlink that points outside it.
    #[error("resolved path escapes the servers directory: {path}")]
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

    /// Creating a directory needed along the path failed.
    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        /// Sanitized display form of the directory that could not be created.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// I/O error resolving the base directory itself.
    #[error("failed to resolve servers directory: {0}")]
    Io(#[from] std::io::Error),
}

/// Validates `server_id` is a single plain path segment: non-empty, and with no `..`, path
/// separator, or root/prefix component.
///
/// `service::introspect_server` already validates `server_id` via `validate_server_id` (a
/// tighter lowercase-letter/digit/hyphen charset check) before either this function or
/// [`resolve_output_dir`] ever runs, but this function does not trust that: it delegates to
/// `mcp_execution_core::validate_path_segment`, shared with `mcp-execution-skill`'s equivalent
/// check for `save_skill`'s `server_id`, so `resolve_output_dir` is self-defending against a
/// `server_id` that reaches it some other way (or after a future change loosens the
/// caller-side check) rather than only being safe today because `introspect_server` happens to
/// validate first.
fn validate_server_id_component(server_id: &str) -> Result<Component<'_>, OutputDirError> {
    mcp_execution_core::validate_path_segment(server_id).ok_or_else(|| {
        OutputDirError::InvalidServerId {
            server_id: server_id.to_string(),
        }
    })
}

/// Validates a caller-supplied `output_dir` and returns it as a safe, base-relative path, or
/// an empty path (meaning "no subdirectory, use `server_id`'s directory directly") when none
/// was supplied.
///
/// Does no filesystem I/O: `introspect_server` calls this alone (discarding the returned path)
/// just to reject an absolute or `..`-containing `output_dir` early, without creating anything
/// or committing to a resolved target. [`resolve_output_dir`] calls it again as the first step
/// of its own, filesystem-touching walk.
///
/// Unlike `mcp_execution_skill`'s `relative_target` (the equivalent function for `save_skill`'s
/// file target), an empty path or `.` is accepted here rather than rejected: the target this
/// function's caller ultimately resolves is a *directory*, so "no subdirectory override" is a
/// legitimate result on its own, not an incomplete one missing a file name.
///
/// # Errors
///
/// Returns [`OutputDirError::AbsolutePath`] if `output_dir` is absolute, or
/// [`OutputDirError::ParentTraversal`] if it contains a `..` component.
pub fn relative_subpath(output_dir: Option<&Path>) -> Result<PathBuf, OutputDirError> {
    let Some(path) = output_dir else {
        return Ok(PathBuf::new());
    };

    if path.is_absolute() {
        return Err(OutputDirError::AbsolutePath {
            path: sanitize_path_for_error(path),
        });
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(OutputDirError::ParentTraversal {
            path: sanitize_path_for_error(path),
        });
    }

    Ok(path.to_path_buf())
}

/// Confines `current` to `root`, rejecting it outright if it already exists as a symlink -
/// regardless of where it points - rather than resolving and re-checking it. Creates the
/// directory if it doesn't exist yet.
///
/// Used only for the `server_id` component: a symlink there could point at a sibling server's
/// own directory, which still resolves under `root` and would therefore pass a resolve-and-
/// confine check. Rejecting any pre-existing symlink at this exact component closes that gap
/// (the same one tracked for `save_skill` in issue #217) rather than merely narrowing it.
async fn resolve_component_strict(current: &Path, root: &Path) -> Result<(), OutputDirError> {
    if !current.starts_with(root) {
        return Err(OutputDirError::Escape {
            path: sanitize_path_for_error(current),
        });
    }
    match tokio::fs::symlink_metadata(current).await {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(OutputDirError::ServerDirIsSymlink {
                    path: sanitize_path_for_error(current),
                });
            }
            if !meta.is_dir() {
                return Err(OutputDirError::NotADirectory {
                    path: sanitize_path_for_error(current),
                });
            }
            Ok(())
        }
        Err(_) => {
            tokio::fs::create_dir(current)
                .await
                .map_err(|source| OutputDirError::CreateDir {
                    path: sanitize_path_for_error(current),
                    source,
                })
        }
    }
}

/// Confines `current` to `root`, resolving (and confirming) an existing symlink rather than
/// rejecting it outright, or creating the directory if it's missing.
///
/// Used for `output_dir` subdirectory components other than `server_id`, mirroring
/// `resolve_skill_output_path`'s intermediate-component handling: a symlink already present
/// here is followed only if it still resolves inside `root`.
async fn resolve_component_lenient(
    current: &mut PathBuf,
    root: &Path,
) -> Result<(), OutputDirError> {
    if !current.starts_with(root) {
        return Err(OutputDirError::Escape {
            path: sanitize_path_for_error(current),
        });
    }
    match tokio::fs::symlink_metadata(&current).await {
        Ok(_) => {
            let resolved = tokio::fs::canonicalize(&current).await?;
            if !resolved.starts_with(root) {
                return Err(OutputDirError::Escape {
                    path: sanitize_path_for_error(current),
                });
            }
            if !tokio::fs::metadata(&resolved).await?.is_dir() {
                return Err(OutputDirError::NotADirectory {
                    path: sanitize_path_for_error(current),
                });
            }
            *current = resolved;
            Ok(())
        }
        Err(_) => {
            tokio::fs::create_dir(&current)
                .await
                .map_err(|source| OutputDirError::CreateDir {
                    path: sanitize_path_for_error(current),
                    source,
                })
        }
    }
}

/// Resolves an `introspect_server` output directory, confining it to `base_dir/server_id`.
///
/// `server_id` is validated as a single plain path segment before anything else (defensively -
/// see [`validate_server_id_component`]) and then pushed onto `base_dir`. `output_dir`, when
/// supplied, is treated as *relative* to `base_dir/server_id`: an absolute path or a path
/// containing a `..` component is rejected before any filesystem work happens. `None` resolves
/// to `base_dir/server_id` itself.
///
/// Every directory component up to (but not including) the final target - `server_id`'s own
/// directory, plus any but the last directory component of `output_dir` - is confinement-
/// checked and created eagerly, exactly like `resolve_skill_output_path`. The final resolved
/// directory is confinement-checked but deliberately **not** created:
/// [`mcp_execution_files::FileSystem::export_to_filesystem`] publishes it atomically via a
/// staged rename, and forcing it to exist first would defeat that atomicity on a first-time
/// `generate`. It is still rejected outright if it already exists as a symlink, dangling or
/// not, since a subsequent export would follow it.
///
/// Unlike an earlier version of this fix, this function is *not* called from `introspect_server`
/// (which only runs the I/O-free [`relative_subpath`] check). It is called fresh by
/// `save_categorized_tools`, immediately before `create_dir_all` and `export_to_filesystem` run,
/// rather than once at `introspect_server` time with the result cached on the session for up to
/// [`crate::types::PendingGeneration::DEFAULT_TIMEOUT_MINUTES`] minutes: caching it would leave a
/// window in which a symlink planted after resolution but before export is never re-checked, and
/// would also mean directories get created for any `introspect_server` call regardless of
/// whether a matching `save_categorized_tools` call ever follows. Calling it immediately before
/// export still does not defend against a symlink planted *during* this function's own walk, or
/// between this call returning and `export_to_filesystem` actually running - `save_categorized_tools`
/// acquires a per-target export lock in between (see `GeneratorService::export_lock_for`), which
/// can block for the duration of a concurrent same-target export. That gap is still bounded by
/// this request's own lifetime rather than an entire session's, and is the same disclaimed
/// racing-process threat model as `resolve_skill_output_path`, not a reopening of the TOCTOU this
/// function exists to close.
///
/// # Errors
///
/// Returns [`OutputDirError`] if `server_id` is empty or not a single plain path segment, if
/// `output_dir` is absolute or contains a `..` component, if `server_id`'s own directory already
/// exists as a symlink, if the resolved path escapes `base_dir`, if a required directory could
/// not be created, or if a path component that must be a directory already exists as the wrong
/// kind of entry.
pub async fn resolve_output_dir(
    base_dir: &Path,
    server_id: &str,
    output_dir: Option<&Path>,
) -> Result<PathBuf, OutputDirError> {
    let server_component = validate_server_id_component(server_id)?;
    let relative = relative_subpath(output_dir)?;

    tokio::fs::create_dir_all(base_dir)
        .await
        .map_err(|source| OutputDirError::CreateDir {
            path: sanitize_path_for_error(base_dir),
            source,
        })?;
    let canonical_root = tokio::fs::canonicalize(base_dir).await?;

    let mut server_dir = canonical_root.clone();
    server_dir.push(server_component);
    resolve_component_strict(&server_dir, &canonical_root).await?;

    let sub_components: Vec<Component<'_>> = relative.components().collect();
    let Some((last, init)) = sub_components.split_last() else {
        // No output_dir override: the target directory is server_dir itself.
        return Ok(server_dir);
    };

    let mut current = server_dir.clone();
    for component in init {
        current.push(component);
        resolve_component_lenient(&mut current, &server_dir).await?;
    }

    // Final component: the directory `export_to_filesystem` later publishes into via an
    // atomic staged rename, so it is confinement-checked but deliberately not created here.
    current.push(last);
    if !current.starts_with(&server_dir) {
        return Err(OutputDirError::Escape {
            path: sanitize_path_for_error(&current),
        });
    }
    if let Ok(meta) = tokio::fs::symlink_metadata(&current).await {
        if meta.file_type().is_symlink() {
            return Err(OutputDirError::Escape {
                path: sanitize_path_for_error(&current),
            });
        }
        let resolved = tokio::fs::canonicalize(&current).await?;
        if !resolved.starts_with(&server_dir) {
            return Err(OutputDirError::Escape {
                path: sanitize_path_for_error(&current),
            });
        }
        if !meta.is_dir() {
            return Err(OutputDirError::NotADirectory {
                path: sanitize_path_for_error(&current),
            });
        }
        current = resolved;
    }

    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn default_path_stays_within_base() {
        let base = TempDir::new().unwrap();
        let resolved = resolve_output_dir(base.path(), "my-server", None)
            .await
            .unwrap();
        let canonical_base = base.path().canonicalize().unwrap();
        assert_eq!(resolved, canonical_base.join("my-server"));
    }

    #[tokio::test]
    async fn legitimate_relative_subdir_is_accepted() {
        let base = TempDir::new().unwrap();
        let resolved =
            resolve_output_dir(base.path(), "my-server", Some(Path::new("nested/custom")))
                .await
                .unwrap();
        let canonical_base = base.path().canonicalize().unwrap();
        assert_eq!(
            resolved,
            canonical_base
                .join("my-server")
                .join("nested")
                .join("custom")
        );
        // The final component is deliberately not created.
        assert!(!resolved.exists());
        // But its parent chain is.
        assert!(resolved.parent().unwrap().is_dir());
    }

    #[tokio::test]
    async fn regeneration_into_an_existing_directory_is_accepted() {
        let base = TempDir::new().unwrap();
        let canonical_base = base.path().canonicalize().unwrap();
        tokio::fs::create_dir_all(canonical_base.join("my-server"))
            .await
            .unwrap();

        let resolved = resolve_output_dir(base.path(), "my-server", None)
            .await
            .unwrap();
        assert_eq!(resolved, canonical_base.join("my-server"));
    }

    #[tokio::test]
    async fn absolute_output_dir_is_rejected() {
        let base = TempDir::new().unwrap();
        // A bare `/etc`-style path has no drive prefix, so `Path::is_absolute()` is false
        // for it on Windows (see `windows_root_relative_path_cannot_escape_base` below,
        // which covers that case separately via the `Escape` variant); use a path that is
        // genuinely absolute on the current platform so this test exercises `AbsolutePath`
        // specifically.
        let absolute = if cfg!(windows) {
            r"C:\Windows\System32\config"
        } else {
            "/etc"
        };
        let err = resolve_output_dir(base.path(), "my-server", Some(Path::new(absolute)))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::AbsolutePath { .. }));
        assert!(!base.path().join("my-server").exists());
    }

    #[tokio::test]
    async fn parent_traversal_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "my-server", Some(Path::new("../../etc")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::ParentTraversal { .. }));
        assert!(!base.path().join("my-server").exists());
    }

    /// Windows path semantics differ enough from Unix (root-without-prefix components,
    /// drive-relative paths) that the confinement guard needs its own coverage rather than
    /// relying on the Unix-shaped tests above.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_root_relative_path_cannot_escape_base() {
        let base = TempDir::new().unwrap();
        // `is_absolute()` is false for a root-without-prefix path like this on Windows, so it
        // passes `relative_subpath`'s absolute-path check; confinement must catch it via
        // `starts_with` instead.
        let err = resolve_output_dir(base.path(), "my-server", Some(Path::new(r"\pwn\evil")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::Escape { .. }));
    }

    #[tokio::test]
    async fn empty_server_id_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "", None).await.unwrap_err();
        assert!(matches!(err, OutputDirError::InvalidServerId { .. }));
    }

    #[tokio::test]
    async fn server_id_with_parent_traversal_is_rejected() {
        let base = TempDir::new().unwrap();
        tokio::fs::create_dir_all(base.path().join("other-server"))
            .await
            .unwrap();

        let err = resolve_output_dir(base.path(), "../other-server", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::InvalidServerId { .. }));
    }

    #[tokio::test]
    async fn server_id_with_path_separator_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "a/b", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::InvalidServerId { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlinked_server_id_directory_escape_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();

        std::os::unix::fs::symlink(outside.path(), base.path().join("my-server")).unwrap();

        let err = resolve_output_dir(base.path(), "my-server", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::ServerDirIsSymlink { .. }));
    }

    /// #217-equivalent regression: a symlink at `server_id`'s own directory pointing at a
    /// *sibling* directory that lives inside the same base must still be rejected outright,
    /// not merely allowed because it resolves under the base.
    #[tokio::test]
    #[cfg(unix)]
    async fn symlinked_server_id_directory_to_sibling_is_rejected() {
        let base = TempDir::new().unwrap();
        tokio::fs::create_dir_all(base.path().join("server-a"))
            .await
            .unwrap();
        std::os::unix::fs::symlink(base.path().join("server-a"), base.path().join("server-b"))
            .unwrap();

        let err = resolve_output_dir(base.path(), "server-b", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::ServerDirIsSymlink { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn server_id_directory_that_is_a_regular_file_is_rejected() {
        let base = TempDir::new().unwrap();
        tokio::fs::write(base.path().join("my-server"), "oops")
            .await
            .unwrap();

        let err = resolve_output_dir(base.path(), "my-server", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::NotADirectory { .. }));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlink_loop_is_rejected() {
        let base = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        // A symlink whose (relative) target is its own name loops on itself.
        std::os::unix::fs::symlink("a", server_dir.join("a")).unwrap();

        let err = resolve_output_dir(base.path(), "my-server", Some(Path::new("a/custom")))
            .await
            .unwrap_err();
        // `canonicalize` fails with `ELOOP`, surfaced as `Io` rather than `Escape` since the
        // loop is detected before any confinement comparison is possible.
        assert!(matches!(err, OutputDirError::Io(_)));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlinked_intermediate_output_dir_component_escape_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();

        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(outside.path(), server_dir.join("escape")).unwrap();

        let err = resolve_output_dir(base.path(), "my-server", Some(Path::new("escape/custom")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::Escape { .. }));
        assert!(!outside.path().join("custom").exists());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn dangling_symlink_at_final_component_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let dangling_target = outside.path().join("does-not-exist");

        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(&dangling_target, server_dir.join("custom")).unwrap();

        let err = resolve_output_dir(base.path(), "my-server", Some(Path::new("custom")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::Escape { .. }));
        assert!(!dangling_target.exists());
    }

    #[tokio::test]
    async fn intermediate_component_that_is_a_regular_file_is_rejected() {
        let base = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        tokio::fs::write(server_dir.join("not-a-dir"), "oops")
            .await
            .unwrap();

        let err = resolve_output_dir(
            base.path(),
            "my-server",
            Some(Path::new("not-a-dir/custom")),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, OutputDirError::NotADirectory { .. }));
    }

    #[tokio::test]
    async fn final_component_that_is_a_regular_file_is_rejected() {
        let base = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        tokio::fs::write(server_dir.join("custom"), "oops")
            .await
            .unwrap();

        let err = resolve_output_dir(base.path(), "my-server", Some(Path::new("custom")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputDirError::NotADirectory { .. }));
    }

    /// A rejection partway through a multi-component `output_dir` does not roll back
    /// components the walk already confirmed - this walk creates and confines one component at
    /// a time rather than staging the whole chain and publishing atomically (the same
    /// trade-off `resolve_skill_output_path` makes for `save_skill`). Documented here as an
    /// accepted, tested property rather than an implicit gap: the surviving directory is empty
    /// and itself correctly confined, and reaching it at all now requires a completed
    /// `introspect_server` round trip plus a `save_categorized_tools` call, not just probing
    /// `introspect_server` in isolation.
    #[tokio::test]
    #[cfg(unix)]
    async fn rejected_deep_path_leaves_earlier_components_but_nothing_outside_base() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        let kept_dir = server_dir.join("kept");

        // `kept` is created by a genuine prior `resolve_output_dir` call - the same code path
        // a legitimate `save_categorized_tools` regeneration would take - rather than seeded
        // directly by the test, so the assertion below exercises the walk's own
        // create-and-move-on behavior instead of merely observing test scaffolding.
        resolve_output_dir(base.path(), "my-server", Some(Path::new("kept/first")))
            .await
            .unwrap();
        assert!(kept_dir.is_dir());

        std::os::unix::fs::symlink(outside.path(), kept_dir.join("escape")).unwrap();

        let err = resolve_output_dir(
            base.path(),
            "my-server",
            Some(Path::new("kept/escape/custom")),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, OutputDirError::Escape { .. }));

        // `kept`, created by the earlier call, is untouched by this call's rejection...
        assert!(kept_dir.is_dir());
        // ...but nothing was created past the rejection, and nothing escaped the base.
        assert!(!kept_dir.join("escape").join("custom").exists());
        assert!(!outside.path().join("custom").exists());
    }
}
