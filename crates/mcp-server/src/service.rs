//! MCP server implementation for progressive loading generation.
//!
//! The `GeneratorService` provides three main tools:
//! 1. `introspect_server` - Connect to and introspect an MCP server
//! 2. `save_categorized_tools` - Generate TypeScript files with categorization
//! 3. `list_generated_servers` - List all servers with generated files

use crate::clock::{Clock, SystemClock};
use crate::state::StateManager;
use crate::types::{
    CategorizedTool, GeneratedServerInfo, IntrospectServerParams, IntrospectServerResult,
    IntrospectedToolSummary, ListGeneratedServersParams, ListGeneratedServersResult,
    PendingGeneration, SaveCategorizedToolsParams, SaveCategorizedToolsResult,
};
use mcp_execution_codegen::progressive::ProgressiveGenerator;
use mcp_execution_core::{ServerConfig, ServerId};
use mcp_execution_files::FilesBuilder;
use mcp_execution_introspector::Introspector;
use mcp_execution_skill::{
    GenerateSkillParams, MAX_TOOL_FILES, OutputPathError, SaveSkillParams, SaveSkillResult,
    ScanError, build_skill_context, extract_skill_metadata, resolve_skill_output_path,
    sanitize_path_for_error, scan_tools_directory, validate_server_id,
};
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ErrorData as McpError, tool, tool_handler, tool_router};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Maximum SKILL.md content size in bytes (100KB).
const MAX_SKILL_CONTENT_SIZE: usize = 100 * 1024;

/// Maximum byte length for a [`CategorizedTool::name`] field.
///
/// A legitimate value is always an already-introspected tool name (a short
/// identifier), never free-form text, so this is a generous ceiling rather
/// than a realistic expectation. Kept below common filesystem path-component
/// limits (255 bytes on ext4/APFS/NTFS): the name feeds into the generated
/// `.ts` filename via `save_categorized_tools`'s codegen/export pipeline, and
/// export staging is directory-level (a sibling temp directory, later
/// renamed into place - see `mcp_execution_files::FileSystem::export`), not a
/// per-file `.tmp` suffix, so the only path-component overhead on the actual
/// file is the `.ts` extension itself (true ceiling: 255 - 3 = 252 bytes).
/// The name also isn't used unchanged: it first passes through
/// `to_camel_case` and then `sanitize_ts_identifier`
/// (`mcp_execution_codegen::common::typescript`), which can only shrink the
/// string (each multi-byte UTF-8 `char` collapses to at most one ASCII byte)
/// plus at most one inserted leading `_`. Combined, this cap has roughly 124
/// bytes of headroom against the true 252-byte ceiling - kept well below it
/// mainly so the check stays meaningful without depending on the exact
/// shrink factor of that transform.
const MAX_CATEGORIZED_TOOL_NAME_LEN: usize = 128;

/// Maximum byte length for a [`CategorizedTool::category`] field.
const MAX_CATEGORY_LEN: usize = 100;

/// Maximum byte length for a [`CategorizedTool::keywords`] field
/// (a comma-separated list).
const MAX_KEYWORDS_LEN: usize = 500;

/// Maximum byte length for a [`CategorizedTool::short_description`] field.
///
/// The field's doc comment targets 80 characters; this cap is 4x that (the
/// maximum UTF-8 bytes per `char`) so legitimate multi-byte text is never
/// rejected while the size is still bounded.
const MAX_SHORT_DESCRIPTION_LEN: usize = 320;

