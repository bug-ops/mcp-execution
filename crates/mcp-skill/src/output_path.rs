//! Confinement of caller-supplied `SKILL.md` output paths to a base directory.
//!
//! `save_skill` accepts an optional `output_path` from the caller. Without
//! confinement, a malicious or buggy caller could supply an absolute path,
//! a `..`-relative path, or a path that walks through a symlink planted
//! inside the base directory to write arbitrary files anywhere on disk
//! (see issue #184). [`resolve_skill_output_path`] mirrors the read-side
//! confinement already used by [`crate::scan_tools_directory`], adapted for
//! a target that may not exist yet.

use mcp_execution_core::sanitize_path_for_error;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Errors from resolving and confining a skill output path to its base directory.
#[derive(Debug, Error)]
pub enum OutputPathError {
    /// `server_id` is empty or is not a single plain path segment (e.g. it
    /// contains a `..`, `/`, or is otherwise not a bare directory name).
    #[error("server_id must be a single non-empty path segment: {server_id:?}")]
    InvalidServerId {
        /// The rejected server id.
        server_id: String,
    },

    /// The caller supplied an absolute `output_path`, which would override
    /// the base directory entirely.
    #[error("output_path must be relative to the skills directory, not absolute: {path}")]
    AbsolutePath {
        /// Sanitized display form of the rejected path.
        path: String,
    },

    /// The caller-supplied `output_path` contains a `..` component.
    #[error("output_path must not contain '..' components: {path}")]
    ParentTraversal {
        /// Sanitized display form of the rejected path.
        path: String,
    },

    /// The caller-supplied `output_path` has no file name component.
    #[error("output_path is not a valid file path: {path}")]
    InvalidPath {
        /// Sanitized display form of the rejected path.
        path: String,
    },

    /// `server_id`'s own directory already exists as a symlink, which is
    /// rejected outright regardless of where it points - including at a
    /// sibling server's own directory, which would otherwise pass a
    /// resolve-and-confine check (issue #217).
    #[error("server_id directory must not be a symlink: {path}")]
    ServerIdIsSymlink {
        /// Sanitized display form of the offending path.
        path: String,
    },

    /// The resolved path escapes the base directory, typically because a
    /// path component resolved through (or is itself) a symlink that points
    /// outside it.
    #[error("resolved path escapes the skills directory: {path}")]
    Escape {
        /// Sanitized display form of the path that escaped confinement.
        path: String,
    },

    /// A path component that must be a directory already exists as
    /// something else (e.g. a regular file).
    #[error("path component is not a directory: {path}")]
    NotADirectory {
        /// Sanitized display form of the offending component.
        path: String,
    },

    /// The resolved output path already exists as a directory.
    #[error("output path is a directory, not a file: {path}")]
    NotAFile {
        /// Sanitized display form of the offending path.
        path: String,
    },

    /// Creating a directory needed to hold the output file failed.
    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        /// Sanitized display form of the directory that could not be created.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// I/O error resolving the base directory itself.
    #[error("failed to resolve skills directory: {0}")]
    Io(#[from] std::io::Error),
}

/// Validates `server_id` is a single plain path segment: non-empty, and
/// with no `..`, path separator, or root/prefix component.
///
/// `server_id` is walked through the same confinement loop as
/// `output_path`'s directory components (see [`resolve_skill_output_path`]),
/// so a `..` or absolute `server_id` would eventually be caught there too —
/// this check rejects it earlier, before any filesystem work, and gives a
/// precise error rather than relying on the generic `Escape` path.
///
/// Returns the validated [`Component`] itself (borrowed from `server_id`),
/// rather than just `Ok(())`, so the caller pushes the exact component this
/// function parsed and approved instead of re-deriving one from the raw
/// string — which could diverge from this validation on an input like
/// `"a/."`, where `Path::components()` normalizes away the trailing `.` and
/// this function sees a single `Normal("a")`, but a fresh
/// `Component::Normal(OsStr::new("a/."))` would still carry the embedded
/// separator. Delegates to `mcp_execution_core::validate_path_segment`, shared
/// with `mcp-execution-server`'s equivalent check for `introspect_server`'s
/// `output_dir`, so both crates reject the same malformed `server_id` shapes.
fn validate_server_id_segment(server_id: &str) -> Result<Component<'_>, OutputPathError> {
    mcp_execution_core::validate_path_segment(server_id).ok_or_else(|| {
        OutputPathError::InvalidServerId {
            server_id: server_id.to_string(),
        }
    })
}

