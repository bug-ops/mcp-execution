//! Home-directory resolution shared by every crate (#637, #634, #604).
//!
//! Stub: signatures only, behaviour lands in the implementation phase.

use std::path::{Path, PathBuf};

/// Absolute home directory of the current user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeDir(PathBuf);

/// Failure to determine a usable home directory.
#[derive(Debug, thiserror::Error)]
pub enum HomeDirError {
    /// The home directory could not be determined.
    #[error("cannot determine the home directory")]
    Unknown,
    /// The home directory is not an absolute path.
    #[error("the home directory is not an absolute path: {}", path.display())]
    NotAbsolute {
        /// The offending home value.
        path: PathBuf,
    },
}

impl HomeDir {
    /// Resolves the home directory of the current user.
    ///
    /// # Errors
    ///
    /// Returns [`HomeDirError`] when the home is unknown or not absolute.
    pub fn from_env() -> Result<Self, HomeDirError> {
        todo!()
    }

    /// Returns the home directory path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Expands a leading `~` component into the home directory.
///
/// # Errors
///
/// Returns [`HomeDirError`] when home is needed and unusable.
pub fn expand_home(_path: &Path) -> Result<PathBuf, HomeDirError> {
    todo!()
}

/// Directory holding generated server trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServersDir(PathBuf);

impl ServersDir {
    /// Resolves the servers directory from an optional custom path.
    ///
    /// # Errors
    ///
    /// Returns [`HomeDirError`] when home is needed and unusable.
    pub fn resolve(_custom: Option<PathBuf>) -> Result<Self, HomeDirError> {
        todo!()
    }

    /// Returns the directory path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Directory holding generated skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsDir(PathBuf);

impl SkillsDir {
    /// Resolves the skills directory from an optional custom path.
    ///
    /// # Errors
    ///
    /// Returns [`HomeDirError`] when home is needed and unusable.
    pub fn resolve(_custom: Option<PathBuf>) -> Result<Self, HomeDirError> {
        todo!()
    }

    /// Returns the directory path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
