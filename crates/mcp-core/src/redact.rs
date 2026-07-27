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
//!
//! [`RedactedUrl`] redacts a value already isolated behind its own field.
//! [`redact_urls_in_text`] handles the other shape: a URL buried inside
//! already-assembled prose (a `reqwest`/`rmcp` error's `Display` text, a log
//! line) where there is no field boundary to wrap — it locates each
//! URL-shaped token itself and redacts it with the same rules.

use std::collections::HashMap;
use std::fmt;

/// Fixed placeholder substituted for every redacted value.
///
/// A single constant so every redacting [`Debug`] impl (and every test that
/// asserts on the placeholder) stays in sync if the text ever changes.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::REDACTED_PLACEHOLDER;
///
/// assert_eq!(REDACTED_PLACEHOLDER, "<redacted>");
/// ```
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

/// Characters legal in a URI scheme, per [`RedactedUrl`]'s own scheme check.
const fn is_scheme_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
}

/// Characters that end a bare URL token embedded in prose: whitespace,
/// control characters, and the punctuation a human or a log formatter
/// commonly wraps a whole URL in (quotes, backtick, parens).
///
/// Deliberately narrow: `[`/`]` (RFC 3986 IP-literal delimiters, needed
/// verbatim inside an IPv6 authority like `http://[::1]:1/path`) and every
/// other RFC 3986 "unsafe" character (`<` `>` `{` `}` `\` `|` `^`) are *not*
/// terminators here, even though none of them can legally appear unescaped
/// in a URL. Terminating on them would truncate the token before the query
/// string that [`RedactedUrl`] needs to see in full to redact it — a
/// dependency's `Display` impl routinely embeds a query value verbatim,
/// unescaped, exactly where this scan runs. Erring toward capturing *too
/// much* surrounding text into the token (over-redaction) is safe; erring
/// toward cutting a token short before its secret is not.
fn is_token_terminator(c: char) -> bool {
    c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '`' | '(' | ')')
}