/// MCP server for progressive loading generation.
///
/// This service helps generate progressive loading TypeScript files for other
/// MCP servers. Claude provides the categorization intelligence through natural
/// language understanding - no separate LLM API needed.
///
/// # Workflow
///
/// 1. Call `introspect_server` to discover tools from a target MCP server
/// 2. Claude analyzes the tools and assigns categories, keywords, descriptions
/// 3. Call `save_categorized_tools` to generate TypeScript files
/// 4. Use `list_generated_servers` to see all generated servers
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_server::service::GeneratorService;
/// use rmcp::transport::stdio;
///
/// # async fn example() {
/// let service = GeneratorService::new();
/// // Service implements rmcp ServerHandler trait
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct GeneratorService {
    /// State manager for pending generations
    state: Arc<StateManager>,

    /// Per-server-id introspector locks.
    ///
    /// Keying the lock by [`ServerId`] means a slow or hung downstream MCP
    /// server only blocks `introspect_server` calls for that same server id,
    /// not for unrelated ids across all sessions. The outer map mutex is only
    /// held long enough to fetch or insert the per-id handle - never across
    /// the `discover_server` await point.
    introspectors: Arc<Mutex<HashMap<ServerId, Arc<Mutex<Introspector>>>>>,

    /// Per-output-directory export locks, keyed by the (uncanonicalized)
    /// output path as supplied to `introspect_server` / stored on the
    /// pending generation. Same rationale as `introspectors`: keying by the
    /// contended resource means an export for one `output_dir` never blocks
    /// an export for a different one.
    exports: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,

    /// Clock used to construct pending generations (shared with `state`)
    clock: Arc<dyn Clock>,

    /// Base directory `save_skill` confines its output to.
    ///
    /// `None` in production, resolving to `~/.claude/skills`. Overridable
    /// only through [`Self::with_skills_base_dir_for_test`] so tests can
    /// exercise `save_skill`'s happy path without writing under the real
    /// home directory.
    skills_base_dir: Option<PathBuf>,

    /// Tool router for MCP protocol
    // Only read via macro-expanded code generated by the `#[tool_router]` attribute
    // macro, so the compiler's static dead-code analysis cannot see the usage.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl GeneratorService {
    /// Creates a new generator service using the real system clock.
    #[must_use]
    pub fn new() -> Self {
        Self::with_clock(Arc::new(SystemClock))
    }

    /// Creates a new generator service backed by a custom clock.
    ///
    /// Used in tests to inject a fake clock so session expiry can be
    /// exercised deterministically.
    fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self {
            state: Arc::new(StateManager::with_clock(Arc::clone(&clock))),
            introspectors: Arc::new(Mutex::new(HashMap::new())),
            exports: Arc::new(Mutex::new(HashMap::new())),
            clock,
            skills_base_dir: None,
            tool_router: Self::tool_router(),
        }
    }

    /// Returns the base directory `save_skill` confines its output to.
    fn skills_base_dir(&self) -> PathBuf {
        self.skills_base_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
                .join("skills")
        })
    }

    /// Overrides the `save_skill` base directory. Test-only: production
    /// callers always confine writes to the real `~/.claude/skills`.
    #[cfg(test)]
    #[must_use]
    fn with_skills_base_dir_for_test(mut self, dir: PathBuf) -> Self {
        self.skills_base_dir = Some(dir);
        self
    }

    /// Returns the per-server-id introspector handle, creating one if absent.
    ///
    /// The outer map lock is released before the returned handle is awaited
    /// on, so discovery of unrelated server ids never contends on it.
    async fn introspector_for(&self, server_id: &ServerId) -> Arc<Mutex<Introspector>> {
        let mut introspectors = self.introspectors.lock().await;
        introspectors
            .entry(server_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(Introspector::new())))
            .clone()
    }

    /// Evicts the per-server-id introspector handle after use, but only if
    /// the map still holds the exact handle the caller obtained.
    ///
    /// `server_id` values are caller-supplied, so without eviction the map
    /// grows without bound as new ids are introspected. Called after
    /// `discover_server` completes, regardless of outcome.
    ///
    /// A caller must pass the same `Arc<Mutex<Introspector>>` it received
    /// from [`Self::introspector_for`]. Removing by `server_id` alone is a
    /// TOCTOU bug: if another in-flight call for the same id already evicted
    /// and a third call inserted a fresh handle, an unconditional `remove`
    /// would prune that live handle out from under the third call. Comparing
    /// with [`Arc::ptr_eq`] ensures a caller can only ever evict the entry it
    /// created.
    async fn evict_introspector(&self, server_id: &ServerId, handle: &Arc<Mutex<Introspector>>) {
        let mut introspectors = self.introspectors.lock().await;
        if let std::collections::hash_map::Entry::Occupied(entry) =
            introspectors.entry(server_id.clone())
            && Arc::ptr_eq(entry.get(), handle)
        {
            entry.remove();
        }
    }

    /// Returns the per-output-directory export lock, creating one if absent.
    ///
    /// Mirrors [`Self::introspector_for`]: the outer map lock is released
    /// before the returned handle is awaited on, so exports to unrelated
    /// output directories never contend on it. Holding this lock across an
    /// [`mcp_execution_files::FileSystem::export_to_filesystem`] call
    /// serializes any two concurrent `save_categorized_tools` calls for the
    /// same `output_dir` that overlap while holding the same handle,
    /// narrowing the in-process trigger for the data-loss race described in
    /// issue #169. This is not an unconditional guarantee across three or
    /// more overlapping calls: a call that fetches a fresh handle only
    /// after an earlier holder has already evicted its own can still run
    /// concurrently with a still-in-flight call holding the stale handle
    /// (same eviction-boundary gap as [`Self::evict_introspector`]). The
    /// age-gated sweep in `mcp-execution-files` is what ultimately prevents
    /// data loss if that happens.
    async fn export_lock_for(&self, output_dir: &Path) -> Arc<Mutex<()>> {
        let mut exports = self.exports.lock().await;
        exports
            .entry(output_dir.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Evicts the per-output-directory export lock after use, but only if
    /// the map still holds the exact handle the caller obtained.
    ///
    /// Same identity-checked eviction as [`Self::evict_introspector`] and
    /// for the same reason: `output_dir` values are caller-supplied, so
    /// without eviction the map grows without bound, and an unconditional
    /// `remove` keyed only by path would be a TOCTOU bug against a
    /// concurrently inserted fresh handle.
    async fn evict_export_lock(&self, output_dir: &Path, handle: &Arc<Mutex<()>>) {
        let mut exports = self.exports.lock().await;
        if let std::collections::hash_map::Entry::Occupied(entry) =
            exports.entry(output_dir.to_path_buf())
            && Arc::ptr_eq(entry.get(), handle)
        {
            entry.remove();
        }
    }
}

impl Default for GeneratorService {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl GeneratorService {
    /// Introspect an MCP server and prepare for categorization.
    ///
    /// Connects to the target MCP server, discovers its tools, and returns
    /// metadata for Claude to categorize. Returns a session ID for use with
    /// `save_categorized_tools`.
    #[tool(
        description = "Connect to an MCP server, discover its tools, and return metadata for categorization. Returns a session ID for use with save_categorized_tools."
    )]
    async fn introspect_server(
        &self,
        Parameters(params): Parameters<IntrospectServerParams>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, McpError> {
        // Validate server_id format
        validate_server_id(&params.server_id)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Extract server_id before consuming params
        let server_id_str = params.server_id;
        let server_id = ServerId::new(&server_id_str);

        // Determine output directory (needs server_id_str)
        let output_dir = params.output_dir.unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
                .join("servers")
                .join(&server_id_str)
        });

        // Build server config (consume args and env to avoid clones)
        let mut config_builder = ServerConfig::builder().command(params.command);

        for arg in params.args {
            config_builder = config_builder.arg(arg);
        }

        for (key, value) in params.env {
            config_builder = config_builder.env(key, value);
        }

        if let Some(secs) = params.connect_timeout_secs {
            config_builder = config_builder.connect_timeout(std::time::Duration::from_secs(secs));
        }

        if let Some(secs) = params.discover_timeout_secs {
            config_builder = config_builder.discover_timeout(std::time::Duration::from_secs(secs));
        }

        let config = config_builder.build().map_err(|e| {
            // A `SecurityViolation` here (shell metacharacters, forbidden env var, etc.) is
            // caused by the caller's own params, same as a `ValidationError` — not an
            // internal server fault — so both map to `invalid_params`.
            if e.is_validation_error() || e.is_security_error() {
                McpError::invalid_params(e.to_string(), None)
            } else {
                McpError::internal_error(format!("Failed to build server config: {e}"), None)
            }
        })?;

        // Connect and introspect, holding only the lock for this server_id. A
        // tokio::select! against `ct.cancelled()` lets a client-issued
        // `notifications/cancelled` interrupt the (potentially up-to-600s)
        // discovery round trip instead of always running it to completion.
        // `biased;` prefers noticing cancellation over starting/continuing
        // discovery, making the cancelled path deterministic rather than
        // depending on `tokio::select!`'s (default-randomised) poll order.
        let introspector_handle = self.introspector_for(&server_id).await;
        let mut introspector = introspector_handle.lock().await;
        let discover_outcome = tokio::select! {
            biased;
            () = ct.cancelled() => None,
            result = introspector.discover_server(server_id.clone(), &config) => Some(result),
        };
        drop(introspector);

        // Evict the per-server-id handle regardless of outcome (including
        // cancellation), so caller-supplied server_id values can't grow the
        // introspectors map without bound. Only removes the entry if it is
        // still this exact handle (see `evict_introspector` docs for why
        // identity matters here).
        self.evict_introspector(&server_id, &introspector_handle)
            .await;

        let discover_result = discover_outcome.ok_or_else(|| {
            McpError::internal_error("introspect_server cancelled by client", None)
        })?;

        let server_info = discover_result.map_err(|e| {
            // See the matching comment above `config_builder.build()`: a `SecurityViolation`
            // is a caller-param problem too, not an internal fault.
            if e.is_validation_error() || e.is_security_error() {
                McpError::invalid_params(e.to_string(), None)
            } else {
                McpError::internal_error(format!("Failed to introspect server: {e}"), None)
            }
        })?;

        // Extract tool metadata for Claude
        let tools: Vec<IntrospectedToolSummary> = server_info
            .tools
            .iter()
            .map(|tool| {
                let parameters = extract_parameter_names(&tool.input_schema);

                IntrospectedToolSummary {
                    name: tool.name.as_str().to_string(),
                    description: tool.description.clone(),
                    parameters,
                }
            })
            .collect();

        // Store pending generation
        let pending = PendingGeneration::new(
            server_id,
            server_info.clone(),
            config,
            output_dir.clone(),
            self.clock.as_ref(),
        );

        let session_id = self.state.store(pending.clone()).await;

        // Build result
        let result = IntrospectServerResult {
            server_id: server_id_str,
            server_name: server_info.name,
            tools_found: tools.len(),
            tools,
            session_id,
            expires_at: pending.expires_at,
        };

        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }

    /// Save categorized tools as TypeScript files.
    ///
    /// Generates progressive loading TypeScript files using Claude's
    /// categorization. Requires `session_id` from a previous `introspect_server`
    /// call.
    ///
    /// Does not observe request cancellation, unlike `introspect_server` and
    /// `generate_skill`. An earlier version raced the wait for the
    /// per-`output_dir` export lock against `ct.cancelled()`, but that
    /// produced two correctness bugs in succession: cancelling while another
    /// caller held the lock either leaked the `exports` map entry, or (once
    /// that leak was fixed by evicting unconditionally) evicted the entry out
    /// from under the still-running holder, handing the *next* caller a fresh
    /// lock that no longer serializes against it - reopening the #169
    /// data-loss race for the whole duration of the in-flight export, not
    /// just a narrow timing window. The export itself was already
    /// deliberately excluded from cancellation (see `export_lock_for`), so
    /// cancelling only the lock *wait* bought little for two rounds of bugs;
    /// removing it entirely, the same call S1 made for `save_skill`, removes
    /// the whole class of problems.
    #[tool(
        description = "Generate progressive loading TypeScript files using Claude's categorization. Requires session_id from a previous introspect_server call."
    )]
    async fn save_categorized_tools(
        &self,
        Parameters(params): Parameters<SaveCategorizedToolsParams>,
    ) -> Result<CallToolResult, McpError> {
        // Retrieve pending generation
        let pending = self.state.take(params.session_id).await.ok_or_else(|| {
            McpError::invalid_params(
                "Session not found or expired. Please run introspect_server again.",
                None,
            )
        })?;

        // Validate categorized tools match introspected tools
        let introspected_names: HashSet<_> = pending
            .server_info
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect();

        // A legitimate call can never submit more entries than there are
        // introspected tools: every name must be a member of
        // `introspected_names` (checked below) and duplicates are rejected,
        // so the entry count is bounded by that set's size. Reject early,
        // before any per-entry validation, HashMap insertion, or codegen
        // work (CWE-400 - see issue #197).
        //
        // `introspected_names.len()` alone is not a trustworthy ceiling: it
        // comes from whatever tool count the *target* MCP server reported to
        // `introspect_server`, so a hostile or buggy target could inflate it
        // arbitrarily. `MAX_TOOL_FILES` is the same per-server tool-count
        // ceiling `generate_skill` already enforces (via
        // `mcp_execution_skill::scan_tools_directory`), so reusing it here
        // keeps the two stages consistent - otherwise this call could
        // happily generate more tool files than `generate_skill` will later
        // accept.
        let max_allowed_tools = introspected_names.len().min(MAX_TOOL_FILES);
        if params.categorized_tools.len() > max_allowed_tools {
            return Err(McpError::invalid_params(
                format!(
                    "categorized_tools has {} entries but at most {} are allowed \
                     (min of {} introspected tools and the {} tool-file cap; \
                     duplicates are not allowed)",
                    params.categorized_tools.len(),
                    max_allowed_tools,
                    introspected_names.len(),
                    MAX_TOOL_FILES,
                ),
                None,
            ));
        }

        let mut seen_names: HashSet<&str> = HashSet::with_capacity(params.categorized_tools.len());
        for cat_tool in &params.categorized_tools {
            if !introspected_names.contains(cat_tool.name.as_str()) {
                return Err(McpError::invalid_params(
                    format!("Tool '{}' not found in introspected tools", cat_tool.name),
                    None,
                ));
            }

            if !seen_names.insert(cat_tool.name.as_str()) {
                return Err(McpError::invalid_params(
                    format!(
                        "Tool '{}' appears more than once in categorized_tools",
                        cat_tool.name
                    ),
                    None,
                ));
            }

            if cat_tool.name.len() > MAX_CATEGORIZED_TOOL_NAME_LEN {
                return Err(McpError::invalid_params(
                    format!(
                        "Tool name '{}' is {} bytes, exceeding the {} byte limit",
                        cat_tool.name,
                        cat_tool.name.len(),
                        MAX_CATEGORIZED_TOOL_NAME_LEN
                    ),
                    None,
                ));
            }

            if cat_tool.category.len() > MAX_CATEGORY_LEN {
                return Err(McpError::invalid_params(
                    format!(
                        "category for tool '{}' is {} bytes, exceeding the {} byte limit",
                        cat_tool.name,
                        cat_tool.category.len(),
                        MAX_CATEGORY_LEN
                    ),
                    None,
                ));
            }

            if cat_tool.keywords.len() > MAX_KEYWORDS_LEN {
                return Err(McpError::invalid_params(
                    format!(
                        "keywords for tool '{}' is {} bytes, exceeding the {} byte limit",
                        cat_tool.name,
                        cat_tool.keywords.len(),
                        MAX_KEYWORDS_LEN
                    ),
                    None,
                ));
            }

            if cat_tool.short_description.len() > MAX_SHORT_DESCRIPTION_LEN {
                return Err(McpError::invalid_params(
                    format!(
                        "short_description for tool '{}' is {} bytes, exceeding the {} byte limit",
                        cat_tool.name,
                        cat_tool.short_description.len(),
                        MAX_SHORT_DESCRIPTION_LEN
                    ),
                    None,
                ));
            }
        }

        // Build categorization map and category stats in single pass (avoid double iteration)
        let tool_count = params.categorized_tools.len();
        let mut categorization: HashMap<String, &CategorizedTool> =
            HashMap::with_capacity(tool_count);
        let mut categories: HashMap<String, usize> = HashMap::with_capacity(tool_count);

        for tool in &params.categorized_tools {
            categorization.insert(tool.name.clone(), tool);
            *categories.entry(tool.category.clone()).or_default() += 1;
        }

        // Generate code with categorization
        let generator = ProgressiveGenerator::new().map_err(|e| {
            McpError::internal_error(format!("Failed to create generator: {e}"), None)
        })?;

        let code = generate_with_categorization(&generator, &pending.server_info, &categorization)
            .map_err(|e| McpError::internal_error(format!("Failed to generate code: {e}"), None))?;

        // Build virtual filesystem
        let vfs = FilesBuilder::from_generated_code(code, "/")
            .build()
            .map_err(|e| McpError::internal_error(format!("Failed to build VFS: {e}"), None))?;

        // Capture file count before moving vfs
        let files_generated = vfs.file_count();

        // Ensure the parent of the output directory exists (async). Only the
        // parent is needed: `export_to_filesystem` publishes `output_dir`
        // itself atomically (single rename on first generate, stage-then-swap
        // on regeneration), so pre-creating it here would force the slower
        // regeneration path even on a brand-new server.
        if let Some(parent) = pending.output_dir.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                McpError::internal_error(format!("Failed to create output directory: {e}"), None)
            })?;
        }

        // Export to filesystem (blocking operation wrapped in spawn_blocking).
        // Held across the export so a second concurrent call for the same
        // output_dir blocks until the first finishes, rather than racing on
        // the underlying staging/swap (see `export_lock_for`).
        let export_lock = self.export_lock_for(&pending.output_dir).await;
        let export_guard = export_lock.lock().await;

        let output_dir = pending.output_dir.clone();
        let export_result =
            tokio::task::spawn_blocking(move || vfs.export_to_filesystem(&output_dir)).await;

        drop(export_guard);
        self.evict_export_lock(&pending.output_dir, &export_lock)
            .await;

        export_result
            .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?
            .map_err(|e| McpError::internal_error(format!("Failed to export files: {e}"), None))?;

        let result = SaveCategorizedToolsResult {
            success: true,
            files_generated,
            output_dir: pending.output_dir.display().to_string(),
            categories,
            errors: vec![],
        };

        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }

    /// List all servers with generated progressive loading files.
    ///
    /// Scans the output directory (default: `~/.claude/servers`) for servers
    /// that have generated TypeScript files.
    ///
    /// Does not observe request cancellation, unlike `introspect_server` and
    /// `generate_skill`. The scan runs inside a
    /// single `spawn_blocking` task with no subprocess, network I/O, or
    /// long-held lock, so it isn't worth the added complexity - but it is
    /// *not* a small bounded read: it is a nested directory walk (one
    /// `read_dir` over `base_dir`, plus a second `read_dir` per
    /// subdirectory), and `base_dir` is caller-supplied and unvalidated, so a
    /// large or adversarial directory tree can still make this call slow and
    /// its result `Vec` large. That pre-existing surface is unrelated to
    /// cancellation and out of scope here.
    #[tool(
        description = "List all MCP servers that have generated progressive loading files in ~/.claude/servers/"
    )]
    async fn list_generated_servers(
        &self,
        Parameters(params): Parameters<ListGeneratedServersParams>,
    ) -> Result<CallToolResult, McpError> {
        let base_dir = params.base_dir.map_or_else(
            || {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".claude")
                    .join("servers")
            },
            PathBuf::from,
        );

        // Scan directories (blocking operation wrapped in spawn_blocking)
        let servers = tokio::task::spawn_blocking(move || {
            let mut servers = Vec::new();

            if base_dir.exists()
                && base_dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&base_dir)
            {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let id = entry.file_name().to_string_lossy().to_string();

                        // Count .ts files (excluding _runtime and starting with _)
                        let tool_count = std::fs::read_dir(entry.path()).map_or(0, |e| {
                            e.flatten()
                                .filter(|f| {
                                    let name = f.file_name();
                                    let name = name.to_string_lossy();
                                    name.ends_with(".ts") && !name.starts_with('_')
                                })
                                .count()
                        });

                        // Get modification time
                        let generated_at = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .map(chrono::DateTime::<chrono::Utc>::from);

                        servers.push(GeneratedServerInfo {
                            id,
                            tool_count,
                            generated_at,
                            output_dir: entry.path().display().to_string(),
                        });
                    }
                }
            }

            servers.sort_by(|a, b| a.id.cmp(&b.id));
            servers
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        let result = ListGeneratedServersResult {
            total_servers: servers.len(),
            servers,
        };

        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }

    /// Generate context for creating a Claude Code skill.
    ///
    /// Analyzes generated TypeScript files and returns structured context
    /// that Claude uses to generate an optimal SKILL.md file.
    ///
    /// # Workflow
    ///
    /// 1. Call `generate_skill` with `server_id`
    /// 2. Claude receives context and `generation_prompt`
    /// 3. Claude generates SKILL.md content
    /// 4. Call `save_skill` with the generated content
    #[tool(
        description = "Analyze generated TypeScript files and return context for Claude to create a SKILL.md file. Returns tool metadata, categories, and a generation prompt."
    )]
    async fn generate_skill(
        &self,
        Parameters(params): Parameters<GenerateSkillParams>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, McpError> {
        // Validate server_id format and length
        validate_server_id(&params.server_id)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Determine servers directory
        let servers_dir = params.servers_dir.unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
                .join("servers")
        });

        let server_dir = servers_dir.join(&params.server_id);

        // Check if server directory exists
        if !server_dir.exists() {
            return Err(McpError::invalid_params(
                format!(
                    "Server directory not found: {}. Run generate first.",
                    server_dir.display()
                ),
                None,
            ));
        }

        // Scan and parse tool files. A missing or version-mismatched sidecar reflects the
        // same "not generated / stale directory" caller situation as the `!server_dir.exists()`
        // check above, so it is reported the same way (`invalid_params`), not as a server fault.
        //
        // The scan walks every tool file in the directory, so a large directory
        // can take a while; a tokio::select! against `ct.cancelled()` lets the
        // client abort instead of always waiting for it to finish. `biased;`
        // prefers noticing cancellation over starting/continuing the scan, so
        // the cancelled path is deterministic rather than depending on
        // `tokio::select!`'s (default-randomised) poll order.
        let scan_outcome = tokio::select! {
            biased;
            () = ct.cancelled() => None,
            result = scan_tools_directory(&server_dir) => Some(result),
        };

        let scan_result = scan_outcome
            .ok_or_else(|| McpError::internal_error("generate_skill cancelled by client", None))?
            .map_err(|e| match e {
                ScanError::MissingMetadata { .. }
                | ScanError::UnsupportedSchema { .. }
                | ScanError::StaleMetadata { .. } => {
                    McpError::invalid_params(format!("Failed to scan tools directory: {e}"), None)
                }
                ScanError::Io(_)
                | ScanError::DirectoryNotFound { .. }
                | ScanError::MetadataParse { .. }
                | ScanError::TooManyFiles { .. }
                | ScanError::FileTooLarge { .. } => {
                    McpError::internal_error(format!("Failed to scan tools directory: {e}"), None)
                }
            })?;

        if scan_result.tools.is_empty() {
            return Err(McpError::invalid_params(
                format!(
                    "No tool files found in {}. Run generate first.",
                    server_dir.display()
                ),
                None,
            ));
        }

        // Build context
        let mut result = build_skill_context(
            &params.server_id,
            &scan_result.tools,
            params.use_case_hints.as_deref(),
        );

        // Surface non-fatal drift warnings (e.g. `.ts` files excluded for lacking
        // a sidecar entry) in the structured response, not just server-side
        // tracing output (issue #161).
        result.warnings = scan_result.warnings;

        // Override skill name if provided
        if let Some(name) = params.skill_name {
            result.skill_name = name;
        }

        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }

    /// Save a generated skill to the filesystem.
    ///
    /// Writes SKILL.md content to `~/.claude/skills/{server_id}/SKILL.md` by
    /// default. `output_path`, if supplied, is confined to
    /// `~/.claude/skills/{server_id}/` (see
    /// [`resolve_skill_output_path`](mcp_execution_skill::resolve_skill_output_path)) —
    /// it cannot be absolute, contain `..`, or reach another server's
    /// directory. Validates that the content contains required YAML
    /// frontmatter.
    ///
    /// Does not observe request cancellation: `tokio::fs::write` runs on the
    /// blocking-task pool and, once started, cannot be interrupted - dropping
    /// its `JoinHandle` does not stop the queued write, it only stops this
    /// handler from waiting for it. Racing it against `ct.cancelled()` would
    /// therefore make the response lie (telling a cancelled client the write
    /// never happened while it still lands on disk moments later), which is
    /// worse than not attempting cancellation at all. The write is also
    /// bounded by [`MAX_SKILL_CONTENT_SIZE`] (100KB), so it is not worth
    /// pursuing genuine interruptibility (e.g. a hand-rolled chunked write)
    /// for the marginal benefit.
    ///
    /// The synchronous YAML frontmatter parse that runs before the write
    /// (`extract_skill_metadata`) is a separate concern: `serde_norway` is not
    /// linear-time on pathologically nested input, so bounding only the
    /// overall [`MAX_SKILL_CONTENT_SIZE`] would not bound parse latency. It is
    /// `extract_skill_metadata`'s own `MAX_FRONTMATTER_SIZE` cap (8KB) on the
    /// extracted `---`-delimited block, applied before parsing, that keeps
    /// this handler's blocking work small regardless of `content`'s overall
    /// size — not the 100KB content bound.
    #[tool(
        description = "Save generated SKILL.md content to ~/.claude/skills/{server_id}/. Use after Claude generates skill content from generate_skill context."
    )]
    async fn save_skill(
        &self,
        Parameters(params): Parameters<SaveSkillParams>,
    ) -> Result<CallToolResult, McpError> {
        // Validate server_id format and length
        validate_server_id(&params.server_id)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        // Validate content size (DoS protection)
        if params.content.len() > MAX_SKILL_CONTENT_SIZE {
            return Err(McpError::invalid_params(
                format!(
                    "content too large: {} bytes exceeds {} limit",
                    params.content.len(),
                    MAX_SKILL_CONTENT_SIZE
                ),
                None,
            ));
        }

        // Validate content has YAML frontmatter
        if !params.content.starts_with("---") {
            return Err(McpError::invalid_params(
                "Content must start with YAML frontmatter (---)",
                None,
            ));
        }

        // Extract metadata from frontmatter
        let metadata = extract_skill_metadata(&params.content)
            .map_err(|e| McpError::invalid_params(format!("Invalid SKILL.md format: {e}"), None))?;

        // Determine and confine the output path to ~/.claude/skills/, rejecting
        // absolute overrides, `..` traversal, and symlink-based escapes (issue #184).
        let output_path = resolve_skill_output_path(
            &self.skills_base_dir(),
            &params.server_id,
            params.output_path.as_deref(),
        )
        .await
        .map_err(|e| match e {
            // `server_id` was already validated above by `validate_server_id`, which is
            // strictly tighter than `resolve_skill_output_path`'s internal check, so this
            // arm is unreachable from this call site — kept distinct (rather than folded
            // into the `output_path` arm below) because `resolve_skill_output_path` is
            // public API other callers may reach without that upstream validation.
            OutputPathError::InvalidServerId { .. } => {
                McpError::invalid_params(format!("Invalid server_id: {e}"), None)
            }
            OutputPathError::AbsolutePath { .. }
            | OutputPathError::ParentTraversal { .. }
            | OutputPathError::InvalidPath { .. }
            | OutputPathError::Escape { .. }
            | OutputPathError::NotADirectory { .. }
            | OutputPathError::NotAFile { .. } => {
                McpError::invalid_params(format!("Invalid output_path: {e}"), None)
            }
            OutputPathError::CreateDir { .. } | OutputPathError::Io(_) => {
                McpError::internal_error(format!("Failed to resolve output path: {e}"), None)
            }
        })?;

        // Check if file exists
        let overwritten = output_path.exists();
        if overwritten && !params.overwrite {
            return Err(McpError::invalid_params(
                format!(
                    "Skill file already exists: {}. Use overwrite=true to replace.",
                    sanitize_path_for_error(&output_path)
                ),
                None,
            ));
        }

        // Write file (parent directory already created and confined by
        // resolve_skill_output_path)
        tokio::fs::write(&output_path, &params.content)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to write file: {e}"), None))?;

        let result = SaveSkillResult {
            success: true,
            output_path: output_path.display().to_string(),
            overwritten,
            metadata,
        };

        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }
}

