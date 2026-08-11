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

use mcp_execution_core::untrusted::sanitize_untrusted_inline;
use mcp_execution_core::{
    ConfinementError, ConfinementTarget, resolve_confined_path, sanitize_path_for_error,
    validate_server_id_slug,
};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Errors from resolving and confining an `introspect_server` output directory.
#[derive(Debug, Error)]
pub enum OutputDirError {
    /// `server_id` failed [`mcp_execution_core::validate_server_id_slug`]'s slug-format check
    /// (e.g. it is empty, too long, or contains a character other than a lowercase ASCII
    /// letter, digit, or hyphen). `source` carries the precise reason; the message is derived
    /// from it rather than hardcoded, so it can't independently drift from the actual rule
    /// enforced.
    #[error("invalid server_id {server_id:?}: {source}")]
    InvalidServerId {
        /// Sanitized display form of the rejected server id (see
        /// [`mcp_execution_core::untrusted::sanitize_untrusted_inline`]). The `{server_id:?}`
        /// (`Debug`) formatting above is a required second layer of defense on top of that
        /// sanitization, not incidental — see
        /// [`mcp_execution_core::ServerIdError`]'s doc comment for why it must not be
        /// "simplified" to `{server_id}` (`Display`).
        server_id: String,
        /// The specific slug-format violation.
        #[source]
        source: mcp_execution_core::ServerIdSlugError,
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

/// Maps the shared [`ConfinementError`] onto [`OutputDirError`]'s own, pre-existing variant set.
///
/// This is a 1:1 rename, not a lossy union: [`ConfinementError::WrongTargetKind`] becomes
/// [`OutputDirError::NotADirectory`] here (the terminal component `resolve_output_dir` walks is
/// always a directory), which is the only variant this crate names differently than
/// `mcp-execution-skill`'s equivalent `From` impl.
///
/// [`ConfinementError::InvalidSegment`] doesn't carry a [`mcp_execution_core::ServerIdSlugError`]
/// (the underlying walk only knows the looser [`mcp_execution_core::validate_path_segment`]
/// rule), so this re-derives one by re-running [`validate_server_id_slug`] on the rejected
/// segment. `resolve_output_dir` already validates with `validate_server_id_slug` before this
/// path can be reached, so in practice that second call always fails the same way the first one
/// did — `unwrap_or` never actually falls back — but doing it this way keeps this `From` impl
/// itself panic-free instead of `expect`-ing an invariant that only holds because of a check made
/// somewhere else.
impl From<ConfinementError> for OutputDirError {
    fn from(err: ConfinementError) -> Self {
        match err {
            ConfinementError::InvalidSegment { segment } => {
                let source = validate_server_id_slug(&segment)
                    .err()
                    .unwrap_or(mcp_execution_core::ServerIdSlugError::InvalidCharacters);
                Self::InvalidServerId {
                    server_id: segment,
                    source,
                }
            }
            ConfinementError::SegmentIsSymlink { path } => Self::ServerDirIsSymlink { path },
            ConfinementError::Escape { path } => Self::Escape { path },
            ConfinementError::NotADirectory { path }
            | ConfinementError::WrongTargetKind { path } => Self::NotADirectory { path },
            ConfinementError::CreateDir { path, source } => Self::CreateDir { path, source },
            ConfinementError::Io(source) => Self::Io(source),
        }
    }
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
///
/// # Examples
///
/// ```
/// use mcp_execution_server::relative_subpath;
/// use std::path::Path;
///
/// // No override: returns empty path (use server_id's directory directly)
/// let result = relative_subpath(None).unwrap();
/// assert!(result.as_os_str().is_empty());
///
/// // Relative subdirectory: valid and accepted
/// let result = relative_subpath(Some(Path::new("nested/custom"))).unwrap();
/// assert_eq!(result.to_str().unwrap(), "nested/custom");
///
/// // Absolute path: rejected. `Path::is_absolute()` requires a drive prefix on Windows, so
/// // the path used here must be genuinely absolute on the current platform.
/// let absolute = if cfg!(windows) { r"C:\Windows\System32\config" } else { "/etc/config" };
/// let err = relative_subpath(Some(Path::new(absolute))).unwrap_err();
/// assert!(err.to_string().contains("absolute"));
///
/// // Parent traversal: rejected
/// let err = relative_subpath(Some(Path::new("../../etc"))).unwrap_err();
/// assert!(err.to_string().contains(".."));
/// ```
pub fn relative_subpath(output_dir: Option<&Path>) -> Result<PathBuf, OutputDirError> {
    let Some(path) = output_dir else {
        return Ok(PathBuf::new());
    };

    if path.is_absolute() {
        return Err(OutputDirError::AbsolutePath {
            path: sanitize_path_for_error(path),
        });
    }
    if mcp_execution_core::contains_parent_dir(path) {
        return Err(OutputDirError::ParentTraversal {
            path: sanitize_path_for_error(path),
        });
    }

    Ok(path.to_path_buf())
}

/// Resolves an `introspect_server` output directory, confining it to `base_dir/server_id`.
///
/// `server_id` is validated against the same slug rule entry validation gates with
/// ([`validate_server_id_slug`]) **before** `output_dir` is checked at all, matching this crate's
/// pre-#395 error precedence: a caller supplying both an invalid `server_id` and an invalid
/// `output_dir` sees [`OutputDirError::InvalidServerId`], not a relative-path error. `server_id`
/// and `output_dir`'s directory components are then walked and confinement-checked by the shared
/// [`resolve_confined_path`], which defensively re-validates `server_id` itself as its first step
/// (against the looser structural [`mcp_execution_core::validate_path_segment`] rule) - even
/// though it is already validated twice by the time it gets there
/// (`service::introspect_server`'s `validate_server_id_slug` charset check, then the early check
/// this function performs) - so `resolve_confined_path` stays self-defending against a
/// `server_id` that reaches it some other way, or after a future change loosens one of the
/// caller-side checks. `output_dir`, when supplied, is treated
/// as *relative* to `base_dir/server_id`: an absolute path or a path containing a `..` component
/// is rejected by [`relative_subpath`] before any filesystem work happens. `None` resolves to
/// `base_dir/server_id` itself.
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
/// Returns [`OutputDirError`] if `server_id` fails [`validate_server_id_slug`], if `output_dir`
/// is absolute or contains a `..` component, if `server_id`'s own directory already exists as a
/// symlink, if the resolved path escapes `base_dir`, if a required directory could not be
/// created, or if a path component that must be a directory already exists as the wrong kind of
/// entry.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_server::{resolve_output_dir, OutputDirError};
/// use std::path::Path;
///
/// # async fn example() -> Result<(), OutputDirError> {
/// let base_dir = Path::new("/home/user/.claude/servers");
///
/// // Resolve with no subdirectory override (uses server_id's directory directly)
/// let resolved = resolve_output_dir(base_dir, "github", None).await?;
/// println!("Resolved to {}", resolved.display());
///
/// // Resolve with custom subdirectory
/// let resolved = resolve_output_dir(base_dir, "github", Some(Path::new("custom"))).await?;
/// println!("Resolved to {}", resolved.display());
/// # Ok(())
/// # }
/// ```
pub async fn resolve_output_dir(
    base_dir: &Path,
    server_id: &str,
    output_dir: Option<&Path>,
) -> Result<PathBuf, OutputDirError> {
    // Validated again, I/O-free, before `relative_subpath` so an invalid `server_id` is reported
    // even when `output_dir` is also invalid - matching this crate's pre-#395 error precedence
    // (`resolve_confined_path` re-validates it a second time, against the looser structural rule,
    // as part of its own walk).
    validate_server_id_slug(server_id).map_err(|source| OutputDirError::InvalidServerId {
        server_id: sanitize_untrusted_inline(server_id),
        source,
    })?;

    let relative = relative_subpath(output_dir)?;

    let sub_components: Vec<Component<'_>> = relative.components().collect();
    let (relative_dirs, target) = match sub_components.split_last() {
        // No output_dir override: the target directory is server_id's own directory.
        None => (PathBuf::new(), None),
        // Final component: the directory `export_to_filesystem` later publishes into via an
        // atomic staged rename, so it is confinement-checked but deliberately not created.
        Some((last, init)) => (
            init.iter().copied().collect::<PathBuf>(),
            Some(ConfinementTarget::Directory(last.as_os_str())),
        ),
    };

    resolve_confined_path(base_dir, server_id, &relative_dirs, target)
        .await
        .map_err(Into::into)
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

    /// When both `server_id` and `output_dir` are invalid, `InvalidServerId` must win - matching
    /// the error precedence this function had before the #395 confinement-walk extraction.
    #[tokio::test]
    async fn invalid_server_id_takes_precedence_over_invalid_output_dir() {
        let base = TempDir::new().unwrap();
        let absolute = if cfg!(windows) {
            r"C:\Windows\System32\config"
        } else {
            "/etc"
        };
        let err = resolve_output_dir(base.path(), "", Some(Path::new(absolute)))
            .await
            .unwrap_err();
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

    /// #401 regression: a `server_id` that is a valid, path-segment-safe `ServerId` but not
    /// slug-shaped (uppercase letters) must be rejected here too, not just at
    /// `service::introspect_server`'s entry gate — entry validation and confinement must agree
    /// on the exact same rule.
    #[tokio::test]
    async fn server_id_with_uppercase_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "My-Server", None)
            .await
            .unwrap_err();
        // Asserts on the specific reason, not just the outer variant, so a regression that
        // rejects "My-Server" for the wrong cause (or with stale wording) is still caught.
        assert!(matches!(
            err,
            OutputDirError::InvalidServerId {
                source: mcp_execution_core::ServerIdSlugError::InvalidCharacters,
                ..
            }
        ));
    }

    /// #401 regression: same as above, for an underscore (also a valid `ServerId` character but
    /// not a valid slug character).
    #[tokio::test]
    async fn server_id_with_underscore_is_rejected() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "my_server", None)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            OutputDirError::InvalidServerId {
                source: mcp_execution_core::ServerIdSlugError::InvalidCharacters,
                ..
            }
        ));
    }

    /// Issue #450: printable-but-disallowed characters (`&`, `<`, `>`) in a rejected `server_id`
    /// must not reach the error message unescaped — `validate_server_id_slug` rejects every
    /// character outside `[a-z0-9-]`, so any of these reaches `InvalidServerId` directly.
    #[tokio::test]
    async fn server_id_with_hostile_characters_is_escaped_in_error() {
        let base = TempDir::new().unwrap();
        for (candidate, escaped) in [
            ("server&name", "server&amp;name"),
            ("server<name", "server&lt;name"),
            ("server>name", "server&gt;name"),
        ] {
            let err = resolve_output_dir(base.path(), candidate, None)
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
    async fn server_id_with_emoji_is_left_unchanged_in_error() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "server\u{1F600}name", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("server\u{1F600}name"));
    }

    /// A legitimate non-ASCII script character carries no structural or injection risk either,
    /// so — like the emoji above — it must survive `sanitize_untrusted_inline` unchanged, even
    /// though `validate_server_id_slug`'s `[a-z0-9-]` charset still rejects it.
    #[tokio::test]
    async fn server_id_with_non_ascii_script_is_left_unchanged_in_error() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "café_日本語", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("café_日本語"));
    }

    /// `sanitize_untrusted_inline` deliberately leaves U+200D (ZERO WIDTH JOINER) untouched (see
    /// its own doc comment), so `InvalidServerId`'s stored `server_id` field still carries a raw
    /// ZWJ. This is only safe because `{server_id:?}` (`Debug`) formatting escapes it to
    /// `\u{200d}` rather than emitting it verbatim — this test pins that second layer of
    /// defense, mirroring `ServerIdError`'s equivalent regression test.
    #[tokio::test]
    async fn server_id_with_zwj_is_debug_escaped_in_error() {
        let base = TempDir::new().unwrap();
        let err = resolve_output_dir(base.path(), "server\u{200D}name", None)
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
