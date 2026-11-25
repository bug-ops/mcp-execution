//! Home-directory resolution shared by every crate.
//!
//! [`HomeDir`] is the single place the workspace learns the user's home directory, so `HOME`
//! (Unix) and `USERPROFILE` (Windows) are honoured identically by the CLI, the MCP server and
//! the file exporter, and tests can redirect the home through the environment on every platform.
//! [`expand_home`] resolves a leading `~`, and [`ServersDir`] / [`SkillsDir`] resolve the base
//! directories of generated servers and skills against it.
//!
//! Calling `dirs::home_dir` or `std::env::home_dir` directly elsewhere is banned by the
//! workspace `clippy.toml`: `dirs::home_dir` ignores both variables on Windows.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::ServerId;

/// Directory of generated servers, relative to the home directory.
const SERVERS_DIR_RELATIVE: [&str; 2] = [".claude", "servers"];

/// Directory of generated skills, relative to the home directory.
const SKILLS_DIR_RELATIVE: [&str; 2] = [".claude", "skills"];

/// The current user's home directory, guaranteed to be an absolute path.
///
/// A relative home would make every path derived from it depend on the working directory, so
/// [`HomeDir::from_env`] refuses one instead of passing it on.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::HomeDir;
///
/// // Fails only for a relative home or, on Windows, an unknown one.
/// if let Ok(home) = HomeDir::from_env() {
///     assert!(home.as_path().is_absolute());
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeDir(PathBuf);

/// Failure to determine a usable home directory.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::HomeDirError;
///
/// assert_eq!(
///     HomeDirError::Unknown.to_string(),
///     "cannot determine the home directory"
/// );
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HomeDirError {
    /// The environment and the account database name no home directory.
    #[error("cannot determine the home directory")]
    Unknown,
    /// The home directory is a relative path.
    #[error("the home directory '{}' is not an absolute path", path.display())]
    NotAbsolute {
        /// The relative value the environment supplied.
        path: PathBuf,
    },
    /// The path starts with `~name`, another user's home, which is never looked up.
    #[error(
        "a '~name' path (another user's home directory) is not supported; use '~/...' or an absolute path, or './~name' for a directory with that literal name"
    )]
    UnsupportedUserHome {
        /// The rejected path as the caller wrote it.
        path: PathBuf,
    },
}

impl HomeDir {
    /// Resolves the home directory of the current user.
    ///
    /// On Unix this is a non-empty `HOME`, else the account database entry; on Windows a
    /// non-empty `USERPROFILE`, else the profile directory of the account. The filesystem is
    /// not touched.
    ///
    /// # Errors
    ///
    /// Returns [`HomeDirError::Unknown`] when no source names a home directory, and
    /// [`HomeDirError::NotAbsolute`] when the one found is a relative path.
    pub fn from_env() -> Result<Self, HomeDirError> {
        #[expect(
            clippy::disallowed_methods,
            reason = "the single sanctioned home lookup; std honours HOME and USERPROFILE (#634, #604)"
        )]
        let home = std::env::home_dir();
        Self::from_path(home.ok_or(HomeDirError::Unknown)?)
    }

    /// Returns the home directory.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    fn from_path(path: PathBuf) -> Result<Self, HomeDirError> {
        if path.is_absolute() {
            Ok(Self(path))
        } else {
            Err(HomeDirError::NotAbsolute { path })
        }
    }
}

/// Replaces a leading `~` component with the home directory.
///
/// Only a first component that is exactly `~` is expanded: `~`, `~/x` and, on Windows, `~\x`.
/// A first component that starts with `~` followed by more characters (`~user/x`) is rejected:
/// the user database is never consulted, and passing it on would create a literal relative
/// directory named `~user`. Every other path is returned unchanged without reading the home, so
/// an absolute path works even when the home is unusable.
///
/// # Errors
///
/// Returns [`HomeDirError::UnsupportedUserHome`] for a `~name` path, and another
/// [`HomeDirError`] when `path` starts with `~` and the home directory is unknown or relative.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use mcp_execution_core::{HomeDirError, expand_home};
///
/// assert_eq!(expand_home(Path::new("/srv/x")).unwrap(), Path::new("/srv/x"));
/// assert!(matches!(
///     expand_home(Path::new("~user/x")),
///     Err(HomeDirError::UnsupportedUserHome { .. })
/// ));
/// ```
pub fn expand_home(path: &Path) -> Result<PathBuf, HomeDirError> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(first)) if first == OsStr::new("~") => {
            let home = HomeDir::from_env()?;
            let rest = components.as_path();
            Ok(if rest.is_empty() {
                home.0
            } else {
                home.0.join(rest)
            })
        }
        Some(Component::Normal(first)) if first.as_encoded_bytes().starts_with(b"~") => {
            Err(HomeDirError::UnsupportedUserHome {
                path: path.to_path_buf(),
            })
        }
        _ => Ok(path.to_path_buf()),
    }
}

