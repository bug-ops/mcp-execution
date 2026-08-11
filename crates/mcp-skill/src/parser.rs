//! Server metadata sidecar reader.
//!
//! Reads the structured `_meta.json` sidecar emitted by
//! `mcp-execution-codegen` alongside a server's generated TypeScript tool
//! files, and maps it into this crate's [`ParsedToolFile`] / [`ParsedParameter`]
//! types for skill generation.
//!
//! Prior to this module, tool metadata was recovered by re-parsing the
//! generated `.ts` files with regexes — a lossy, fragile round-trip that,
//! among other issues, could never recover parameter descriptions. The
//! sidecar is a structured, serde-derived contract shared with codegen via
//! `mcp_execution_core::metadata`, so no re-parsing of generated source is
//! needed at all.

use mcp_execution_core::metadata::{
    INDEX_FILE_NAME, METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ServerMetadata,
};
use regex::Regex;
use serde::Deserialize;
use serde_saphyr::budget::{BudgetBreach, BudgetReport};
use serde_saphyr::options::{DuplicateKeyPolicy, MergeKeyPolicy};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::LazyLock;
use thiserror::Error;

/// Maximum number of tools accepted from a single sidecar (denial-of-service protection).
pub const MAX_TOOL_FILES: usize = 500;

/// Maximum sidecar file size to read in bytes (1MB).
pub const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Maximum size of a `SKILL.md`'s extracted YAML frontmatter block, in bytes.
///
/// This is an independent pre-parse cap, not derived from the enclosing
/// `SKILL.md` size limit: bounding only the whole-document size would not by
/// itself bound this block's parse latency. A real `name`/`description`
/// frontmatter is a few hundred bytes at most, so 8KB is already generous
/// while keeping [`extract_skill_metadata`] cheap enough to run
/// synchronously on `save_skill`'s request-handling task. `serde-saphyr`'s
/// explicit parse `Budget` (see `frontmatter_options`) is a second,
/// parser-level bound layered on top of this cap, not a replacement for it.
///
/// This cap is the project-wide contract for any YAML entry point, not a local
/// detail of this one — see the project constitution's security section.
pub const MAX_FRONTMATTER_SIZE: usize = 8 * 1024;

// Locates the raw YAML block between a SKILL.md's `---` delimiters. The
// block's contents are handed to `serde-saphyr` for actual parsing, so this
// regex never inspects individual field values (see `extract_skill_metadata`).
static FRONTMATTER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^---\s*\n([\s\S]*?)\n---").expect("valid regex"));

// `sanitize_path_for_error` lives in `mcp-execution-core`, the workspace's
// security-validation foundation crate.
use mcp_execution_core::sanitize_path_for_error;

/// Errors that can occur while scanning a server directory for its `_meta.json` sidecar.
#[derive(Debug, Error)]
pub enum ScanError {
    /// I/O error reading the directory or sidecar file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Directory does not exist.
    #[error("directory does not exist: {path}")]
    DirectoryNotFound {
        /// Sanitized path of the missing directory.
        path: String,
    },

    /// The `_meta.json` sidecar is missing from the server directory.
    #[error("metadata sidecar not found: {path} (was the server directory regenerated?)")]
    MissingMetadata {
        /// Sanitized path of the expected sidecar file.
        path: String,
    },

    /// The `_meta.json` sidecar could not be parsed as valid `ServerMetadata` JSON.
    #[error("failed to parse metadata sidecar {path}: {source}")]
    MetadataParse {
        /// Sanitized path of the sidecar file that failed to parse.
        path: String,
        /// Underlying JSON deserialization error.
        #[source]
        source: serde_json::Error,
    },

    /// The sidecar's `schema_version` does not match the version this crate understands.
    #[error("unsupported metadata schema version: found {found}, expected {expected}")]
    UnsupportedSchema {
        /// Schema version read from the sidecar.
        found: u32,
        /// Schema version this crate supports (`METADATA_SCHEMA_VERSION`).
        expected: u32,
    },

    /// Too many tools in the sidecar (denial-of-service protection).
    #[error("too many tools: {count} exceeds limit of {limit}")]
    TooManyFiles {
        /// Number of tools listed in the sidecar.
        count: usize,
        /// Maximum allowed number of tools (`MAX_TOOL_FILES`).
        limit: usize,
    },

    /// Sidecar file too large to process.
    #[error("file too large: {path} ({size} bytes exceeds {limit} limit)")]
    FileTooLarge {
        /// Sanitized path of the oversized sidecar file.
        path: String,
        /// Actual size of the file, in bytes.
        size: u64,
        /// Maximum allowed size, in bytes (`MAX_FILE_SIZE`).
        limit: u64,
    },

    /// A tool listed in the `_meta.json` sidecar has no corresponding `.ts`
    /// file on disk.
    ///
    /// This indicates the sidecar and the generated TypeScript files have
    /// drifted apart — e.g. the file was deleted manually, or a `generate`
    /// run was interrupted before writing it.
    #[error(
        "stale metadata: tool '{tool}' is listed in {sidecar_path} but its file '{expected_file}' \
         is missing (re-run 'generate' to regenerate this server)"
    )]
    StaleMetadata {
        /// MCP tool name listed in the sidecar.
        tool: String,
        /// `.ts` file name expected on disk for `tool`.
        expected_file: String,
        /// Sanitized path of the sidecar that references `tool`.
        sidecar_path: String,
    },
}

/// Result of [`scan_tools_directory`]: the parsed tools plus any non-fatal
/// drift warnings.
///
/// A warning does not fail the scan — it flags a `.ts` file on disk that was
/// excluded from `tools` because the sidecar has no matching entry for it.
/// Callers that only inspect `tools` would otherwise have no way to detect
/// this drift short of tailing server-side `tracing` output.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// Parsed tools, sorted by name.
    pub tools: Vec<ParsedToolFile>,

    /// Non-fatal warnings, e.g. `.ts` files excluded for lacking a sidecar entry.
    pub warnings: Vec<String>,
}

/// Parsed metadata from a server's generated tool set.
#[derive(Debug, Clone)]
pub struct ParsedToolFile {
    /// Original MCP tool name.
    pub name: String,

    /// TypeScript function name (`PascalCase` filename).
    pub typescript_name: String,

    /// Server identifier.
    pub server_id: String,

    /// Category for grouping.
    pub category: Option<String>,

    /// Keywords for discovery.
    pub keywords: Vec<String>,

    /// Tool description.
    pub description: Option<String>,

    /// Parsed parameters for the tool.
    pub parameters: Vec<ParsedParameter>,
}

/// A parsed parameter from a tool's metadata.
#[derive(Debug, Clone)]
pub struct ParsedParameter {
    /// Parameter name.
    pub name: String,

    /// TypeScript type (e.g., "string", "number", "boolean").
    pub typescript_type: String,

    /// Whether the parameter is required.
    pub required: bool,

    /// Parameter description.
    pub description: Option<String>,
}

impl From<mcp_execution_core::metadata::ParameterMetadata> for ParsedParameter {
    fn from(meta: mcp_execution_core::metadata::ParameterMetadata) -> Self {
        Self {
            name: meta.name,
            typescript_type: meta.typescript_type,
            required: meta.required,
            description: meta.description,
        }
    }
}

/// Builds a [`ParsedToolFile`] from a sidecar tool entry and the server ID it belongs to.
///
/// A plain function rather than a `From<ToolMetadata>` impl: `ToolMetadata` carries no
/// `server_id` of its own (it lives once on the enclosing [`ServerMetadata`]), so a `From` impl
/// could only ever produce a `ParsedToolFile` with a placeholder `server_id` that every caller
/// then had to patch in after construction — a representable-but-wrong intermediate state.
/// Taking `server_id` as a parameter removes that sentinel from this construction path;
/// `ParsedToolFile`'s fields remain public, so callers that build one directly (e.g. test
/// fixtures) are unaffected and can still set an arbitrary `server_id` themselves.
fn parsed_tool_file_from_metadata(
    meta: mcp_execution_core::metadata::ToolMetadata,
    server_id: &str,
) -> ParsedToolFile {
    ParsedToolFile {
        name: meta.name.into_inner(),
        typescript_name: meta.typescript_name,
        server_id: server_id.to_string(),
        category: meta.category,
        keywords: meta.keywords,
        description: meta.description,
        parameters: meta.parameters.into_iter().map(Into::into).collect(),
    }
}

