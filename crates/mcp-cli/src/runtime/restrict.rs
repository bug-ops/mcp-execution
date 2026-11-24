//! Optional `--restrict`: narrows what the script process may touch via Node's permission model.
//!
//! Unix only: file reads are limited to the script, the servers directory and the run directory;
//! no file writes, child processes or workers are granted, so a script cannot spawn processes
//! either. Network is narrowed on Node 25 and later only (the permission model does not cover it
//! on Node 22 to 24); the inherited helper channel needs no network grant. Windows refuses the
//! flag. Reads the script imports from other locations are denied by design.

use std::ffi::OsString;
use std::path::Path;

use anyhow::Result;
use mcp_execution_core::Error as CoreError;

fn unsupported(reason: &str) -> anyhow::Error {
    anyhow::Error::new(CoreError::InvalidArgument(reason.to_string()))
}

/// Builds the node arguments that enforce `--restrict`.
///
/// # Errors
///
/// Returns an invalid-argument error on platforms that do not support `--restrict` (Windows),
/// and when a path contains a comma, which the permission flag's list syntax cannot carry.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::restrict::permission_args;
/// use std::path::Path;
///
/// let args = permission_args(
///     Path::new("script.ts"),
///     Path::new("/home/user/.claude/servers"),
///     Path::new("/state/runs/abc"),
/// );
/// assert_eq!(args.is_ok(), cfg!(unix));
/// ```
pub fn permission_args(script: &Path, servers_dir: &Path, run_dir: &Path) -> Result<Vec<OsString>> {
    if !cfg!(unix) {
        return Err(unsupported("--restrict is not supported on this platform"));
    }
    let paths = [script, servers_dir, run_dir];
    if paths
        .iter()
        .any(|path| path.to_string_lossy().contains(','))
    {
        return Err(unsupported(
            "--restrict cannot grant access to a path that contains a comma",
        ));
    }
    let mut args = vec![OsString::from("--permission")];
    // Node 26 no longer splits one flag on commas, so every path gets its own flag.
    for path in paths {
        let mut flag = OsString::from("--allow-fs-read=");
        flag.push(path);
        args.push(flag);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn grants_reads_of_exactly_the_three_paths_and_nothing_else() {
        let args = permission_args(Path::new("/a.ts"), Path::new("/s"), Path::new("/r")).unwrap();
        assert_eq!(
            args,
            [
                "--permission",
                "--allow-fs-read=/a.ts",
                "--allow-fs-read=/s",
                "--allow-fs-read=/r",
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_path_with_a_comma() {
        assert!(permission_args(Path::new("/a,b.ts"), Path::new("/s"), Path::new("/r")).is_err());
    }

    #[cfg(not(unix))]
    #[test]
    fn refuses_every_request_off_unix() {
        assert!(permission_args(Path::new("a"), Path::new("s"), Path::new("r")).is_err());
    }
}
