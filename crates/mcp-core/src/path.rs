//! Path sanitization and validation shared by path-confinement checks across the workspace.
//!
//! Confinement checks in `mcp-execution-skill` (`save_skill`'s `output_path`) and
//! `mcp-execution-server` (`introspect_server`'s `output_dir`) report the offending path back
//! to the caller. [`sanitize_path_for_error`] is the one place that redaction happens, so both
//! crates report errors with the same privacy guarantee. [`validate_path_segment`] backs
//! `ServerId::new`/`ToolName::new`'s own baseline path-segment invariant; the stricter,
//! filesystem-safe-*slug* rule both crates' `server_id` confinement checks enforce lives in
//! [`crate::validate_server_id_slug`] instead (see its own doc comment for why).

use std::path::{Component, Path};

/// Sanitizes a file path for inclusion in an error message, to prevent information disclosure.
///
/// Replaces the home directory with `~` to avoid leaking usernames and full filesystem paths
/// in error messages returned to callers (e.g. over the MCP protocol). Note that the rebuilt
/// suffix is composed from normalized path components, so incidental input artifacts such as
/// repeated separators or `.` segments are not preserved verbatim.
///
/// The comparison walks path components rather than matching raw strings, so a `/`-separated
/// input matches a backslash-separated home directory (and vice versa) on platforms where both
/// separators are valid. On Windows and macOS, components are also compared
/// Unicode-case-insensitively (via [`str::to_lowercase`]) and Unicode-normalization-insensitively
/// (via NFC normalization), matching those platforms' case-insensitive-but-case-preserving
/// filesystem semantics and the fact that the same visible username can arrive pre-composed
/// (NFC, e.g. `"Jos\u{e9}"`) from one source and decomposed (NFD, e.g. `"Jose\u{301}"`) from
/// another; elsewhere the comparison stays case-sensitive and normalization-sensitive.
///
/// When `path` does not begin with `home` — e.g. it reaches this function through a different
/// mount point, or (on Windows) as a `\\?\`-verbatim canonicalized path whose prefix shape the
/// component walk does not recognize as equivalent to `home`'s — this falls back to scrubbing
/// the bare username (`home`'s final component) wherever it appears in the path, so the
/// username itself is never disclosed verbatim even when the fuller `~`-collapse of the whole
/// home directory isn't achieved. On Windows/macOS, that fallback also returns the *whole* path
/// NFC-normalized (not just the redacted span), since the underlying comparison it uses
/// normalizes `path` up front — a segment outside the redacted username that happens to be
/// NFD-spelled comes back precomposed. This is harmless for display, but on Windows, where NTFS
/// treats an NFC- and an NFD-spelled filename as different files on disk, the rendered path is
/// not guaranteed to name a file that literally exists under that exact spelling.
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
        |home| strip_home_prefix(path, &home).unwrap_or_else(|| scrub_username(path, &home)),
    )
}

/// Returns `path` with its leading `home` components replaced by `~`, or `None` if `path` is
/// not rooted at `home`.
fn strip_home_prefix(path: &Path, home: &Path) -> Option<String> {
    let mut path_components = path.components();
    for home_component in home.components() {
        if !components_match(home_component, path_components.next()?) {
            return None;
        }
    }
    let mut result = String::from("~");
    for component in path_components {
        result.push(std::path::MAIN_SEPARATOR);
        result.push_str(&component.as_os_str().to_string_lossy());
    }
    Some(result)
}

/// Defense-in-depth redaction for when `path` is not rooted at `home` (see
/// [`sanitize_path_for_error`]'s doc comment for when this triggers): scrubs `home`'s bare
/// username wherever it textually appears in `path`, independent of path structure.
///
/// This scrub is a plain substring match, not scoped to a path component: on this fallback path
/// only, a component that merely contains the username as a substring (e.g. `alice-website` when
/// the username is `alice`) is partially mangled (`~-website`) rather than left alone.
/// Over-redaction is the accepted safe-failure direction for an information-disclosure guard.
fn scrub_username(path: &Path, home: &Path) -> String {
    let path_str = path.display().to_string();
    let Some(username) = home.file_name() else {
        return path_str;
    };
    let username = username.to_string_lossy();
    if username.is_empty() {
        return path_str;
    }
    replace_case_aware(&path_str, &username, "~")
}