/// Validates a caller-supplied `output_path` and returns it as a safe,
/// base-relative path, or `SKILL.md` when none was supplied.
fn relative_target(output_path: Option<&Path>) -> Result<PathBuf, OutputPathError> {
    let Some(path) = output_path else {
        return Ok(PathBuf::from("SKILL.md"));
    };

    if path.is_absolute() {
        return Err(OutputPathError::AbsolutePath {
            path: sanitize_path_for_error(path),
        });
    }
    if mcp_execution_core::contains_parent_dir(path) {
        return Err(OutputPathError::ParentTraversal {
            path: sanitize_path_for_error(path),
        });
    }
    if path.file_name().is_none() {
        return Err(OutputPathError::InvalidPath {
            path: sanitize_path_for_error(path),
        });
    }

    Ok(path.to_path_buf())
}

/// Resolves a `save_skill` output path, confining it to `base_dir/server_id`.
///
/// `server_id` and `output_path` are both attacker-influenced, so both are
/// walked through the *same* checked component loop, rooted at `base_dir`
/// (canonicalized once, since `base_dir` itself is trusted, first-party
/// configuration rather than caller input): `server_id` is pushed as the
/// first component, followed by `output_path`'s directory components, if
/// any. `output_path`, when supplied, is treated as *relative* to
/// `base_dir/server_id`: an absolute path or a path containing a `..`
/// component is rejected before any filesystem work happens. `None`
/// resolves to the default `SKILL.md`. Confining to `base_dir/server_id`
/// (rather than just `base_dir`) matches the documented `save_skill`
/// contract, in the sense that both the default path and every accepted
/// `output_path` land under `server_id`'s own directory.
///
/// `server_id`'s own directory is resolved before anything else and is
/// rejected outright if it already exists as a symlink, regardless of where
/// it points: a resolve-and-confine check alone would accept a symlink from
/// `base_dir/server-b` to `base_dir/server-a`, since the target still
/// resolves under `base_dir` (issue #217). Every subsequent `output_path`
/// directory component is then confined to that resolved `server_id`
/// directory specifically, not merely to `base_dir` as a whole, so no
/// deeper component can reach a different server's directory either.
///
/// Each component is created and confinement-checked one at a time (rather
/// than via a single recursive create-and-canonicalize), so that a symlink
/// already present under `base_dir` when this call starts — whether *at*
/// `server_id` or at any deeper `output_path` directory component — is
/// resolved and rejected *before* this function creates anything under it
/// or descends into it. This is a check against pre-existing state, not a
/// concurrency guarantee — it does not defend against a symlink planted by
/// a racing process between this function's checks and the caller's
/// subsequent write. The final path component is rejected outright if it
/// already exists as a symlink, dangling or not: a dangling symlink can't
/// be resolved by `canonicalize`, but would still be followed by a
/// subsequent write, so it is checked with `symlink_metadata` instead.
///
/// # Errors
///
/// Returns [`OutputPathError`] if `server_id` is empty or not a single
/// plain path segment, if `output_path` is absolute, contains a `..`
/// component, or has no file name, if the resolved path escapes `base_dir`
/// (including via a symlink at `server_id` itself or any deeper
/// component), if a required directory could not be created, or if a path
/// component that must be a directory (or must hold the output file)
/// already exists as the wrong kind of entry.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::resolve_skill_output_path;
/// use std::path::Path;
///
/// # async fn example() -> Result<(), mcp_execution_skill::OutputPathError> {
/// let path =
///     resolve_skill_output_path(Path::new("/home/user/.claude/skills"), "github", None).await?;
/// println!("Resolved to {}", path.display());
/// # Ok(())
/// # }
/// ```
pub async fn resolve_skill_output_path(
    base_dir: &Path,
    server_id: &str,
    output_path: Option<&Path>,
) -> Result<PathBuf, OutputPathError> {
    let server_component = validate_server_id_segment(server_id)?;
    let relative = relative_target(output_path)?;

    tokio::fs::create_dir_all(base_dir)
        .await
        .map_err(|source| OutputPathError::CreateDir {
            path: sanitize_path_for_error(base_dir),
            source,
        })?;
    let canonical_root = tokio::fs::canonicalize(base_dir).await?;

    // Resolve server_id's own directory first, rejecting it outright if it already exists as
    // a symlink - regardless of where it points - rather than resolving and re-checking it as
    // the loop below does for deeper components. A symlink here could point at a *sibling*
    // server's own directory, which still resolves under `canonical_root` and would therefore
    // pass a resolve-and-confine check; only an unconditional rejection closes that gap
    // (issue #217). Every subsequent component is then confined to `server_dir` specifically,
    // not just to `canonical_root` as a whole, so a caller scoped to this `server_id` can never
    // reach another server's directory through a deeper `output_path` component either.
    let mut server_dir = canonical_root.clone();
    server_dir.push(server_component);
    if !server_dir.starts_with(&canonical_root) {
        return Err(OutputPathError::Escape {
            path: sanitize_path_for_error(&server_dir),
        });
    }
    match tokio::fs::symlink_metadata(&server_dir).await {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(OutputPathError::ServerIdIsSymlink {
                    path: sanitize_path_for_error(&server_dir),
                });
            }
            if !meta.is_dir() {
                return Err(OutputPathError::NotADirectory {
                    path: sanitize_path_for_error(&server_dir),
                });
            }
        }
        Err(_) => {
            tokio::fs::create_dir(&server_dir).await.map_err(|source| {
                OutputPathError::CreateDir {
                    path: sanitize_path_for_error(&server_dir),
                    source,
                }
            })?;
        }
    }

    let output_dir_components = relative
        .parent()
        .map(|parent| parent.components().collect::<Vec<_>>())
        .unwrap_or_default();

    let mut current = server_dir.clone();
    for component in output_dir_components {
        current.push(component);
        // Cheap for a `..`-free relative path pushed onto an absolute root (a
        // named component can't leave it), but load-bearing on Windows: a
        // root-without-prefix component like `\pwn` is not caught by
        // `Path::is_absolute` and would otherwise reach `create_dir` unconfined.
        if !current.starts_with(&server_dir) {
            return Err(OutputPathError::Escape {
                path: sanitize_path_for_error(&current),
            });
        }
        match tokio::fs::symlink_metadata(&current).await {
            Ok(_) => {
                let resolved = tokio::fs::canonicalize(&current).await?;
                if !resolved.starts_with(&server_dir) {
                    return Err(OutputPathError::Escape {
                        path: sanitize_path_for_error(&current),
                    });
                }
                if !tokio::fs::metadata(&resolved).await?.is_dir() {
                    return Err(OutputPathError::NotADirectory {
                        path: sanitize_path_for_error(&current),
                    });
                }
                current = resolved;
            }
            Err(_) => {
                tokio::fs::create_dir(&current).await.map_err(|source| {
                    OutputPathError::CreateDir {
                        path: sanitize_path_for_error(&current),
                        source,
                    }
                })?;
            }
        }
    }

    // `relative_target` already guarantees a file name is present; re-checked
    // here (rather than `.expect`-ed) so this function has no panic surface.
    let Some(file_name) = relative.file_name() else {
        return Err(OutputPathError::InvalidPath {
            path: sanitize_path_for_error(&relative),
        });
    };
    let final_path = current.join(file_name);

    if let Ok(meta) = tokio::fs::symlink_metadata(&final_path).await {
        // Reject unconditionally, independent of whether `canonicalize`
        // would succeed: a dangling symlink (target does not exist) makes
        // canonicalize fail, which must not be treated as "safe, doesn't
        // exist yet" — a subsequent write still follows the link.
        if meta.file_type().is_symlink() {
            return Err(OutputPathError::Escape {
                path: sanitize_path_for_error(&final_path),
            });
        }
        if meta.is_dir() {
            return Err(OutputPathError::NotAFile {
                path: sanitize_path_for_error(&final_path),
            });
        }
    }

    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn default_path_stays_within_base() {
        let base = TempDir::new().unwrap();
        let resolved = resolve_skill_output_path(base.path(), "my-server", None)
            .await
            .unwrap();
        let canonical_base = base.path().canonicalize().unwrap();
        assert!(resolved.starts_with(&canonical_base));
        assert_eq!(resolved, canonical_base.join("my-server").join("SKILL.md"));
    }

    #[tokio::test]
    async fn legitimate_relative_path_is_accepted() {
        let base = TempDir::new().unwrap();
        let resolved =
            resolve_skill_output_path(base.path(), "my-server", Some(Path::new("custom/SKILL.md")))
                .await
                .unwrap();
        let canonical_base = base.path().canonicalize().unwrap();
        assert!(resolved.starts_with(&canonical_base));
        assert_eq!(
            resolved,
            canonical_base
                .join("my-server")
                .join("custom")
                .join("SKILL.md")
        );
    }

    #[tokio::test]
    async fn empty_server_id_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_skill_output_path(base.path(), "", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::InvalidServerId { .. }));
        // Rejected before any filesystem work, so nothing was created inside
        // the (already-existing, since `TempDir::new` creates it) base dir.
        assert!(
            tokio::fs::read_dir(base.path())
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn server_id_with_parent_traversal_is_rejected() {
        let base = TempDir::new().unwrap();
        tokio::fs::create_dir_all(base.path().join("other-server"))
            .await
            .unwrap();

        let err = resolve_skill_output_path(base.path(), "../other-server", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::InvalidServerId { .. }));
    }

    #[tokio::test]
    async fn server_id_with_path_separator_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_skill_output_path(base.path(), "a/b", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::InvalidServerId { .. }));
    }

    #[tokio::test]
    async fn absolute_path_is_rejected() {
        let base = TempDir::new().unwrap();
        // A bare `/etc/passwd`-style path has no drive prefix, so
        // `Path::is_absolute()` is false for it on Windows (see
        // `windows_root_relative_path_cannot_escape_base` below, which
        // covers that case separately via the `Escape` variant); use a
        // path that is genuinely absolute on the current platform so this
        // test exercises `AbsolutePath` specifically.
        let absolute = if cfg!(windows) {
            r"C:\Windows\System32\config"
        } else {
            "/etc/passwd"
        };
        let err = resolve_skill_output_path(base.path(), "my-server", Some(Path::new(absolute)))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::AbsolutePath { .. }));
        assert!(!base.path().join("my-server").exists());
    }

    #[tokio::test]
    async fn parent_traversal_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_skill_output_path(
            base.path(),
            "my-server",
            Some(Path::new("../../etc/passwd")),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, OutputPathError::ParentTraversal { .. }));
        assert!(!base.path().join("my-server").exists());
    }

    #[tokio::test]
    async fn path_with_no_file_name_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_skill_output_path(base.path(), "my-server", Some(Path::new(".")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::InvalidPath { .. }));
    }

    #[tokio::test]
    async fn empty_output_path_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_skill_output_path(base.path(), "my-server", Some(Path::new("")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::InvalidPath { .. }));
    }

    // Windows path semantics differ enough from Unix (root-without-prefix
    // components, drive-relative paths) that the confinement guard needs
    // its own coverage rather than relying on the Unix-shaped tests above.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_root_relative_path_cannot_escape_base() {
        let base = TempDir::new().unwrap();
        // `is_absolute()` is false for a root-without-prefix path like this
        // on Windows, so it passes `relative_target`'s absolute-path check;
        // confinement must catch it via `starts_with` instead.
        let err =
            resolve_skill_output_path(base.path(), "my-server", Some(Path::new(r"\pwn\evil.md")))
                .await
                .unwrap_err();
        assert!(matches!(err, OutputPathError::Escape { .. }));
    }

    /// S4 regression: `base_dir/server_id` itself is a symlink to outside
    /// the base — planted before this call, e.g. by an earlier `save_skill`
    /// invocation or another process with write access to the base. Exercises
    /// the same loop iteration (`server_id` as the first walked component)
    /// used for `symlinked_parent_directory_escape_is_rejected` below, one
    /// level up.
    #[tokio::test]
    #[cfg(unix)]
    async fn symlinked_server_id_directory_escape_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();

        std::os::unix::fs::symlink(outside.path(), base.path().join("my-server")).unwrap();

        let err = resolve_skill_output_path(base.path(), "my-server", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::ServerIdIsSymlink { .. }));
        assert!(!outside.path().join("SKILL.md").exists());
    }

    /// #217 regression: a symlink at `server_id`'s own directory pointing at
    /// a *sibling* directory that lives inside the same base must still be
    /// rejected outright, not merely allowed because it resolves under the
    /// base as a whole.
    #[tokio::test]
    #[cfg(unix)]
    async fn symlinked_server_id_directory_to_sibling_is_rejected() {
        let base = TempDir::new().unwrap();
        tokio::fs::create_dir_all(base.path().join("server-a"))
            .await
            .unwrap();
        std::os::unix::fs::symlink(base.path().join("server-a"), base.path().join("server-b"))
            .unwrap();

        let err = resolve_skill_output_path(base.path(), "server-b", None)
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::ServerIdIsSymlink { .. }));
        assert!(!base.path().join("server-a").join("SKILL.md").exists());
    }

    /// S2 regression guard, now running through the unified walk shared
    /// with the `server_id` (S4) and final-component (S1) checks.
    #[tokio::test]
    #[cfg(unix)]
    async fn symlinked_parent_directory_escape_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();

        // Plant the symlink *inside* the confinement root (base/my-server),
        // matching the threat model: a symlink already present there when
        // `save_skill` is called.
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(outside.path(), server_dir.join("escape")).unwrap();

        let err =
            resolve_skill_output_path(base.path(), "my-server", Some(Path::new("escape/SKILL.md")))
                .await
                .unwrap_err();
        assert!(matches!(err, OutputPathError::Escape { .. }));

        // Nothing should have been written outside the base directory.
        assert!(!outside.path().join("SKILL.md").exists());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlinked_file_escape_is_rejected() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("real.md");
        tokio::fs::write(&outside_file, "outside").await.unwrap();

        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(&outside_file, server_dir.join("SKILL.md")).unwrap();

        let err = resolve_skill_output_path(base.path(), "my-server", Some(Path::new("SKILL.md")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::Escape { .. }));
    }

    /// S1 regression guard, now running through the unified walk shared
    /// with the `server_id` (S4) and intermediate-component (S2) checks.
    #[tokio::test]
    #[cfg(unix)]
    async fn dangling_symlink_at_final_component_is_rejected() {
        // Regression for the bypass where `canonicalize` fails on a symlink
        // whose target doesn't exist, and the old code fell through to
        // `Ok(final_path)` — a subsequent write would follow the link
        // outside the base directory.
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let dangling_target = outside.path().join("does-not-exist.md");

        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(&dangling_target, server_dir.join("SKILL.md")).unwrap();

        let err = resolve_skill_output_path(base.path(), "my-server", Some(Path::new("SKILL.md")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::Escape { .. }));
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

        let err = resolve_skill_output_path(
            base.path(),
            "my-server",
            Some(Path::new("not-a-dir/SKILL.md")),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, OutputPathError::NotADirectory { .. }));
    }

    #[tokio::test]
    async fn final_path_that_is_an_existing_directory_is_rejected() {
        let base = TempDir::new().unwrap();
        let server_dir = base.path().join("my-server");
        tokio::fs::create_dir_all(server_dir.join("SKILL.md"))
            .await
            .unwrap();

        let err = resolve_skill_output_path(base.path(), "my-server", Some(Path::new("SKILL.md")))
            .await
            .unwrap_err();
        assert!(matches!(err, OutputPathError::NotAFile { .. }));
    }
}
