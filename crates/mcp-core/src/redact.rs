//! Shared `Debug`-redaction helpers for secret-shaped fields.
//!
//! `ServerConfig` and the transport types built on top of it (in
//! `mcp-execution-cli`) carry fields that routinely hold secrets: header/env
//! values, CLI argument lists, and URLs with embedded credentials or a
//! `?token=`-style query string. Every one of those types needs the same
//! redaction behavior in its hand-written [`Debug`] impl, so the wrapper
//! types here are the single source of truth — implement it once, reuse it
//! everywhere a `{:?}` might otherwise leak a credential into a log line or
//! error message.

use std::collections::HashMap;
use std::fmt;

/// Fixed placeholder substituted for every redacted value.
///
/// A single constant so every redacting [`Debug`] impl (and every test that
/// asserts on the placeholder) stays in sync if the text ever changes.
pub const REDACTED_PLACEHOLDER: &str = "<redacted>";

/// Debug-formats a `String`-valued map with keys visible and every value
/// replaced by [`REDACTED_PLACEHOLDER`].
///
/// Intended for `env`/`headers`-style maps: the key (e.g. `"Authorization"`
/// or `"GITHUB_PERSONAL_ACCESS_TOKEN"`) is a caller-chosen identifier and
/// useful for debugging, but the value routinely holds a bearer token or API
/// key and must never be echoed.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::RedactedMapValues;
/// use std::collections::HashMap;
///
/// let mut headers = HashMap::new();
/// headers.insert("Authorization".to_string(), "Bearer sk-secret".to_string());
///
/// let debug_output = format!("{:?}", RedactedMapValues(&headers));
/// assert!(debug_output.contains("Authorization"));
/// assert!(!debug_output.contains("sk-secret"));
/// ```
#[derive(Clone, Copy)]
pub struct RedactedMapValues<'a>(pub &'a HashMap<String, String>);

impl fmt::Debug for RedactedMapValues<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.0.keys().map(|key| (key, REDACTED_PLACEHOLDER)))
            .finish()
    }
}

/// Debug-formats a list of strings, replacing every entry wholesale with
/// [`REDACTED_PLACEHOLDER`].
///
/// Unlike [`RedactedMapValues`] (which redacts only the value half of an
/// already-split key/value pair), this is for lists where a single entry may
/// not have a discernible key at all — a raw pre-parse `KEY=VALUE` CLI
/// argument may not even contain a `=`, and a `ServerConfig::args` entry can
/// itself be an entire `--api-key sk-...`-style secret. Since there is no
/// safe-to-keep half, the whole entry is replaced.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::RedactedItems;
///
/// let args = vec!["--api-key".to_string(), "sk-secret".to_string()];
/// let debug_output = format!("{:?}", RedactedItems(&args));
/// assert!(!debug_output.contains("sk-secret"));
/// assert!(debug_output.contains("<redacted>"));
/// ```
#[derive(Clone, Copy)]
pub struct RedactedItems<'a>(pub &'a [String]);

impl fmt::Debug for RedactedItems<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|_| REDACTED_PLACEHOLDER))
            .finish()
    }
}

/// Debug-formats a URL with userinfo credentials and query string hidden,
/// keeping the scheme, host, and path readable.
///
/// A URL can carry a secret two ways: `user:pass@host` userinfo, or a
/// `?api_key=...`-style query parameter. Both are stripped; everything else
/// (scheme, host, path) is left intact since it's the most useful part of a
/// URL for telling two server entries apart in a log.
///
/// This is deliberately parse-free (mcp-core does not depend on the `url`
/// crate) rather than a strict parser: if the input doesn't contain `://`, if
/// the scheme before it contains a character that is never valid in a URI
/// scheme, or if userinfo redaction would be ambiguous (see below), the whole
/// input is treated as unparseable and redacted in full — mirroring the
/// discard-on-parse-failure rule `mcp-execution-cli` already applies when
/// deriving a server ID from a URL. Ambiguity arises when the authority
/// terminator (the first `/`, `?`, or `#` after the scheme) lands *inside*
/// unencoded userinfo rather than at a true authority boundary — e.g. an
/// unencoded `/` in a password — which would otherwise let the userinfo
/// escape redaction entirely. Detected by checking whether an `@` still
/// appears after that terminator.
///
/// # Examples
///
/// A URL with userinfo and a query string has both hidden, while the host
/// and path stay readable:
///
/// ```
/// use mcp_execution_core::RedactedUrl;
///
/// let url = "https://user:sk-secret@api.example.com/mcp?token=sk-secret";
/// let debug_output = format!("{:?}", RedactedUrl(url));
/// assert!(!debug_output.contains("sk-secret"));
/// assert!(debug_output.contains("api.example.com/mcp"));
/// ```
///
/// A plain URL with neither is unchanged:
///
/// ```
/// use mcp_execution_core::RedactedUrl;
///
/// let url = "https://api.example.com/mcp";
/// let debug_output = format!("{:?}", RedactedUrl(url));
/// assert_eq!(debug_output, "https://api.example.com/mcp");
/// ```
#[derive(Clone, Copy)]
pub struct RedactedUrl<'a>(pub &'a str);

