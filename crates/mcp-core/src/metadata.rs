//! Structured sidecar metadata describing a server's generated tools.
//!
//! `mcp-execution-codegen` emits a `_meta.json` file alongside the generated
//! TypeScript tool files for each server. `mcp-execution-skill` (and
//! `mcp-execution-server`) read that file back to build `SKILL.md` and
//! runtime tool listings, instead of re-parsing the generated `.ts` source.
//!
//! This module is the shared wire contract between the two sides: the
//! producer (codegen) and the consumer (skill/server) both depend on
//! `mcp-execution-core`, so the schema lives here rather than in either
//! crate directly.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_core::metadata::{
//!     ResultTypeOrigin, ServerMetadata, ToolMetadata, METADATA_SCHEMA_VERSION,
//! };
//! use mcp_execution_core::provenance::GenerationProvenance;
//! use mcp_execution_core::{ServerConfig, ServerId, ToolName};
//!
//! let config = ServerConfig::builder().command("docker".to_string()).build().unwrap();
//!
//! let meta = ServerMetadata {
//!     schema_version: METADATA_SCHEMA_VERSION,
//!     server_id: ServerId::new("github").unwrap(),
//!     server_name: "GitHub".to_string(),
//!     server_version: "1.0.0".to_string(),
//!     tools: vec![ToolMetadata {
//!         name: ToolName::new("create_issue").unwrap(),
//!         typescript_name: "createIssue".to_string(),
//!         category: Some("issues".to_string()),
//!         keywords: vec!["create".to_string(), "issue".to_string()],
//!         description: Some("Creates a new issue".to_string()),
//!         parameters: vec![],
//!         annotations: None,
//!         result_type: None,
//!         result_type_origin: ResultTypeOrigin::Undeclared,
//!         manifest_size_bytes: 0,
//!     }],
//!     provenance: GenerationProvenance::capture(&config, &[]),
//! };
//!
//! let json = serde_json::to_string(&meta).unwrap();
//! let round_tripped: ServerMetadata = serde_json::from_str(&json).unwrap();
//! assert_eq!(round_tripped, meta);
//! ```

use crate::annotations::ToolAnnotations;
use crate::provenance::GenerationProvenance;
use crate::{ServerId, ToolName};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Current schema version of the `_meta.json` sidecar format.
///
/// Bump this when making a breaking change to [`ServerMetadata`] or its
/// nested types, so that a consumer built against an older schema fails
/// loudly (via a schema-version mismatch check) instead of silently
/// misinterpreting the new shape.
///
/// Bumped from `1` to `2` when [`ServerMetadata::provenance`] was added: a `schema_version: 1`
/// sidecar has no `provenance` key at all, so a consumer must check this value *before*
/// attempting a typed deserialization (see `mcp-execution-skill`'s parser).
///
/// Bumped from `2` to `3` when [`ToolMetadata`] gained `annotations`, `result_type`,
/// `result_type_origin` and `manifest_size_bytes`.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::METADATA_SCHEMA_VERSION;
///
/// assert_eq!(METADATA_SCHEMA_VERSION, 3);
/// ```
pub const METADATA_SCHEMA_VERSION: u32 = 3;

/// Filename of the sidecar metadata file emitted alongside generated tool files.
///
/// Shared between the producer (`mcp-execution-codegen`) and the consumer
/// (`mcp-execution-skill`) to avoid a stringly-typed filename duplicated in
/// two crates.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::METADATA_FILE_NAME;
///
/// assert_eq!(METADATA_FILE_NAME, "_meta.json");
/// ```
pub const METADATA_FILE_NAME: &str = "_meta.json";

/// Filename of the generated re-export entry point emitted alongside per-tool files.
///
/// Shared between the producer (`mcp-execution-codegen`, which renders it) and its consumers
/// (`mcp-execution-skill` and `mcp-execution-server`, which must recognize it as the package's
/// aggregator file rather than a per-tool file) to avoid a stringly-typed filename duplicated
/// across crates.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::INDEX_FILE_NAME;
///
/// assert_eq!(INDEX_FILE_NAME, "index.ts");
/// ```
pub const INDEX_FILE_NAME: &str = "index.ts";