/// Scan a server directory and read its `_meta.json` sidecar.
///
/// Reads the structured metadata sidecar written by `mcp-execution-codegen`
/// and maps each tool entry into a [`ParsedToolFile`]. Unlike the former
/// regex-based `.ts` scanner, tool metadata (name, category, keywords,
/// parameters) is never re-parsed from generated TypeScript source — the
/// sidecar remains the single source of truth for that. However, each
/// sidecar entry's `.ts` file is cross-checked for existence on disk to
/// detect drift between the sidecar and the generated files (see issues
/// #154, #155): a missing file is a hard error, while an unreferenced `.ts`
/// file on disk is logged via `tracing::warn!`, omitted from the result, and
/// named in the returned [`ScanResult::warnings`] (see issue #161).
///
/// # Arguments
///
/// * `dir` - Path to server directory (e.g., `~/.claude/servers/github`)
///
/// # Returns
///
/// [`ScanResult`] with one `ParsedToolFile` per tool in the sidecar (sorted
/// by name) plus any non-fatal drift warnings.
///
/// # Errors
///
/// Returns `ScanError` if the directory doesn't exist, the sidecar is
/// missing or malformed, the sidecar's tool count exceeds
/// [`MAX_TOOL_FILES`], or a sidecar entry's `.ts` file is missing from disk
/// ([`ScanError::StaleMetadata`]).
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::scan_tools_directory;
/// use std::path::Path;
///
/// # async fn example() -> Result<(), mcp_execution_skill::ScanError> {
/// let result = scan_tools_directory(Path::new("/home/user/.claude/servers/github")).await?;
/// println!("Found {} tools", result.tools.len());
/// # Ok(())
/// # }
/// ```
pub async fn scan_tools_directory(dir: &Path) -> Result<ScanResult, ScanError> {
    // Canonicalize the base directory to resolve symlinks and get absolute path
    let canonical_base = tokio::fs::canonicalize(dir).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ScanError::DirectoryNotFound {
                path: sanitize_path_for_error(dir),
            }
        } else {
            ScanError::Io(err)
        }
    })?;

    let meta_path = canonical_base.join(METADATA_FILE_NAME);

    // SECURITY: Canonicalize the sidecar path and validate it stays within the base
    // directory, preventing path traversal via a symlinked `_meta.json`.
    let canonical_meta = match tokio::fs::canonicalize(&meta_path).await {
        Ok(path) if path.starts_with(&canonical_base) => path,
        Ok(_) => {
            return Err(ScanError::MissingMetadata {
                path: sanitize_path_for_error(&meta_path),
            });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ScanError::MissingMetadata {
                path: sanitize_path_for_error(&meta_path),
            });
        }
        Err(err) => return Err(ScanError::Io(err)),
    };

    let file_metadata = tokio::fs::metadata(&canonical_meta).await?;
    if file_metadata.len() > MAX_FILE_SIZE {
        return Err(ScanError::FileTooLarge {
            path: sanitize_path_for_error(&meta_path),
            size: file_metadata.len(),
            limit: MAX_FILE_SIZE,
        });
    }

    let content = tokio::fs::read_to_string(&canonical_meta).await?;

    let meta: ServerMetadata =
        serde_json::from_str(&content).map_err(|source| ScanError::MetadataParse {
            path: sanitize_path_for_error(&meta_path),
            source,
        })?;

    if meta.schema_version != METADATA_SCHEMA_VERSION {
        return Err(ScanError::UnsupportedSchema {
            found: meta.schema_version,
            expected: METADATA_SCHEMA_VERSION,
        });
    }

    if meta.tools.len() > MAX_TOOL_FILES {
        return Err(ScanError::TooManyFiles {
            count: meta.tools.len(),
            limit: MAX_TOOL_FILES,
        });
    }

    let warnings = verify_tool_files_on_disk(&canonical_base, &meta.tools, &meta_path).await?;

    let server_id = meta.server_id.into_inner();
    let mut tools: Vec<ParsedToolFile> = meta
        .tools
        .into_iter()
        .map(|tool| parsed_tool_file_from_metadata(tool, &server_id))
        .collect();

    // Sort by name for consistent ordering
    tools.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ScanResult { tools, warnings })
}

/// Cross-checks sidecar tool entries against the `.ts` files actually
/// present in `dir`, guarding against drift between `_meta.json` and the
/// generated TypeScript output (see issues #154, #155).
///
/// Every sidecar entry must have a matching `{typescript_name}.ts` file, or
/// this returns [`ScanError::StaleMetadata`]. `.ts` files present on disk
/// but not referenced by the sidecar are not fatal — regenerating tool
/// files is a normal part of `generate` — but are logged via
/// `tracing::warn!` and returned as human-readable warning strings so the
/// drift isn't silently dropped from `SKILL.md` or invisible to structured
/// callers (issue #161).
///
/// # Errors
///
/// Returns `ScanError::Io` if the directory cannot be read, or
/// `ScanError::StaleMetadata` if a sidecar entry's `.ts` file is missing.
async fn verify_tool_files_on_disk(
    dir: &Path,
    tools: &[mcp_execution_core::metadata::ToolMetadata],
    meta_path: &Path,
) -> Result<Vec<String>, ScanError> {
    let mut expected_files: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(tools.len());

    for tool in tools {
        let file_name = format!("{}.ts", tool.typescript_name);
        if !dir.join(&file_name).is_file() {
            return Err(ScanError::StaleMetadata {
                tool: tool.name.to_string(),
                expected_file: file_name,
                sidecar_path: sanitize_path_for_error(meta_path),
            });
        }
        expected_files.insert(file_name);
    }

    let mut warnings = Vec::new();
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("ts") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
            continue;
        };
        if file_name == INDEX_FILE_NAME || expected_files.contains(file_name) {
            continue;
        }
        tracing::warn!(
            file = %file_name,
            "found .ts tool file not referenced by _meta.json; it will be omitted from SKILL.md \
             (re-run 'generate' to refresh the sidecar)"
        );
        warnings.push(format!(
            "'{file_name}' is not referenced by _meta.json and was excluded from SKILL.md \
             (re-run 'generate' to refresh the sidecar)"
        ));
    }

    Ok(warnings)
}

/// Errors that can occur while extracting [`SkillMetadata`](crate::types::SkillMetadata)
/// from a `SKILL.md`'s YAML frontmatter.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SkillMetadataError {
    /// The content did not start with a `---`-delimited YAML frontmatter block.
    #[error("YAML frontmatter not found")]
    MissingFrontmatter,

    /// The extracted frontmatter block exceeded `MAX_FRONTMATTER_SIZE`.
    #[error("YAML frontmatter too large: {size} bytes exceeds {limit} limit")]
    FrontmatterTooLarge {
        /// Actual size of the rejected frontmatter block, in bytes.
        size: usize,
        /// Maximum allowed size (`MAX_FRONTMATTER_SIZE`).
        limit: usize,
    },

    /// The frontmatter block was not valid YAML.
    ///
    /// The message is built by this crate from `serde-saphyr`'s structured
    /// [`serde_saphyr::Error::location`] and a small fixed vocabulary of failure kinds
    /// (see `yaml_error_kind`) — never from the parser's own rendered `Display` text. This
    /// keeps both frontmatter source content and internal `serde-saphyr` hints (e.g. a
    /// duplicate-key error's config-option suggestion) out of this LLM/client-facing error.
    #[error("failed to parse YAML frontmatter: {0}")]
    InvalidYaml(String),

    /// The frontmatter block breached `serde-saphyr`'s parse [`Budget`](serde_saphyr::Budget)
    /// (see `frontmatter_options`) — e.g. an alias-bomb-shaped input placed under a key
    /// `RawFrontmatter` does not declare. A bomb placed directly under a *declared* field
    /// (`name`/`description`) instead short-circuits on a type mismatch and surfaces as
    /// [`SkillMetadataError::InvalidYaml`]; see `RawFrontmatter`'s doc comment.
    #[error("YAML frontmatter is too complex to parse")]
    FrontmatterTooComplex,

    /// A required field was absent, or present but empty, in an otherwise
    /// valid frontmatter block.
    #[error("'{field}' field is missing or empty in frontmatter")]
    MissingField {
        /// Name of the missing/empty field (e.g. `"name"`, `"description"`).
        field: &'static str,
    },

    /// The frontmatter `name` exceeded [`MAX_SKILL_NAME_LENGTH`](crate::types::MAX_SKILL_NAME_LENGTH).
    ///
    /// Enforced here rather than only on the two `generate` call sites
    /// (`crates/mcp-cli/src/commands/skill.rs`, `crates/mcp-server/src/service.rs`) so that
    /// `save_skill` — which writes caller-supplied `SKILL.md` content directly and never routes
    /// through [`crate::types::validate_skill_name`] — cannot persist an unbounded `name` into
    /// the always-loaded skill index (issue #419).
    #[error("skill_name too long: {len} chars exceeds {limit} limit")]
    NameTooLong {
        /// Actual length of the rejected name, in `char`s.
        len: usize,
        /// Maximum allowed length (`MAX_SKILL_NAME_LENGTH`).
        limit: usize,
    },
}