/// Finds every URL-shaped token in `text` and redacts each one.
///
/// Each token is redacted by handing it to [`RedactedUrl`] — the actual masking decision (what
/// counts as authority/query, what gets hidden) always defers to that one implementation, so it
/// can't drift between the two.
///
/// Unlike [`RedactedUrl`], which redacts a value already isolated behind its
/// own field, this scans arbitrary already-assembled text — typically a
/// `reqwest`/`rmcp` transport error's `Display` output, which embeds the
/// full request URL (query string included) inline in a sentence — and
/// redacts each `scheme://…` run it finds in place, leaving the surrounding
/// prose untouched.
///
/// A token's boundaries are found by walking left from `://` over
/// scheme-legal characters, then walking right to the first whitespace,
/// control character, or wrapping-punctuation character (quote, backtick,
/// paren — or the end of `text`), then trimming trailing sentence
/// punctuation (`,` `.` `;` `:`) that a log message or `Display` impl
/// commonly appends after a URL. That terminator set is deliberately
/// narrower than "every character invalid in a URL" — RFC 3986 IP-literal
/// delimiters (`[`/`]`, needed verbatim for an IPv6 authority like
/// `http://[::1]:1/path`) and every other RFC 3986 "unsafe" character are
/// left out on purpose, because capturing too much into the token is the
/// safe failure mode here, unlike [`RedactedUrl`]'s ambiguity handling,
/// which fails closed by redacting a whole *field* in full. This function
/// instead fails toward widening a *token*: it does not know a URL's true
/// end any more precisely than "the next character a log line would use to
/// wrap one", so a token capturing a few extra characters of trailing
/// prose is the accepted cost of never capturing too few and truncating a
/// secret.
///
/// The one residual gap from this heuristic: a raw, un-percent-encoded
/// instance of *any* terminator character — whitespace and control
/// characters, not just the quote/backtick/paren wrappers (`"` `'` `` ` ``
/// `(` `)`), though the latter is the realistic case, since a real
/// `reqwest`-sent URL would already have percent-encoded whitespace/control
/// bytes — *inside* the secret itself, rather than used by the surrounding
/// log line to wrap the URL, still ends the token early, exactly the same
/// class of unencoded-delimiter ambiguity [`RedactedUrl`] documents for an
/// unencoded `/` or `?` inside userinfo. Distinguishing the two would need
/// a real URL parser; this module stays parse-free by design (see
/// [`RedactedUrl`]'s doc comment).
///
/// One boundary case needs a deliberate widening step on the *left* side
/// too: if the character immediately left of the scheme run is itself a
/// non-scheme, non-terminator character (e.g. `ghp_leakedtoken://host.com/`,
/// where `_` breaks the scheme-char walk but isn't a terminator either), the
/// token is widened left to the nearest terminator before redaction.
/// Without this, the walk would stop at `_`, treat `leakedtoken` as a valid
/// scheme, and leave `ghp_leakedtoken` exposed — whereas `RedactedUrl` given
/// that same text as a whole string redacts it in full, because
/// `leakedtoken` alone isn't what the malformed-scheme check sees. Widening
/// first makes the two agree: the widened token's "scheme"
/// (`ghp_leakedtoken`) fails `RedactedUrl`'s validity check and so is
/// redacted wholesale. This widening never looks further left than the end
/// of the previous token this function already emitted — it cannot rewind
/// into text it has already committed to the output.
///
/// Widening only ever runs when at least one scheme-legal character
/// immediately precedes `://` (`leakedtoken` above): if the very first
/// character to its left is itself not scheme-legal (e.g. `tok_://host.com/`,
/// where `_` sits right against `://`), there is no scheme run to widen from
/// at all, and the whole `://` is left untouched as ordinary text rather
/// than redacted — not a realistic URL shape a dependency's `Display` impl
/// would ever produce, so not a regression from before this function
/// existed, but also not the same guarantee `RedactedUrl` gives a whole
/// string containing that same text.
///
/// Known accepted limitation, inherited unchanged from `RedactedUrl`: a
/// secret glued to a URL by scheme-*legal* characters (letters, digits,
/// `+`, `-`, `.` — e.g. `Bearer sk-abc.https://h/p?t=1`) is absorbed into
/// the token as part of its "scheme" and survives redaction, since nothing
/// distinguishes it from a URL that legitimately has a long scheme name.
/// This is exact parity with what `RedactedUrl` itself does when given that
/// same string, not a gap introduced here, and is not worth a heuristic
/// that would risk swallowing legitimate text instead.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::redact_urls_in_text;
///
/// let line = "error sending request for url (https://api.example.com/mcp?token=hunter2), \
///             when send initialize request";
/// let redacted = redact_urls_in_text(line);
/// assert!(!redacted.contains("hunter2"));
/// assert!(redacted.contains("https://api.example.com/mcp?<redacted>"));
/// assert!(redacted.contains("when send initialize request"));
/// ```
///
/// An IPv6-literal authority is redacted correctly — `[`/`]` are RFC 3986
/// authority syntax, not token boundaries, so the query string after them
/// is still reached and hidden:
///
/// ```
/// use mcp_execution_core::redact_urls_in_text;
///
/// let line = "error sending request for url (http://[::1]:1/mcp?token=hunter2), when send initialize request";
/// let redacted = redact_urls_in_text(line);
/// assert!(!redacted.contains("hunter2"));
/// assert!(redacted.contains("http://[::1]:1/mcp?<redacted>"));
/// ```
///
/// Text with no URL at all passes through unchanged, and a malformed
/// "scheme" glued to secret-shaped text is redacted in full:
///
/// ```
/// use mcp_execution_core::redact_urls_in_text;
///
/// assert_eq!(redact_urls_in_text("connecting to 127.0.0.1:18801"), "connecting to 127.0.0.1:18801");
///
/// let leaked = "weird ghp_leakedtoken://host.com/ token";
/// let redacted = redact_urls_in_text(leaked);
/// assert!(!redacted.contains("ghp_leakedtoken"));
/// ```
#[must_use]
pub fn redact_urls_in_text(text: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;

    while let Some(relative) = text[cursor..].find("://") {
        let separator = cursor + relative;

        let mut start = separator;
        for (i, c) in text[cursor..separator].char_indices().rev() {
            if is_scheme_char(c) {
                start = cursor + i;
            } else {
                break;
            }
        }

        if start == separator {
            // No scheme characters immediately precede "://" -- nothing to
            // redact here, emit it literally and keep scanning past it.
            out.push_str(&text[cursor..separator + 3]);
            cursor = separator + 3;
            continue;
        }

        if let Some(preceding) = text[..start].chars().next_back()
            && !is_token_terminator(preceding)
        {
            // Bounded to `text[cursor..start]`, never `text[..start]`: widening
            // past `cursor` would rewind `start` behind the start of the still-
            // unemitted slice below, an out-of-order range that panics. Falling
            // back to `cursor` itself (not `0`) when no terminator appears in
            // that bounded window keeps the same invariant.
            start = text[cursor..start]
                .rfind(is_token_terminator)
                .and_then(|i| {
                    text[cursor + i..]
                        .chars()
                        .next()
                        .map(|c| cursor + i + c.len_utf8())
                })
                .unwrap_or(cursor);
        }

        let mut end = text.len();
        for (i, c) in text[separator + 3..].char_indices() {
            if is_token_terminator(c) {
                end = separator + 3 + i;
                break;
            }
        }

        let token = text[start..end].trim_end_matches([',', '.', ';', ':']);

        out.push_str(&text[cursor..start]);
        // Infallible: `String`'s `fmt::Write` impl never returns `Err`.
        let _ = write!(out, "{:?}", RedactedUrl(token));
        out.push_str(&text[start + token.len()..end]);
        cursor = end;
    }

    out.push_str(&text[cursor..]);
    out
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

    #[test]
    fn redact_urls_in_text_hides_query_string_keeps_surrounding_prose() {
        let line = "error sending request for url (https://api.example.invalid/mcp?token=hunter2secret), when send initialize request";
        let redacted = redact_urls_in_text(line);
        assert!(!redacted.contains("hunter2secret"));
        assert!(redacted.contains("error sending request for url ("));
        assert!(redacted.contains("https://api.example.invalid/mcp?<redacted>"));
        assert!(redacted.contains("when send initialize request"));
    }

    #[test]
    fn redact_urls_in_text_leaves_plain_text_unchanged() {
        let line = "connecting to 127.0.0.1:18801 with no scheme";
        assert_eq!(redact_urls_in_text(line), line);
    }

    #[test]
    fn redact_urls_in_text_leaves_urls_without_secrets_unchanged() {
        let line = "see https://docs.rs/rmcp for details";
        assert_eq!(redact_urls_in_text(line), line);
    }

    #[test]
    fn redact_urls_in_text_redacts_multiple_urls_in_one_line() {
        let line = "two urls http://a.com/p?x=1 and http://b.com/q?y=2 done";
        let redacted = redact_urls_in_text(line);
        assert!(!redacted.contains("x=1"));
        assert!(!redacted.contains("y=2"));
        assert!(redacted.contains("http://a.com/p?<redacted>"));
        assert!(redacted.contains("http://b.com/q?<redacted>"));
        assert!(redacted.starts_with("two urls "));
        assert!(redacted.ends_with(" done"));
    }

    #[test]
    fn redact_urls_in_text_hides_glued_prefix_that_breaks_a_naive_scheme_walk() {
        // Regression for the audit-flagged edge case: a naive left-walk over
        // scheme characters stops at '_', would treat "leakedtoken" as a
        // valid scheme, and would leave "ghp_leakedtoken" exposed --
        // whereas `RedactedUrl` on the same whole string redacts it fully.
        // Widening the token left to the nearest terminator before handing
        // it to `RedactedUrl` must match that behavior exactly.
        let secret = "ghp_leakedtoken";
        let line = format!("weird {secret}://host.com/ token");
        let redacted = redact_urls_in_text(&line);
        assert!(!redacted.contains(secret));
        assert!(!redacted.contains("leakedtoken"));
        assert_eq!(redacted, format!("weird {REDACTED_PLACEHOLDER} token"));
    }

    #[test]
    fn redact_urls_in_text_quoted_and_paren_wrapped_shapes() {
        let quoted = redact_urls_in_text(
            r#"url "https://user:hunter2@api.example.com/mcp?token=s3cr3t" failed"#,
        );
        assert!(!quoted.contains("hunter2"));
        assert!(!quoted.contains("s3cr3t"));
        assert!(quoted.contains(r#""https://<redacted>@api.example.com/mcp?<redacted>""#));

        let paren = redact_urls_in_text(
            "error sending request for url (http://127.0.0.1:1/mcp?token=REFUSEDSECRET), when send initialize request",
        );
        assert!(!paren.contains("REFUSEDSECRET"));
        assert!(paren.contains("(http://127.0.0.1:1/mcp?<redacted>)"));
    }

    #[test]
    fn redact_urls_in_text_handles_non_ascii_prose() {
        let line = "unicode ünïcode https://h.example.com/p?t=sec end";
        let redacted = redact_urls_in_text(line);
        assert!(!redacted.contains("t=sec"));
        assert!(redacted.contains("unicode ünïcode "));
        assert!(redacted.ends_with(" end"));
    }

    #[test]
    fn redact_urls_in_text_documents_residual_scheme_legal_glue_limitation() {
        // Accepted parity limitation (see doc comment): a secret glued to a
        // URL by scheme-*legal* characters is absorbed into the token's
        // "scheme" and survives, identically to `RedactedUrl` on the same
        // whole string. Pin the behavior so a future change doesn't silently
        // alter it without updating the doc comment.
        let line = "Bearer sk-abc.https://h.example.com/p?t=1 tail";
        let redacted = redact_urls_in_text(line);
        assert!(redacted.contains("sk-abc.https://h.example.com/p?<redacted>"));
        assert!(redacted.ends_with(" tail"));
    }

    /// Regression for a critic-found panic (C1): a second `://` run whose left-widen step
    /// scanned unbounded back to the start of `text` (instead of stopping at the previous
    /// token's already-emitted end) could rewind `start` behind `cursor`, producing an
    /// out-of-order range in a later slice and panicking. Each of these four inputs panicked on
    /// the unfixed version; they must now redact without panicking.
    #[test]
    fn redact_urls_in_text_does_not_panic_on_adjacent_scheme_runs() {
        for input in [
            "_://a://b",
            "msg: _://a://b",
            "prefix ?://a_bc://x tail",
            "ü://abc://x",
        ] {
            let _ = redact_urls_in_text(input);
        }
    }

    /// Regression for the actual #353 vulnerability (C2): an IPv6-literal authority
    /// (`http://[::1]:1/...`) must still be recognized and have its query string redacted.
    /// `[`/`]` are RFC 3986 authority syntax, not prose delimiters, so they must not terminate
    /// the token before the query string is reached -- unlike the pre-fix version, which cut the
    /// token at `[`, leaving everything after it (the secret) exposed.
    #[test]
    fn redact_urls_in_text_redacts_ipv6_literal_authority_query_string() {
        let line = "error sending request for url (http://[::1]:1/mcp?token=IPV6LEAKTEST), when send initialize request";
        let redacted = redact_urls_in_text(line);
        assert!(
            !redacted.contains("IPV6LEAKTEST"),
            "secret leaked: {redacted}"
        );
        assert!(redacted.contains("http://[::1]:1/mcp?<redacted>"));
        assert!(redacted.contains("when send initialize request"));
    }

    /// Same IPv6 case without a trailing paren wrapper, to confirm the fix isn't an artifact of
    /// that shape specifically.
    #[test]
    fn redact_urls_in_text_redacts_ipv6_literal_authority_at_end_of_text() {
        let line = "connecting to http://[::1]:8080/mcp?token=IPV6LEAKTEST2";
        let redacted = redact_urls_in_text(line);
        assert!(
            !redacted.contains("IPV6LEAKTEST2"),
            "secret leaked: {redacted}"
        );
        assert!(redacted.contains("http://[::1]:8080/mcp?<redacted>"));
    }

    /// A query value containing `|`, `^`, or `\` -- RFC 3986 "unsafe" characters that can appear
    /// raw in an unescaped secret -- must not truncate the token and leak the remainder, unlike
    /// the pre-fix terminator set which treated all three as token boundaries.
    #[test]
    fn redact_urls_in_text_redacts_query_value_containing_unsafe_chars() {
        let line = "url http://host.example.com/p?token=abc|def^ghi\\jkl failed";
        let redacted = redact_urls_in_text(line);
        // `assert_eq!` against the exact expected string, not just a `!contains` check: the
        // pre-fix terminator set already passed a `!contains(secret)` assertion here too, since
        // `abc` (before the first unsafe char) was already swallowed into the marker -- only an
        // exact match on the whole line proves the *rest* of the query (`def^ghi\jkl`) didn't
        // survive as trailing raw text after an early-truncated token.
        assert_eq!(redacted, "url http://host.example.com/p?<redacted> failed");
    }

    /// Documents the residual gap left by narrowing the terminator set to close C2: a raw quote
    /// or paren character *inside* the secret itself (not used by the log line to wrap the URL)
    /// still ends the token early. Everything in the query *before* the embedded quote is safely
    /// swallowed into the redaction marker, but the quote is still treated as a token-ending
    /// wrapper, so whatever follows it survives verbatim -- the same class of ambiguity
    /// `RedactedUrl` accepts for an unencoded `/` or `?` inside userinfo. Pinned so a future
    /// change to the terminator set doesn't silently alter this without updating the doc comment.
    #[test]
    fn redact_urls_in_text_residual_gap_raw_quote_inside_secret_still_truncates() {
        let line = r#"url http://host.example.com/p?token=abc"def failed"#;
        let redacted = redact_urls_in_text(line);
        assert!(!redacted.contains("abc"));
        assert!(redacted.contains("http://host.example.com/p?<redacted>"));
        assert!(redacted.contains("def"));
    }

    /// Applying `redact_urls_in_text` to text that already contains a `RedactedUrl`-redacted URL
    /// (e.g. a `ServerConfig` `Debug` line, then re-scanned by `RedactingWriter`) must not double
    /// up the marker into `?<redacted><redacted>`. Since `[`/`]`/`<`/`>` are no longer terminators
    /// (C2 fix), the whole already-redacted URL is re-captured as one token and handed back to
    /// `RedactedUrl`, which reproduces the same output -- making the function idempotent on its
    /// own output rather than needing a dedicated already-redacted check.
    #[test]
    fn redact_urls_in_text_is_idempotent_on_already_redacted_url() {
        let line = "url http://127.0.0.1:1/mcp?<redacted> failed";
        let redacted = redact_urls_in_text(line);
        assert_eq!(redacted, line);
    }
}