/// Filename of the generated top-level catalog of tool signatures.
///
/// Shared between the producer (`mcp-execution-codegen`, which renders it) and its consumers
/// (`mcp-execution-skill` and `mcp-execution-server`, which must not mistake it for a per-tool
/// file) to avoid a stringly-typed filename duplicated across crates.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::CATALOG_FILE_NAME;
///
/// assert_eq!(CATALOG_FILE_NAME, "catalog.ts");
/// ```
pub const CATALOG_FILE_NAME: &str = "catalog.ts";

/// Maximum number of tools accepted from a single MCP server (denial-of-service protection,
/// CWE-400).
///
/// An MCP server is untrusted input: without this cap, a malicious or misbehaving server
/// could return an unbounded tool list, which downstream codegen turns into one `.ts` file
/// per tool. 1000 is generous headroom over any real-world server's tool count while still
/// bounding the worst case.
///
/// Defined here, the crate every layer already depends on, so that introspection, code
/// generation and the skill scan share one bound: data accepted by one layer is never rejected
/// by the next for being as large as already allowed.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::MAX_TOOL_COUNT;
///
/// assert_eq!(MAX_TOOL_COUNT, 1000);
/// ```
pub const MAX_TOOL_COUNT: usize = 1000;

/// Maximum size, in bytes, of a `_meta.json` sidecar that consumers will read.
///
/// Shared between the producer (`mcp-execution-codegen`, which warns when a sidecar exceeds
/// it) and the consumer (`mcp-execution-skill`, which rejects a larger file), so the two
/// cannot drift.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::MAX_METADATA_FILE_BYTES;
///
/// assert_eq!(MAX_METADATA_FILE_BYTES, 1024 * 1024);
/// ```
pub const MAX_METADATA_FILE_BYTES: u64 = 1024 * 1024;

/// Structured sidecar describing one server's generated tools.
///
/// Serialized as `_meta.json` by `mcp-execution-codegen` and deserialized by
/// `mcp-execution-skill` / `mcp-execution-server`, replacing a fragile
/// regex-based re-parse of the generated TypeScript files.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::{ServerMetadata, METADATA_SCHEMA_VERSION};
/// use mcp_execution_core::provenance::GenerationProvenance;
/// use mcp_execution_core::{ServerConfig, ServerId};
///
/// let config = ServerConfig::builder().command("docker".to_string()).build().unwrap();
///
/// let meta = ServerMetadata {
///     schema_version: METADATA_SCHEMA_VERSION,
///     server_id: ServerId::new("github").unwrap(),
///     server_name: "GitHub".to_string(),
///     server_version: "1.0.0".to_string(),
///     tools: vec![],
///     provenance: GenerationProvenance::capture(&config, &[]),
/// };
///
/// assert_eq!(meta.tools.len(), 0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerMetadata {
    /// Schema version this sidecar was produced with.
    ///
    /// Consumers should compare this against [`METADATA_SCHEMA_VERSION`] and
    /// fail loudly on a mismatch rather than risk misinterpreting an
    /// incompatible future shape.
    pub schema_version: u32,

    /// MCP server identifier (e.g. `github`).
    ///
    /// [`ServerId`]'s derived `Serialize`/`Deserialize` round-trip through a plain JSON string
    /// (single-field newtype structs serialize transparently), so this field's on-the-wire
    /// shape is unchanged from when it was a bare `String`.
    pub server_id: ServerId,

    /// Human-readable server name.
    pub server_name: String,

    /// Server version string, as reported by the MCP server.
    pub server_version: String,

    /// Metadata for every generated tool, in generation order.
    pub tools: Vec<ToolMetadata>,

    /// When and against what server state this sidecar was generated.
    ///
    /// Required rather than `Option`: a `schema_version: 1` sidecar (produced before this
    /// field existed) is rejected by the schema-version check before a consumer ever
    /// constructs a `ServerMetadata`, so every value that exists already carries real
    /// provenance — see `mcp-execution-skill`'s parser and
    /// [`crate::provenance::GenerationProvenance`]'s own doc comment.
    pub provenance: GenerationProvenance,
}

