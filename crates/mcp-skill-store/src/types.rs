//! Core types for skill metadata and storage.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata for a saved skill.
///
/// Contains all information about a skill including server details,
/// generation timestamp, checksums for integrity verification, and tool list.
/// This is serialized to `skill.json` in the skill directory.
///
/// # Format Version
///
/// The `format_version` field enables future schema migrations. Current
/// version is "1.0".
///
/// # Examples
///
/// ```
/// use mcp_skill_store::{SkillMetadata, ServerInfo, Checksums, ToolInfo};
/// use chrono::Utc;
/// use std::collections::HashMap;
///
/// let metadata = SkillMetadata {
///     format_version: "1.0".to_string(),
///     server: ServerInfo {
///         name: "my-server".to_string(),
///         version: "1.0.0".to_string(),
///         protocol_version: "2024-11-05".to_string(),
///     },
///     generated_at: Utc::now(),
///     generator_version: "0.1.0".to_string(),
///     checksums: Checksums {
///         wasm: "blake3:abc123".to_string(),
///         generated: HashMap::new(),
///     },
///     tools: vec![
///         ToolInfo {
///             name: "send_message".to_string(),
///             description: "Send a message".to_string(),
///         }
///     ],
/// };
///
/// // Can be serialized to JSON
/// let json = serde_json::to_string_pretty(&metadata).unwrap();
/// assert!(json.contains("format_version"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Metadata format version for future schema migrations.
    ///
    /// Current version: "1.0"
    pub format_version: String,

    /// Server identification and version information.
    pub server: ServerInfo,

    /// Timestamp when this skill was generated.
    pub generated_at: DateTime<Utc>,

    /// Version of mcp-execution that generated this plugin.
    pub generator_version: String,

    /// Blake3 checksums for integrity verification.
    pub checksums: Checksums,

    /// List of tools provided by this plugin.
    pub tools: Vec<ToolInfo>,
}

/// Server identification information.
///
/// Identifies the MCP server that this skill was generated from, including
/// version information and MCP protocol version.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::ServerInfo;
///
/// let info = ServerInfo {
///     name: "github".to_string(),
///     version: "0.1.0".to_string(),
///     protocol_version: "2024-11-05".to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerInfo {
    /// Unique server identifier (used as directory name).
    ///
    /// This should be a valid directory name without path separators.
    pub name: String,

    /// Server version string (e.g., "1.0.0").
    pub version: String,

    /// MCP protocol version (e.g., "2024-11-05").
    pub protocol_version: String,
}

/// Checksum information for skill integrity verification.
///
/// Contains Blake3 hashes for the WASM module and all generated files.
/// Checksums are verified when loading a skill to detect corruption or
/// tampering.
///
/// # Format
///
/// Checksums are stored as `"blake3:<hex>"` where `<hex>` is the Blake3 hash
/// in lowercase hexadecimal.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::Checksums;
/// use std::collections::HashMap;
///
/// let mut generated = HashMap::new();
/// generated.insert("index.ts".to_string(), "blake3:abc123".to_string());
/// generated.insert("types.ts".to_string(), "blake3:def456".to_string());
///
/// let checksums = Checksums {
///     wasm: "blake3:789xyz".to_string(),
///     generated,
/// };
///
/// assert_eq!(checksums.generated.len(), 2);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checksums {
    /// Blake3 checksum of the WASM module.
    ///
    /// Format: `"blake3:<hex>"`
    pub wasm: String,

    /// Blake3 checksums of generated TypeScript files.
    ///
    /// Map from file path (relative to `generated/` directory) to checksum.
    /// Format: `"blake3:<hex>"`
    pub generated: HashMap<String, String>,
}

/// Tool information summary.
///
/// Brief description of a tool provided by the skill. Used for quick
/// reference without loading the full plugin.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::ToolInfo;
///
/// let tool = ToolInfo {
///     name: "send_message".to_string(),
///     description: "Sends a message to a chat".to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolInfo {
    /// Tool name as defined in MCP server.
    pub name: String,

    /// Human-readable tool description.
    pub description: String,
}

