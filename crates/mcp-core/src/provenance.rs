//! Generation provenance for the `_meta.json` sidecar: when, and against what server state,
//! a `generate` run produced its output.
//!
//! `mcp-execution-codegen` writes an exported TypeScript bindings tree alongside a `_meta.json`
//! sidecar, but nothing recorded when that generation happened or what the connected server
//! looked like at the time. This module gives [`crate::metadata::ServerMetadata`] a
//! [`GenerationProvenance`] field so a future comparison mechanism can detect that a server's
//! configuration or tool surface has changed since the bindings were generated — see
//! `.local/specs/016-meta-json-generation-provenance/spec.md` for the full design record.
//!
//! # What provenance does and does not answer
//!
//! Provenance answers "has the server's exposed surface, or the identity of the endpoint we
//! generated from, changed since generation?" It does **not** answer "would re-running
//! `generate` today produce byte-identical files?" — collision-disambiguating TypeScript names
//! and tool categorization both affect generated output without affecting either digest below.
//!
//! # Hashing approach
//!
//! Both [`ConfigFingerprint`] and [`ToolDigest`] are SHA-256 digests (hex-encoded, lowercase)
//! over a hand-built preimage — never over `Debug` output (no stability guarantee) or a
//! `serde_json::Value`'s serialized bytes (this workspace enables `serde_json/preserve_order`
//! transitively via `handlebars`, and even without that, a `HashMap`-backed preimage would vary
//! by process). Every preimage is built from the `Preimage` length-framing primitive over an
//! explicitly sorted, fixed field order, so two semantically identical inputs always hash
//! identically regardless of map iteration order — see [`ConfigFingerprint::compute`] and
//! [`ToolDigest::compute`] for the exact field lists.
//!
//! [`ConfigFingerprint`] deliberately excludes every secret-bearing *value* (argument values,
//! environment variable values, header values, query-parameter values, userinfo) from its
//! preimage — only structural signal (names, counts, the URL's scheme/authority/path) goes in.
//! Rotating a credential must never register as configuration drift.

use crate::redact::{UrlTailKind, split_url};
use crate::server_config::{ServerConfig, Transport};
use crate::untrusted::sanitize_untrusted_inline;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Domain-separation tag for [`ConfigFingerprint::compute`]'s preimage.
const CONFIG_FINGERPRINT_DOMAIN: &str = "mcp-execution:config-fingerprint:v1";

/// Domain-separation tag for a single tool entry's preimage, hashed inside
/// [`ToolDigest::compute`].
const TOOL_ENTRY_DOMAIN: &str = "mcp-execution:tool-entry:v1";

/// Domain-separation tag for [`ToolDigest::compute`]'s aggregate preimage.
const TOOL_DIGEST_DOMAIN: &str = "mcp-execution:tool-digest:v1";

/// Tag byte marking a URL that [`split_url`] parsed successfully, in
/// [`push_url_and_headers`]'s preimage contribution.
const URL_PARSED: u8 = 0;
/// Tag byte marking a URL [`split_url`] could not parse, standing in for the whole
/// scheme/authority/path/query contribution (N3: a distinct tag, not a literal `<unparseable>`
/// string, so no real URL content can collide with it).
const URL_UNPARSEABLE: u8 = 1;

/// Tag byte marking a named query parameter in [`push_query_param_names`]'s preimage
/// contribution, followed by the framed name.
const QUERY_PARAM_NAMED: u8 = 0;
/// Tag byte marking a bare query parameter (no `=`) — carries no following bytes, so it can
/// never collide with a real parameter literally named `<bare>` (N3).
const QUERY_PARAM_BARE: u8 = 1;

/// Type tag for [`hash_value_into`]'s recursive walk over a `serde_json::Value`.
mod value_tag {
    pub(super) const NULL: u8 = 0;
    pub(super) const BOOL: u8 = 1;
    pub(super) const NUMBER: u8 = 2;
    pub(super) const STRING: u8 = 3;
    pub(super) const ARRAY: u8 = 4;
    pub(super) const OBJECT: u8 = 5;
}

/// Accumulates a hash preimage as a byte buffer under one framing convention: every
/// variable-length value is prefixed with its length as a big-endian `u64` before its bytes
/// ([`Self::str`]/[`Self::bytes`]); every fixed-width value (a count, a presence/tag byte) is
/// written raw ([`Self::u64`]/[`Self::byte`]/[`Self::raw_32`]). Fixed field order plus this
/// framing makes the encoding unambiguous without any separator or escaping — see the module
/// docs for why this replaces both `Debug` output and JSON serialization as a preimage source.
struct Preimage(Vec<u8>);

impl Preimage {
    const fn new() -> Self {
        Self(Vec::new())
    }