/// Structured metadata for a single generated tool.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::{ResultTypeOrigin, ToolMetadata};
/// use mcp_execution_core::ToolName;
///
/// let tool = ToolMetadata {
///     name: ToolName::new("create_issue").unwrap(),
///     typescript_name: "createIssue".to_string(),
///     category: Some("issues".to_string()),
///     keywords: vec!["create".to_string(), "issue".to_string()],
///     description: Some("Creates a new issue".to_string()),
///     parameters: vec![],
///     annotations: None,
///     result_type: None,
///     result_type_origin: ResultTypeOrigin::Undeclared,
///     manifest_size_bytes: 0,
/// };
///
/// assert_eq!(tool.name.as_str(), "create_issue");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolMetadata {
    /// Original MCP tool name (the call identifier), unmodified.
    ///
    /// [`ToolName`]'s derived `Serialize`/`Deserialize` round-trip through a plain JSON string
    /// (see [`ServerMetadata::server_id`]'s doc comment), so this field's on-the-wire shape is
    /// unchanged from when it was a bare `String`.
    pub name: ToolName,

    /// TypeScript-friendly name (camelCase), matching the generated file's
    /// basename (e.g. `createIssue` for `createIssue.ts`).
    pub typescript_name: String,

    /// Optional category for tool grouping.
    pub category: Option<String>,

    /// Keywords for discovery, split from the source comma-separated string.
    pub keywords: Vec<String>,

    /// Human-readable tool description, as reported by the MCP server.
    pub description: Option<String>,

    /// Metadata for each of the tool's input parameters.
    pub parameters: Vec<ParameterMetadata>,

    /// Annotations the server advertised for the tool, if any (advisory and untrusted).
    pub annotations: Option<ToolAnnotations>,

    /// Compact TypeScript text of the tool's result type.
    ///
    /// `None` when the text would exceed the stored-text cap or the result type degraded to the
    /// generic form.
    pub result_type: Option<String>,

    /// Where the result type came from.
    pub result_type_origin: ResultTypeOrigin,

    /// Length in bytes of the compact JSON serialization of the tool's introspected manifest.
    pub manifest_size_bytes: u64,
}

/// Where a tool's generated result type came from.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::ResultTypeOrigin;
///
/// let json = serde_json::to_string(&ResultTypeOrigin::Degraded).unwrap();
/// assert_eq!(json, "\"degraded\"");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultTypeOrigin {
    /// Converted from an output schema the server declared.
    Declared,
    /// The server declared an output schema but the generator fell back to the generic type.
    Degraded,
    /// The server declared no output schema.
    Undeclared,
}

/// Structured metadata for a single tool parameter.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::ParameterMetadata;
///
/// let param = ParameterMetadata {
///     name: "title".to_string(),
///     typescript_type: "string".to_string(),
///     required: true,
///     description: Some("Issue title".to_string()),
/// };
///
/// assert!(param.required);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParameterMetadata {
    /// Parameter name.
    pub name: String,

    /// TypeScript type (e.g. `string`, `number`, `boolean`).
    pub typescript_type: String,

    /// Whether the parameter is required.
    pub required: bool,

    /// Parameter description, sourced from the tool's input JSON Schema.
    pub description: Option<String>,
}

/// Refusal reasons of [`read_server_metadata`], one variant per distinct failure.
#[derive(Debug, thiserror::Error)]
pub enum MetadataReadError {
    /// The sidecar file does not exist.
    #[error("metadata sidecar not found")]
    NotFound,

    /// The sidecar path is a symbolic link.
    #[error("metadata sidecar is a symbolic link")]
    Link,

    /// The sidecar path is not a regular file.
    #[error("metadata sidecar is not a regular file")]
    NotRegularFile,

    /// The sidecar exceeds the size limit.
    #[error("metadata sidecar is {size} bytes, over the {limit} byte limit")]
    TooLarge {
        /// Observed size in bytes.
        size: u64,
        /// Maximum accepted size in bytes.
        limit: u64,
    },