impl fmt::Debug for RedactedUrl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((scheme, rest)) = self.0.split_once("://") else {
            return f.write_str(REDACTED_PLACEHOLDER);
        };

        let scheme_is_valid = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !scheme_is_valid {
            return f.write_str(REDACTED_PLACEHOLDER);
        }

        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, remainder) = rest.split_at(authority_end);

        // See the type doc comment: an `@` past the authority terminator
        // means the terminator landed inside unencoded userinfo rather than
        // at a true authority boundary, so the split can't be trusted.
        if remainder.contains('@') {
            return f.write_str(REDACTED_PLACEHOLDER);
        }

        let authority = authority.rfind('@').map_or_else(
            || authority.to_string(),
            |at| format!("{REDACTED_PLACEHOLDER}@{}", &authority[at + 1..]),
        );

        let separator_pos = remainder.find(['?', '#']);
        let path = separator_pos.map_or(remainder, |pos| &remainder[..pos]);

        write!(f, "{scheme}://{authority}{path}")?;
        if let Some(pos) = separator_pos {
            let separator = &remainder[pos..=pos];
            write!(f, "{separator}{REDACTED_PLACEHOLDER}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_map_values_keeps_keys_hides_values() {
        let mut map = HashMap::new();
        map.insert("Authorization".to_string(), "Bearer secret".to_string());

        let debug_output = format!("{:?}", RedactedMapValues(&map));
        assert!(debug_output.contains("Authorization"));
        assert!(!debug_output.contains("Bearer secret"));
    }

    #[test]
    fn redacted_items_hides_every_entry() {
        let items = vec!["KEY=VALUE".to_string(), "just-a-secret".to_string()];
        let debug_output = format!("{:?}", RedactedItems(&items));
        assert!(!debug_output.contains("KEY=VALUE"));
        assert!(!debug_output.contains("just-a-secret"));
        assert_eq!(debug_output.matches(REDACTED_PLACEHOLDER).count(), 2);
    }

    #[test]
    fn redacted_url_hides_userinfo() {
        let debug_output = format!("{:?}", RedactedUrl("https://user:pass@host.com/path"));
        assert_eq!(debug_output, "https://<redacted>@host.com/path");
    }

    #[test]
    fn redacted_url_hides_query_string() {
        let debug_output = format!(
            "{:?}",
            RedactedUrl("https://host.com/path?api_key=sk-secret")
        );
        assert_eq!(debug_output, "https://host.com/path?<redacted>");
        assert!(!debug_output.contains("sk-secret"));
    }

    #[test]
    fn redacted_url_hides_fragment() {
        // M2: a fragment-only URL must be labeled `#<redacted>`, not
        // `?<redacted>` — no query string was ever present.
        let debug_output = format!("{:?}", RedactedUrl("https://host.com/path#sk-secret"));
        assert_eq!(debug_output, "https://host.com/path#<redacted>");
    }

    #[test]
    fn redacted_url_hides_userinfo_and_query_together() {
        let debug_output = format!(
            "{:?}",
            RedactedUrl("https://user:pass@host.com/path?token=secret")
        );
        assert_eq!(debug_output, "https://<redacted>@host.com/path?<redacted>");
    }

    #[test]
    fn redacted_url_leaves_plain_url_unchanged() {
        let debug_output = format!("{:?}", RedactedUrl("https://api.example.com/mcp"));
        assert_eq!(debug_output, "https://api.example.com/mcp");
    }

    #[test]
    fn redacted_url_no_path_with_userinfo() {
        let debug_output = format!("{:?}", RedactedUrl("https://user:pass@host.com?q=secret"));
        assert_eq!(debug_output, "https://<redacted>@host.com?<redacted>");
    }

    #[test]
    fn redacted_url_redacts_unparseable_input_entirely() {
        let debug_output = format!("{:?}", RedactedUrl("not-a-url"));
        assert_eq!(debug_output, REDACTED_PLACEHOLDER);
    }

    #[test]
    fn redacted_url_uses_last_at_sign_for_authority_split() {
        // A literal '@' can legally appear (percent-decoded) inside userinfo;
        // splitting on the *last* '@' keeps the host resolution correct.
        let debug_output = format!("{:?}", RedactedUrl("https://a@b:pass@host.com/path"));
        assert_eq!(debug_output, "https://<redacted>@host.com/path");
    }

    #[test]
    fn redacted_url_redacts_entirely_when_userinfo_contains_slash() {
        // S1: an unencoded '/' inside a password moves the authority
        // terminator into the middle of the credentials, so the naive split
        // would leak them verbatim. The whole URL must be redacted instead.
        let secret = "p/assw0rd";
        let debug_output = format!(
            "{:?}",
            RedactedUrl(&format!("https://user:{secret}@host.com/mcp"))
        );
        assert_eq!(debug_output, REDACTED_PLACEHOLDER);
        assert!(!debug_output.contains(secret));
        assert!(!debug_output.contains("host.com"));
    }

    #[test]
    fn redacted_url_redacts_entirely_when_userinfo_contains_query_marker() {
        // Same ambiguity as above, via '?' instead of '/'.
        let secret = "pa?ssw0rd";
        let debug_output = format!(
            "{:?}",
            RedactedUrl(&format!("https://user:{secret}@host.com/mcp"))
        );
        assert_eq!(debug_output, REDACTED_PLACEHOLDER);
        assert!(!debug_output.contains(secret));
    }

    #[test]
    fn redacted_url_redacts_entirely_when_scheme_is_malformed() {
        // M3: a scheme containing a character that can never legally appear
        // in a URI scheme (e.g. '_') is a sign the "scheme" is actually
        // secret-shaped text that happens to contain "://" — redact in full
        // rather than echoing it verbatim.
        let secret = "ghp_leakedtoken";
        let debug_output = format!("{:?}", RedactedUrl(&format!("{secret}://host.com/")));
        assert_eq!(debug_output, REDACTED_PLACEHOLDER);
        assert!(!debug_output.contains(secret));
    }
}
