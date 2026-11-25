//! Node.js detection shared by `setup` and the script runner.
//!
//! Generated tools and saved scripts are TypeScript files run directly by Node.js, so both
//! commands need the same rule for "this Node.js can run a `.ts` file without a flag".

use anyhow::{Context, Result, anyhow};
use tokio::process::Command;

use super::probe::{CaptureError, PROBE_TIMEOUT, capped_stdout};

/// Parsed Node.js version, as printed by `node --version`.
///
/// Exists so the "can run `.ts` files unflagged" rule lives in one typed place
/// instead of an ad-hoc major-version comparison.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::node::NodeVersion;
///
/// let version = NodeVersion::parse("v22.18.0").unwrap();
/// assert!(version.runs_typescript_natively());
/// assert_eq!(version.major(), 22);
/// assert!(!NodeVersion::parse("22.17.1").unwrap().runs_typescript_natively());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl NodeVersion {
    /// First release on each release line that runs `.ts` files without a flag, ascending by
    /// major. Every later major runs them too. The check and the message both derive from this.
    const FIRST_NATIVE_TS: [Self; 2] = [
        Self {
            major: 22,
            minor: 18,
            patch: 0,
        },
        Self {
            major: 23,
            minor: 6,
            patch: 0,
        },
    ];

    /// Human-readable minimum shown in messages, e.g. `22.18.0 (23.6.0 on the 23 line)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::node::NodeVersion;
    ///
    /// assert_eq!(NodeVersion::minimum_text(), "22.18.0 (23.6.0 on the 23 line)");
    /// ```
    #[must_use]
    pub fn minimum_text() -> String {
        let [first, second] = Self::FIRST_NATIVE_TS;
        format!("{first} ({second} on the {} line)", second.major)
    }

    /// Parses `v22.18.0` or `22.18.0`; a pre-release suffix on the patch is ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::node::NodeVersion;
    ///
    /// assert!(NodeVersion::parse("24.1.2-nightly2025").is_some());
    /// assert!(NodeVersion::parse("garbage").is_none());
    /// ```
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let mut parts = raw.trim().trim_start_matches('v').split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch_digits: String = parts
            .next()?
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let patch = patch_digits.parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Major version number.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::node::NodeVersion;
    ///
    /// assert_eq!(NodeVersion::parse("26.1.2").unwrap().major(), 26);
    /// ```
    #[must_use]
    pub const fn major(self) -> u32 {
        self.major
    }

    /// Whether Node.js executes `.ts` files without `--experimental-strip-types`: at or after
    /// the first native release of its own line (see `FIRST_NATIVE_TS`), or on a later
    /// major than any listed line.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::node::NodeVersion;
    ///
    /// assert!(NodeVersion::parse("23.6.0").unwrap().runs_typescript_natively());
    /// assert!(!NodeVersion::parse("23.5.0").unwrap().runs_typescript_natively());
    /// ```
    #[must_use]
    pub fn runs_typescript_natively(self) -> bool {
        let newest_listed = Self::FIRST_NATIVE_TS[Self::FIRST_NATIVE_TS.len() - 1].major;
        self.major > newest_listed
            || Self::FIRST_NATIVE_TS
                .iter()
                .any(|first| first.major == self.major && self >= *first)
    }
}

impl std::fmt::Display for NodeVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Checks Node.js version requirement.
///
/// Verifies that a Node.js able to run generated `.ts` files without a flag
/// is installed (22.18.0 or later; 23.6.0 or later on the 23 line) and
/// accessible, and returns its version string without the leading `v`.
///
/// `node --version` gets 10 seconds. It runs as a process tree that is killed on timeout and
/// when the returned future is dropped, so a hanging `node` shim leaves nothing behind.
///
/// # Errors
///
/// Returns error if:
/// - Node.js command not found in PATH
/// - `node --version` does not answer within 10 seconds
/// - Node.js version cannot be determined
/// - Node.js cannot execute `.ts` files without a flag
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::node::check_node_version;
///
/// # async fn example() -> anyhow::Result<()> {
/// let version = check_node_version().await?;
/// println!("Node.js {version}");
/// # Ok(())
/// # }
/// ```
pub async fn check_node_version() -> Result<String> {
    let mut command = Command::new("node");
    command.arg("--version");
    let output = tokio::time::timeout(PROBE_TIMEOUT, capped_stdout(command))
        .await
        .with_context(|| {
            format!(
                "Node.js did not answer `node --version` within {} s",
                PROBE_TIMEOUT.as_secs()
            )
        })?
        .map_err(node_failure)?;

    let version_str = String::from_utf8_lossy(&output);
    let version_str = version_str.trim().trim_start_matches('v');
    let version = NodeVersion::parse(version_str).context("Failed to parse Node.js version")?;

    if !version.runs_typescript_natively() {
        anyhow::bail!(
            "Node.js version {version_str} cannot run generated TypeScript tools.\n\
             \n\
             Required: Node.js {minimum} or higher\n\
             Current:  Node.js {version_str}\n\
             \n\
             Please upgrade Node.js:\n\
             - Download: https://nodejs.org\n\
             - Or use nvm: nvm install 22",
            minimum = NodeVersion::minimum_text(),
        );
    }

    Ok(version.to_string())
}

