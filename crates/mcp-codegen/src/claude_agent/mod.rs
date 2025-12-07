//! Claude Agent SDK code generation.
//!
//! This module generates TypeScript files for integration with the
//! Claude Agent SDK, using Zod schemas for type-safe tool definitions.
//!
//! # Generated Structure
//!
//! For a server with 3 tools, generates:
//!
//! ```text
//! ~/.claude/agent-sdk/{server-id}/
//! ├── index.ts              # Entry point with exports
//! ├── server.ts             # MCP server definition
//! └── tools/
//!     ├── createIssue.ts   # Individual tool with Zod schema
//!     ├── updateIssue.ts
//!     └── deleteIssue.ts
//! ```
//!
//! # Example
//!
//! ```no_run
//! use mcp_codegen::claude_agent::ClaudeAgentGenerator;
//! use mcp_introspector::ServerInfo;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let generator = ClaudeAgentGenerator::new()?;
//! // generator.generate(&server_info)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Generated Code Example
//!
//! The generated `tools/createIssue.ts` looks like:
//!
//! ```typescript
//! import { tool } from "@anthropic-ai/claude-agent-sdk";
//! import { z } from "zod";
//!
//! export const createIssue = tool(
//!   "create_issue",
//!   "Creates a new issue",
//!   {
//!     title: z.string().describe("Issue title"),
//!     body: z.string().optional().describe("Issue body")
//!   },
//!   async (args) => {
//!     // Implementation stub
//!     return { content: [{ type: "text", text: JSON.stringify(args) }] };
//!   }
//! );
//! ```

pub mod generator;
pub mod types;
pub mod zod;

pub use generator::ClaudeAgentGenerator;
pub use types::{IndexContext, PropertyInfo, ServerContext, ToolContext, ToolSummary};
