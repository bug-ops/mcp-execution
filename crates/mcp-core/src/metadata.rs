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
//! use mcp_execution_core::metadata::{ServerMetadata, ToolMetadata, METADATA_SCHEMA_VERSION};
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
//!     }],
//!     provenance: GenerationProvenance::capture(&config, &[]),
//! };
//!
//! let json = serde_json::to_string_pretty(&meta).unwrap();
//! let round_tripped: ServerMetadata = serde_json::from_str(&json).unwrap();
//! assert_eq!(round_tripped, meta);
//! ```

use crate::provenance::GenerationProvenance;
use crate::{ServerId, ToolName};
use serde::{Deserialize, Serialize};

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
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::METADATA_SCHEMA_VERSION;
///
/// assert_eq!(METADATA_SCHEMA_VERSION, 2);
/// ```
pub const METADATA_SCHEMA_VERSION: u32 = 2;

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
/// use mcp_execution_core::metadata::ToolMetadata;
/// use mcp_execution_core::ToolName;
///
/// let tool = ToolMetadata {
///     name: ToolName::new("create_issue").unwrap(),
///     typescript_name: "createIssue".to_string(),
///     category: Some("issues".to_string()),
///     keywords: vec!["create".to_string(), "issue".to_string()],
///     description: Some("Creates a new issue".to_string()),
///     parameters: vec![],
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

#[cfg(test)]
mod tests {
    use super::{METADATA_SCHEMA_VERSION, ParameterMetadata, ServerMetadata, ToolMetadata};
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
            }],
            provenance: test_provenance(),
        };

        let json = serde_json::to_string_pretty(&meta).unwrap();
        let round_tripped: ServerMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped, meta);
    }

    #[test]
    fn deserializes_minimal_tool() {
        let json = r#"{
            "schema_version": 2,
            "server_id": "github",
            "server_name": "GitHub",
            "server_version": "1.0.0",
            "tools": [{
                "name": "get_user",
                "typescript_name": "getUser",
                "category": null,
                "keywords": [],
                "description": null,
                "parameters": []
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
        assert!(meta.tools[0].keywords.is_empty());
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