/// Loaded skill with all components.
///
/// Contains everything needed to use a skill: metadata, VFS with generated
/// code, and compiled WASM module.
///
/// This is returned by [`SkillStore::load_skill()`](crate::SkillStore::load_skill)
/// after verifying all checksums.
///
/// # Examples
///
/// ```no_run
/// use mcp_skill_store::SkillStore;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let store = SkillStore::new("./skills")?;
/// let plugin = store.load_skill("my-server")?;
///
/// println!("Server: {} v{}", plugin.metadata.server.name, plugin.metadata.server.version);
/// println!("Tools: {}", plugin.metadata.tools.len());
/// println!("VFS files: {}", plugin.vfs.file_count());
/// println!("WASM size: {} bytes", plugin.wasm_module.len());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    /// Plugin metadata including checksums and tool list.
    pub metadata: SkillMetadata,

    /// Virtual filesystem with generated TypeScript code.
    pub vfs: mcp_vfs::Vfs,

    /// Compiled WASM module bytes.
    pub wasm_module: Vec<u8>,
}

/// Brief skill information for listing.
///
/// Lightweight summary of a skill suitable for displaying in lists without
/// loading the entire plugin.
///
/// Returned by [`SkillStore::list_skills()`](crate::SkillStore::list_skills).
///
/// # Examples
///
/// ```no_run
/// use mcp_skill_store::SkillStore;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let store = SkillStore::new("./skills")?;
///
/// for skill in store.list_skills()? {
///     println!("{} v{} - {} tools (generated {})",
///         skill.server_name,
///         skill.version,
///         skill.tool_count,
///         skill.generated_at.format("%Y-%m-%d")
///     );
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct SkillInfo {
    /// Server name (identifier).
    pub server_name: String,

    /// Server version string.
    pub version: String,

    /// When the skill was generated.
    pub generated_at: DateTime<Utc>,

    /// Number of tools in the skill.
    pub tool_count: usize,
}

// Constants for legacy plugin structure (deprecated)
/// Current metadata format version.
pub const FORMAT_VERSION: &str = "1.0";

/// Name of the metadata file in each skill directory.
pub const METADATA_FILE: &str = "skill.json";

/// Name of the WASM module file in each skill directory.
pub const WASM_FILE: &str = "module.wasm";

/// Name of the directory containing generated TypeScript files.
pub const GENERATED_DIR: &str = "generated";

// Constants for Claude skill format
/// Name of the main skill file in Claude format.
pub const CLAUDE_SKILL_FILE: &str = "SKILL.md";

/// Name of the reference documentation file in Claude format.
pub const CLAUDE_REFERENCE_FILE: &str = "REFERENCE.md";

/// Name of the metadata file for Claude skills.
pub const CLAUDE_METADATA_FILE: &str = ".metadata.json";

/// Loaded Claude skill with all components.
///
/// Contains everything needed to use a Claude skill: SKILL.md content,
/// optional REFERENCE.md content, and metadata.
///
/// # Examples
///
/// ```no_run
/// use mcp_skill_store::SkillStore;
/// use mcp_core::SkillName;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let store = SkillStore::new_claude()?;
/// let skill_name = SkillName::new("my-skill")?;
/// let skill = store.load_claude_skill(&skill_name)?;
///
/// println!("Skill: {}", skill.name);
/// println!("Tools: {}", skill.metadata.tool_count);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct LoadedClaudeSkill {
    /// Skill name
    pub name: String,
    /// SKILL.md content
    pub skill_md: String,
    /// REFERENCE.md content (optional)
    pub reference_md: Option<String>,
    /// Metadata
    pub metadata: ClaudeSkillMetadata,
}

