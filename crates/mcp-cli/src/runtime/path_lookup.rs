//! Executable lookup on `PATH` that ignores relative entries.
//!
//! The generated bridge resolves the helper binary the same way (absolute `PATH` entries only,
//! plain file check), so `setup` and `run` never report a binary the bridge would not start. A
//! relative entry would resolve against the current directory, which can be an untrusted
//! checkout.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Finds `stem` on `PATH`, looking only in absolute entries.
///
/// On Unix the file name is `stem`; on Windows it is `stem` plus each of `windows_suffixes` in
/// order (for example `[".exe"]`, or `[".cmd", ".exe"]`).
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::path_lookup::find_on_path;
///
/// if let Some(tsc) = find_on_path("tsc", &[".cmd", ".exe"]) {
///     println!("{}", tsc.display());
/// }
/// ```
#[must_use]
pub fn find_on_path(stem: &str, windows_suffixes: &[&str]) -> Option<PathBuf> {
    find_in(
        std::env::var_os("PATH")?.as_os_str(),
        stem,
        windows_suffixes,
        Path::is_file,
    )
}

fn find_in(
    path_value: &OsStr,
    stem: &str,
    windows_suffixes: &[&str],
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let file_names: Vec<String> = if cfg!(windows) {
        windows_suffixes
            .iter()
            .map(|suffix| format!("{stem}{suffix}"))
            .collect()
    } else {
        vec![stem.to_string()]
    };
    std::env::split_paths(path_value)
        .filter(|entry| entry.is_absolute())
        .flat_map(|entry| file_names.iter().map(move |name| entry.join(name)))
        .find(|candidate| is_file(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute(path: &str) -> String {
        if cfg!(windows) {
            format!(r"C:\{path}")
        } else {
            format!("/{path}")
        }
    }

    fn joined(entries: &[String]) -> std::ffi::OsString {
        std::env::join_paths(entries).unwrap()
    }

    #[test]
    fn relative_and_empty_entries_are_ignored() {
        let path = joined(&[
            "relative-bin".to_string(),
            String::new(),
            ".".to_string(),
            absolute("trusted"),
        ]);

        let found = find_in(&path, "tool", &[".exe"], |_| true).unwrap();

        assert!(
            found.starts_with(absolute("trusted")),
            "{}",
            found.display()
        );
    }

    #[test]
    fn nothing_is_found_when_only_relative_entries_hold_the_file() {
        let path = joined(&["relative-bin".to_string()]);

        assert_eq!(find_in(&path, "tool", &[".exe"], |_| true), None);
    }

    #[test]
    fn the_first_absolute_entry_with_the_file_wins() {
        let path = joined(&[absolute("first"), absolute("second")]);

        let found = find_in(&path, "tool", &[".exe"], |p| {
            p.starts_with(absolute("second"))
        })
        .unwrap();

        assert!(found.starts_with(absolute("second")));
    }
}