    /// Length-prefixed raw bytes.
    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        let len = bytes.len() as u64;
        self.0.extend_from_slice(&len.to_be_bytes());
        self.0.extend_from_slice(bytes);
        self
    }

    /// Length-prefixed UTF-8 text.
    fn str(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }

    /// A fixed-width count. Not length-prefixed: a `u64`'s width is already fixed, so framing
    /// it would only waste bytes without resolving any ambiguity.
    fn u64(&mut self, n: u64) -> &mut Self {
        self.0.extend_from_slice(&n.to_be_bytes());
        self
    }

    /// A single presence/tag byte.
    fn byte(&mut self, b: u8) -> &mut Self {
        self.0.push(b);
        self
    }

    /// 32 raw bytes (a nested SHA-256 digest). Fixed-width by construction, so — like
    /// [`Self::u64`] — no length prefix is needed.
    fn raw_32(&mut self, bytes: [u8; 32]) -> &mut Self {
        self.0.extend_from_slice(&bytes);
        self
    }

    /// Hashes the accumulated buffer with SHA-256.
    fn finish(&self) -> [u8; 32] {
        Sha256::digest(&self.0).into()
    }
}

/// Hex-encodes `bytes` as lowercase hex, one `{:02x}` pair per byte.
///
/// `sha2`'s digest output type does not implement [`std::fmt::LowerHex`], so `format!("{:x}",
/// ...)` is not available — this explicit loop is the documented replacement (see spec §10)
/// rather than pulling in a dedicated hex-encoding crate for eight lines.
fn hex_encode(bytes: [u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Infallible: `String`'s `fmt::Write` impl never returns `Err`.
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Error returned when a candidate string is not a well-formed digest: exactly 64 lowercase
/// ASCII hex characters (a hex-encoded SHA-256 digest).
///
/// Returned by both [`ConfigFingerprint`] and [`ToolDigest`]'s `TryFrom<String>` impl, which
/// their `#[serde(try_from = "String")]` `Deserialize` routes every deserialization through —
/// mirroring [`crate::ServerId`]/[`crate::ToolName`]'s validated-newtype pattern (see their own
/// doc comments): no separate, unvalidated deserialization path exists for either type, so a
/// hand-edited `_meta.json` cannot produce a value violating `as_str`'s documented
/// "64-character lowercase-hex" contract.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::provenance::{ConfigFingerprint, DigestFormatError};
///
/// let err = ConfigFingerprint::try_from("not-a-digest".to_string()).unwrap_err();
/// assert!(matches!(err, DigestFormatError { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid digest {value:?}: must be exactly 64 lowercase hex characters")]
pub struct DigestFormatError {
    /// Sanitized form of the rejected input (see
    /// [`sanitize_untrusted_inline`](crate::untrusted::sanitize_untrusted_inline)): this value
    /// comes from an on-disk `_meta.json` a caller may have hand-edited, so it is treated as
    /// untrusted the same way `ServerIdError`/`ToolNameError` treat their own rejected input.
    value: String,
}

/// Validates that `candidate` is exactly 64 lowercase ASCII hex characters, the shape every
/// [`ConfigFingerprint`]/[`ToolDigest`] produced by [`hex_encode`] always has.
fn validate_digest_string(candidate: String) -> Result<String, DigestFormatError> {
    let is_valid = candidate.len() == 64
        && candidate
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if is_valid {
        Ok(candidate)
    } else {
        Err(DigestFormatError {
            value: sanitize_untrusted_inline(&candidate),
        })
    }
}

/// A stable fingerprint of the [`ServerConfig`] used to connect to and introspect a server,
/// recorded so a later comparison can detect that connection parameters changed.
///
/// Newtype over a 64-character lowercase-hex `String` (a SHA-256 digest), rather than a bare
/// `String`, for the same reason [`crate::ServerId`]/[`crate::ToolName`] are newtypes: two
/// same-shaped hex strings sitting adjacent in `crate::metadata::GenerationProvenance` are
/// trivially swappable by accident, and a single-field newtype still serializes transparently
/// as a JSON string. [`Deserialize`] is routed through [`TryFrom<String>`] (via
/// `#[serde(try_from = "String")]`), so a value read back from disk is validated the same way
/// [`crate::ServerId`]/[`crate::ToolName`] are — see [`DigestFormatError`].
///
/// # Examples
///
/// ```
/// use mcp_execution_core::provenance::ConfigFingerprint;
/// use mcp_execution_core::ServerConfig;
///
/// let config = ServerConfig::builder().command("docker".to_string()).build().unwrap();
/// let fingerprint = ConfigFingerprint::compute(&config);
/// assert_eq!(fingerprint.as_str().len(), 64);
/// assert!(fingerprint.as_str().chars().all(|c| c.is_ascii_hexdigit()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ConfigFingerprint(String);

impl TryFrom<String> for ConfigFingerprint {
    type Error = DigestFormatError;

    /// Delegates to `validate_digest_string` — the sole entry point [`Deserialize`] uses.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_digest_string(value).map(Self)
    }
}

impl ConfigFingerprint {
    /// Computes a fingerprint of `config`, sufficient to detect that connection parameters
    /// changed, without persisting any secret-bearing value.
    ///
    /// The preimage carries, in a fixed order: a domain tag; the transport discriminant
    /// (`stdio`/`http`/`sse`); for `stdio`, `command`, `cwd` presence, argument *count*, and
    /// every environment variable *name* (sorted); for `http`/`sse`, the URL's canonical
    /// `scheme://authority/path` form, its query-parameter *names* (deduplicated, sorted), a
    /// userinfo-present marker, and every header *name* (ASCII-lowercased, sorted). No argument
    /// value, environment/header value, query-parameter value, or userinfo is ever fed — see
    /// the module docs.
    ///
    /// `ServerConfig::connect_timeout`/`discover_timeout` are deliberately excluded: they bound
    /// how long the client waits for a response, not what the server exposes, so changing one
    /// must not register as a change to the server's identity or tool surface.
    ///
    /// Residual collision, documented rather than fixed: every URL `split_url` cannot parse —
    /// including two configs whose only difference is inside the ambiguous userinfo case it
    /// rejects, e.g. `https://u:p/w@a.com` vs. `https://u:p/w@b.com` — collapses onto the same
    /// `URL_UNPARSEABLE` marker and therefore the same fingerprint. This is the same class of
    /// secrecy-over-precision tradeoff as the other residual collisions in this family (query
    /// values, userinfo credentials): the input a real fingerprint would need to distinguish
    /// them is exactly the text this function refuses to hash.
    ///
    /// # Examples
    ///
    /// Configs differing only in secret-bearing values fingerprint identically:
    ///
    /// ```
    /// use mcp_execution_core::provenance::ConfigFingerprint;
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let a = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .env("TOKEN".to_string(), "secret-a".to_string())
    ///     .build()
    ///     .unwrap();
    /// let b = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .env("TOKEN".to_string(), "secret-b".to_string())
    ///     .build()
    ///     .unwrap();
    ///
    /// assert_eq!(ConfigFingerprint::compute(&a), ConfigFingerprint::compute(&b));
    /// ```
    #[must_use]
    pub fn compute(config: &ServerConfig) -> Self {
        let mut pre = Preimage::new();
        pre.str(CONFIG_FINGERPRINT_DOMAIN);

        match config.transport() {
            Transport::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                pre.str("stdio");
                pre.str(command);
                match cwd {
                    Some(path) => {
                        pre.byte(1);
                        pre.bytes(path.as_os_str().as_encoded_bytes());
                    }
                    None => {
                        pre.byte(0);
                    }
                }
                pre.u64(args.len() as u64);

                let mut names: Vec<&str> = env.keys().map(String::as_str).collect();
                names.sort_unstable();
                pre.u64(names.len() as u64);
                for name in names {
                    pre.str(name);
                }
            }
            Transport::Http { url, headers } => {
                pre.str("http");
                push_url_and_headers(&mut pre, url, headers);
            }
            Transport::Sse { url, headers } => {
                pre.str("sse");
                push_url_and_headers(&mut pre, url, headers);
            }
        }

        Self(hex_encode(pre.finish()))
    }

    /// Returns the fingerprint as a 64-character lowercase-hex string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::provenance::ConfigFingerprint;
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder().command("docker".to_string()).build().unwrap();
    /// let fingerprint = ConfigFingerprint::compute(&config);
    /// assert!(!fingerprint.as_str().is_empty());
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Query-parameter name classification fed into [`push_query_param_names`]'s preimage
/// contribution. `Bare` sorts before every `Named` variant (see the derived [`Ord`]), giving a
/// deterministic total order regardless of how many bare parameters a query string carries —
/// they collapse to a single deduplicated entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum QueryParamName<'a> {
    /// A `?token` segment with no `=`: the text is indistinguishable from a value, so only the
    /// fact that *some* bare parameter existed is preserved (see spec §6).
    Bare,
    /// A `name=value` segment's `name` half.
    Named(&'a str),
}

/// Parses `query`'s `&`-separated segments into deduplicated, sorted [`QueryParamName`]s.
///
/// Only the segment layout (`name=value` vs. a bare token) is inspected; every value is
/// discarded before it ever reaches the caller, let alone the hash preimage.
///
/// `query` is truncated at the first `#`, if any, before splitting on `&`. This matters because
/// [`split_url`]'s `Query` tail runs verbatim to the end of the URL by design — a `?`-triggered
/// tail does not stop at a later `#`, since [`crate::RedactedUrl`]'s `Debug` impl (which shares
/// that parser) only needs to know *that* a separator was hit, not parse what follows it. This
/// function does need to parse what follows, so without this truncation a URL fragment
/// containing its own `&`-separated text would be misread as query parameters — collapsing two
/// configs with different query strings (`?a=1&b=2` vs `?a=1#&b=2`) onto the same fingerprint,
/// exactly the false-negative direction NFR-004 forbids.
fn parse_query_param_names(query: &str) -> Vec<QueryParamName<'_>> {
    let query = query.find('#').map_or(query, |pos| &query[..pos]);

    let mut names: Vec<QueryParamName<'_>> = query
        .split('&')
        .filter(|segment| !segment.is_empty())
        .map(|segment| match segment.split_once('=') {
            Some((name, _value)) => QueryParamName::Named(name),
            None => QueryParamName::Bare,
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Writes each `name`'s preimage contribution: a type-tag byte (N3) followed by the framed
/// name for [`QueryParamName::Named`], or nothing further for [`QueryParamName::Bare`] — the
/// tag byte alone is enough to distinguish a bare marker from a parameter literally named
/// `<bare>`, which would instead be tagged `QUERY_PARAM_NAMED` and carry that literal text.
fn push_query_param_names(pre: &mut Preimage, names: &[QueryParamName<'_>]) {
    pre.u64(names.len() as u64);
    for name in names {
        match name {
            QueryParamName::Named(n) => {
                pre.byte(QUERY_PARAM_NAMED);
                pre.str(n);
            }
            QueryParamName::Bare => {
                pre.byte(QUERY_PARAM_BARE);
            }
        }
    }
}

/// Writes the shared `http`/`sse` portion of [`ConfigFingerprint::compute`]'s preimage: the
/// URL's canonical form, query-parameter names, a userinfo-present marker, and header names.
///
/// Uses [`split_url`] rather than [`crate::RedactedUrl`]'s `Debug` impl (module docs explain
/// why a `Debug` rendering is unfit as a preimage source) — one parser, two independent
/// renderings, so the fingerprint's own wire format is pinned by its own tests instead of
/// inheriting `RedactedUrl`'s.
fn push_url_and_headers(
    pre: &mut Preimage,
    url: &str,
    headers: &std::collections::HashMap<String, String>,
) {
    match split_url(url) {
        Some(parts) => {
            pre.byte(URL_PARSED);
            let canonical = format!("{}://{}{}", parts.scheme, parts.authority, parts.path);
            pre.str(&canonical);

            let query_names = match parts.tail {
                Some((UrlTailKind::Query, query)) => parse_query_param_names(query),
                Some((UrlTailKind::Fragment, _)) | None => Vec::new(),
            };
            push_query_param_names(pre, &query_names);

            pre.byte(u8::from(parts.userinfo_present));
        }
        None => {
            pre.byte(URL_UNPARSEABLE);
        }
    }

    let mut header_names: Vec<String> = headers.keys().map(|h| h.to_ascii_lowercase()).collect();
    header_names.sort_unstable();
    pre.u64(header_names.len() as u64);
    for name in &header_names {
        pre.str(name);
    }
}

/// A borrowed view over one discovered tool, shaped for [`ToolDigest::compute`] without coupling
/// `mcp-execution-core` to `mcp-execution-introspector`'s `ToolInfo`.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::provenance::ToolDigestEntry;
/// use serde_json::json;
///
/// let schema = json!({"type": "object"});
/// let entry = ToolDigestEntry {
///     name: "create_issue",
///     description: "Creates a new issue",
///     input_schema: &schema,
///     output_schema: None,
/// };
/// assert_eq!(entry.name, "create_issue");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ToolDigestEntry<'a> {
    /// The tool's MCP name (the call identifier).
    pub name: &'a str,
    /// The tool's description, as reported by the server.
    pub description: &'a str,
    /// The tool's input JSON Schema.
    pub input_schema: &'a serde_json::Value,
    /// The tool's output JSON Schema, if the server reported one.
    pub output_schema: Option<&'a serde_json::Value>,
}

/// A stable digest of the discovered tool list (names, descriptions, and input/output schemas)
/// at generation time, recorded so a later comparison can detect that the server's tool surface
/// changed.
///
/// Newtype over a 64-character lowercase-hex `String` — see [`ConfigFingerprint`]'s doc comment
/// for why this is a newtype rather than a bare `String`, and for the `TryFrom<String>`/
/// [`DigestFormatError`] validation its [`Deserialize`] is routed through.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::provenance::{ToolDigest, ToolDigestEntry};
/// use serde_json::json;
///
/// let schema = json!({"type": "object"});
/// let entries = vec![ToolDigestEntry {
///     name: "create_issue",
///     description: "Creates a new issue",
///     input_schema: &schema,
///     output_schema: None,
/// }];
///
/// let digest = ToolDigest::compute(&entries);
/// assert_eq!(digest.as_str().len(), 64);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ToolDigest(String);

impl TryFrom<String> for ToolDigest {
    type Error = DigestFormatError;

    /// Delegates to `validate_digest_string` — the sole entry point [`Deserialize`] uses.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_digest_string(value).map(Self)
    }
}

impl ToolDigest {
    /// Computes an aggregate digest of `entries`, insensitive to input order (including two
    /// tools sharing the same name in swapped order) but sensitive to any schema/name/
    /// description edit, tool addition, or tool removal.
    ///
    /// Two-level construction: each entry is hashed on its own (domain tag, framed `name`,
    /// framed `description`, then `input_schema`/`output_schema` via the recursive
    /// `hash_value_into` walk — `output_schema`'s presence is tagged explicitly, so `None`
    /// and `Some(Value::Null)` hash differently), the resulting 32-byte digests are sorted, and
    /// the sorted sequence is hashed into the final aggregate. Sorting digests rather than tool
    /// names gives a total order even for duplicate tool names, which name-sorting alone would
    /// leave ambiguous.
    ///
    /// # Examples
    ///
    /// Reordering the input tool list does not change the digest:
    ///
    /// ```
    /// use mcp_execution_core::provenance::{ToolDigest, ToolDigestEntry};
    /// use serde_json::json;
    ///
    /// let schema_a = json!({"type": "object"});
    /// let schema_b = json!({"type": "string"});
    /// let a = ToolDigestEntry { name: "a", description: "", input_schema: &schema_a, output_schema: None };
    /// let b = ToolDigestEntry { name: "b", description: "", input_schema: &schema_b, output_schema: None };
    ///
    /// assert_eq!(
    ///     ToolDigest::compute(&[a, b]),
    ///     ToolDigest::compute(&[b, a]),
    /// );
    /// ```
    #[must_use]
    pub fn compute(entries: &[ToolDigestEntry<'_>]) -> Self {
        let mut entry_digests: Vec<[u8; 32]> = entries.iter().map(hash_tool_entry).collect();
        entry_digests.sort_unstable();

        let mut pre = Preimage::new();
        pre.str(TOOL_DIGEST_DOMAIN);
        pre.u64(entries.len() as u64);
        for digest in entry_digests {
            pre.raw_32(digest);
        }

        Self(hex_encode(pre.finish()))
    }

    /// Returns the digest as a 64-character lowercase-hex string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::provenance::{ToolDigest, ToolDigestEntry};
    ///
    /// let digest = ToolDigest::compute(&[] as &[ToolDigestEntry<'_>]);
    /// assert!(!digest.as_str().is_empty());
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Hashes a single [`ToolDigestEntry`] into its own 32-byte digest, before the aggregate
/// [`ToolDigest::compute`] sorts and re-hashes every entry's digest together.
fn hash_tool_entry(entry: &ToolDigestEntry<'_>) -> [u8; 32] {
    let mut pre = Preimage::new();
    pre.str(TOOL_ENTRY_DOMAIN);
    pre.str(entry.name);
    pre.str(entry.description);
    hash_value_into(&mut pre, entry.input_schema);
    match entry.output_schema {
        Some(schema) => {
            pre.byte(1);
            hash_value_into(&mut pre, schema);
        }
        None => {
            pre.byte(0);
        }
    }
    pre.finish()
}

/// Recursively feeds a `serde_json::Value` into `pre`, sorting object keys at hash time so the
/// result never depends on whether `serde_json/preserve_order` is enabled, and never calling
/// the serializer at all.
///
/// Every variant is prefixed with a one-byte type tag ([`value_tag`]) so, for example, the
/// string `"5"` and the number `5` cannot collide even though their framed bytes would
/// otherwise be identical. Numbers are hashed via their `to_string()` bytes — `serde_json`'s
/// `arbitrary_precision` feature is not enabled anywhere in this workspace, so `Value::Number`
/// is the ordinary, deterministic enum. Array order is preserved (JSON arrays are ordered in
/// general, e.g. `enum`/`prefixItems`), so reordering one is treated as drift; object key order
/// is normalized, since JSON objects are unordered by the spec.
fn hash_value_into(pre: &mut Preimage, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => {
            pre.byte(value_tag::NULL);
        }
        serde_json::Value::Bool(b) => {
            pre.byte(value_tag::BOOL);
            pre.byte(u8::from(*b));
        }
        serde_json::Value::Number(n) => {
            pre.byte(value_tag::NUMBER);
            pre.str(&n.to_string());
        }
        serde_json::Value::String(s) => {
            pre.byte(value_tag::STRING);
            pre.str(s);
        }
        serde_json::Value::Array(items) => {
            pre.byte(value_tag::ARRAY);
            pre.u64(items.len() as u64);
            for item in items {
                hash_value_into(pre, item);
            }
        }
        serde_json::Value::Object(map) => {
            pre.byte(value_tag::OBJECT);
            pre.u64(map.len() as u64);
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            for key in keys {
                pre.str(key);
                hash_value_into(pre, &map[key]);
            }
        }
    }
}

/// Generation provenance recorded in [`crate::metadata::ServerMetadata`].
///
/// Records when a `_meta.json` sidecar was produced, and a fingerprint/digest pair sufficient
/// for a later comparison to detect that the server changed.
///
/// Deliberately not wrapped in `Option` anywhere it's stored: a schema-version check rejects a
/// pre-provenance (`schema_version: 1`) sidecar before a consumer ever constructs a
/// [`crate::metadata::ServerMetadata`], so every value that exists already carries real
/// provenance — see `mcp-execution-skill`'s parser.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::provenance::{GenerationProvenance, ToolDigestEntry};
/// use mcp_execution_core::ServerConfig;
///
/// let config = ServerConfig::builder().command("docker".to_string()).build().unwrap();
/// let provenance = GenerationProvenance::capture(&config, &[]);
/// assert_eq!(provenance.tool_digest.as_str().len(), 64);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationProvenance {
    /// Wall-clock time generation completed, from the `generate` command's own clock.
    pub generated_at: DateTime<Utc>,
    /// Fingerprint of the [`ServerConfig`] used to connect to and introspect the server.
    pub config_fingerprint: ConfigFingerprint,
    /// Digest of the discovered tool list at generation time.
    pub tool_digest: ToolDigest,
}

impl GenerationProvenance {
    /// Captures provenance for a `generate` run: stamps the current time and computes both the
    /// config fingerprint and tool digest from the same inputs the run is generating from, so
    /// the digest can never drift from the emitted files.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::provenance::{GenerationProvenance, ToolDigestEntry};
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder().command("docker".to_string()).build().unwrap();
    /// let provenance = GenerationProvenance::capture(&config, &[]);
    /// assert!(provenance.generated_at <= chrono::Utc::now());
    /// ```
    #[must_use]
    pub fn capture(config: &ServerConfig, tools: &[ToolDigestEntry<'_>]) -> Self {
        Self {
            generated_at: Utc::now(),
            config_fingerprint: ConfigFingerprint::compute(config),
            tool_digest: ToolDigest::compute(tools),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn stdio_config(command: &str) -> ServerConfig {
        ServerConfig::builder()
            .command(command.to_string())
            .build()
            .unwrap()
    }

    // -- Config fingerprint: determinism --

    /// The mandated test: two `HashMap`s built with *different insertion order* must yield
    /// equal fingerprints. A same-process repeat call cannot catch a `HashMap`-iteration-order
    /// bug, since a single process's iteration order for a given map is stable across repeated
    /// reads of the *same* map instance.
    #[test]
    fn fingerprint_determinism_env_insertion_order() {
        let mut env_a = HashMap::new();
        env_a.insert("ALPHA".to_string(), "1".to_string());
        env_a.insert("BETA".to_string(), "2".to_string());
        env_a.insert("GAMMA".to_string(), "3".to_string());

        let mut env_b = HashMap::new();
        env_b.insert("GAMMA".to_string(), "3".to_string());
        env_b.insert("ALPHA".to_string(), "1".to_string());
        env_b.insert("BETA".to_string(), "2".to_string());

        let a = ServerConfig::builder()
            .command("docker".to_string())
            .environment(env_a)
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .command("docker".to_string())
            .environment(env_b)
            .build()
            .unwrap();

        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_determinism_header_insertion_order() {
        let mut headers_a = HashMap::new();
        headers_a.insert("X-One".to_string(), "1".to_string());
        headers_a.insert("X-Two".to_string(), "2".to_string());

        let mut headers_b = HashMap::new();
        headers_b.insert("X-Two".to_string(), "2".to_string());
        headers_b.insert("X-One".to_string(), "1".to_string());

        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .headers(headers_a)
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .headers(headers_b)
            .build()
            .unwrap();

        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    // -- Config fingerprint: security guarantee (positive assertion) --

    #[test]
    fn fingerprint_equal_when_only_env_values_differ() {
        let a = ServerConfig::builder()
            .command("docker".to_string())
            .env("TOKEN".to_string(), "secret-a".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .command("docker".to_string())
            .env("TOKEN".to_string(), "secret-b".to_string())
            .build()
            .unwrap();
        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_equal_when_only_header_values_differ() {
        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), "Bearer a".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), "Bearer b".to_string())
            .build()
            .unwrap();
        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_equal_when_only_arg_values_differ() {
        let a = ServerConfig::builder()
            .command("npx".to_string())
            .arg("pkg-a".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .command("npx".to_string())
            .arg("pkg-b".to_string())
            .build()
            .unwrap();
        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_equal_when_only_query_param_values_differ() {
        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp?tenant=alpha".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp?tenant=beta".to_string())
            .build()
            .unwrap();
        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_equal_when_only_userinfo_differs() {
        let a = ServerConfig::builder()
            .http_transport("https://user:pass-a@api.example.com/mcp".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://user:pass-b@api.example.com/mcp".to_string())
            .build()
            .unwrap();
        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    // -- Config fingerprint: sensitivity --

    #[test]
    fn fingerprint_differs_on_command() {
        assert_ne!(
            ConfigFingerprint::compute(&stdio_config("docker")),
            ConfigFingerprint::compute(&stdio_config("npx")),
        );
    }

    #[test]
    fn fingerprint_differs_on_cwd() {
        let a = ServerConfig::builder()
            .command("docker".to_string())
            .cwd("/tmp/a".into())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .command("docker".to_string())
            .cwd("/tmp/b".into())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_differs_on_arg_count() {
        let a = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .arg("--rm".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_differs_on_env_key() {
        let a = ServerConfig::builder()
            .command("docker".to_string())
            .env("ALPHA".to_string(), "1".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .command("docker".to_string())
            .env("BETA".to_string(), "1".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_differs_on_header_name() {
        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("X-One".to_string(), "1".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("X-Two".to_string(), "1".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_header_name_case_does_not_change_it() {
        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), "1".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("authorization".to_string(), "1".to_string())
            .build()
            .unwrap();
        assert_eq!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_differs_on_scheme() {
        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .sse_transport("https://api.example.com/mcp".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    /// Unlike `fingerprint_differs_on_scheme` above (which varies the *transport discriminant*,
    /// http vs sse, while holding the URL's own scheme fixed at `https://` in both branches),
    /// this isolates the URL's own scheme component — `http://` vs `https://` on an otherwise
    /// identical `http_transport` config. `ServerConfig` accepts both schemes, so this case is
    /// reachable and must be covered independently.
    #[test]
    fn fingerprint_differs_on_url_scheme_itself() {
        let a = ServerConfig::builder()
            .http_transport("http://api.example.com/mcp".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_differs_on_authority() {
        let a = ServerConfig::builder()
            .http_transport("https://api-a.example.com/mcp".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api-b.example.com/mcp".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_differs_on_path() {
        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp-a".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp-b".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    #[test]
    fn fingerprint_differs_on_query_param_name() {
        let a = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp?alpha=1".to_string())
            .build()
            .unwrap();
        let b = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp?beta=1".to_string())
            .build()
            .unwrap();
        assert_ne!(
            ConfigFingerprint::compute(&a),
            ConfigFingerprint::compute(&b)
        );
    }

    // -- URL splitter: exact-string pin + `<bare>`/`<unparseable>` marker cases --

    /// Pins the fingerprint's canonical URL rendering as a wire-format contract: any change to
    /// this preimage requires a `METADATA_SCHEMA_VERSION` bump.
    #[test]
    fn url_canonical_form_exact_string() {
        let parts = split_url("https://user:pass@api.example.com:8443/mcp/v1?a=1&b=2#frag")
            .expect("parses");
        let canonical = format!("{}://{}{}", parts.scheme, parts.authority, parts.path);
        assert_eq!(canonical, "https://api.example.com:8443/mcp/v1");
        assert!(parts.userinfo_present);
    }

    #[test]
    fn fingerprint_bare_query_param_uses_marker_not_text() {
        let bare = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp?sk-live-token".to_string())
            .build()
            .unwrap();
        let named = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp?<bare>=1".to_string())
            .build()
            .unwrap();

        // The bare marker's tag byte, not literal text, means an actual bare secret-shaped
        // token and a parameter literally named `<bare>` must fingerprint differently (N3).
        assert_ne!(
            ConfigFingerprint::compute(&bare),
            ConfigFingerprint::compute(&named),
        );
    }

    /// Regression for a critic-found collision: `split_url`'s `Query` tail runs verbatim to the
    /// end of the URL (by design — see its own doc comment), so it can contain a later `#`.
    /// Without truncating at that `#` before splitting on `&`, `?a=1&b=2` and `?a=1#&b=2` parsed
    /// to the identical `[Named("a"), Named("b")]` list and therefore the identical fingerprint,
    /// despite being different query strings (a fragment-only edit registering as false-positive
    /// query drift, and a real second query parameter hiding behind `#` as a false negative —
    /// the direction NFR-004 forbids). `parse_query_param_names` must now stop at the first `#`.
    #[test]
    fn fingerprint_query_param_names_stop_at_fragment_boundary() {
        let two_query_params = ServerConfig::builder()
            .http_transport("https://h.example.com/p?a=1&b=2".to_string())
            .build()
            .unwrap();
        let one_query_param_plus_fragment = ServerConfig::builder()
            .http_transport("https://h.example.com/p?a=1#&b=2".to_string())
            .build()
            .unwrap();

        assert_ne!(
            ConfigFingerprint::compute(&two_query_params),
            ConfigFingerprint::compute(&one_query_param_plus_fragment),
        );

        // The fragment-bearing config must fingerprint the same as one with no `#&b=2` tail at
        // all — proving the fragment text is excluded entirely, not merely hashed differently.
        let one_query_param_no_fragment = ServerConfig::builder()
            .http_transport("https://h.example.com/p?a=1".to_string())
            .build()
            .unwrap();
        assert_eq!(
            ConfigFingerprint::compute(&one_query_param_plus_fragment),
            ConfigFingerprint::compute(&one_query_param_no_fragment),
        );
    }

    #[test]
    fn fingerprint_unparseable_url_uses_marker() {
        // `ServerConfig::build` only enforces an `http`/`https` scheme prefix, so the only way
        // to reach `split_url`'s `None` case through a validated config is the userinfo
        // ambiguity it documents: an unencoded '/' inside the password moves the authority
        // terminator into the middle of the credentials.
        let config = ServerConfig::builder()
            .http_transport("https://user:pa/ssw0rd@api.example.com/mcp".to_string())
            .build()
            .unwrap();
        // Must not panic, and must produce a stable digest distinct from a parseable URL.
        let fingerprint = ConfigFingerprint::compute(&config);
        assert_eq!(fingerprint.as_str().len(), 64);

        let parseable = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .build()
            .unwrap();
        assert_ne!(fingerprint, ConfigFingerprint::compute(&parseable));
    }

    // -- Tool digest --

    fn entry<'a>(name: &'a str, schema: &'a serde_json::Value) -> ToolDigestEntry<'a> {
        ToolDigestEntry {
            name,
            description: "desc",
            input_schema: schema,
            output_schema: None,
        }
    }

    #[test]
    fn tool_digest_equal_under_reordered_input() {
        let schema_a = serde_json::json!({"type": "object"});
        let schema_b = serde_json::json!({"type": "string"});
        let a = entry("a", &schema_a);
        let b = entry("b", &schema_b);

        assert_eq!(ToolDigest::compute(&[a, b]), ToolDigest::compute(&[b, a]),);
    }

    #[test]
    fn tool_digest_equal_for_duplicate_names_swapped_order() {
        let schema_1 = serde_json::json!({"variant": 1});
        let schema_2 = serde_json::json!({"variant": 2});
        let first = ToolDigestEntry {
            name: "dup",
            description: "",
            input_schema: &schema_1,
            output_schema: None,
        };
        let second = ToolDigestEntry {
            name: "dup",
            description: "",
            input_schema: &schema_2,
            output_schema: None,
        };

        assert_eq!(
            ToolDigest::compute(&[first, second]),
            ToolDigest::compute(&[second, first]),
        );
    }

    #[test]
    fn tool_digest_differs_on_schema_edit() {
        let schema_a = serde_json::json!({"type": "object"});
        let schema_b = serde_json::json!({"type": "string"});
        assert_ne!(
            ToolDigest::compute(&[entry("a", &schema_a)]),
            ToolDigest::compute(&[entry("a", &schema_b)]),
        );
    }

    #[test]
    fn tool_digest_differs_on_tool_added() {
        let schema = serde_json::json!({"type": "object"});
        assert_ne!(
            ToolDigest::compute(&[entry("a", &schema)]),
            ToolDigest::compute(&[entry("a", &schema), entry("b", &schema)]),
        );
    }

    #[test]
    fn tool_digest_differs_on_tool_removed() {
        let schema = serde_json::json!({"type": "object"});
        assert_ne!(
            ToolDigest::compute(&[entry("a", &schema), entry("b", &schema)]),
            ToolDigest::compute(&[entry("a", &schema)]),
        );
    }

    #[test]
    fn tool_digest_equal_for_nested_object_key_reordering() {
        let schema_a = serde_json::json!({
            "type": "object",
            "properties": {"b": {"type": "string"}, "a": {"type": "number"}}
        });
        let schema_b = serde_json::json!({
            "properties": {"a": {"type": "number"}, "b": {"type": "string"}},
            "type": "object"
        });

        assert_eq!(
            ToolDigest::compute(&[entry("t", &schema_a)]),
            ToolDigest::compute(&[entry("t", &schema_b)]),
        );
    }

    /// N1: `output_schema`'s `None` and `Some(Value::Null)` must hash differently — the
    /// presence byte, not the value walk alone, is what distinguishes them.
    #[test]
    fn tool_digest_distinguishes_absent_output_schema_from_null_output_schema() {
        let input_schema = serde_json::json!({"type": "object"});
        let null_schema = serde_json::Value::Null;

        let without = ToolDigestEntry {
            name: "t",
            description: "",
            input_schema: &input_schema,
            output_schema: None,
        };
        let with_null = ToolDigestEntry {
            name: "t",
            description: "",
            input_schema: &input_schema,
            output_schema: Some(&null_schema),
        };

        assert_ne!(
            ToolDigest::compute(&[without]),
            ToolDigest::compute(&[with_null]),
        );
    }

    #[test]
    fn generation_provenance_capture_stamps_current_time() {
        let config = stdio_config("docker");
        let before = Utc::now();
        let provenance = GenerationProvenance::capture(&config, &[]);
        let after = Utc::now();
        assert!(provenance.generated_at >= before && provenance.generated_at <= after);
    }

    #[test]
    fn provenance_types_are_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<GenerationProvenance>();
        assert_sync::<GenerationProvenance>();
        assert_send::<ConfigFingerprint>();
        assert_sync::<ConfigFingerprint>();
        assert_send::<ToolDigest>();
        assert_sync::<ToolDigest>();
    }

    // -- Digest format validation (ConfigFingerprint/ToolDigest TryFrom<String>) --

    #[test]
    fn digest_try_from_accepts_valid_lowercase_hex() {
        let valid = "a".repeat(64);
        assert!(ConfigFingerprint::try_from(valid.clone()).is_ok());
        assert!(ToolDigest::try_from(valid).is_ok());
    }

    #[test]
    fn digest_try_from_rejects_wrong_length() {
        assert!(ConfigFingerprint::try_from("a".repeat(63)).is_err());
        assert!(ConfigFingerprint::try_from("a".repeat(65)).is_err());
        assert!(ConfigFingerprint::try_from(String::new()).is_err());
    }

    #[test]
    fn digest_try_from_rejects_uppercase_hex() {
        let uppercase = "A".repeat(64);
        assert!(ConfigFingerprint::try_from(uppercase).is_err());
    }

    #[test]
    fn digest_try_from_rejects_non_hex_characters() {
        let mut candidate = "a".repeat(63);
        candidate.push('g');
        assert!(ConfigFingerprint::try_from(candidate).is_err());
    }

    /// A `ConfigFingerprint`/`ToolDigest` deserialized from JSON goes through the same
    /// validation as direct `TryFrom<String>` construction — there is no bypass via `serde`.
    #[test]
    fn digest_deserialize_rejects_malformed_value() {
        let result: Result<ConfigFingerprint, _> = serde_json::from_str(r#""not-a-digest""#);
        assert!(result.is_err());
    }

    #[test]
    fn digest_deserialize_accepts_valid_value() {
        let valid = "b".repeat(64);
        let json = serde_json::to_string(&valid).unwrap();
        let fingerprint: ConfigFingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(fingerprint.as_str(), valid);
    }

    #[test]
    fn digest_format_error_sanitizes_rejected_value() {
        let err = ConfigFingerprint::try_from("bad&value".to_string()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("bad&amp;value"));
        assert!(!message.contains("bad&value"));
    }
}
