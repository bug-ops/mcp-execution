//! Path sanitization and validation shared by path-confinement checks across the workspace.
//!
//! Confinement checks in `mcp-execution-skill` (`save_skill`'s `output_path`) and
//! `mcp-execution-server` (`introspect_server`'s `output_dir`) report the offending path back
//! to the caller. [`sanitize_path_for_error`] is the one place that redaction happens, so both
//! crates report errors with the same privacy guarantee. [`validate_path_segment`] is the one
//! place both crates validate a caller-supplied `server_id` as a single plain path component,
//! so a `..` or path-separator smuggled into it is rejected identically by both.

use std::path::{Component, Path};

/// Sanitizes a file path for inclusion in an error message, to prevent information disclosure.
///
/// Replaces the home directory with `~` to avoid leaking usernames and full filesystem paths
/// in error messages returned to callers (e.g. over the MCP protocol).
///
/// # Examples
///
/// ```
/// use mcp_execution_core::sanitize_path_for_error;
/// use std::path::Path;
///
/// // A path outside the home directory is left unchanged.
/// assert_eq!(sanitize_path_for_error(Path::new("/tmp/x")), "/tmp/x");
///
/// // A path under the home directory has it redacted to `~`.
/// let home = dirs::home_dir().expect("home dir available in this environment");
/// let under_home = home.join("secret-file.md");
/// assert_eq!(
///     sanitize_path_for_error(&under_home),
///     format!("~{}secret-file.md", std::path::MAIN_SEPARATOR),
/// );
/// ```
#[must_use]
pub fn sanitize_path_for_error(path: &Path) -> String {
    dirs::home_dir().map_or_else(
        || path.display().to_string(),
        |home| {
            let path_str = path.display().to_string();
            path_str.replace(&home.display().to_string(), "~")
        },
    )
}

/// Validates that `segment` is a single plain path component: non-empty, and with no `..`,
/// path separator, or root/prefix component.
///
/// Intended for validating a caller-supplied identifier (e.g. `server_id`) that will be pushed
/// onto a confined base directory: constructing a fresh `Component::Normal` from the raw
/// string instead of using the one this function returns would defeat the check on an input
/// like `"a/."`, where `Path::components()` normalizes away the trailing `.` and this function
/// sees a single `Normal("a")`, but a fresh `Component::Normal(OsStr::new("a/."))` would still
/// carry the embedded separator. Callers should push the returned [`Component`] itself.
///
/// Returns `None` (rather than an error) so each caller can report the failure in its own
/// crate-specific error type with whatever context it has (e.g. which parameter was invalid).
///
/// # Examples
///
/// ```
/// use mcp_execution_core::validate_path_segment;
///
/// assert!(validate_path_segment("my-server").is_some());
/// assert!(validate_path_segment("").is_none());
/// assert!(validate_path_segment("..").is_none());
/// assert!(validate_path_segment("a/b").is_none());
/// ```
#[must_use]
pub fn validate_path_segment(segment: &str) -> Option<Component<'_>> {
    let mut components = Path::new(segment).components();
    match (components.next(), components.next()) {
        (Some(component @ Component::Normal(_)), None) => Some(component),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_path_segment_accepts_plain_name() {
        assert!(validate_path_segment("my-server").is_some());
    }

    #[test]
    fn validate_path_segment_rejects_empty() {
        assert!(validate_path_segment("").is_none());
    }

    #[test]
    fn validate_path_segment_rejects_parent_traversal() {
        assert!(validate_path_segment("../other").is_none());
        assert!(validate_path_segment("..").is_none());
    }

    #[test]
    fn validate_path_segment_rejects_path_separator() {
        assert!(validate_path_segment("a/b").is_none());
    }

    #[test]
    fn sanitize_path_for_error_redacts_home_directory() {
        let home = dirs::home_dir().unwrap();
        let under_home = home.join(".claude").join("skills");
        assert_eq!(
            sanitize_path_for_error(&under_home),
            format!(
                "~{}.claude{}skills",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            )
        );
    }

    #[test]
    fn sanitize_path_for_error_leaves_non_home_path_unchanged() {
        assert_eq!(sanitize_path_for_error(Path::new("/tmp/x")), "/tmp/x");
    }
}