/// Fixed vocabulary of `serde-saphyr` failure kinds, keyed by [`serde_saphyr::Error`] variant.
///
/// Never matches on `err`'s rendered message: `serde_saphyr::Error`'s `Display`/`to_string()`
/// can carry attacker-controlled frontmatter content (e.g. a duplicate key's name) even with
/// [`serde_saphyr::Options::with_snippet`] disabled, which [`describe_yaml_error`] must not
/// reproduce into an LLM/client-facing [`SkillMetadataError::InvalidYaml`].
const fn yaml_error_kind(err: &serde_saphyr::Error) -> &'static str {
    match err {
        serde_saphyr::Error::DuplicateMappingKey { .. } => "duplicate key in YAML frontmatter",
        serde_saphyr::Error::MultipleDocuments { .. } => "multiple YAML documents in frontmatter",
        serde_saphyr::Error::UnknownAnchor { .. } | serde_saphyr::Error::Unexpected { .. } => {
            "unexpected YAML value"
        }
        _ => "invalid YAML",
    }
}

/// Renders a `serde-saphyr` deserialization error from its structured
/// [`serde_saphyr::Error::location`] and [`yaml_error_kind`], correcting the line number to be
/// relative to the whole `SKILL.md` file rather than the frontmatter block passed to
/// `serde_saphyr::from_str_with_options` (see [`SkillMetadataError::InvalidYaml`]).
///
/// Deliberately does not touch `err`'s own `Display`/`to_string()` output — see
/// [`yaml_error_kind`]'s doc comment for why.
fn describe_yaml_error(err: &serde_saphyr::Error) -> String {
    let kind = yaml_error_kind(err);
    let Some(location) = err.location() else {
        return kind.to_string();
    };
    // The block starts one line after the file's opening `---`, so the
    // block-relative line number under-counts the file line by exactly one.
    format!(
        "{kind} at line {} column {}",
        location.line() + 1,
        location.column()
    )
}

/// Whether a parse failure reflects a breach of `frontmatter_options`'s
/// [`serde_saphyr::Budget`] or alias-replay limits, as opposed to an ordinary syntax/type
/// error.
///
/// `budget_breach` is the authoritative signal, threaded from `frontmatter_options`'s
/// registered budget-report callback (`serde_saphyr::budget::BudgetReport::breached`) — it
/// reflects the actual budget scan outcome, not a guess based on which `Error` variant the
/// parse failure happens to surface as.
///
/// `err`'s own variant is consulted only as a fallback, and deliberately does **not** include
/// `Error::AliasError`: that variant is a generic wrapper `serde-saphyr` attaches to *any*
/// error raised while deserializing a value reached through an alias, not a budget-specific
/// one — e.g. `base: &a [1, 2]\nname: *a` (an ordinary type mismatch under an alias) also
/// surfaces as `AliasError`. An earlier version of this function matched `AliasError`
/// unconditionally and misclassified that case as `FrontmatterTooComplex`
/// (`tests::test_alias_wrapped_type_mismatch_is_not_a_budget_breach` pins the fix).
/// `Error::Budget` is the direct (non-alias) breach path — e.g. a depth or raw-node breach with
/// no aliases involved. The four `AliasReplay*`/`AliasExpansion*` variants are additional
/// direct alias-limit failures ([`serde_saphyr::Options::alias_limits`]) distinct from a
/// [`serde_saphyr::Budget`] breach. `Error::UnknownAnchor` (an alias referencing an anchor that
/// was never defined — a typo, not amplification) deliberately falls through to `false`.
const fn is_budget_breach(err: &serde_saphyr::Error, budget_breach: Option<&BudgetBreach>) -> bool {
    budget_breach.is_some()
        || matches!(
            err,
            serde_saphyr::Error::Budget { .. }
                | serde_saphyr::Error::AliasReplayLimitExceeded { .. }
                | serde_saphyr::Error::AliasExpansionLimitExceeded { .. }
                | serde_saphyr::Error::AliasReplayStackDepthExceeded { .. }
                | serde_saphyr::Error::AliasReplayCounterOverflow { .. }
        )
}

/// Builds the `serde-saphyr` parse configuration for `SKILL.md` frontmatter, plus a shared
/// handle that captures the parse's [`BudgetBreach`] (if any) once parsing completes.
///
/// Every field of the [`serde_saphyr::Budget`] below is set explicitly (never left to
/// `Budget`'s own defaults) so a future upstream default change cannot silently loosen this
/// crate's parse-time bound; `Options`' handful of other fields not named here (e.g.
/// `legacy_octal_numbers`, `strict_booleans`) are left at their defaults deliberately — they
/// affect scalar parsing semantics, not the parse-time bound this function exists to configure.
/// Every numeric `Budget` limit is sized so that legitimate frontmatter which already cleared
/// [`MAX_FRONTMATTER_SIZE`] (8 KiB) cannot be rejected by the budget — see
/// `specs/decisions/ADR-405-adopt-serde-saphyr.md` for the measured margins behind each value.
///
/// Two settings named in this crate's original design table —
/// `max_buffered_comment_events` and `simple_key_max_lookahead` — do not exist as
/// [`serde_saphyr::Budget`]/[`serde_saphyr::Options`] fields in `serde-saphyr` 1.0.1 (verified
/// against the published source, not assumed) and are omitted here rather than guessed at.
///
/// The returned handle is populated by a `budget_report` callback
/// ([`serde_saphyr::Options::with_budget_report`]) registered on the returned `Options`, which
/// `serde-saphyr` invokes once per parse — both on success and on a budget breach — with the
/// scan's [`BudgetReport`]. This is what [`is_budget_breach`] uses as its authoritative signal,
/// rather than inferring a breach from which `serde_saphyr::Error` variant a parse failure
/// happens to surface as (see [`is_budget_breach`]'s doc comment for why that inference is
/// unreliable). Each call to this function returns a fresh, independent handle, so a caller
/// must call it once per parse and read the handle only after that specific `from_str_with_options`
/// call returns.
fn frontmatter_options() -> (serde_saphyr::Options, Rc<Cell<Option<BudgetBreach>>>) {
    let budget_breach: Rc<Cell<Option<BudgetBreach>>> = Rc::new(Cell::new(None));
    let budget_breach_for_callback = Rc::clone(&budget_breach);

    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            // 2 bytes/node is the densest valid construction (`[a,a,...]`); <=4096 nodes from
            // 8 KiB. 2x margin over that ceiling.
            max_nodes: 8_192,
            // 2 events/node, aligned with max_nodes.
            max_events: 16_384,
            // `&a` is >=2 bytes; <=4096 anchors from 8 KiB, 2x margin. Must stay non-zero or
            // `merge_keys` below becomes dead configuration (anchors alone cannot amplify
            // without aliases).
            max_anchors: 8_192,
            // `*a` is >=2 bytes; <=4096 aliases from 8 KiB, 2x margin.
            max_aliases: 8_192,
            // 8x the input cap; bounds scalar bytes materialized by alias replay.
            max_total_scalar_bytes: 65_536,
            // `<<:` is >=3 bytes; <=2730 merge keys from 8 KiB, ~1.5x margin.
            max_merge_keys: 4_096,
            // This crate's single-document call path never lets the budget's own
            // `max_documents` counter observe a second document: `from_str_with_options`
            // fully parses (and budget-checks) the first document, THEN peeks for trailing
            // content and raises `Error::MultipleDocuments` directly if any is found -- a
            // second document's own `DocumentStart` event is never budget-checked through this
            // entry point. Kept above the theoretical minimum of 1 anyway, matching this
            // table's general margin convention, even though it is not reachable here.
            max_documents: 2,
            // Deliberate exception to the size-derived sizing rule above (see the project
            // constitution's YAML parse-time bound section): 8 KiB of `[[[[...` nests
            // thousands deep, so no size-derived value would be meaningful. This only matters
            // on the unknown-key/buffering path — a bomb under a *declared* `Option<String>`
            // field short-circuits on the same type mismatch regardless of depth (see
            // `RawFrontmatter`'s doc comment); `max_depth` does not protect that path.
            max_depth: 64,
            // Built-in heuristic, kept at its default; does not fire on this crate's reference
            // alias-bomb fixture (57 aliases < 100) — `max_nodes` is what fires first.
            enforce_alias_anchor_ratio: true,
            alias_anchor_min_aliases: 100,
            alias_anchor_ratio_multiplier: 10,
            // Default value; unreachable from an 8 KiB input, set explicitly for the record.
            max_total_comment_bytes: 64 * 1024 * 1024,
            // Default value; reader-only, `from_str_with_options` never consults it for a
            // `&str` input, set explicitly for the record.
            max_reader_input_bytes: Some(256 * 1024 * 1024),
            // Default value; the `include` feature is not enabled, set explicitly for the
            // record.
            max_inclusion_depth: 24,
        },
        alias_limits: serde_saphyr::alias_limits! {
            // Aligned with `max_events`; the direct billion-laughs bound.
            max_total_replayed_events: 16_384,
        },
        // `<<: *anchor` must not inject fields from an anchored map into `name`/`description`;
        // treat it as an ordinary (unrecognized) key instead.
        merge_keys: MergeKeyPolicy::AsOrdinary,
        // Equals the default; set explicitly so an upstream default change cannot loosen it.
        duplicate_keys: DuplicateKeyPolicy::Error,
        // Load-bearing: `Error::WithSnippet` is then never constructed, so even an accidental
        // `to_string()` cannot echo frontmatter source text back to an MCP client/LLM (see the
        // project constitution's no-untrusted-source-echo rule). `describe_yaml_error`
        // additionally never touches `Display`/`to_string()` at all (see `yaml_error_kind`).
        with_snippet: false,
    }
    .with_budget_report(move |report: BudgetReport| {
        budget_breach_for_callback.set(report.breached);
    });

    (options, budget_breach)
}