#[tool_handler]
impl ServerHandler for GeneratorService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.protocol_version = ProtocolVersion::V_2025_06_18;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        info.instructions = Some(
            "Generate progressive loading TypeScript files for MCP servers. \
             Use introspect_server to discover tools, then save_categorized_tools \
             with your categorization."
                .to_string(),
        );
        info
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Extracts parameter names from a JSON Schema.
fn extract_parameter_names(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default()
}

/// Generates code with categorization metadata.
///
/// Converts the categorization map to the format expected by the generator
/// and calls `generate_with_categories`.
fn generate_with_categorization(
    generator: &ProgressiveGenerator,
    server_info: &mcp_execution_introspector::ServerInfo,
    categorization: &HashMap<String, &CategorizedTool>,
) -> mcp_execution_core::Result<mcp_execution_codegen::GeneratedCode> {
    use mcp_execution_codegen::progressive::ToolCategorization;

    // Convert CategorizedTool map to ToolCategorization map
    let categorizations: HashMap<String, ToolCategorization> = categorization
        .iter()
        .map(|(tool_name, cat_tool)| {
            (
                tool_name.clone(),
                ToolCategorization {
                    category: cat_tool.category.clone(),
                    keywords: cat_tool.keywords.clone(),
                    short_description: cat_tool.short_description.clone(),
                },
            )
        })
        .collect();

    generator.generate_with_categories(server_info, &categorizations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mcp_execution_core::ToolName;
    use mcp_execution_introspector::{ServerCapabilities, ToolInfo};
    use rmcp::model::ErrorCode;
    use uuid::Uuid;

    // ========================================================================
    // Helper Functions Tests
    // ========================================================================

    #[test]
    fn test_extract_parameter_names() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "number" }
            }
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 2);
        assert!(params.contains(&"name".to_string()));
        assert!(params.contains(&"age".to_string()));
    }

    #[test]
    fn test_extract_parameter_names_empty() {
        let schema = serde_json::json!({
            "type": "object"
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_parameter_names_no_properties() {
        let schema = serde_json::json!({
            "type": "string"
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_parameter_names_nested_object() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                },
                "age": { "type": "number" }
            }
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 2);
        assert!(params.contains(&"user".to_string()));
        assert!(params.contains(&"age".to_string()));
    }

    #[test]
    fn test_generate_with_categorization() {
        let generator = ProgressiveGenerator::new().unwrap();

        let server_info = mcp_execution_introspector::ServerInfo {
            id: ServerId::new("test"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new("test_tool"),
                description: "Test tool description".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "param1": { "type": "string" }
                    }
                }),
                output_schema: None,
            }],
        };

        let categorized_tool = CategorizedTool {
            name: "test_tool".to_string(),
            category: "testing".to_string(),
            keywords: "test,tool".to_string(),
            short_description: "Test tool for testing".to_string(),
        };

        let mut categorization = HashMap::new();
        categorization.insert("test_tool".to_string(), &categorized_tool);

        let result = generate_with_categorization(&generator, &server_info, &categorization);
        assert!(result.is_ok());

        let code = result.unwrap();
        assert!(code.file_count() > 0, "Should generate at least one file");
    }

    #[test]
    fn test_generate_with_categorization_multiple_tools() {
        let generator = ProgressiveGenerator::new().unwrap();

        let server_info = mcp_execution_introspector::ServerInfo {
            id: ServerId::new("test"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![
                ToolInfo {
                    name: ToolName::new("tool1"),
                    description: "First tool".to_string(),
                    input_schema: serde_json::json!({"type": "object"}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool2"),
                    description: "Second tool".to_string(),
                    input_schema: serde_json::json!({"type": "object"}),
                    output_schema: None,
                },
            ],
        };

        let tool1 = CategorizedTool {
            name: "tool1".to_string(),
            category: "category1".to_string(),
            keywords: "test".to_string(),
            short_description: "Tool 1".to_string(),
        };

        let tool2 = CategorizedTool {
            name: "tool2".to_string(),
            category: "category2".to_string(),
            keywords: "test".to_string(),
            short_description: "Tool 2".to_string(),
        };

        let mut categorization = HashMap::new();
        categorization.insert("tool1".to_string(), &tool1);
        categorization.insert("tool2".to_string(), &tool2);

        let result = generate_with_categorization(&generator, &server_info, &categorization);
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_with_categorization_empty_tools() {
        let generator = ProgressiveGenerator::new().unwrap();

        let server_id = ServerId::new("test");
        let server_info = mcp_execution_introspector::ServerInfo {
            id: server_id,
            name: "Empty Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![],
        };

        let categorization = HashMap::new();

        let result = generate_with_categorization(&generator, &server_info, &categorization);
        assert!(result.is_ok());
    }

    // ========================================================================
    // Service Tests
    // ========================================================================

    #[test]
    fn test_generator_service_new() {
        let service = GeneratorService::new();
        assert!(service.introspectors.try_lock().is_ok());
        assert!(service.exports.try_lock().is_ok());
    }

    #[test]
    fn test_generator_service_default() {
        let service = GeneratorService::default();
        assert!(service.introspectors.try_lock().is_ok());
        assert!(service.exports.try_lock().is_ok());
    }

    #[test]
    fn test_get_info() {
        let service = GeneratorService::new();
        let info = service.get_info();

        assert_eq!(info.protocol_version, ProtocolVersion::V_2025_06_18);
        assert!(info.capabilities.tools.is_some());
        assert!(info.instructions.is_some());
        assert_eq!(info.server_info.name, env!("CARGO_PKG_NAME"));
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }

    // ========================================================================
    // Input Validation Tests
    // ========================================================================

    #[tokio::test]
    async fn test_introspect_server_invalid_server_id_uppercase() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "GitHub".to_string(), // Invalid: contains uppercase
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS); // Invalid params error code
    }

    #[tokio::test]
    async fn test_introspect_server_invalid_server_id_underscore() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "git_hub".to_string(), // Invalid: contains underscore
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_introspect_server_invalid_server_id_special_chars() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "git@hub".to_string(), // Invalid: contains @
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_introspect_server_valid_server_id_with_hyphens() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "git-hub-server".to_string(), // Valid
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        // This will fail because echo is not an MCP server, but validation should pass
        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;

        // Should fail with internal error (connection), not invalid params
        if let Err(err) = result {
            assert_ne!(
                err.code,
                ErrorCode::INVALID_PARAMS,
                "Should not be invalid params error"
            );
        }
    }

    #[tokio::test]
    async fn test_introspect_server_valid_server_id_digits() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "server123".to_string(), // Valid: lowercase + digits
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;

        // Should fail with internal error (connection), not invalid params
        if let Err(err) = result {
            assert_ne!(err.code, ErrorCode::INVALID_PARAMS);
        }
    }

    /// A zero timeout is a client input error, not a server-side connection
    /// failure — it must surface as `INVALID_PARAMS`, matching the sibling
    /// `validate_server_id` behavior, not `internal_error`.
    #[tokio::test]
    async fn test_introspect_server_zero_connect_timeout_is_invalid_params() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "zero-timeout-test".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: Some(0),
            discover_timeout_secs: None,
        };

        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;

        let err = result.expect_err("zero connect_timeout must be rejected");
        assert_eq!(
            err.code,
            ErrorCode::INVALID_PARAMS,
            "zero timeout is a client input error, not an internal error"
        );
    }

    /// Critic follow-up (M1): `Error::SecurityViolation` (shell metacharacters, forbidden env
    /// vars, ...) is a caller-supplied-param problem, same as `Error::ValidationError` — it
    /// must also surface as `INVALID_PARAMS`, not `internal_error`, which would otherwise
    /// blame the server for a hostile client argument.
    #[tokio::test]
    async fn test_introspect_server_shell_metacharacter_is_invalid_params() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "metachar-test".to_string(),
            command: "echo".to_string(),
            args: vec!["run; rm -rf /".to_string()],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;

        let err = result.expect_err("shell metacharacter in args must be rejected");
        assert_eq!(
            err.code,
            ErrorCode::INVALID_PARAMS,
            "a security violation in caller-supplied params is a client input error, not an \
             internal error"
        );
    }

    // ========================================================================
    // Cancellation Tests (issue #191)
    // ========================================================================

    /// A pre-cancelled token must short-circuit `discover_server` rather than
    /// always running it to completion. The token is cancelled before the
    /// call, and `discover_server` (spawning a real subprocess) can never
    /// resolve on its first poll, so `tokio::select!` deterministically picks
    /// the cancellation branch.
    #[tokio::test]
    async fn test_introspect_server_honors_pre_cancelled_token() {
        let service = GeneratorService::new();
        let ct = CancellationToken::new();
        ct.cancel();

        let params = IntrospectServerParams {
            server_id: "cancel-test".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = service.introspect_server(Parameters(params), ct).await;

        let err = result.expect_err("a cancelled request must return an error");
        assert!(err.message.contains("cancelled"));
        assert!(
            service.introspectors.lock().await.is_empty(),
            "the introspector handle must still be evicted on the cancellation path"
        );
    }

    // ========================================================================
    // Per-server-id locking Tests (issue #120)
    //
    // These test the exact `Arc<Mutex<Introspector>>` handles and keyed-lock
    // pattern that `introspect_server` relies on via `introspector_for`,
    // rather than driving a real (or fake) subprocess through
    // `discover_server`. This keeps the tests deterministic and
    // platform-independent while still exercising the production locking
    // primitive: `introspect_server` does nothing more than fetch a handle
    // via `introspector_for` and `.lock().await` it around the
    // `discover_server` call, so proving the handles behave correctly here
    // proves the concurrency property end to end.
    // ========================================================================

    /// `introspect_server` must evict its per-server-id entry from the
    /// `introspectors` map once `discover_server` completes, regardless of
    /// outcome - otherwise caller-supplied `server_id`s would grow the map
    /// without bound.
    #[tokio::test]
    async fn test_introspect_server_evicts_map_entry_after_completion() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "evict-after-completion".to_string(),
            command: "echo".to_string(), // not an MCP server, discover_server fails fast
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = service
            .introspect_server(Parameters(params), CancellationToken::new())
            .await;
        assert!(
            result.is_err(),
            "echo is not an MCP server, expected a connection failure"
        );

        assert!(
            service.introspectors.lock().await.is_empty(),
            "introspectors map should be empty after introspect_server completes, \
             regardless of success or failure"
        );
    }

    /// Same `server_id` must resolve to the same introspector lock, so a
    /// second `introspect_server` call for that id cannot start
    /// `discover_server` until the first releases it.
    #[tokio::test]
    async fn test_introspector_for_same_id_shares_one_lock() {
        let service = GeneratorService::new();
        let server_id = ServerId::new("same-id-lock-test");

        let handle_a = service.introspector_for(&server_id).await;
        let handle_b = service.introspector_for(&server_id).await;

        assert!(
            Arc::ptr_eq(&handle_a, &handle_b),
            "the same server_id must reuse one introspector lock"
        );
    }

    /// Different `server_id`s must resolve to independent introspector
    /// locks, so calls for unrelated ids never contend on the same mutex.
    #[tokio::test]
    async fn test_introspector_for_different_ids_get_independent_locks() {
        let service = GeneratorService::new();

        let handle_a = service
            .introspector_for(&ServerId::new("diff-id-lock-a"))
            .await;
        let handle_b = service
            .introspector_for(&ServerId::new("diff-id-lock-b"))
            .await;

        assert!(
            !Arc::ptr_eq(&handle_a, &handle_b),
            "different server_ids must get independent introspector locks"
        );
    }

    /// Two holders of the *same* per-id lock (as returned by
    /// `introspector_for` for one `server_id`) must serialize: the second
    /// critical section cannot start until the first releases the lock, so
    /// total wall time is roughly additive (~2x the hold time).
    #[tokio::test]
    async fn test_same_id_lock_serializes_concurrent_holders() {
        let service = GeneratorService::new();
        let server_id = ServerId::new("same-id-timing-test");
        let hold_time = std::time::Duration::from_millis(150);
        let serialized_threshold = std::time::Duration::from_millis(250);

        let handle_a = service.introspector_for(&server_id).await;
        let handle_b = service.introspector_for(&server_id).await;

        let started = std::time::Instant::now();
        tokio::join!(
            async {
                let _guard = handle_a.lock().await;
                tokio::time::sleep(hold_time).await;
            },
            async {
                let _guard = handle_b.lock().await;
                tokio::time::sleep(hold_time).await;
            },
        );
        let elapsed = started.elapsed();

        assert!(
            elapsed >= serialized_threshold,
            "holders of the same per-id lock should serialize \
             (expected >= {serialized_threshold:?}, i.e. two back-to-back {hold_time:?} \
             critical sections); took {elapsed:?}"
        );
    }

    /// Two holders of *different* per-id locks (as returned by
    /// `introspector_for` for different `server_id`s) must not serialize:
    /// both critical sections run concurrently, so total wall time stays
    /// close to a single hold, not double it.
    #[tokio::test]
    async fn test_different_id_locks_do_not_serialize() {
        let service = GeneratorService::new();
        let hold_time = std::time::Duration::from_millis(150);
        let serialized_threshold = std::time::Duration::from_millis(250);

        let handle_a = service
            .introspector_for(&ServerId::new("diff-id-timing-a"))
            .await;
        let handle_b = service
            .introspector_for(&ServerId::new("diff-id-timing-b"))
            .await;

        let started = std::time::Instant::now();
        tokio::join!(
            async {
                let _guard = handle_a.lock().await;
                tokio::time::sleep(hold_time).await;
            },
            async {
                let _guard = handle_b.lock().await;
                tokio::time::sleep(hold_time).await;
            },
        );
        let elapsed = started.elapsed();

        assert!(
            elapsed < serialized_threshold,
            "holders of different per-id locks should not serialize \
             (expected < {serialized_threshold:?}, i.e. close to a single {hold_time:?} hold); \
             took {elapsed:?}"
        );
    }

    /// Regression test for the TOCTOU eviction bug (issue #130): eviction
    /// must be identity-checked, not just keyed by `server_id`.
    ///
    /// Simulates three overlapping callers for the same `server_id`:
    /// - A and B both call `introspector_for` while an entry already exists,
    ///   so (per `test_introspector_for_same_id_shares_one_lock`) they share
    ///   the exact same `Arc<Mutex<Introspector>>`.
    /// - A finishes first and evicts, removing the shared entry, while B is
    ///   still "in flight" (still holding its clone of that same `Arc`).
    /// - C then arrives, finds the map empty, and gets a brand-new `Arc` -
    ///   distinct from A/B's.
    /// - B finally finishes and attempts to evict using its (now stale)
    ///   handle. Because eviction is identity-checked via `Arc::ptr_eq`, this
    ///   must be a no-op: C's live entry must survive. Only C's own eviction
    ///   should remove it.
    #[tokio::test]
    async fn test_stale_eviction_does_not_remove_unrelated_entry() {
        let service = GeneratorService::new();
        let server_id = ServerId::new("toctou-abc-test");

        // A and B both fetch the handle for the same id before either
        // evicts, so they end up sharing one Arc (mirrors the "shares one
        // lock" behavior already covered by
        // `test_introspector_for_same_id_shares_one_lock`).
        let handle_a = service.introspector_for(&server_id).await;
        let handle_b = service.introspector_for(&server_id).await;
        assert!(
            Arc::ptr_eq(&handle_a, &handle_b),
            "A and B must share one introspector handle for the same server_id"
        );

        // A finishes first and evicts. B is still "in flight", holding its
        // clone of the now-removed shared Arc.
        service.evict_introspector(&server_id, &handle_a).await;
        assert!(
            service.introspectors.lock().await.is_empty(),
            "map should be empty right after A's eviction"
        );

        // C arrives after A's eviction, finds the map empty, and gets a
        // fresh, distinct handle.
        let handle_c = service.introspector_for(&server_id).await;
        assert!(
            !Arc::ptr_eq(&handle_b, &handle_c),
            "C must get a handle distinct from A/B's stale one"
        );

        // B finally finishes and tries to evict using its stale (A/B
        // shared) handle. This must be a no-op: C's live entry, keyed by
        // the same server_id, must survive because it is a different Arc.
        service.evict_introspector(&server_id, &handle_b).await;
        let introspectors = service.introspectors.lock().await;
        let current = introspectors
            .get(&server_id)
            .expect("C's entry must survive B's stale eviction attempt");
        assert!(
            Arc::ptr_eq(current, &handle_c),
            "the surviving entry must be C's handle, unaffected by B's stale eviction"
        );
        drop(introspectors);

        // Only C's own eviction removes its entry.
        service.evict_introspector(&server_id, &handle_c).await;
        assert!(
            service.introspectors.lock().await.is_empty(),
            "map should be empty after C's own eviction"
        );
    }

    // ========================================================================
    // Per-output-directory export locking Tests (issue #169)
    //
    // Mirrors the `introspector_for` tests above: these exercise the exact
    // `Arc<Mutex<()>>` handles and keyed-lock pattern that
    // `save_categorized_tools` relies on via `export_lock_for`, without
    // driving a real export through the filesystem.
    // ========================================================================

    /// Same `output_dir` must resolve to the same export lock, so a second
    /// concurrent export for that directory cannot proceed until the first
    /// releases it.
    #[tokio::test]
    async fn test_export_lock_for_same_output_dir_shares_one_lock() {
        let service = GeneratorService::new();
        let output_dir = PathBuf::from("/tmp/same-output-dir-lock-test");

        let handle_a = service.export_lock_for(&output_dir).await;
        let handle_b = service.export_lock_for(&output_dir).await;

        assert!(
            Arc::ptr_eq(&handle_a, &handle_b),
            "the same output_dir must reuse one export lock"
        );
    }

    /// Different `output_dir`s must resolve to independent export locks, so
    /// exports for unrelated directories never contend on the same mutex.
    #[tokio::test]
    async fn test_export_lock_for_different_output_dirs_get_independent_locks() {
        let service = GeneratorService::new();

        let handle_a = service
            .export_lock_for(&PathBuf::from("/tmp/diff-output-dir-lock-a"))
            .await;
        let handle_b = service
            .export_lock_for(&PathBuf::from("/tmp/diff-output-dir-lock-b"))
            .await;

        assert!(
            !Arc::ptr_eq(&handle_a, &handle_b),
            "different output_dirs must get independent export locks"
        );
    }

    /// `evict_export_lock` must be identity-checked, not just keyed by
    /// `output_dir`, mirroring `test_stale_eviction_does_not_remove_unrelated_entry`.
    #[tokio::test]
    async fn test_export_lock_stale_eviction_does_not_remove_unrelated_entry() {
        let service = GeneratorService::new();
        let output_dir = PathBuf::from("/tmp/toctou-export-lock-test");

        let handle_a = service.export_lock_for(&output_dir).await;
        let handle_b = service.export_lock_for(&output_dir).await;
        assert!(Arc::ptr_eq(&handle_a, &handle_b));

        service.evict_export_lock(&output_dir, &handle_a).await;
        assert!(service.exports.lock().await.is_empty());

        let handle_c = service.export_lock_for(&output_dir).await;
        assert!(!Arc::ptr_eq(&handle_b, &handle_c));

        // B's stale eviction attempt must be a no-op: C's live entry survives.
        service.evict_export_lock(&output_dir, &handle_b).await;
        let exports = service.exports.lock().await;
        let current = exports
            .get(&output_dir)
            .expect("C's entry must survive B's stale eviction attempt");
        assert!(Arc::ptr_eq(current, &handle_c));
        drop(exports);
    }

    // ========================================================================
    // save_categorized_tools Error Tests
    // ========================================================================

    #[tokio::test]
    async fn test_save_categorized_tools_invalid_session() {
        let service = GeneratorService::new();

        let params = SaveCategorizedToolsParams {
            session_id: Uuid::new_v4(), // Random UUID not in state
            categorized_tools: vec![],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS); // Invalid params
        assert!(err.message.contains("Session not found"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_tool_mismatch() {
        let service = GeneratorService::new();

        // Create a pending generation with tool1
        let server_id = ServerId::new("test");
        let server_info = mcp_execution_introspector::ServerInfo {
            id: server_id.clone(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new("tool1"),
                description: "Tool 1".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: None,
            }],
        };

        let pending = PendingGeneration::new(
            server_id,
            server_info,
            ServerConfig::builder()
                .command("echo".to_string())
                .build()
                .unwrap(),
            PathBuf::from("/tmp/test"),
            &SystemClock,
        );

        let session_id = service.state.store(pending).await;

        // Try to save with tool2 (doesn't exist)
        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![CategorizedTool {
                name: "tool2".to_string(), // Mismatch!
                category: "test".to_string(),
                keywords: "test".to_string(),
                short_description: "Test".to_string(),
            }],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("not found in introspected tools"));
    }

    // ========================================================================
    // save_categorized_tools Bounds Tests (issue #197)
    // ========================================================================

    /// Builds a pending generation whose `server_info.tools` contains `count`
    /// distinct tools named `tool0`..`tool{count-1}`, with a fixed
    /// `output_dir` that is never actually written to. Only safe for tests
    /// that expect `save_categorized_tools` to return before reaching the
    /// export step (e.g. bounds/validation rejections); a test that expects
    /// `Ok(..)` must use [`pending_with_tool_count_and_output_dir`] with its
    /// own `TempDir` instead, so concurrent test runs don't race a real
    /// export against this same shared path (issue #169, inside the test
    /// suite itself).
    fn pending_with_tool_count(count: usize) -> PendingGeneration {
        pending_with_tool_count_and_output_dir(count, PathBuf::from("/tmp/test"))
    }

    /// Same as [`pending_with_tool_count`], but with a caller-supplied
    /// `output_dir` - use a fresh `tempfile::TempDir` per test that actually
    /// exercises the export step.
    fn pending_with_tool_count_and_output_dir(
        count: usize,
        output_dir: PathBuf,
    ) -> PendingGeneration {
        let tools = (0..count)
            .map(|i| ToolInfo {
                name: ToolName::new(format!("tool{i}")),
                description: "Test tool".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: None,
            })
            .collect();

        let server_info = mcp_execution_introspector::ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools,
        };

        PendingGeneration::new(
            ServerId::new("test"),
            server_info,
            ServerConfig::builder()
                .command("echo".to_string())
                .build()
                .unwrap(),
            output_dir,
            &SystemClock,
        )
    }

    fn categorized_tool(name: &str) -> CategorizedTool {
        CategorizedTool {
            name: name.to_string(),
            category: "cat".to_string(),
            keywords: "kw".to_string(),
            short_description: "desc".to_string(),
        }
    }

    /// More entries than introspected tools can only happen via repeats of
    /// valid names (since each name must be introspected), so this also
    /// proves the length cap closes the CWE-400 array-bloat path even before
    /// the per-entry duplicate check runs.
    #[tokio::test]
    async fn test_save_categorized_tools_rejects_more_entries_than_introspected() {
        let service = GeneratorService::new();
        let session_id = service.state.store(pending_with_tool_count(2)).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![
                categorized_tool("tool0"),
                categorized_tool("tool1"),
                categorized_tool("tool0"),
            ],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        let err = result.expect_err("more entries than introspected tools must be rejected");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("at most 2 are allowed"));
    }

    /// The entry-count cap must also hold when a (possibly hostile) target
    /// server reports more introspected tools than `MAX_TOOL_FILES`: the
    /// effective ceiling is `min(introspected count, MAX_TOOL_FILES)`, not
    /// the introspected count alone, so this can never generate more tool
    /// files than `generate_skill` will later accept.
    #[tokio::test]
    async fn test_save_categorized_tools_caps_at_max_tool_files_regardless_of_introspected_count() {
        let service = GeneratorService::new();
        let session_id = service
            .state
            .store(pending_with_tool_count(MAX_TOOL_FILES + 10))
            .await;

        let categorized_tools = (0..=MAX_TOOL_FILES)
            .map(|i| categorized_tool(&format!("tool{i}")))
            .collect();
        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools,
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        let err = result.expect_err("entry count above MAX_TOOL_FILES must be rejected");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(
            err.message
                .contains(&format!("at most {MAX_TOOL_FILES} are allowed"))
        );
    }

    #[tokio::test]
    async fn test_save_categorized_tools_rejects_duplicate_name() {
        let service = GeneratorService::new();
        let session_id = service.state.store(pending_with_tool_count(2)).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![categorized_tool("tool0"), categorized_tool("tool0")],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        let err = result.expect_err("a repeated tool name must be rejected");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("appears more than once"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_rejects_oversized_name() {
        let service = GeneratorService::new();
        let long_name = "n".repeat(MAX_CATEGORIZED_TOOL_NAME_LEN + 1);

        let server_info = mcp_execution_introspector::ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new(long_name.clone()),
                description: "Test tool".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: None,
            }],
        };
        let pending = PendingGeneration::new(
            ServerId::new("test"),
            server_info,
            ServerConfig::builder()
                .command("echo".to_string())
                .build()
                .unwrap(),
            PathBuf::from("/tmp/test"),
            &SystemClock,
        );
        let session_id = service.state.store(pending).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![categorized_tool(&long_name)],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        let err = result.expect_err("an oversized tool name must be rejected");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("byte limit"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_rejects_oversized_category() {
        let service = GeneratorService::new();
        let session_id = service.state.store(pending_with_tool_count(1)).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![CategorizedTool {
                category: "x".repeat(MAX_CATEGORY_LEN + 1),
                ..categorized_tool("tool0")
            }],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        let err = result.expect_err("an oversized category must be rejected");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("category for tool 'tool0'"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_rejects_oversized_keywords() {
        let service = GeneratorService::new();
        let session_id = service.state.store(pending_with_tool_count(1)).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![CategorizedTool {
                keywords: "x".repeat(MAX_KEYWORDS_LEN + 1),
                ..categorized_tool("tool0")
            }],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        let err = result.expect_err("oversized keywords must be rejected");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("keywords for tool 'tool0'"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_rejects_oversized_short_description() {
        let service = GeneratorService::new();
        let session_id = service.state.store(pending_with_tool_count(1)).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![CategorizedTool {
                short_description: "x".repeat(MAX_SHORT_DESCRIPTION_LEN + 1),
                ..categorized_tool("tool0")
            }],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        let err = result.expect_err("an oversized short_description must be rejected");
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("short_description for tool 'tool0'"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_accepts_exact_introspected_count() {
        use tempfile::TempDir;

        let service = GeneratorService::new();
        let temp_dir = TempDir::new().unwrap();
        let pending = pending_with_tool_count_and_output_dir(2, temp_dir.path().join("out"));
        let session_id = service.state.store(pending).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![categorized_tool("tool0"), categorized_tool("tool1")],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(
            result.is_ok(),
            "submitting exactly one entry per introspected tool must be accepted: {:?}",
            result.err()
        );
    }

    /// Pins the boundary semantics (`>`, not `>=`) for all four per-entry
    /// byte caps at once: a `name`/`category`/`keywords`/`short_description`
    /// each exactly at its limit must be accepted, not rejected.
    #[tokio::test]
    async fn test_save_categorized_tools_accepts_fields_at_exact_byte_caps() {
        use tempfile::TempDir;

        let service = GeneratorService::new();
        let temp_dir = TempDir::new().unwrap();
        let name_at_cap = "n".repeat(MAX_CATEGORIZED_TOOL_NAME_LEN);

        let server_info = mcp_execution_introspector::ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new(name_at_cap.clone()),
                description: "Test tool".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: None,
            }],
        };
        let pending = PendingGeneration::new(
            ServerId::new("test"),
            server_info,
            ServerConfig::builder()
                .command("echo".to_string())
                .build()
                .unwrap(),
            temp_dir.path().join("out"),
            &SystemClock,
        );
        let session_id = service.state.store(pending).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![CategorizedTool {
                name: name_at_cap,
                category: "c".repeat(MAX_CATEGORY_LEN),
                keywords: "k".repeat(MAX_KEYWORDS_LEN),
                short_description: "d".repeat(MAX_SHORT_DESCRIPTION_LEN),
            }],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(
            result.is_ok(),
            "fields exactly at their byte caps must be accepted, not rejected: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_save_categorized_tools_expired_session() {
        use crate::clock::TestClock;
        use chrono::Duration;

        let service = GeneratorService::new();

        // Create an expired pending generation
        let server_id = ServerId::new("test");
        let server_info = mcp_execution_introspector::ServerInfo {
            id: server_id.clone(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![],
        };

        // Inject a clock fixed an hour in the past so `expires_at` is already
        // behind us, instead of rewinding `expires_at` after construction.
        let past_clock = TestClock::new(Utc::now() - Duration::hours(1));
        let pending = PendingGeneration::new(
            server_id,
            server_info,
            ServerConfig::builder()
                .command("echo".to_string())
                .build()
                .unwrap(),
            PathBuf::from("/tmp/test"),
            &past_clock,
        );

        let session_id = service.state.store(pending).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    /// Proves `GeneratorService::with_clock` actually drives session expiry end
    /// to end through `save_categorized_tools`: a session stored while the
    /// shared clock is fresh must become unreachable once that same clock (not
    /// the real wall clock) is advanced past the TTL. This exercises the
    /// `Arc<dyn Clock>` shared between `GeneratorService` and its
    /// `StateManager` (`with_clock` clones the same `Arc` into both).
    #[tokio::test]
    async fn test_shared_clock_drives_save_categorized_tools_expiry() {
        use crate::clock::TestClock;
        use chrono::Duration;

        let start = Utc::now();
        let clock = Arc::new(TestClock::new(start));
        let service = GeneratorService::with_clock(Arc::clone(&clock) as Arc<dyn Clock>);

        let server_id = ServerId::new("test");
        let server_info = mcp_execution_introspector::ServerInfo {
            id: server_id.clone(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![],
        };

        let pending = PendingGeneration::new(
            server_id,
            server_info,
            ServerConfig::builder()
                .command("echo".to_string())
                .build()
                .unwrap(),
            PathBuf::from("/tmp/test"),
            clock.as_ref(),
        );

        let session_id = service.state.store(pending).await;

        // Advance the service's own shared clock, not the real wall clock, past the TTL.
        clock.advance(
            Duration::minutes(PendingGeneration::DEFAULT_TIMEOUT_MINUTES) + Duration::seconds(1),
        );

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    // ========================================================================
    // list_generated_servers Tests
    // ========================================================================

    #[tokio::test]
    async fn test_list_generated_servers_nonexistent_dir() {
        let service = GeneratorService::new();

        let params = ListGeneratedServersParams {
            base_dir: Some("/nonexistent/path/that/does/not/exist".to_string()),
        };

        let result = service.list_generated_servers(Parameters(params)).await;

        assert!(result.is_ok());
        let content = result.unwrap();
        let text_content = content.content[0].as_text().unwrap();
        let parsed: ListGeneratedServersResult = serde_json::from_str(&text_content.text).unwrap();

        assert_eq!(parsed.total_servers, 0);
        assert_eq!(parsed.servers.len(), 0);
    }

    #[tokio::test]
    async fn test_list_generated_servers_default_dir() {
        let service = GeneratorService::new();

        let params = ListGeneratedServersParams { base_dir: None };

        let result = service.list_generated_servers(Parameters(params)).await;

        // Should succeed even if directory doesn't exist
        assert!(result.is_ok());
    }

    // ========================================================================
    // generate_skill Error Tests
    // ========================================================================

    #[tokio::test]
    async fn test_generate_skill_invalid_server_id_uppercase() {
        let service = GeneratorService::new();

        let params = GenerateSkillParams {
            server_id: "GitHub".to_string(), // Invalid: uppercase
            skill_name: None,
            use_case_hints: None,
            servers_dir: None,
        };

        let result = service
            .generate_skill(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("lowercase"));
    }

    #[tokio::test]
    async fn test_generate_skill_invalid_server_id_special_chars() {
        let service = GeneratorService::new();

        let params = GenerateSkillParams {
            server_id: "git@hub".to_string(), // Invalid: special chars
            skill_name: None,
            use_case_hints: None,
            servers_dir: None,
        };

        let result = service
            .generate_skill(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_generate_skill_server_directory_not_found() {
        let service = GeneratorService::new();

        let params = GenerateSkillParams {
            server_id: "nonexistent-server".to_string(),
            skill_name: None,
            use_case_hints: None,
            servers_dir: Some(PathBuf::from("/nonexistent/path")),
        };

        let result = service
            .generate_skill(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("not found"));
    }

    /// A pre-cancelled token must short-circuit `scan_tools_directory` rather
    /// than always running it to completion. The server directory exists (so
    /// the synchronous `!server_dir.exists()` check passes and the call
    /// reaches the scan), and the scan's first poll can never resolve
    /// immediately, so `tokio::select!` deterministically picks the
    /// cancellation branch.
    #[tokio::test]
    async fn test_generate_skill_honors_pre_cancelled_token() {
        use tempfile::TempDir;

        let service = GeneratorService::new();
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();
        let target_dir = base_dir.join("test-server");
        tokio::fs::create_dir_all(&target_dir).await.unwrap();

        let ct = CancellationToken::new();
        ct.cancel();

        let params = GenerateSkillParams {
            server_id: "test-server".to_string(),
            skill_name: None,
            use_case_hints: None,
            servers_dir: Some(base_dir),
        };

        let result = service.generate_skill(Parameters(params), ct).await;

        let err = result.expect_err("a cancelled request must return an error");
        assert!(err.message.contains("cancelled"));
    }

    #[tokio::test]
    async fn test_generate_skill_missing_metadata_sidecar() {
        use tempfile::TempDir;

        let service = GeneratorService::new();
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        // Create server directory but no `_meta.json` sidecar (e.g. a directory
        // generated by a pre-#141 version, or never generated at all).
        let target_dir = base_dir.join("test-server");
        tokio::fs::create_dir_all(&target_dir).await.unwrap();

        let params = GenerateSkillParams {
            server_id: "test-server".to_string(),
            skill_name: None,
            use_case_hints: None,
            servers_dir: Some(base_dir),
        };

        let result = service
            .generate_skill(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::INVALID_PARAMS,
            "a missing sidecar is the same 'not generated' caller situation as a missing \
             server directory, and must be reported the same way"
        );
        assert!(err.message.contains("Failed to scan tools directory"));
    }

    #[tokio::test]
    async fn test_generate_skill_stale_metadata_missing_ts_file() {
        use mcp_execution_core::metadata::{
            METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ParameterMetadata, ServerMetadata,
            ToolMetadata as SidecarToolMetadata,
        };
        use tempfile::TempDir;

        let service = GeneratorService::new();
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        // Sidecar references a tool whose `.ts` file was never written (or was
        // deleted) — the drift `StaleMetadata` (issues #154/#155) exists to
        // catch, routed through the `generate_skill` MCP tool this time.
        let target_dir = base_dir.join("test-server");
        tokio::fs::create_dir_all(&target_dir).await.unwrap();
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: "test-server".to_string(),
            server_name: "Test Server".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![SidecarToolMetadata {
                name: "create_issue".to_string(),
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
            }],
        };
        let content = serde_json::to_string_pretty(&meta).unwrap();
        tokio::fs::write(target_dir.join(METADATA_FILE_NAME), content)
            .await
            .unwrap();
        // Deliberately do not write `createIssue.ts`.

        let params = GenerateSkillParams {
            server_id: "test-server".to_string(),
            skill_name: None,
            use_case_hints: None,
            servers_dir: Some(base_dir),
        };

        let result = service
            .generate_skill(Parameters(params), CancellationToken::new())
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::INVALID_PARAMS,
            "stale metadata is the same 'not generated / drifted directory' caller situation \
             as a missing sidecar, and must be reported the same way"
        );
        assert!(err.message.contains("Failed to scan tools directory"));
        assert!(err.message.contains("create_issue"));
    }

    #[tokio::test]
    async fn test_generate_skill_reports_orphan_ts_file_as_warning() {
        // Issue #161: a `.ts` file on disk with no matching `_meta.json` entry
        // is non-fatal, but must be surfaced in the structured JSON-RPC
        // response's `warnings` field, not just in server-side tracing output.
        use mcp_execution_core::metadata::{
            METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ParameterMetadata, ServerMetadata,
            ToolMetadata as SidecarToolMetadata,
        };
        use mcp_execution_skill::GenerateSkillResult;
        use tempfile::TempDir;

        let service = GeneratorService::new();
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let target_dir = base_dir.join("test-server");
        tokio::fs::create_dir_all(&target_dir).await.unwrap();
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: "test-server".to_string(),
            server_name: "Test Server".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![SidecarToolMetadata {
                name: "create_issue".to_string(),
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
            }],
        };
        let content = serde_json::to_string_pretty(&meta).unwrap();
        tokio::fs::write(target_dir.join(METADATA_FILE_NAME), content)
            .await
            .unwrap();
        tokio::fs::write(target_dir.join("createIssue.ts"), "export {}")
            .await
            .unwrap();
        // Left over on disk with no sidecar entry — must not be fatal.
        tokio::fs::write(target_dir.join("orphanTool.ts"), "export {}")
            .await
            .unwrap();

        let params = GenerateSkillParams {
            server_id: "test-server".to_string(),
            skill_name: None,
            use_case_hints: None,
            servers_dir: Some(base_dir),
        };

        let result = service
            .generate_skill(Parameters(params), CancellationToken::new())
            .await;

        assert!(
            result.is_ok(),
            "an orphaned .ts file must not fail the call"
        );
        let content = result.unwrap();
        let text_content = content.content[0].as_text().unwrap();
        let parsed: GenerateSkillResult = serde_json::from_str(&text_content.text).unwrap();

        assert_eq!(
            parsed.warnings.len(),
            1,
            "the orphaned .ts file must be surfaced as a warning"
        );
        assert!(
            parsed.warnings[0].contains("orphanTool.ts"),
            "warning must name the excluded file: {:?}",
            parsed.warnings[0]
        );
    }

    // ========================================================================
    // save_skill Error Tests
    // ========================================================================

    #[tokio::test]
    async fn test_save_skill_invalid_server_id() {
        let service = GeneratorService::new();

        let params = SaveSkillParams {
            server_id: "Invalid_Server".to_string(), // Invalid: uppercase and underscore
            content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
            output_path: None,
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("lowercase"));
    }

    #[tokio::test]
    async fn test_save_skill_missing_yaml_frontmatter() {
        let service = GeneratorService::new();

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "# Test Skill\n\nNo YAML frontmatter here.".to_string(),
            output_path: None,
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("YAML frontmatter"));
    }

    #[tokio::test]
    async fn test_save_skill_invalid_frontmatter_no_name() {
        let service = GeneratorService::new();

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\ndescription: test\n---\n# Test".to_string(),
            output_path: None,
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("Invalid SKILL.md format"));
    }

    #[tokio::test]
    async fn test_save_skill_invalid_frontmatter_no_description() {
        let service = GeneratorService::new();

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test-skill\n---\n# Test".to_string(),
            output_path: None,
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("Invalid SKILL.md format"));
    }

    #[tokio::test]
    async fn test_save_skill_file_exists_no_overwrite() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());
        let server_dir = temp_dir.path().join("test");
        let output_path = server_dir.join("SKILL.md");

        // Create existing file
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        tokio::fs::write(&output_path, "existing content")
            .await
            .unwrap();

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
            output_path: Some(PathBuf::from("SKILL.md")),
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("already exists"));
        assert!(err.message.contains("overwrite=true"));
    }

    #[tokio::test]
    async fn test_save_skill_file_exists_with_overwrite() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());
        let server_dir = temp_dir.path().join("test");
        let output_path = server_dir.join("SKILL.md");

        // Create existing file
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        tokio::fs::write(&output_path, "existing content")
            .await
            .unwrap();

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test\ndescription: test skill\n---\n# Test".to_string(),
            output_path: Some(PathBuf::from("SKILL.md")),
            overwrite: true,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_ok());
        let content = result.unwrap();
        let text = content.content[0].as_text().unwrap();
        let parsed: SaveSkillResult = serde_json::from_str(&text.text).unwrap();

        assert!(parsed.success);
        assert!(parsed.overwritten);
        assert_eq!(parsed.metadata.name, "test");
        assert_eq!(parsed.metadata.description, "test skill");
    }

    #[tokio::test]
    async fn test_save_skill_valid_content() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());
        let output_path = temp_dir.path().join("test").join("nested").join("SKILL.md");

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test-skill\ndescription: A test skill\n---\n\n# Test Skill\n\n## Section 1\n\nContent here.".to_string(),
            output_path: Some(PathBuf::from("nested/SKILL.md")),
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_ok());
        let content = result.unwrap();
        let text = content.content[0].as_text().unwrap();
        let parsed: SaveSkillResult = serde_json::from_str(&text.text).unwrap();

        assert!(parsed.success);
        assert!(!parsed.overwritten);
        assert_eq!(parsed.metadata.name, "test-skill");
        assert_eq!(parsed.metadata.description, "A test skill");
        assert!(parsed.metadata.section_count >= 1);
        assert!(parsed.metadata.word_count > 0);

        // Verify file was written under the confined base directory
        assert!(output_path.exists());
    }

    #[tokio::test]
    async fn test_save_skill_quoted_description_with_colon_round_trips() {
        // `GENERATION_INSTRUCTIONS` (mcp-execution-skill) tells the model to always
        // double-quote `description`, since an unquoted value containing `:` is
        // invalid YAML (`serde_norway` errors instead of the old regex, which
        // captured the whole line regardless). Pin that a quoted description
        // containing a colon round-trips through `save_skill` unchanged.
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test-skill\ndescription: \"GitHub: issues and CI\"\n---\n\n# Test Skill\n\n## Section 1\n\nContent here.".to_string(),
            output_path: None,
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_ok());
        let content = result.unwrap();
        let text = content.content[0].as_text().unwrap();
        let parsed: SaveSkillResult = serde_json::from_str(&text.text).unwrap();

        assert_eq!(parsed.metadata.description, "GitHub: issues and CI");
    }

    #[tokio::test]
    async fn test_save_skill_default_path_still_works() {
        use tempfile::TempDir;

        // No output_path override: exercises the default `{server_id}/SKILL.md`
        // branch and confirms it still clears the new confinement check
        // (defense in depth), without touching the real home directory.
        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
            output_path: None,
            overwrite: false,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_ok());
        let content = result.unwrap();
        let text = content.content[0].as_text().unwrap();
        let parsed: SaveSkillResult = serde_json::from_str(&text.text).unwrap();
        assert!(parsed.success);

        let expected_path = temp_dir.path().join("test").join("SKILL.md");
        assert!(expected_path.exists());
    }

    #[tokio::test]
    async fn test_save_skill_rejects_absolute_output_path() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());

        // A bare `/etc/passwd`-style path has no drive prefix, so
        // `Path::is_absolute()` is false for it on Windows and it would be
        // rejected later, via the confinement walk's `Escape` variant,
        // after the (safe, still-confined) `server_id` directory is
        // already created. Use a path that is genuinely absolute on the
        // current platform so this test exercises the early
        // `AbsolutePath` rejection, before any filesystem work.
        let absolute = if cfg!(windows) {
            r"C:\Windows\System32\config"
        } else {
            "/etc/passwd"
        };
        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
            output_path: Some(PathBuf::from(absolute)),
            overwrite: true,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("output_path"));
        // Rejected before any filesystem work happened.
        assert!(!temp_dir.path().join("test").exists());
    }

    #[tokio::test]
    async fn test_save_skill_rejects_parent_traversal() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
            output_path: Some(PathBuf::from("../../../etc/passwd")),
            overwrite: true,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("output_path"));
        // Rejected before any filesystem work happened.
        assert!(!temp_dir.path().join("test").exists());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_save_skill_rejects_symlinked_parent_directory_escape() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());

        // Plant a symlink inside the confined base (base/server_id) that
        // points outside it.
        let server_dir = temp_dir.path().join("test");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(outside_dir.path(), server_dir.join("escape")).unwrap();

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
            output_path: Some(PathBuf::from("escape/SKILL.md")),
            overwrite: true,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(!outside_dir.path().join("SKILL.md").exists());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_save_skill_rejects_dangling_symlink_at_output_path() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());
        let dangling_target = outside_dir.path().join("does-not-exist.md");

        let server_dir = temp_dir.path().join("test");
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        std::os::unix::fs::symlink(&dangling_target, server_dir.join("SKILL.md")).unwrap();

        let params = SaveSkillParams {
            server_id: "test".to_string(),
            content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
            output_path: Some(PathBuf::from("SKILL.md")),
            overwrite: true,
        };

        let result = service.save_skill(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(!dangling_target.exists());
    }

    #[tokio::test]
    async fn test_save_skill_confines_each_server_to_its_own_directory() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let service =
            GeneratorService::new().with_skills_base_dir_for_test(temp_dir.path().to_path_buf());

        for server_id in ["server-a", "server-b"] {
            let params = SaveSkillParams {
                server_id: server_id.to_string(),
                content: "---\nname: test\ndescription: test\n---\n# Test".to_string(),
                output_path: None,
                overwrite: false,
            };
            let result = service.save_skill(Parameters(params)).await;
            assert!(result.is_ok());
        }

        assert!(temp_dir.path().join("server-a").join("SKILL.md").exists());
        assert!(temp_dir.path().join("server-b").join("SKILL.md").exists());

        // Genuine negative case: server-b must not be able to reach into
        // server-a's directory via output_path, and server-a's file must
        // come out of the attempt untouched.
        let cross_server_params = SaveSkillParams {
            server_id: "server-b".to_string(),
            content: "---\nname: hijack\ndescription: hijack\n---\n# Hijack".to_string(),
            output_path: Some(PathBuf::from("../server-a/SKILL.md")),
            overwrite: true,
        };
        let cross_server_result = service.save_skill(Parameters(cross_server_params)).await;
        assert!(cross_server_result.is_err());
        assert_eq!(
            cross_server_result.unwrap_err().code,
            ErrorCode::INVALID_PARAMS
        );

        let server_a_content =
            tokio::fs::read_to_string(temp_dir.path().join("server-a").join("SKILL.md"))
                .await
                .unwrap();
        assert!(server_a_content.contains("name: test"));
        assert!(!server_a_content.contains("hijack"));
    }
}