    /// The sidecar could not be read.
    #[error("metadata sidecar is unreadable: {0}")]
    Unreadable(#[source] io::Error),

    /// The sidecar is not valid JSON of the expected shape.
    #[error("metadata sidecar is malformed: {0}")]
    Malformed(#[source] serde_json::Error),

    /// The sidecar was written with a different schema version.
    #[error("metadata schema version {found} does not match the expected {expected}")]
    SchemaMismatch {
        /// Version found in the file.
        found: u32,
        /// Version this build understands.
        expected: u32,
    },
}

/// Reads and validates the `_meta.json` sidecar inside `dir`.
///
/// The sidecar must be a regular, non-symlink file of at most [`MAX_METADATA_FILE_BYTES`] that
/// carries [`METADATA_SCHEMA_VERSION`]; `dir` itself may be a symlink. The schema version is
/// checked before the typed parse so an old sidecar reports a mismatch rather than a shape error.
/// Error messages never contain the path.
///
/// # Errors
///
/// Returns a [`MetadataReadError`] naming the first refusal.
///
/// # Examples
///
/// ```
/// use std::assert_matches;
/// use mcp_execution_core::metadata::{MetadataReadError, read_server_metadata};
///
/// let dir = std::env::temp_dir().join("mcp-execution-doc-absent-server");
/// assert_matches!(
///     read_server_metadata(&dir),
///     Err(MetadataReadError::NotFound)
/// );
/// ```
pub fn read_server_metadata(dir: &Path) -> Result<ServerMetadata, MetadataReadError> {
    let file = open_regular_sidecar(&dir.join(METADATA_FILE_NAME))?;
    let size = file.metadata().map_err(read_error)?.len();
    if size > MAX_METADATA_FILE_BYTES {
        return Err(MetadataReadError::TooLarge {
            size,
            limit: MAX_METADATA_FILE_BYTES,
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(MetadataReadError::Unreadable)?;
    if let Ok(len) = u64::try_from(bytes.len())
        && len > MAX_METADATA_FILE_BYTES
    {
        return Err(MetadataReadError::TooLarge {
            size: len,
            limit: MAX_METADATA_FILE_BYTES,
        });
    }

    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(MetadataReadError::Malformed)?;
    let found = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64);
    if let Some(found) = found
        && found != u64::from(METADATA_SCHEMA_VERSION)
    {
        return Err(MetadataReadError::SchemaMismatch {
            found: u32::try_from(found).unwrap_or(u32::MAX),
            expected: METADATA_SCHEMA_VERSION,
        });
    }
    serde_json::from_value(value).map_err(MetadataReadError::Malformed)
}

/// Async counterpart of [`read_server_metadata`] that keeps blocking I/O off the executor.
///
/// # Errors
///
/// Same as [`read_server_metadata`]; a failed blocking task is reported as
/// [`MetadataReadError::Unreadable`].
///
/// # Panics
///
/// Resumes a panic raised inside the blocking task.
pub async fn read_server_metadata_async(dir: PathBuf) -> Result<ServerMetadata, MetadataReadError> {
    match tokio::task::spawn_blocking(move || read_server_metadata(&dir)).await {
        Ok(outcome) => outcome,
        Err(join) if join.is_panic() => std::panic::resume_unwind(join.into_panic()),
        Err(join) => Err(MetadataReadError::Unreadable(io::Error::other(join))),
    }
}

/// Opens `path` for reading and proves the opened file is a regular, non-symlink file.
///
/// On Unix the open carries `O_NOFOLLOW | O_NONBLOCK` and the type check is an `fstat` on the
/// opened file, so there is no window between check and open and a FIFO never blocks. On Windows
/// the open carries `FILE_FLAG_OPEN_REPARSE_POINT` and the opened handle's attributes are checked,
/// so every reparse point is rejected.
fn open_regular_sidecar(path: &Path) -> Result<File, MetadataReadError> {
    let file = open_no_follow(path)?;
    if file.metadata().map_err(read_error)?.is_file() {
        Ok(file)
    } else {
        Err(MetadataReadError::NotRegularFile)
    }
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> Result<File, MetadataReadError> {
    use std::os::unix::fs::OpenOptionsExt as _;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                MetadataReadError::Link
            } else {
                read_error(error)
            }
        })
}

// FILE_FLAG_OPEN_REPARSE_POINT opens a link itself instead of its target; the handle is then
// judged by its own attributes, so there is no check-then-open window.
#[cfg(windows)]
fn open_no_follow(path: &Path) -> Result<File, MetadataReadError> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    // Lets a directory named like the sidecar open, so it is reported as `NotRegularFile`
    // instead of an access error.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(read_error)?;
    let metadata = file.metadata().map_err(read_error)?;
    if metadata.file_type().is_symlink() {
        Err(MetadataReadError::Link)
    } else if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Err(MetadataReadError::NotRegularFile)
    } else {
        Ok(file)
    }
}

