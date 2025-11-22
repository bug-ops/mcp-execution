//! Skill persistence layer for MCP execution.
//!
//! Provides functionality to save generated skills to disk and load them later,
//! enabling offline usage and skill distribution. Each skill consists of:
//! - Generated TypeScript code (from VFS)
//! - Compiled WASM module
//! - Metadata with checksums for integrity verification
//!
//! # Architecture
//!
//! Skills are stored in a simple directory structure:
//! ```text
//! ./skills/
//! ├── server-name/
//! │   ├── skill.json       # Metadata + checksums
//! │   ├── generated/        # TypeScript files
//! │   │   ├── tools/
//! │   │   │   └── *.ts
//! │   │   ├── index.ts
//! │   │   └── types.ts
//! │   └── module.wasm       # Compiled WASM
//! ```
//!
//! # Features
//!
//! - **Save/Load**: Persist skills to disk and restore them
//! - **Integrity**: Blake3 checksums verify file contents on load
//! - **Management**: List, check existence, and remove skills
//! - **Version Control**: Simple file-based format suitable for git
//! - **Security**: Checksum verification prevents tampering
//!
//! # Examples
//!
//! ## Saving a skill
//!
//! ```no_run
//! use mcp_skill_store::{SkillStore, ServerInfo, ToolInfo};
//! use mcp_vfs::VfsBuilder;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create skill store
//! let store = SkillStore::new("./skills")?;
//!
//! // Prepare skill data
//! let vfs = VfsBuilder::new()
//!     .add_file("/tools/tool.ts", "export function tool() {}")
//!     .build()?;
//! let wasm_module = vec![0x00, 0x61, 0x73, 0x6D]; // Real WASM bytes
//!
//! let server_info = ServerInfo {
//!     name: "my-server".to_string(),
//!     version: "1.0.0".to_string(),
//!     protocol_version: "2024-11-05".to_string(),
//! };
//!
//! let tools = vec![ToolInfo {
//!     name: "tool".to_string(),
//!     description: "Example tool".to_string(),
//! }];
//!
//! // Save skill
//! let metadata = store.save_skill("my-server", &vfs, &wasm_module, server_info, tools)?;
//! println!("Skill saved with {} tools", metadata.tools.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## Loading a skill
//!
//! ```no_run
//! use mcp_skill_store::SkillStore;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let store = SkillStore::new("./skills")?;
//!
//! // Load skill with checksum verification
//! let skill = store.load_skill("my-server")?;
//!
//! println!("Loaded skill with {} tools", skill.metadata.tools.len());
//! println!("VFS has {} files", skill.vfs.file_count());
//! println!("WASM module size: {} bytes", skill.wasm_module.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## Listing skills
//!
//! ```no_run
//! use mcp_skill_store::SkillStore;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let store = SkillStore::new("./skills")?;
//!
//! for skill in store.list_skills()? {
//!     println!("{} v{} ({} tools)",
//!         skill.server_name,
//!         skill.version,
//!         skill.tool_count
//!     );
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Security
//!
//! Blake3 checksums are used for integrity verification but not for security
//! against adversarial attacks. This crate assumes skills are from trusted
//! sources. For untrusted skills, additional cryptographic signatures would
//! be required (not implemented in MVP).
//!
//! # Performance
//!
//! - **Small skills** (10 files): < 50ms save/load
//! - **Large skills** (1000+ files): < 1s save/load
//! - **Checksum calculation**: < 10ms for 1MB WASM module

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

pub mod checksum;
pub mod error;
pub mod store;
pub mod types;

// Re-export main types
pub use checksum::constant_time_compare;
pub use error::{Result, SkillStoreError};
pub use store::SkillStore;

// Claude format types (primary API)
pub use types::{
    CLAUDE_METADATA_FILE, CLAUDE_REFERENCE_FILE, CLAUDE_SKILL_FILE, ClaudeSkillMetadata,
    ClaudeSkillSummary, LoadedClaudeSkill, SkillChecksums,
};

// Legacy format types (deprecated - for backward compatibility only)
pub use types::{Checksums, LoadedSkill, ServerInfo, SkillInfo, SkillMetadata, ToolInfo};