/// Raw shape of a `SKILL.md`'s YAML frontmatter block.
///
/// Both fields are optional at the YAML level so that a missing `name` and a
/// missing `description` are reported as distinct
/// [`SkillMetadataError::MissingField`] variants rather than being folded
/// into one generic deserialization failure.
///
/// # `DoS` defense (issue #405 / ADR-405)
///
/// Parsing is bounded by `frontmatter_options`'s explicit [`serde_saphyr::Budget`], not by this
/// struct's field shape — unlike this crate's previous `serde_norway`-based parser, whose
/// alias-bomb resistance was incidental to `RawFrontmatter` having no buffering field. The
/// budget is *not* shape-independent, though: an alias bomb placed under an *undeclared* YAML
/// key is materialized (nothing here declares it, so it falls through to the generic visitor)
/// and reaches the budget, surfacing as [`SkillMetadataError::FrontmatterTooComplex`]
/// (`tests::test_extract_skill_metadata_alias_bomb_under_unknown_key_rejected_by_budget`). A
/// bomb placed directly under a *declared* `Option<String>` field instead short-circuits on
/// serde's type mismatch before the budget accumulates anything, exactly as the previous parser
/// did, surfacing as [`SkillMetadataError::InvalidYaml`]
/// (`tests::test_extract_skill_metadata_alias_bomb_under_declared_field_short_circuits`).
///
/// If either field is ever changed to a buffering type (e.g. `serde_json::Value`, an untagged
/// enum) or gains a buffering `#[serde(deserialize_with)]`, a bomb placed under that field
/// reopens the materialization path and reaches the budget instead of short-circuiting — still
/// bounded and rejected, not silently expanded, but via `FrontmatterTooComplex` rather than a
/// type-mismatch `InvalidYaml`. See
/// `tests::test_alias_bomb_rejection_survives_a_buffering_field_shape`, which pins this against
/// a local test-only struct shaped like that hypothetical retype.
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

/// Returns `value` if present and non-blank, otherwise a
/// [`SkillMetadataError::MissingField`] naming `field`.
///
/// Treats an absent key, a null/empty scalar (`name:`, `name: null`,
/// `name: ~`), and a blank string (`name: ""`, `name: "   "`) as equally
/// invalid — a skill with an empty name or description is not usable.
fn require_field(value: Option<String>, field: &'static str) -> Result<String, SkillMetadataError> {
    match value {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(SkillMetadataError::MissingField { field }),
    }
}