fn read_error(error: io::Error) -> MetadataReadError {
    if error.kind() == io::ErrorKind::NotFound {
        MetadataReadError::NotFound
    } else {
        MetadataReadError::Unreadable(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        METADATA_SCHEMA_VERSION, ParameterMetadata, ResultTypeOrigin, ServerMetadata, ToolMetadata,
    };
    use crate::provenance::GenerationProvenance;
    use crate::{ServerConfig, ServerId, ToolName};

    fn test_provenance() -> GenerationProvenance {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build()
            .unwrap();
        GenerationProvenance::capture(&config, &[])
    }

    #[test]
    fn round_trips_through_json() {
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: ServerId::new("github").unwrap(),
            server_name: "GitHub".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![ToolMetadata {
                name: ToolName::new("create_issue").unwrap(),
                typescript_name: "createIssue".to_string(),
                category: Some("issues".to_string()),
                keywords: vec!["create".to_string(), "issue".to_string()],
                description: Some("Creates a new issue".to_string()),
                parameters: vec![ParameterMetadata {
                    name: "title".to_string(),
                    typescript_type: "string".to_string(),
                    required: true,
                    description: Some("Issue title".to_string()),
                }],
                annotations: None,
                result_type: Some("string".to_string()),
                result_type_origin: ResultTypeOrigin::Declared,
                manifest_size_bytes: 123,
            }],
            provenance: test_provenance(),
        };

        let json = serde_json::to_string(&meta).unwrap();
        let round_tripped: ServerMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped, meta);
    }

    fn json_keys(value: &serde_json::Value) -> std::collections::BTreeSet<&str> {
        value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    /// Pins the on-disk shape of `_meta.json`: a change to a field set must come with a conscious
    /// `METADATA_SCHEMA_VERSION` bump and an update of this test.
    #[test]
    fn sidecar_shape_is_pinned_to_the_schema_version() {
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: ServerId::new("github").unwrap(),
            server_name: "GitHub".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![ToolMetadata {
                name: ToolName::new("create_issue").unwrap(),
                typescript_name: "createIssue".to_string(),
                category: None,
                keywords: vec![],
                description: None,
                parameters: vec![ParameterMetadata {
                    name: "title".to_string(),
                    typescript_type: "string".to_string(),
                    required: true,
                    description: None,
                }],
                annotations: None,
                result_type: None,
                result_type_origin: ResultTypeOrigin::Undeclared,
                manifest_size_bytes: 0,
            }],
            provenance: test_provenance(),
        };

        let value = serde_json::to_value(&meta).unwrap();

        assert_eq!(METADATA_SCHEMA_VERSION, 3);
        assert_eq!(value["schema_version"], 3);
        assert_eq!(
            json_keys(&value),
            [
                "provenance",
                "schema_version",
                "server_id",
                "server_name",
                "server_version",
                "tools"
            ]
            .into()
        );
        assert_eq!(
            json_keys(&value["tools"][0]),
            [
                "annotations",
                "category",
                "description",
                "keywords",
                "manifest_size_bytes",
                "name",
                "parameters",
                "result_type",
                "result_type_origin",
                "typescript_name"
            ]
            .into()
        );
        assert_eq!(
            json_keys(&value["tools"][0]["parameters"][0]),
            ["description", "name", "required", "typescript_type"].into()
        );
    }

    #[test]
    fn deserializes_minimal_tool() {
        let json = r#"{
            "schema_version": 3,
            "server_id": "github",
            "server_name": "GitHub",
            "server_version": "1.0.0",
            "tools": [{
                "name": "get_user",
                "typescript_name": "getUser",
                "category": null,
                "keywords": [],
                "description": null,
                "parameters": [],
                "annotations": null,
                "result_type": null,
                "result_type_origin": "undeclared",
                "manifest_size_bytes": 0
            }],
            "provenance": {
                "generated_at": "2026-01-01T00:00:00Z",
                "config_fingerprint": "0000000000000000000000000000000000000000000000000000000000000000",
                "tool_digest": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        }"#;

        let meta: ServerMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(meta.tools.len(), 1);
        assert!(meta.tools[0].category.is_none());
        assert_eq!(meta.tools[0].keywords, Vec::<String>::new());
    }

    /// A genuine `schema_version: 1` sidecar has no `provenance` key at all — typed
    /// deserialization must fail (a consumer is expected to check `schema_version` *first*, see
    /// `mcp-execution-skill`'s parser, rather than rely on this generic failure).
    #[test]
    fn deserialize_rejects_v1_shaped_document_missing_provenance() {
        let json = r#"{
            "schema_version": 1,
            "server_id": "github",
            "server_name": "GitHub",
            "server_version": "1.0.0",
            "tools": []
        }"#;

        let result: Result<ServerMetadata, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