/// Metadata for Claude skills (stored in .metadata.json).
///
/// Contains information about skill generation, checksums for integrity
/// verification, and tool count.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::{ClaudeSkillMetadata, SkillChecksums};
/// use chrono::Utc;
///
/// let metadata = ClaudeSkillMetadata {
///     skill_name: "my-skill".to_string(),
///     server_name: "my-server".to_string(),
///     server_version: "1.0.0".to_string(),
///     protocol_version: "1.0".to_string(),
///     tool_count: 3,
///     generated_at: Utc::now(),
///     generator_version: "0.1.0".to_string(),
///     checksums: SkillChecksums {
///         skill_md: "blake3:abc123".to_string(),
///         reference_md: Some("blake3:def456".to_string()),
///     },
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeSkillMetadata {
    /// Skill name
    pub skill_name: String,
    /// Server name
    pub server_name: String,
    /// Server version
    pub server_version: String,
    /// Protocol version
    pub protocol_version: String,
    /// Number of tools
    pub tool_count: usize,
    /// Generation timestamp
    pub generated_at: DateTime<Utc>,
    /// Generator version
    pub generator_version: String,
    /// Blake3 checksums for integrity
    pub checksums: SkillChecksums,
}

/// Blake3 checksums for Claude skill files.
///
/// Contains checksums for SKILL.md and optionally REFERENCE.md.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::SkillChecksums;
///
/// let checksums = SkillChecksums {
///     skill_md: "blake3:abc123".to_string(),
///     reference_md: Some("blake3:def456".to_string()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillChecksums {
    /// SKILL.md checksum
    pub skill_md: String,
    /// REFERENCE.md checksum (if present)
    pub reference_md: Option<String>,
}

/// Summary of a Claude skill (for listing).
///
/// Lightweight summary suitable for displaying in lists without loading
/// the entire skill.
///
/// # Examples
///
/// ```no_run
/// use mcp_skill_store::SkillStore;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let store = SkillStore::new_claude()?;
///
/// for skill in store.list_claude_skills()? {
///     println!("{} v{} - {} tools",
///         skill.skill_name,
///         skill.server_version,
///         skill.tool_count
///     );
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct ClaudeSkillSummary {
    /// Skill name
    pub skill_name: String,
    /// Server name
    pub server_name: String,
    /// Server version
    pub server_version: String,
    /// Number of tools
    pub tool_count: usize,
    /// Generation timestamp
    pub generated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_metadata_serialization() {
        let metadata = SkillMetadata {
            format_version: FORMAT_VERSION.to_string(),
            server: ServerInfo {
                name: "test-server".to_string(),
                version: "1.0.0".to_string(),
                protocol_version: "2024-11-05".to_string(),
            },
            generated_at: Utc::now(),
            generator_version: "0.1.0".to_string(),
            checksums: Checksums {
                wasm: "blake3:test".to_string(),
                generated: HashMap::new(),
            },
            tools: vec![ToolInfo {
                name: "test_tool".to_string(),
                description: "Test tool".to_string(),
            }],
        };

        // Should serialize and deserialize correctly
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: SkillMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.format_version, FORMAT_VERSION);
        assert_eq!(deserialized.server.name, "test-server");
        assert_eq!(deserialized.tools.len(), 1);
    }

    #[test]
    fn test_server_info_equality() {
        let info1 = ServerInfo {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            protocol_version: "2024-11-05".to_string(),
        };

        let info2 = ServerInfo {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            protocol_version: "2024-11-05".to_string(),
        };

        assert_eq!(info1, info2);
    }

    #[test]
    fn test_tool_info_equality() {
        let tool1 = ToolInfo {
            name: "tool".to_string(),
            description: "desc".to_string(),
        };

        let tool2 = ToolInfo {
            name: "tool".to_string(),
            description: "desc".to_string(),
        };

        assert_eq!(tool1, tool2);
    }
}
