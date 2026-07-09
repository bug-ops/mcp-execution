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
//!
//! let meta = ServerMetadata {
//!     schema_version: METADATA_SCHEMA_VERSION,
//!     server_id: "github".to_string(),
//!     server_name: "GitHub".to_string(),
//!     server_version: "1.0.0".to_string(),
//!     tools: vec![ToolMetadata {
//!         name: "create_issue".to_string(),
//!         typescript_name: "createIssue".to_string(),
//!         category: Some("issues".to_string()),
//!         keywords: vec!["create".to_string(), "issue".to_string()],
//!         description: Some("Creates a new issue".to_string()),
//!         parameters: vec![],
//!     }],
//! };
//!
//! let json = serde_json::to_string_pretty(&meta).unwrap();
//! let round_tripped: ServerMetadata = serde_json::from_str(&json).unwrap();
//! assert_eq!(round_tripped, meta);
//! ```

use serde::{Deserialize, Serialize};

/// Current schema version of the `_meta.json` sidecar format.
///
/// Bump this when making a breaking change to [`ServerMetadata`] or its
/// nested types, so that a consumer built against an older schema fails
/// loudly (via a schema-version mismatch check) instead of silently
/// misinterpreting the new shape.
pub const METADATA_SCHEMA_VERSION: u32 = 1;

/// Filename of the sidecar metadata file emitted alongside generated tool files.
///
/// Shared between the producer (`mcp-execution-codegen`) and the consumer
/// (`mcp-execution-skill`) to avoid a stringly-typed filename duplicated in
/// two crates.
pub const METADATA_FILE_NAME: &str = "_meta.json";

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
///
/// let meta = ServerMetadata {
///     schema_version: METADATA_SCHEMA_VERSION,
///     server_id: "github".to_string(),
///     server_name: "GitHub".to_string(),
///     server_version: "1.0.0".to_string(),
///     tools: vec![],
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
    pub server_id: String,

    /// Human-readable server name.
    pub server_name: String,

    /// Server version string, as reported by the MCP server.
    pub server_version: String,

    /// Metadata for every generated tool, in generation order.
    pub tools: Vec<ToolMetadata>,
}

/// Structured metadata for a single generated tool.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::metadata::ToolMetadata;
///
/// let tool = ToolMetadata {
///     name: "create_issue".to_string(),
///     typescript_name: "createIssue".to_string(),
///     category: Some("issues".to_string()),
///     keywords: vec!["create".to_string(), "issue".to_string()],
///     description: Some("Creates a new issue".to_string()),
///     parameters: vec![],
/// };
///
/// assert_eq!(tool.name, "create_issue");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolMetadata {
    /// Original MCP tool name (the call identifier), unmodified.
    pub name: String,

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

    #[test]
    fn round_trips_through_json() {
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: "github".to_string(),
            server_name: "GitHub".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![ToolMetadata {
                name: "create_issue".to_string(),
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
        };

        let json = serde_json::to_string_pretty(&meta).unwrap();
        let round_tripped: ServerMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped, meta);
    }

    #[test]
    fn deserializes_minimal_tool() {
        let json = r#"{
            "schema_version": 1,
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
            }]
        }"#;

        let meta: ServerMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(meta.tools.len(), 1);
        assert!(meta.tools[0].category.is_none());
        assert!(meta.tools[0].keywords.is_empty());
    }
}