/// Resolves `custom` through [`expand_home`], or `relative` under the home directory.
fn resolve_under_home(
    custom: Option<PathBuf>,
    relative: [&str; 2],
) -> Result<PathBuf, HomeDirError> {
    if let Some(path) = custom {
        return expand_home(&path);
    }
    let [first, second] = relative;
    Ok(HomeDir::from_env()?.0.join(first).join(second))
}

/// Base directory that holds one generated TypeScript package per server.
///
/// Every command that reads or writes generated servers resolves its `--servers-dir` option
/// through this type, so `~` expansion and the default location stay identical everywhere.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ServerId, ServersDir};
///
/// let dir = ServersDir::resolve(Some("/srv/servers".into())).unwrap();
/// let server = ServerId::new("github").unwrap();
/// assert_eq!(dir.server(&server), std::path::Path::new("/srv/servers/github"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServersDir(PathBuf);

impl ServersDir {
    /// Resolves `custom` through [`expand_home`], or the default `~/.claude/servers`.
    ///
    /// # Errors
    ///
    /// Returns a [`HomeDirError`] when the home directory is needed and unknown or relative.
    pub fn resolve(custom: Option<PathBuf>) -> Result<Self, HomeDirError> {
        resolve_under_home(custom, SERVERS_DIR_RELATIVE).map(Self)
    }

    /// Directory of the generated package of `server`.
    #[must_use]
    pub fn server(&self, server: &ServerId) -> PathBuf {
        self.0.join(server.as_str())
    }

    /// The base directory itself.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Base directory that holds one skill directory per server.
///
/// Resolved like [`ServersDir`], with the default `~/.claude/skills`.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ServerId, SkillsDir};
///
/// let dir = SkillsDir::resolve(Some("/srv/skills".into())).unwrap();
/// let server = ServerId::new("github").unwrap();
/// assert_eq!(dir.skill(&server), std::path::Path::new("/srv/skills/github"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsDir(PathBuf);

impl SkillsDir {
    /// Resolves `custom` through [`expand_home`], or the default `~/.claude/skills`.
    ///
    /// # Errors
    ///
    /// Returns a [`HomeDirError`] when the home directory is needed and unknown or relative.
    pub fn resolve(custom: Option<PathBuf>) -> Result<Self, HomeDirError> {
        resolve_under_home(custom, SKILLS_DIR_RELATIVE).map(Self)
    }

    /// Skill directory of `server`.
    #[must_use]
    pub fn skill(&self, server: &ServerId) -> PathBuf {
        self.0.join(server.as_str())
    }

    /// The base directory itself.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn from_path_accepts_only_an_absolute_path() {
        let home = std::env::current_dir().unwrap();
        assert_eq!(HomeDir::from_path(home.clone()).unwrap().as_path(), home);
        assert_matches!(
            HomeDir::from_path(PathBuf::from("relative/home")),
            Err(HomeDirError::NotAbsolute { .. })
        );
    }

    #[test]
    fn not_absolute_error_names_the_path() {
        let error = HomeDirError::NotAbsolute {
            path: PathBuf::from("relative/home"),
        };
        assert!(error.to_string().contains("relative/home"));
    }

    #[test]
    fn expand_home_leaves_a_path_without_a_leading_tilde_alone() {
        for path in ["/abs/x", "rel/x", "x/~", ".~", "./~user"] {
            assert_eq!(
                expand_home(Path::new(path)).unwrap(),
                PathBuf::from(path),
                "{path}"
            );
        }
    }

    #[test]
    fn expand_home_rejects_a_tilde_name_first_component() {
        for path in ["~user", "~user/x", "~~", "~."] {
            assert_matches!(
                expand_home(Path::new(path)),
                Err(HomeDirError::UnsupportedUserHome { .. }),
                "{path}"
            );
        }
    }

    #[test]
    fn unsupported_user_home_error_does_not_echo_the_path() {
        let error = HomeDirError::UnsupportedUserHome {
            path: PathBuf::from("~zzq/x"),
        };
        assert!(!error.to_string().contains("zzq"));
    }

    #[test]
    fn unsupported_user_home_hint_names_a_form_that_expand_home_accepts() {
        let message = HomeDirError::UnsupportedUserHome {
            path: PathBuf::from("~zzq"),
        }
        .to_string();
        assert!(message.contains("./~name"));
        assert_eq!(
            expand_home(Path::new("./~name")).unwrap(),
            PathBuf::from("./~name")
        );
    }

    #[test]
    fn server_and_skill_join_the_server_id() {
        let server = ServerId::new("github").unwrap();
        let servers = ServersDir::resolve(Some("/s".into())).unwrap();
        let skills = SkillsDir::resolve(Some("/k".into())).unwrap();
        assert_eq!(servers.server(&server), Path::new("/s").join("github"));
        assert_eq!(skills.skill(&server), Path::new("/k").join("github"));
    }
}