#[cfg(any(windows, target_os = "macos"))]
fn components_match(home: Component<'_>, path: Component<'_>) -> bool {
    // Unicode-aware case folding (not `eq_ignore_ascii_case`, which only folds ASCII bytes and
    // misses non-ASCII usernames such as Cyrillic), plus NFC normalization so a component that
    // differs from the other only by composition form (e.g. precomposed "é" vs. "e" + combining
    // acute) still compares equal. This compares whole components, so there is no byte-offset
    // slicing to keep valid across either transform, unlike `replace_case_aware` below.
    normalize_and_fold(&home.as_os_str().to_string_lossy())
        == normalize_and_fold(&path.as_os_str().to_string_lossy())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn components_match(home: Component<'_>, path: Component<'_>) -> bool {
    home == path
}

/// NFC-normalizes, Unicode-case-folds, then NFC-normalizes `s` again, so two strings that differ
/// only by composition form (NFC vs. NFD) or by case compare equal after this transform.
///
/// The trailing re-normalization is required, not cosmetic: `str::to_lowercase` can turn an
/// already-NFC string back into a non-NFC one. For example, `"J\u{30C}"` (capital J + combining
/// caron) has no precomposed uppercase form, so it is already NFC; lowering it maps `J` to `j`
/// character-by-character and leaves the combining caron untouched, producing `"j\u{30C}"` —
/// which is *not* NFC, because the precomposed lowercase `"\u{1F0}"` (LATIN SMALL LETTER J WITH
/// CARON) exists. Without the second `nfc()` pass, that decomposed fold would compare unequal to
/// an already-precomposed `"\u{1F0}"` on the other side, even though they render identically.
#[cfg(any(windows, target_os = "macos"))]
fn normalize_and_fold(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfc().collect::<String>().to_lowercase().nfc().collect()
}

#[cfg(any(windows, target_os = "macos"))]
fn replace_case_aware(haystack: &str, needle: &str, replacement: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    // Case-folds each candidate window with whole-string `str::to_lowercase` (not a per-char
    // fold), so this agrees with `components_match`'s folding on Unicode's context-sensitive
    // rules — e.g. Greek final sigma: "ΣΑΣ".to_lowercase() == "σας", which a char-by-char fold
    // would render "σασ" and so fail to match.
    //
    // `haystack` and `needle` are each NFC-normalized as a whole *before* windowing starts,
    // rather than only inside each candidate window. This matters because the window is sized
    // from `needle`'s char count: normalizing only inside the window (as an earlier version of
    // this function did) left the *raw*, pre-normalization char counts of `needle` and of a
    // decomposed (NFD) matching span in `haystack` mismatched — e.g. NFC needle `"Jos\u{e9}"`
    // (4 raw chars) against an NFD-spelled `"Jose\u{301}"` span in `haystack` (5 raw chars) never
    // lined up a window of the right size, so `scrub_username`'s fallback silently failed to
    // redact the username (issue #416). Normalizing both operands whole, up front, resolves this
    // for any composition-form mismatch between them, because the same composed form is reached
    // by both sides before their lengths are ever compared.
    //
    // The window is still sized from `needle`'s (now-normalized) char count, so `haystack`'s own
    // char boundaries — computed from this same normalized string via `char_indices` — are always
    // valid slice points for the output. The comparison itself, though, runs on the *folded* form
    // (`normalize_and_fold`, which case-folds and re-normalizes), whose char count can differ from
    // the window's normalized-but-unfolded char count. So a residual, accepted limitation remains:
    // a needle/haystack pair whose folded forms only line up at a different char count than their
    // normalized forms is missed. This covers both the original case (Turkish "İ" folding to two
    // chars, or German "ß" needle against a haystack spelled "ss") and a normalization-adjacent one
    // introduced by `normalize_and_fold`'s own post-fold re-normalization: a needle like
    // `"J\u{30C}an"` (whose first char folds-and-recomposes to the 1-char "\u{1F0}", shortening the
    // folded form relative to the normalized one) is not matched — see
    // `replace_case_aware_preserves_byte_offsets_when_fold_changes_length`.
    if needle.is_empty() {
        return haystack.to_owned();
    }
    let haystack: String = haystack.nfc().collect();
    let needle: String = needle.nfc().collect();
    let needle_len = needle.chars().count();
    let needle_folded = normalize_and_fold(&needle);
    let boundaries: Vec<usize> = haystack
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(haystack.len()))
        .collect();

    let mut result = String::with_capacity(haystack.len());
    let mut last_end = 0;
    let mut i = 0;
    while i + needle_len < boundaries.len() {
        let start = boundaries[i];
        let end = boundaries[i + needle_len];
        if normalize_and_fold(&haystack[start..end]) == needle_folded {
            result.push_str(&haystack[last_end..start]);
            result.push_str(replacement);
            last_end = end;
            i += needle_len;
        } else {
            i += 1;
        }
    }
    result.push_str(&haystack[last_end..]);
    result
}