fn node_failure(failure: CaptureError) -> anyhow::Error {
    match failure {
        CaptureError::Start(error) => anyhow!(error).context(format!(
            "Node.js not found in PATH.\n\
             \n\
             Node.js {}+ is required to run generated TypeScript tools.\n\
             Install from: https://nodejs.org\n\
             \n\
             Or use a version manager:\n\
             - nvm: https://github.com/nvm-sh/nvm\n\
             - fnm: https://github.com/Schniz/fnm",
            NodeVersion::FIRST_NATIVE_TS[0]
        )),
        failure @ (CaptureError::Read(_) | CaptureError::Wait(_) | CaptureError::Exit(_)) => {
            anyhow!(failure).context("Node.js is installed but not working correctly")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(unix, windows))]
    #[test]
    fn node_failure_tells_a_missing_node_from_a_broken_one() {
        #[cfg(unix)]
        let status = std::os::unix::process::ExitStatusExt::from_raw(3 << 8);
        #[cfg(windows)]
        let status = std::os::windows::process::ExitStatusExt::from_raw(3);

        let missing = node_failure(CaptureError::Start(std::io::ErrorKind::NotFound.into()));
        assert!(
            missing.to_string().starts_with("Node.js not found in PATH"),
            "{missing:#}"
        );
        let broken = node_failure(CaptureError::Exit(status));
        assert_eq!(
            broken.to_string(),
            "Node.js is installed but not working correctly"
        );
    }

    #[test]
    fn node_version_floor_matches_native_typescript_support() {
        let runs = |raw: &str| NodeVersion::parse(raw).unwrap().runs_typescript_natively();
        for rejected in [
            "18.20.4", "20.19.0", "21.7.3", "22.6.0", "22.17.1", "23.0.0", "23.5.0",
        ] {
            assert!(!runs(rejected), "{rejected} must be rejected");
        }
        for accepted in [
            "22.18.0", "22.20.1", "23.6.0", "23.11.0", "24.0.0", "26.1.2",
        ] {
            assert!(runs(accepted), "{accepted} must be accepted");
        }
    }

    #[test]
    fn node_version_minimum_text_and_check_share_the_constants() {
        assert_eq!(
            NodeVersion::minimum_text(),
            "22.18.0 (23.6.0 on the 23 line)"
        );
        for first in NodeVersion::FIRST_NATIVE_TS {
            assert!(first.runs_typescript_natively(), "{first} must be accepted");
            let before = NodeVersion {
                minor: first.minor - 1,
                patch: 99,
                ..first
            };
            assert!(
                !before.runs_typescript_natively(),
                "{before} must be rejected"
            );
        }
    }

    #[test]
    fn node_version_parses_prefix_and_prerelease() {
        assert_eq!(
            NodeVersion::parse("v22.18.0"),
            Some(NodeVersion {
                major: 22,
                minor: 18,
                patch: 0
            })
        );
        assert_eq!(
            NodeVersion::parse("24.1.2-nightly2025").map(|v| v.patch),
            Some(2)
        );
        assert_eq!(NodeVersion::parse("22.18"), None);
        assert_eq!(NodeVersion::parse("garbage"), None);
    }

    #[tokio::test]
    async fn test_check_node_version() {
        // Passes when a suitable Node.js is installed; otherwise the error must be helpful
        let result = check_node_version().await;

        // We can't assert success because Node.js might not be installed
        // in CI environment, but we can verify error messages are helpful
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("Node.js") || error_msg.contains("version"),
                "Error message should be helpful: {error_msg}"
            );
        }
    }
}