/// Extract skill metadata from SKILL.md content.
///
/// Parses YAML frontmatter to extract name and description, and counts
/// sections (H2 headers) and words. Frontmatter is parsed with a real YAML
/// parser (`serde-saphyr`), so block scalars (`description: |` / `>`) and
/// quoted scalars (`name: "my-name"`) are handled per the YAML spec rather
/// than by a single-line regex capture. Parsing is bounded by an explicit
/// [`serde_saphyr::Budget`] (see `frontmatter_options`), not just the
/// pre-parse `MAX_FRONTMATTER_SIZE` size cap.
///
/// # Arguments
///
/// * `content` - SKILL.md content with YAML frontmatter
///
/// # Returns
///
/// `SkillMetadata` with extracted information.
///
/// # Errors
///
/// Returns [`SkillMetadataError`] if the YAML frontmatter is missing, too
/// large (`MAX_FRONTMATTER_SIZE`), too complex to parse within its `Budget`,
/// malformed, a required field (`name`, `description`) is absent or empty, or
/// `name` exceeds [`MAX_SKILL_NAME_LENGTH`](crate::types::MAX_SKILL_NAME_LENGTH).
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::extract_skill_metadata;
///
/// let content = r"---
/// name: github-progressive
/// description: GitHub MCP server operations
/// ---
///
/// # GitHub Progressive
///
/// ## Quick Start
///
/// Content here.
/// ";
///
/// let metadata = extract_skill_metadata(content).unwrap();
/// assert_eq!(metadata.name, "github-progressive");
/// assert_eq!(metadata.description, "GitHub MCP server operations");
/// ```
pub fn extract_skill_metadata(
    content: &str,
) -> Result<crate::types::SkillMetadata, SkillMetadataError> {
    use crate::types::SkillMetadata;

    // Locate the raw YAML block between the `---` delimiters (using pre-compiled regex).
    let frontmatter_block = FRONTMATTER_REGEX
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or(SkillMetadataError::MissingFrontmatter)?;

    // Bound parse cost before handing the block to `serde-saphyr`: even with an explicit parse
    // Budget, the overall SKILL.md size limit (`MAX_SKILL_CONTENT_SIZE` in mcp-execution-server)
    // alone would not bound this block's parse latency — the budget's own limits are sized
    // against this narrower 8 KiB cap (see `frontmatter_options`).
    if frontmatter_block.len() > MAX_FRONTMATTER_SIZE {
        return Err(SkillMetadataError::FrontmatterTooLarge {
            size: frontmatter_block.len(),
            limit: MAX_FRONTMATTER_SIZE,
        });
    }

    let (options, budget_breach) = frontmatter_options();
    let frontmatter: RawFrontmatter =
        serde_saphyr::from_str_with_options(frontmatter_block, options).map_err(|e| {
            if is_budget_breach(&e, budget_breach.take().as_ref()) {
                SkillMetadataError::FrontmatterTooComplex
            } else {
                SkillMetadataError::InvalidYaml(describe_yaml_error(&e))
            }
        })?;

    let name = require_field(frontmatter.name, "name")?;
    match crate::types::validate_skill_name(&name) {
        Ok(()) => {}
        // `require_field` already rejects a blank/empty `name`, so this arm is unreachable via
        // this call site today — mapped onto the equivalent `MissingField` rather than a `panic!`
        // or a second, inconsistent "empty" error so the match stays exhaustive and safe even if
        // `require_field`'s blank-rejection is ever relaxed.
        Err(crate::types::SkillNameError::Empty) => {
            return Err(SkillMetadataError::MissingField { field: "name" });
        }
        Err(crate::types::SkillNameError::TooLong { len, limit }) => {
            return Err(SkillMetadataError::NameTooLong { len, limit });
        }
    }
    let description = require_field(frontmatter.description, "description")?;

    // Count sections (H2 headers)
    let section_count = content.lines().filter(|l| l.starts_with("## ")).count();

    // Count words (approximate)
    let word_count = content.split_whitespace().count();

    Ok(SkillMetadata {
        name,
        description,
        section_count,
        word_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::metadata::{ParameterMetadata, ToolMetadata};
    use mcp_execution_core::{ServerId, ToolName};
    use tempfile::TempDir;

    fn sample_metadata(tool_count: usize) -> ServerMetadata {
        ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: ServerId::new("github").unwrap(),
            server_name: "GitHub".to_string(),
            server_version: "1.0.0".to_string(),
            tools: (0..tool_count)
                .map(|i| ToolMetadata {
                    name: ToolName::new(format!("tool_{i}")).unwrap(),
                    typescript_name: format!("tool{i}"),
                    category: Some("test".to_string()),
                    keywords: vec!["test".to_string()],
                    description: Some(format!("Tool {i}")),
                    parameters: vec![ParameterMetadata {
                        name: "param".to_string(),
                        typescript_type: "string".to_string(),
                        required: true,
                        description: Some("A parameter".to_string()),
                    }],
                })
                .collect(),
        }
    }

    /// Writes `_meta.json` plus a matching stub `.ts` file for each tool, since
    /// `scan_tools_directory` cross-checks the sidecar against files on disk.
    async fn write_metadata(dir: &Path, meta: &ServerMetadata) {
        let content = serde_json::to_string_pretty(meta).unwrap();
        tokio::fs::write(dir.join(METADATA_FILE_NAME), content)
            .await
            .unwrap();

        for tool in &meta.tools {
            tokio::fs::write(
                dir.join(format!("{}.ts", tool.typescript_name)),
                "export {}",
            )
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_round_trip_preserves_parameter_descriptions() {
        // Issue #141 regression: the old regex-based parser hard-coded parameter
        // descriptions to `None`. The sidecar-backed scanner must preserve them.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(2);
        write_metadata(temp_dir.path(), &meta).await;

        let result = scan_tools_directory(temp_dir.path()).await.unwrap();
        let tools = result.tools;

        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "tool_0");
        assert_eq!(tools[0].server_id, "github");
        assert_eq!(tools[0].parameters.len(), 1);
        assert_eq!(
            tools[0].parameters[0].description,
            Some("A parameter".to_string()),
            "parameter descriptions must survive the sidecar round-trip"
        );
        assert!(result.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_scan_tools_directory_sorts_by_name() {
        let temp_dir = TempDir::new().unwrap();
        let mut meta = sample_metadata(0);
        meta.tools = vec![
            ToolMetadata {
                name: ToolName::new("zebra").unwrap(),
                typescript_name: "zebra".to_string(),
                category: None,
                keywords: vec![],
                description: None,
                parameters: vec![],
            },
            ToolMetadata {
                name: ToolName::new("alpha").unwrap(),
                typescript_name: "alpha".to_string(),
                category: None,
                keywords: vec![],
                description: None,
                parameters: vec![],
            },
        ];
        write_metadata(temp_dir.path(), &meta).await;

        let tools = scan_tools_directory(temp_dir.path()).await.unwrap().tools;

        assert_eq!(tools[0].name, "alpha");
        assert_eq!(tools[1].name, "zebra");
    }

    #[tokio::test]
    async fn test_scan_tools_directory_stale_metadata_missing_ts_file() {
        // Issue #154/#155 regression: a sidecar entry whose `.ts` file was
        // deleted (or never written, e.g. an interrupted `generate`) must be
        // reported instead of silently vanishing from `SKILL.md`.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(1);
        // Write only the sidecar, not the tool's `.ts` file.
        let content = serde_json::to_string_pretty(&meta).unwrap();
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), content)
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::StaleMetadata {
                tool,
                expected_file,
                ..
            }) => {
                assert_eq!(tool, "tool_0");
                assert_eq!(expected_file, "tool0.ts");
            }
            other => panic!("expected StaleMetadata, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_stale_metadata_reports_first_missing_in_sidecar_order() {
        // With multiple tools in the sidecar, only some of which have a missing
        // `.ts` file, the check short-circuits on the first missing entry in
        // sidecar order rather than scanning every tool up front.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(3);
        let content = serde_json::to_string_pretty(&meta).unwrap();
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), content)
            .await
            .unwrap();

        // Only write the `.ts` file for the middle tool; `tool_0` and `tool_2`
        // are both missing, but `tool_0` is first in sidecar order.
        tokio::fs::write(temp_dir.path().join("tool1.ts"), "export {}")
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::StaleMetadata {
                tool,
                expected_file,
                ..
            }) => {
                assert_eq!(tool, "tool_0");
                assert_eq!(expected_file, "tool0.ts");
            }
            other => panic!("expected StaleMetadata for tool_0, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_extra_ts_file_excluded_from_result() {
        // Issue #154/#155 regression: a `.ts` file on disk that the sidecar
        // does not reference (e.g. left over from a renamed/removed tool) must
        // not be fatal and must not leak into the scan result — it is logged
        // via `tracing::warn!` instead.
        //
        // Issue #161: the drift must also be visible in the returned
        // `ScanResult::warnings`, not just in the tracing log line.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(1);
        write_metadata(temp_dir.path(), &meta).await;

        tokio::fs::write(temp_dir.path().join("orphan.ts"), "export {}")
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await.unwrap();

        assert_eq!(
            result.tools.len(),
            1,
            "the orphaned .ts file must not be reported as a tool"
        );
        assert_eq!(result.tools[0].name, "tool_0");
        assert_eq!(
            result.warnings.len(),
            1,
            "the orphaned .ts file must be surfaced as a warning"
        );
        assert!(
            result.warnings[0].contains("orphan.ts"),
            "warning must name the excluded file: {:?}",
            result.warnings[0]
        );
    }

    #[tokio::test]
    async fn test_scan_tools_directory_index_ts_not_treated_as_extra() {
        // `index.ts` is the generated aggregator file and is never listed in
        // the sidecar; its presence alone must not affect the scan result.
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(1);
        write_metadata(temp_dir.path(), &meta).await;

        tokio::fs::write(temp_dir.path().join("index.ts"), "export * from './tool0';")
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await.unwrap();

        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "tool_0");
        assert!(
            result.warnings.is_empty(),
            "index.ts must not be reported as a warning"
        );
    }

    #[test]
    fn test_stale_metadata_error_message_tells_user_to_regenerate() {
        let err = ScanError::StaleMetadata {
            tool: "create_issue".to_string(),
            expected_file: "createIssue.ts".to_string(),
            sidecar_path: "~/.claude/servers/github/_meta.json".to_string(),
        };

        let message = err.to_string();
        assert!(
            message.contains("create_issue"),
            "message must name the affected tool"
        );
        assert!(
            message.contains("createIssue.ts"),
            "message must name the missing file"
        );
        assert!(
            message.contains("re-run 'generate'"),
            "message must tell the user how to fix it: {message}"
        );
    }

    #[tokio::test]
    async fn test_scan_tools_directory_missing_metadata() {
        let temp_dir = TempDir::new().unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        assert!(matches!(result, Err(ScanError::MissingMetadata { .. })));
    }

    #[tokio::test]
    async fn test_scan_tools_directory_corrupt_json() {
        let temp_dir = TempDir::new().unwrap();
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), "{not valid json")
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        assert!(matches!(result, Err(ScanError::MetadataParse { .. })));
    }

    /// Regression test for issue #317: `ServerMetadata.server_id`/`ToolMetadata.name` now go
    /// through `ServerId`/`ToolName`'s `#[serde(try_from = "String")]`, so a sidecar that is
    /// syntactically valid JSON but carries a semantically-invalid `server_id` (here, one
    /// containing a path separator) now fails to deserialize at all — surfacing as
    /// `ScanError::MetadataParse` — where before this change it would have deserialized as a
    /// plain, unvalidated `String`.
    #[tokio::test]
    async fn test_scan_tools_directory_rejects_invalid_server_id_in_valid_json() {
        let temp_dir = TempDir::new().unwrap();
        let json = r#"{
            "schema_version": 1,
            "server_id": "not/a/valid/id",
            "server_name": "GitHub",
            "server_version": "1.0.0",
            "tools": []
        }"#;
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), json)
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        assert!(matches!(result, Err(ScanError::MetadataParse { .. })));
    }

    /// Same regression as above, for `ToolMetadata.name`.
    #[tokio::test]
    async fn test_scan_tools_directory_rejects_invalid_tool_name_in_valid_json() {
        let temp_dir = TempDir::new().unwrap();
        let json = r#"{
            "schema_version": 1,
            "server_id": "github",
            "server_name": "GitHub",
            "server_version": "1.0.0",
            "tools": [{
                "name": "../escape",
                "typescript_name": "escape",
                "category": null,
                "keywords": [],
                "description": null,
                "parameters": []
            }]
        }"#;
        tokio::fs::write(temp_dir.path().join(METADATA_FILE_NAME), json)
            .await
            .unwrap();

        let result = scan_tools_directory(temp_dir.path()).await;

        assert!(matches!(result, Err(ScanError::MetadataParse { .. })));
    }

    #[tokio::test]
    async fn test_scan_tools_directory_unsupported_schema() {
        let temp_dir = TempDir::new().unwrap();
        let mut meta = sample_metadata(1);
        meta.schema_version = METADATA_SCHEMA_VERSION + 1;
        write_metadata(temp_dir.path(), &meta).await;

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::UnsupportedSchema { found, expected }) => {
                assert_eq!(found, METADATA_SCHEMA_VERSION + 1);
                assert_eq!(expected, METADATA_SCHEMA_VERSION);
            }
            other => panic!("expected UnsupportedSchema, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_too_many_tools() {
        let temp_dir = TempDir::new().unwrap();
        let meta = sample_metadata(MAX_TOOL_FILES + 1);
        write_metadata(temp_dir.path(), &meta).await;

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::TooManyFiles { count, limit }) => {
                assert_eq!(count, MAX_TOOL_FILES + 1);
                assert_eq!(limit, MAX_TOOL_FILES);
            }
            other => panic!("expected TooManyFiles, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_file_too_large() {
        let temp_dir = TempDir::new().unwrap();
        let mut meta = sample_metadata(1);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MAX_FILE_SIZE (1MB) always fits in usize; the cast cannot truncate."
        )]
        let padding = "a".repeat((MAX_FILE_SIZE as usize) + 1);
        meta.tools[0].description = Some(padding);
        write_metadata(temp_dir.path(), &meta).await;

        let result = scan_tools_directory(temp_dir.path()).await;

        match result {
            Err(ScanError::FileTooLarge { size, limit, .. }) => {
                assert!(size > MAX_FILE_SIZE);
                assert_eq!(limit, MAX_FILE_SIZE);
            }
            other => panic!("expected FileTooLarge, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_scan_tools_directory_nonexistent() {
        let result = scan_tools_directory(Path::new("/nonexistent/path/for/testing")).await;

        assert!(matches!(result, Err(ScanError::DirectoryNotFound { .. })));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_scan_tools_directory_canonicalize_non_not_found_error_propagates_as_io() {
        // Issue #302 regression: a symlink loop makes `canonicalize` fail with
        // `ErrorKind::FilesystemLoop`/`Other` (never `NotFound`). Before the fix,
        // every canonicalize failure — regardless of kind — collapsed into
        // `DirectoryNotFound`, silently discarding the real error.
        let temp_dir = TempDir::new().unwrap();
        let loop_path = temp_dir.path().join("loop");
        std::os::unix::fs::symlink(&loop_path, &loop_path).unwrap();

        let result = scan_tools_directory(&loop_path).await;

        match result {
            Err(ScanError::Io(err)) => {
                assert_ne!(err.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected ScanError::Io, got: {other:?}"),
        }
    }

    // ========================================================================
    // extract_skill_metadata Tests
    // ========================================================================

    #[test]
    fn test_extract_skill_metadata_valid() {
        let content = r"---
name: github-progressive
description: GitHub MCP server operations
---

# GitHub Progressive

## Quick Start

Content here.

## Common Tasks

More content.
";

        let result = extract_skill_metadata(content);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.name, "github-progressive");
        assert_eq!(metadata.description, "GitHub MCP server operations");
        assert_eq!(metadata.section_count, 2);
        assert!(metadata.word_count > 0);
    }

    #[test]
    fn test_extract_skill_metadata_no_frontmatter() {
        let content = "# Test\n\nNo frontmatter";

        let result = extract_skill_metadata(content);
        assert!(matches!(
            result,
            Err(SkillMetadataError::MissingFrontmatter)
        ));
    }

    #[test]
    fn test_extract_skill_metadata_missing_name() {
        let content = "---\ndescription: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        assert!(matches!(
            result,
            Err(SkillMetadataError::MissingField { field: "name" })
        ));
    }

    #[test]
    fn test_extract_skill_metadata_missing_description() {
        let content = "---\nname: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        assert!(matches!(
            result,
            Err(SkillMetadataError::MissingField {
                field: "description"
            })
        ));
    }

    #[test]
    fn test_extract_skill_metadata_invalid_yaml() {
        // Syntactically invalid YAML (an unterminated flow sequence) must surface
        // as `SkillMetadataError::InvalidYaml`, built from the underlying serde-saphyr error's
        // structured location plus a fixed-vocabulary kind (never the parser's own message).
        let content = "---\nname: [unterminated\ndescription: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        let Err(SkillMetadataError::InvalidYaml(message)) = &result else {
            panic!("expected InvalidYaml, got: {result:?}");
        };
        // Issue #203 follow-up (M3): the error's line number must be file-relative, not
        // relative to the extracted frontmatter block. `name: [unterminated` is file line 2
        // (after the opening `---` on line 1); the block-relative location.line() the
        // underlying serde-saphyr error reports for this input is 1.
        assert!(
            message.contains("line 2"),
            "expected file-relative 'line 2', got: {message:?}"
        );
    }

    #[test]
    fn test_extract_skill_metadata_frontmatter_too_large() {
        // Issue #203 follow-up (S2): a pathologically large frontmatter block must be
        // rejected before it reaches `serde-saphyr::from_str_with_options`, since YAML parsing
        // is not linear-time on deeply nested input.
        let padding = "a".repeat(MAX_FRONTMATTER_SIZE + 1);
        let content = format!("---\nname: test\ndescription: {padding}\n---\n# Test");

        let result = extract_skill_metadata(&content);

        match result {
            Err(SkillMetadataError::FrontmatterTooLarge { size, limit }) => {
                assert!(size > MAX_FRONTMATTER_SIZE);
                assert_eq!(limit, MAX_FRONTMATTER_SIZE);
            }
            other => panic!("expected FrontmatterTooLarge, got: {other:?}"),
        }
    }

    #[test]
    fn test_extract_skill_metadata_null_name_rejected() {
        // Issue #203 follow-up (M4): `name:`/`name: null`/`name: ~` all deserialize to
        // `None`, same as an absent key, and must be rejected the same way.
        let content = "---\nname: ~\ndescription: test\n---\n# Test";

        let result = extract_skill_metadata(content);

        assert!(matches!(
            result,
            Err(SkillMetadataError::MissingField { field: "name" })
        ));
    }

    #[test]
    fn test_extract_skill_metadata_empty_string_name_rejected() {
        // Issue #203 follow-up (M4): an empty-string name/description is present but
        // useless, and must not silently reach `SkillMetadata`.
        let content = "---\nname: \"\"\ndescription: test\n---\n# Test";

        let result = extract_skill_metadata(content);

        assert!(matches!(
            result,
            Err(SkillMetadataError::MissingField { field: "name" })
        ));
    }

    #[test]
    fn test_extract_skill_metadata_name_too_long_rejected() {
        // Issue #419: `save_skill` writes caller-supplied SKILL.md content directly, never
        // routing `name` through `validate_skill_name` the way the two `generate` call sites do.
        // `extract_skill_metadata` is the one chokepoint both paths share, so the bound has to
        // live here.
        let long_name = "a".repeat(crate::types::MAX_SKILL_NAME_LENGTH + 1);
        let content = format!("---\nname: {long_name}\ndescription: test\n---\n# Test");

        let result = extract_skill_metadata(&content);

        assert!(matches!(
            result,
            Err(SkillMetadataError::NameTooLong {
                len,
                limit
            }) if len == crate::types::MAX_SKILL_NAME_LENGTH + 1
                && limit == crate::types::MAX_SKILL_NAME_LENGTH
        ));
    }

    #[test]
    fn test_extract_skill_metadata_name_at_max_length_accepted() {
        let max_name = "a".repeat(crate::types::MAX_SKILL_NAME_LENGTH);
        let content = format!("---\nname: {max_name}\ndescription: test\n---\n# Test");

        let result = extract_skill_metadata(&content);

        assert_eq!(result.unwrap().name, max_name);
    }

    #[test]
    fn test_extract_skill_metadata_with_extra_fields() {
        let content = r"---
name: test-skill
description: Test description
version: 1.0.0
author: Test Author
---

# Test
";

        let result = extract_skill_metadata(content);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.name, "test-skill");
        assert_eq!(metadata.description, "Test description");
    }

    #[test]
    fn test_extract_skill_metadata_block_literal_scalar() {
        // Issue #203 regression: a `description: |` block literal scalar must have its
        // real multi-line body captured, not just the `|` marker character.
        //
        // Issue #405 (ADR-405 ruling 1, BREAKING): `serde-saphyr` is YAML-1.2-correct and keeps
        // the clip-chomped trailing newline that the previous parser silently stripped. This
        // assertion is pinned to exact equality (not `contains`) specifically to catch that
        // trailing `\n`.
        let content = "---\nname: test-skill\ndescription: |\n  one\n  two\n---\n\n# Test\n";

        let metadata = extract_skill_metadata(content).unwrap();
        assert_eq!(metadata.description, "one\ntwo\n");
    }

    #[test]
    fn test_extract_skill_metadata_folded_block_scalar() {
        // Issue #203 regression: a `description: >` folded block scalar must have its
        // real multi-line body captured, not just the `>` marker character.
        //
        // Issue #405 (ADR-405 ruling 1, BREAKING): same untrimmed-trailing-newline behavior as
        // the block-literal test above, but folding also collapses the internal line break into
        // a space.
        let content = "---\nname: test-skill\ndescription: >\n  one\n  two\n---\n\n# Test\n";

        let metadata = extract_skill_metadata(content).unwrap();
        assert_eq!(metadata.description, "one two\n");
    }

    #[test]
    fn test_extract_skill_metadata_quoted_scalars() {
        // Issue #203 regression: quote characters must be stripped by the YAML
        // parser, not captured verbatim into the field value.
        let content = r#"---
name: "quoted-name"
description: 'quoted text'
---

# Test
"#;

        let metadata = extract_skill_metadata(content).unwrap();
        assert_eq!(metadata.name, "quoted-name");
        assert_eq!(metadata.description, "quoted text");
    }

    /// Builds the flow-sequence body of an 8-anchor alias bomb shared by every alias-bomb test
    /// below: 8 anchors (`a0..a7`), each referencing the previous 8 times — 8^8 ~= 16.7M leaves
    /// if ever fully expanded. `preamble` is prepended verbatim; it decides which YAML key
    /// (declared or not) the bomb ends up under. At a few hundred bytes the result sits well
    /// under `MAX_FRONTMATTER_SIZE` (8 KiB); shrinking the branching factor, the anchor count,
    /// or the frontmatter cap changes that margin and must be re-checked against callers' size
    /// assertions.
    fn alias_bomb_fixture(preamble: &str) -> String {
        use std::fmt::Write as _;

        let mut frontmatter = String::from(preamble);
        writeln!(frontmatter, "  - &a0 [x, x, x, x, x, x, x, x]").unwrap();
        for level in 1..=7 {
            let prev = level - 1;
            let refs = (0..8)
                .map(|_| format!("*a{prev}"))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(frontmatter, "  - &a{level} [{refs}]").unwrap();
        }
        writeln!(frontmatter, "  - *a7").unwrap();
        frontmatter
    }

    #[test]
    fn test_extract_skill_metadata_alias_bomb_under_unknown_key_rejected_by_budget() {
        // Issue #405 / ADR-405 C1/C2 (BREAKING vs the previous serde_norway-based parser):
        // `frontmatter_options`'s explicit Budget is what defends this path now, not
        // `RawFrontmatter`'s field shape. The bomb below sits under a key `RawFrontmatter`
        // does not declare, so it is materialized by the generic visitor (unlike the previous
        // parser's lazy, non-expanding visitor) and reaches the budget: 8 anchors x 8 refs
        // each amplify well past `max_nodes: 8_192`.
        //
        // Fixture shape: see `alias_bomb_fixture`'s doc comment.
        use std::time::{Duration, Instant};

        let frontmatter =
            alias_bomb_fixture("name: test-skill\ndescription: valid description\nunknown_key:\n");

        let content = format!("---\n{frontmatter}---\n# Test\n");
        assert!(
            content.len() <= MAX_FRONTMATTER_SIZE,
            "fixture must stay under the frontmatter cap to exercise the parser, not the size guard"
        );

        let start = Instant::now();
        let result = extract_skill_metadata(&content);
        let elapsed = start.elapsed();

        assert!(
            matches!(result, Err(SkillMetadataError::FrontmatterTooComplex)),
            "expected FrontmatterTooComplex: an alias bomb under a key RawFrontmatter does not \
             declare must be materialized and rejected by the Budget. got: {result:?}"
        );
        // Sanity bound only: the budget already bounds the parse deterministically, this only
        // guards against an outright hang.
        assert!(
            elapsed < Duration::from_secs(1),
            "parse took {elapsed:?}, unexpectedly long even accounting for cold-process noise"
        );
    }

    #[test]
    fn test_alias_bomb_rejection_survives_a_buffering_field_shape() {
        // Issue #359 / ADR-405 C3: the budget is NOT shape-independent — a bomb under a
        // *declared* `Option<String>` field short-circuits on a type mismatch before the
        // budget ever accumulates anything (see the sibling
        // `test_extract_skill_metadata_alias_bomb_under_declared_field_short_circuits` test
        // below), so that fixture alone does not prove the budget defends a buffering field
        // shape. This test covers the shape that DOES reach the budget: a field with a
        // buffering `#[serde(deserialize_with)]`. `serde_saphyr::Value` does not exist under
        // this crate's `default-features = false, features = ["deserialize"]` build (verified
        // against the published source, not assumed), so `serde_json::Value` — already a
        // dependency of this crate — stands in as the buffering vehicle instead.
        use std::time::{Duration, Instant};

        fn buffer_via_json_value<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let value = serde_json::Value::deserialize(deserializer)?;
            Ok(value.as_str().map(str::to_string))
        }

        #[derive(Debug, Deserialize)]
        #[expect(
            dead_code,
            reason = "fields exist only to mirror RawFrontmatter's shape"
        )]
        struct RawFrontmatterBufferedDescription {
            name: Option<String>,
            #[serde(deserialize_with = "buffer_via_json_value")]
            description: Option<String>,
        }

        let frontmatter = alias_bomb_fixture("name: test-skill\ndescription:\n");
        assert!(
            frontmatter.len() <= MAX_FRONTMATTER_SIZE,
            "fixture unexpectedly exceeds MAX_FRONTMATTER_SIZE; re-check alias_bomb_fixture's margin"
        );

        let (options, budget_breach) = frontmatter_options();
        let start = Instant::now();
        let result = serde_saphyr::from_str_with_options::<RawFrontmatterBufferedDescription>(
            &frontmatter,
            options,
        );
        let elapsed = start.elapsed();

        let err = result.expect_err(
            "expected Err: buffering a declared field forces alias expansion before per-field \
             routing, which should breach frontmatter_options's Budget",
        );
        assert!(
            is_budget_breach(&err, budget_breach.take().as_ref()),
            "expected a Budget/alias-limit breach specifically, not some unrelated parse \
             error; got: {err:?}"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "parse took {elapsed:?}, unexpectedly long even accounting for cold-process noise"
        );
    }

    #[test]
    fn test_extract_skill_metadata_alias_bomb_under_declared_field_short_circuits() {
        // Issue #359 / ADR-405 C3: a bomb placed directly under a *declared*
        // `Option<String>` field short-circuits on serde's type mismatch before
        // `frontmatter_options`'s Budget ever accumulates anything — unchanged from this
        // crate's previous serde_norway-based parser, and NOT a complexity-budget rejection.
        // See `RawFrontmatter`'s doc comment and the sibling
        // `test_alias_bomb_rejection_survives_a_buffering_field_shape`, which covers the shape
        // that DOES reach the budget.
        let content = format!(
            "---\n{}---\n# Test\n",
            alias_bomb_fixture("name: test-skill\ndescription:\n")
        );
        assert!(
            content.len() <= MAX_FRONTMATTER_SIZE,
            "fixture must stay under the frontmatter cap to exercise the parser, not the size guard"
        );

        let result = extract_skill_metadata(&content);
        assert!(
            matches!(result, Err(SkillMetadataError::InvalidYaml(_))),
            "expected InvalidYaml (a type-mismatch short-circuit, not a complexity-budget \
             rejection): if this is now FrontmatterTooComplex, RawFrontmatter's field shape \
             likely changed (e.g. a buffering deserialize_with) and reopened the amplification \
             path this test guards against. got: {result:?}"
        );
    }

    #[test]
    fn test_budget_config_is_explicitly_set() {
        // Critic M12: the primary discriminator that `frontmatter_options` configures a real
        // budget rather than silently leaving it `None`/default — pins the configured value
        // directly via public fields, with no coupling to serde-saphyr's Debug-rendered breach
        // text. A dropped `budget:` line would still compile (`Budget` is itself `Default`),
        // so this test would fail loudly rather than silently regressing to
        // `Budget::default()`'s far looser `max_nodes: 250_000`.
        let (options, _budget_breach) = frontmatter_options();
        let budget = options
            .budget
            .expect("frontmatter_options must configure a budget");
        assert_eq!(budget.max_nodes, 8_192);
    }

    #[test]
    fn test_budget_breach_fires_at_configured_max_nodes_not_default() {
        // Review moderate: critic M12 required both the config-value test above AND a
        // behavioral test proving the breach fires at *our* configured threshold rather than
        // some other (e.g. serde-saphyr's much looser default) budget field. Rebuilt against
        // the authoritative `BudgetReport` (via `frontmatter_options`'s registered callback),
        // not `Error::AliasError`'s Debug-rendered message -- the previous version of this test
        // asserted on that message, which is what the Critical `is_budget_breach` fix above
        // removed as a classification signal.
        let frontmatter =
            alias_bomb_fixture("name: test-skill\ndescription: valid description\nunknown_key:\n");

        let (options, budget_breach) = frontmatter_options();
        let result = serde_saphyr::from_str_with_options::<RawFrontmatter>(&frontmatter, options);
        assert!(
            result.is_err(),
            "expected the alias bomb to be rejected, got: {result:?}"
        );

        match budget_breach.take() {
            Some(BudgetBreach::Nodes { nodes }) => {
                assert!(
                    nodes <= 8_192 + 100,
                    "breach fired at {nodes} nodes -- far past our configured max_nodes: \
                     8_192, suggesting frontmatter_options's budget was silently dropped or \
                     loosened to serde-saphyr's default (max_nodes: 250_000)"
                );
            }
            other => panic!("expected a Nodes budget breach, got: {other:?}"),
        }
    }

    #[test]
    fn test_alias_wrapped_type_mismatch_is_not_a_budget_breach() {
        // Review Critical fix: `Error::AliasError` is a generic wrapper serde-saphyr attaches
        // to *any* error raised while deserializing a value reached through an alias, not a
        // budget-specific variant. An ordinary type mismatch under an alias (no amplification,
        // no budget breach involved) must still classify as `InvalidYaml`, not
        // `FrontmatterTooComplex` -- the previous version of `is_budget_breach` matched
        // `Error::AliasError` unconditionally and misclassified exactly this case.
        let content = "---\nbase: &a [1, 2]\nname: *a\ndescription: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        assert!(
            matches!(result, Err(SkillMetadataError::InvalidYaml(_))),
            "expected InvalidYaml for an ordinary type mismatch under an alias, not a \
             complexity-budget rejection; got: {result:?}"
        );
    }

    #[test]
    fn test_budget_rejects_deeply_nested_unknown_key_value() {
        // Review Minor fix: `max_depth: 64` is documented as the sole nesting control and a
        // deliberate exception to the sizing rule, but had zero direct test coverage. Pinned to
        // a specific depth (per M10/M13: 65-70, comfortably past `max_depth: 64` so this
        // breaches there, but nowhere near granit-parser's own internal recursion limit
        // (~270+), which would surface as a different, unlisted error variant instead). Placed
        // under an *undeclared* key: a declared `Option<String>` field's type mismatch
        // short-circuits before depth accumulates (see `RawFrontmatter`'s doc comment) --
        // `max_depth` does not protect that path.
        let depth = 67;
        let nested: String = "[".repeat(depth) + &"]".repeat(depth);
        let content = format!(
            "---\nname: test-skill\ndescription: test\nunknown_key: {nested}\n---\n# Test\n"
        );

        let result = extract_skill_metadata(&content);
        assert!(
            matches!(result, Err(SkillMetadataError::FrontmatterTooComplex)),
            "expected FrontmatterTooComplex for nesting past max_depth: 64, got: {result:?}"
        );
    }

    #[test]
    fn test_budget_accepts_500_unknown_keys() {
        // ADR-405: legitimate frontmatter with many unrecognized keys must not be falsely
        // rejected by the budget just for exceeding today's two declared fields.
        use std::fmt::Write as _;

        let mut frontmatter_block = String::from("name: test-skill\ndescription: test\n");
        for i in 0..500 {
            writeln!(frontmatter_block, "k{i}: v{i}").unwrap();
        }
        assert!(
            frontmatter_block.len() <= MAX_FRONTMATTER_SIZE,
            "fixture must stay under the frontmatter cap to exercise the budget, not the size \
             guard: {} bytes",
            frontmatter_block.len()
        );

        let content = format!("---\n{frontmatter_block}---\n# Test\n");
        let result = extract_skill_metadata(&content);
        assert!(
            result.is_ok(),
            "expected Ok for 500 legitimate unknown keys, got: {result:?}"
        );
    }

    #[test]
    fn test_budget_accepts_node_dense_flow_sequence() {
        // ADR-405 C4: the densest valid YAML construction is a flat flow sequence of
        // single-char plain scalars (`[a,a,a,...]`, 2 bytes/node) — NOT `[,,,,]`, which is not
        // valid YAML ("unexpected EOF while parsing an implicit flow mapping"). This fixture
        // sits just under `MAX_FRONTMATTER_SIZE` and must clear `max_nodes: 8_192` with margin
        // to spare.
        let preamble = "name: test-skill\ndescription: test\nbig: [";
        let suffix = "]\n";
        let available = MAX_FRONTMATTER_SIZE - preamble.len() - suffix.len();
        let mut body = "a,".repeat(available / 2);
        body.truncate(body.len().saturating_sub(1));
        let frontmatter_block = format!("{preamble}{body}{suffix}");
        assert!(frontmatter_block.len() <= MAX_FRONTMATTER_SIZE);

        let content = format!("---\n{frontmatter_block}---\n# Test\n");
        let result = extract_skill_metadata(&content);
        assert!(
            result.is_ok(),
            "expected Ok for a dense-but-legitimate flow sequence, got: {result:?}"
        );
    }

    #[test]
    fn test_budget_accepts_8kib_plain_scalar() {
        // ADR-405: a legitimate, scalar-byte-heavy ~8 KiB description must not be falsely
        // rejected by `max_total_scalar_bytes` (65_536, 8x the input cap).
        let preamble = "name: test-skill\ndescription: ";
        let suffix = "\n";
        let padding_len = MAX_FRONTMATTER_SIZE - preamble.len() - suffix.len();
        let frontmatter_block = format!("{preamble}{}{suffix}", "a".repeat(padding_len));
        assert!(frontmatter_block.len() <= MAX_FRONTMATTER_SIZE);

        let content = format!("---\n{frontmatter_block}---\n# Test\n");
        let result = extract_skill_metadata(&content);
        assert!(
            result.is_ok(),
            "expected Ok for a legitimate ~8 KiB plain scalar, got: {result:?}"
        );
    }

    #[test]
    fn test_yaml_error_does_not_echo_frontmatter_content() {
        // Critic M9: a duplicate key named after a distinctive marker. Even with
        // `with_snippet: false`, serde-saphyr's own rendered duplicate-key message still
        // carries both the attacker-controlled key name and an internal "set
        // DuplicateKeyPolicy in Options" hint. Neither must reach
        // `SkillMetadataError::InvalidYaml`'s message, since `describe_yaml_error` never
        // touches the parser's own `Display`/`to_string()` output (see `yaml_error_kind`).
        let content = "---\nname: test-skill\nMARKER_TOKEN_abc123: 1\nMARKER_TOKEN_abc123: 2\n\
                        description: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        let Err(SkillMetadataError::InvalidYaml(message)) = &result else {
            panic!("expected InvalidYaml, got: {result:?}");
        };
        assert!(
            !message.contains("MARKER_TOKEN_abc123"),
            "error must not echo the frontmatter's key name: {message:?}"
        );
        assert!(
            !message.contains("DuplicateKeyPolicy"),
            "error must not leak serde-saphyr's internal config-option hint: {message:?}"
        );
    }

    #[test]
    fn test_merge_key_treated_as_ordinary_key() {
        // `merge_keys: MergeKeyPolicy::AsOrdinary` must treat `<<` as a plain, unrecognized
        // key rather than expanding it into the surrounding mapping — an anchored map
        // containing `name`/`description`, referenced via `<<: *defaults`, must not inject
        // those fields.
        let content = r"---
base: &defaults
  name: INJECTED
  description: INJECTED
<<: *defaults
---
# Test
";

        let result = extract_skill_metadata(content);
        assert!(
            matches!(
                result,
                Err(SkillMetadataError::MissingField { field: "name" })
            ),
            "merge key must not inject `name` from the anchored map, got: {result:?}"
        );
    }

    #[test]
    fn test_duplicate_key_rejected() {
        // `duplicate_keys: DuplicateKeyPolicy::Error` (equal to serde-saphyr's own default,
        // set explicitly so an upstream default change cannot silently loosen this) must
        // reject a frontmatter block that redefines the same key twice.
        let content = "---\nname: test-skill\nname: other-name\ndescription: test\n---\n# Test";

        let result = extract_skill_metadata(content);
        assert!(
            matches!(result, Err(SkillMetadataError::InvalidYaml(_))),
            "expected InvalidYaml for a duplicate key, got: {result:?}"
        );
    }
}