#[cfg(not(any(windows, target_os = "macos")))]
fn replace_case_aware(haystack: &str, needle: &str, replacement: &str) -> String {
    haystack.replace(needle, replacement)
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

/// Returns `true` if `path` contains a `..` (parent-directory) component.
///
/// Shared by every crate that confines a caller-supplied path to a base directory
/// (`mcp-execution-skill`'s `output_path`, `mcp-execution-server`'s `output_dir`,
/// `mcp-execution-cli`'s skill commands), so the traversal check itself has one
/// implementation rather than three copies that could silently drift apart.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::contains_parent_dir;
/// use std::path::Path;
///
/// assert!(contains_parent_dir(Path::new("../secret")));
/// assert!(contains_parent_dir(Path::new("a/../b")));
/// assert!(!contains_parent_dir(Path::new("a/b")));
/// ```
#[must_use]
pub fn contains_parent_dir(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
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
    fn contains_parent_dir_detects_traversal() {
        // Bare `..`.
        assert!(contains_parent_dir(Path::new("..")));
        // Leading position.
        assert!(contains_parent_dir(Path::new("../b")));
        // Middle position.
        assert!(contains_parent_dir(Path::new("a/../b")));
        // Trailing position.
        assert!(contains_parent_dir(Path::new("a/..")));
        assert!(!contains_parent_dir(Path::new("a/b")));
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

    // `Path::components()` only treats `/` as a separator alongside `\` on Windows, so this
    // variation is only meaningful, and only exercised, on that platform.
    #[cfg(windows)]
    #[test]
    fn sanitize_path_for_error_redacts_home_directory_with_forward_slashes() {
        let home = dirs::home_dir().unwrap();
        let home_str = home.display().to_string().replace('\\', "/");
        let under_home = format!("{home_str}/secret-file.md");
        assert_eq!(
            sanitize_path_for_error(Path::new(&under_home)),
            format!("~{}secret-file.md", std::path::MAIN_SEPARATOR),
        );
    }

    // Windows and macOS both have case-insensitive-but-case-preserving default filesystems, so
    // this is exercised on both.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn sanitize_path_for_error_redacts_home_directory_case_insensitively() {
        let home = dirs::home_dir().unwrap();
        let flipped_case: String = home
            .display()
            .to_string()
            .chars()
            .map(|c| {
                if c.is_ascii_uppercase() {
                    c.to_ascii_lowercase()
                } else if c.is_ascii_lowercase() {
                    c.to_ascii_uppercase()
                } else {
                    c
                }
            })
            .collect();
        let under_home = format!("{flipped_case}{}secret-file.md", std::path::MAIN_SEPARATOR);
        assert_eq!(
            sanitize_path_for_error(Path::new(&under_home)),
            format!("~{}secret-file.md", std::path::MAIN_SEPARATOR),
        );
    }

    // Regression test for a non-ASCII username case leak: `eq_ignore_ascii_case` only folds
    // ASCII bytes, so a Cyrillic username differing only by case was never recognized as a
    // match, silently defeating the case-insensitive redaction on Windows/macOS.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn components_match_is_unicode_case_insensitive() {
        let home_path = Path::new("Аня");
        let path_path = Path::new("аня");
        let home = home_path.components().next().unwrap();
        let path = path_path.components().next().unwrap();
        assert!(components_match(home, path));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn replace_case_aware_matches_non_ascii_case_variants() {
        assert_eq!(
            replace_case_aware("Аня/secret.md", "аня", "~"),
            "~/secret.md"
        );
    }

    // `str::to_lowercase()` can change a character's encoded length: Turkish "İ" folds to two
    // chars, "i" + a combining dot above (verified: `"İ".to_lowercase().chars().count()` is 2).
    // A naive port of the old byte-offset/`match_indices` approach to `to_lowercase` would slice
    // a lowered buffer using needle-derived byte offsets that no longer line up with the
    // original string once folding expands a character. This test proves the windowed
    // comparison — which only ever slices at `haystack`'s own char boundaries — matches and
    // replaces correctly instead of panicking or corrupting output, even though the window's
    // folded form is longer than its raw form.
    //
    // Known limitation, not exercised here: the window is sized to `needle`'s *normalized* char
    // count, but the comparison itself runs on the further *folded* form, whose char count can
    // differ from the normalized-but-unfolded one — so a needle/haystack pair whose folded forms
    // only line up at a different char count than their normalized forms is missed. This covers
    // German "ß" needle against a haystack spelled "ss" (1 char vs. 2) and a normalization-adjacent
    // case introduced by `normalize_and_fold`'s own post-fold re-normalization, e.g. needle
    // `"J\u{30C}an"` against haystack `"\u{1F0}an"` (4-char normalized needle vs. 3-char folded
    // form). Not a regression: the pre-S1/S3 code missed this identical input too. Accepted per the
    // original design: the fallback's over-redaction bias makes a missed match here a false
    // negative, not an information leak on its own, since the primary
    // `strip_home_prefix`/`components_match` path (whole-component comparison, not a windowed
    // substring search) still catches the common case.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn replace_case_aware_preserves_byte_offsets_when_fold_changes_length() {
        assert_eq!(replace_case_aware("aİb", "İ", "~"), "a~b");
        // A plain ASCII "i" is not a case variant of "İ" under this fold, so no match — the
        // differing fold length must not cause a panic or a false match.
        assert_eq!(replace_case_aware("aİb", "i", "~"), "aİb");
    }

    // Regression test for #416: two components that render identically ("José") but differ in
    // Unicode composition form — one precomposed (NFC: "e" + U+00E9 "é"), one decomposed (NFD:
    // "e" + "e" + U+0301 combining acute accent) — must still be recognized as the same
    // component once both sides are NFC-normalized before folding.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn components_match_is_unicode_normalization_insensitive() {
        let home_path = Path::new("Jos\u{e9}");
        let path_path = Path::new("Jose\u{301}");
        let home = home_path.components().next().unwrap();
        let path = path_path.components().next().unwrap();
        assert!(components_match(home, path));
    }

    // Regression test for critic finding S3: "J\u{30C}" (capital J + combining caron) has no
    // precomposed uppercase form, so it is already NFC; its lowercase, "\u{1F0}" (LATIN SMALL
    // LETTER J WITH CARON), *does* have a precomposed form. A fold that does not re-normalize
    // after lowering emits "j" + combining caron (not NFC) for the first operand while the second
    // stays precomposed, comparing unequal even though both render as "ǰ". `replace_case_aware`
    // does not have a matching test for this exact pair: the same fold-driven shortening it fixes
    // here also *shrinks* the folded form relative to the window's normalized size there, which is
    // exactly the residual limitation documented on
    // `replace_case_aware_preserves_byte_offsets_when_fold_changes_length` above.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn components_match_handles_fold_without_precomposed_uppercase() {
        let home_path = Path::new("J\u{30C}");
        let path_path = Path::new("\u{1F0}");
        let home = home_path.components().next().unwrap();
        let path = path_path.components().next().unwrap();
        assert!(components_match(home, path));
    }

    // Regression test for critic finding S1: prior to NFC-normalizing `haystack` and `needle` as
    // whole strings before windowing, a window sized from the *raw* (already-NFC) needle's char
    // count did not line up with a longer, NFD-decomposed matching span in `haystack`, so
    // `scrub_username`'s fallback path left the username visible verbatim in the redacted output.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn replace_case_aware_matches_nfc_needle_against_nfd_haystack_span() {
        let needle = "Jos\u{e9}"; // NFC, 4 raw chars
        let haystack = "/Volumes/Data/Users/Jose\u{301}/notes.md"; // NFD "Jose" + combining acute
        assert_eq!(
            replace_case_aware(haystack, needle, "~"),
            "/Volumes/Data/Users/~/notes.md"
        );
    }

    // Reverse direction of the same #416/S1 gap: an NFD-decomposed needle against an
    // NFC-precomposed haystack span.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn replace_case_aware_matches_nfd_needle_against_nfc_haystack_span() {
        let needle = "Jose\u{301}"; // NFD, 5 raw chars
        let haystack = "/Volumes/Data/Users/Jos\u{e9}/notes.md"; // NFC "José"
        assert_eq!(
            replace_case_aware(haystack, needle, "~"),
            "/Volumes/Data/Users/~/notes.md"
        );
    }

    // Regression test for critic finding S2: the previous version of this test used OHM SIGN
    // (U+2126), which `str::to_lowercase` alone already folds to the same value as GREEK CAPITAL
    // LETTER OMEGA (U+03A9) — so it passed even without any normalization fix and proved nothing
    // about normalization specifically. U+0387 GREEK ANO TELEIA NFC-normalizes to U+00B7 MIDDLE
    // DOT (a canonical singleton mapping); neither character has a case, so `to_lowercase` alone
    // cannot merge them — only NFC normalization does, making this a genuine test of the
    // normalization path. Note this does *not* exercise `normalize_and_fold`'s post-fold
    // re-normalization (S3): both operands are caseless, so folding is a no-op here — see
    // `components_match_handles_fold_without_precomposed_uppercase` above for that case instead.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn replace_case_aware_matches_caseless_singleton_normalization() {
        assert_eq!(replace_case_aware("a \u{387} b", "\u{b7}", "~"), "a ~ b");
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn replace_case_aware_replaces_multiple_occurrences() {
        assert_eq!(
            replace_case_aware("Alice/Alice/notes.md", "alice", "~"),
            "~/~/notes.md"
        );
    }

    // Regression test for the Greek final-sigma inconsistency the critic caught: whole-string
    // `str::to_lowercase()` applies Unicode's context-sensitive rule ("ΣΑΣ".to_lowercase() ==
    // "σας", using final sigma "ς"), which a naive per-char fold (`char::to_lowercase` on each
    // char independently) would render "σασ" — disagreeing with `components_match`, which folds
    // the same way `replace_case_aware` does here. Proves both functions now agree.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn replace_case_aware_matches_greek_final_sigma_case_variant() {
        assert_eq!(
            replace_case_aware("/home/ΣΑΣ/secret.md", "σας", "~"),
            "/home/~/secret.md"
        );
        assert_eq!(
            replace_case_aware("/home/σας/secret.md", "ΣΑΣ", "~"),
            "/home/~/secret.md"
        );
    }

    /// Reproduces the mounted/bind-mount scenario from the critic review: `home` appears as a
    /// non-leading substring (e.g. under `/mnt/snapshot`), so the leading-prefix component walk
    /// in `strip_home_prefix` cannot match it. Verifies the `scrub_username` fallback still
    /// keeps the username out of the rendered output.
    #[test]
    fn sanitize_path_for_error_scrubs_username_when_home_is_not_a_leading_prefix() {
        let home = dirs::home_dir().unwrap();
        let username = home.file_name().unwrap().to_string_lossy().into_owned();

        let mut mounted = std::path::PathBuf::from("mnt");
        mounted.push("snapshot");
        for component in home
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
        {
            mounted.push(component.as_os_str());
        }
        mounted.push("secret.md");

        let sanitized = sanitize_path_for_error(&mounted);
        assert!(!sanitized.to_lowercase().contains(&username.to_lowercase()));
        assert!(sanitized.contains('~'));
    }

    /// Reproduces the Windows `\\?\`-verbatim canonicalized-path regression from the critic
    /// review: `std::fs::canonicalize` prefixes the drive with `\\?\`, which
    /// `strip_home_prefix`'s component walk does not recognize as equivalent to a plain `C:\`
    /// prefix. Verifies the `scrub_username` fallback still keeps the username out of the
    /// rendered output.
    #[cfg(windows)]
    #[test]
    fn sanitize_path_for_error_scrubs_username_from_canonicalized_home_path() {
        let home = dirs::home_dir().unwrap();
        let username = home.file_name().unwrap().to_string_lossy().into_owned();
        let canonical = std::fs::canonicalize(&home).unwrap();

        let sanitized = sanitize_path_for_error(&canonical);
        assert!(!sanitized.to_lowercase().contains(&username.to_lowercase()));
    }
}
