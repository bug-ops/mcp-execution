//! Progressive loading code generator.
//!
//! Generates TypeScript files for progressive loading where each tool
//! is in a separate file, enabling Claude Code to load only what it needs.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_execution_codegen::progressive::ProgressiveGenerator;
//! use mcp_execution_introspector::{Introspector, ServerInfo};
//! use mcp_execution_core::{ServerId, ServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut introspector = Introspector::new();
//! let server_id = ServerId::new("github").unwrap();
//! let config = ServerConfig::builder().command("/path/to/server".to_string()).build()?;
//! let info = introspector.discover_server(server_id, &config).await?;
//!
//! let generator = ProgressiveGenerator::new()?;
//! let code = generator.generate(&info)?;
//!
//! // Generated files:
//! // - index.ts (re-exports)
//! // - createIssue.ts
//! // - updateIssue.ts
//! // - ...
//! // - _runtime/mcp-bridge.ts
//! println!("Generated {} files", code.file_count());
//! # Ok(())
//! # }
//! ```

use crate::common::types::{GeneratedCode, GeneratedFile};
use crate::common::typescript::{
    MAX_SCHEMA_RECURSION_DEPTH, disambiguate_identifier, extract_properties,
    sanitize_ts_identifier, to_camel_case,
};
use crate::progressive::types::{
    BridgeContext, CategoryInfo, IndexContext, PropertyInfo, ToolCategorization, ToolContext,
    ToolSummary,
};
use crate::template_engine::TemplateEngine;
use mcp_execution_core::metadata::{
    METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ParameterMetadata, ServerMetadata, ToolMetadata,
};
use mcp_execution_core::{Error, Result};
use mcp_execution_introspector::{ServerInfo, ToolInfo};
use std::collections::{HashMap, HashSet};

/// Files emitted by every `generate`/`generate_with_categories` call regardless of tool
/// count: `index.ts`, the runtime bridge, `package.json`, `tsconfig.json`, and the `_meta.json`
/// sidecar.
const FIXED_FILE_COUNT: usize = 5;

/// Maximum number of files a single `generate`/`generate_with_categories` call will produce
/// (denial-of-service protection, CWE-400).
///
/// Each tool becomes its own `.ts` file, so this bounds the file-count amplification of a
/// single generation run. Derived directly from
/// `mcp_execution_introspector::MAX_TOOL_COUNT` (rather than an independently chosen number)
/// plus this module's `FIXED_FILE_COUNT`, so a `ServerInfo` that already cleared introspection's
/// own tool-count bound can never be deterministically rejected here for simply having "as many tools
/// as introspection already allows" (issue #198 M1). This check remains meaningful
/// defense-in-depth for callers that construct a `ServerInfo` directly rather than going
/// through introspection.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::progressive::generator::MAX_GENERATED_FILES;
///
/// assert!(MAX_GENERATED_FILES > 0);
/// ```
pub const MAX_GENERATED_FILES: usize =
    mcp_execution_introspector::MAX_TOOL_COUNT + FIXED_FILE_COUNT;

/// Maximum total bytes across every file in a single `generate`/`generate_with_categories`
/// call's output (denial-of-service protection, CWE-400).
///
/// Derived from `mcp_execution_introspector`'s own per-tool bounds — up to `MAX_TOOL_COUNT`
/// tools, each up to `MAX_TOOL_NAME_LEN` + `MAX_TOOL_DESCRIPTION_LEN` + `MAX_SCHEMA_SIZE_BYTES`
/// — rather than chosen independently, so a `ServerInfo` that already cleared introspection's
/// own bounds can never be deterministically rejected here for simply being "as large as
/// introspection already allows" (issue #198 M1). The 2x multiplier accounts for `_meta.json`
/// re-embedding every tool's raw name/description/schema alongside the already-rendered `.ts`
/// file content, roughly doubling the total.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::progressive::generator::MAX_GENERATED_BYTES;
///
/// assert!(MAX_GENERATED_BYTES > 0);
/// ```
pub const MAX_GENERATED_BYTES: usize = 2
    * mcp_execution_introspector::MAX_TOOL_COUNT
    * (mcp_execution_introspector::MAX_TOOL_NAME_LEN
        + mcp_execution_introspector::MAX_TOOL_DESCRIPTION_LEN
        + mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES);

/// Contents of the generated `package.json`.
///
/// Declares `@types/node` as a `devDependency` — pinned to major version 22, matching the
/// Node.js version this project's own CI targets — because `_runtime/mcp-bridge.ts` imports
/// Node builtins (`child_process`, `fs/promises`, `os`, `path`) and references ambient globals
/// (`process`, the `NodeJS` namespace) that only resolve once type declarations for Node are
/// present; without it, `tsc --noEmit` fails on the generated package even with a correct
/// `tsconfig.json` (see `TSCONFIG_JSON` below).
const PACKAGE_JSON: &str = "{\"type\":\"module\",\"devDependencies\":{\"@types/node\":\"^22\"}}\n";

/// Contents of the generated `tsconfig.json`.
///
/// ## Implementation rationale (for maintainers)
///
/// Tool files import the runtime bridge with an explicit `.ts` extension (see
/// `tool.ts.hbs`), which requires `allowImportingTsExtensions`. TypeScript requires `noEmit`
/// (or a declaration/emit setting) whenever `allowImportingTsExtensions` is set, since that
/// option only changes how imports are type-checked, not how output is emitted.
///
/// `"types": ["node"]` is explicit rather than relying on automatic `@types/*` acquisition:
/// TypeScript 5.x auto-includes every package under `node_modules/@types` when `types` is
/// unset, but TypeScript 7 (the native/Go-based compiler) does not extend that same implicit
/// behavior to `@types/node`'s ambient globals (`process`, the `NodeJS` namespace) — `tsc
/// --noEmit` fails with `TS2591`/`TS2503` under TS 7 even with `@types/node` correctly
/// installed, unless `types` names it explicitly. Listing it explicitly works identically
/// under both major versions.
///
/// ## Consumer guidance (intended behavior for users)
///
/// **This is a leaf configuration, not intended to be extended.** If a consumer's own
/// `tsconfig.json` uses `"extends"` to reference the generated one, `"noEmit": true` will be
/// inherited silently, preventing their own build from emitting output. **Do not extend this
/// file.**
///
/// **This file is regenerated on every `generate` call.** Any manual edits are lost when
/// `mcp-execution generate` runs again, the same as `package.json`. Treat the generated
/// `tsconfig.json` as read-only.
///
/// **How to use the generated package:** The generated TypeScript files are a standalone
/// package meant to be executed or type-checked as a separate process, not merged into your
/// own TypeScript compilation:
/// - Execute the generated code directly via a TS-aware runtime: `tsx` (for Node.js),
///   `deno`, or Node.js's native type-stripping (when available).
/// - Or type-check it independently: run `tsc -p <generated-dir>` as a separate build step,
///   without merging it into your own `include`-based program.
/// - If your own build uses a bundler (esbuild, swc, Vite, etc.) that doesn't enforce
///   TypeScript's `noEmit` constraint, you may be able to bundle the generated files
///   alongside your code (consult your bundler's documentation for mixing `noEmit`
///   and emitting configurations).
const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true,
    "allowImportingTsExtensions": true,
    "skipLibCheck": true,
    "types": ["node"]
  },
  "include": ["**/*.ts"]
}
"#;

/// Generator for progressive loading TypeScript files.
///
/// Creates one file per tool plus an index file and runtime bridge,
/// enabling progressive loading where only needed tools are loaded.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing safe use across threads.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::progressive::ProgressiveGenerator;
///
/// let generator = ProgressiveGenerator::new().unwrap();
/// ```
#[derive(Debug)]
pub struct ProgressiveGenerator<'a> {
    engine: TemplateEngine<'a>,
}

impl ProgressiveGenerator<'_> {
    /// Creates a new progressive generator.
    ///
    /// Initializes the template engine and registers all progressive
    /// loading templates.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails (should not happen
    /// with valid built-in templates).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::progressive::ProgressiveGenerator;
    ///
    /// let generator = ProgressiveGenerator::new().unwrap();
    /// ```
    pub fn new() -> Result<Self> {
        let engine = TemplateEngine::new()?;
        Ok(Self { engine })
    }

    /// Generates progressive loading files for a server.
    ///
    /// Creates one TypeScript file per tool, plus:
    /// - `index.ts`: Re-exports all tools
    /// - `_runtime/mcp-bridge.ts`: Runtime bridge for calling MCP tools
    /// - `package.json`: ES module type declaration
    /// - `tsconfig.json`: compiler options allowing the `.ts`-extensioned imports above
    ///
    /// Delegates to [`generate_with_categories`](Self::generate_with_categories) with an empty
    /// categorization map, so `index.ts` contains no category grouping.
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server introspection data
    ///
    /// # Returns
    ///
    /// Generated code with one file per tool plus index and runtime bridge.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Template rendering fails
    /// - Type conversion fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_codegen::progressive::ProgressiveGenerator;
    /// use mcp_execution_introspector::{ServerInfo, ServerCapabilities};
    /// use mcp_execution_core::ServerId;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let generator = ProgressiveGenerator::new()?;
    ///
    /// let info = ServerInfo {
    ///     id: ServerId::new("github").unwrap(),
    ///     name: "GitHub".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     tools: vec![],
    ///     capabilities: ServerCapabilities {
    ///         supports_tools: true,
    ///         supports_resources: false,
    ///         supports_prompts: false,
    ///     },
    /// };
    ///
    /// let code = generator.generate(&info)?;
    ///
    /// // Files generated:
    /// // - index.ts
    /// // - _runtime/mcp-bridge.ts
    /// // - package.json
    /// // - tsconfig.json
    /// // - one file per tool
    /// println!("Generated {} files", code.file_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate(&self, server_info: &ServerInfo) -> Result<GeneratedCode> {
        self.generate_with_categories(server_info, &HashMap::new())
    }

    /// Generates progressive loading files with categorization metadata.
    ///
    /// Like `generate`, but includes full categorization information from Claude's
    /// analysis. Categories, keywords, and short descriptions are displayed in
    /// the index file and included in individual tool file headers.
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server introspection data
    /// * `categorizations` - Map of tool name to categorization metadata
    ///
    /// # Returns
    ///
    /// Generated code with categorization metadata included.
    ///
    /// # Errors
    ///
    /// Returns error if template rendering fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_codegen::progressive::{ProgressiveGenerator, ToolCategorization};
    /// use mcp_execution_introspector::{ServerInfo, ServerCapabilities};
    /// use mcp_execution_core::ServerId;
    /// use std::collections::HashMap;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let generator = ProgressiveGenerator::new()?;
    ///
    /// let info = ServerInfo {
    ///     id: ServerId::new("github").unwrap(),
    ///     name: "GitHub".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     tools: vec![],
    ///     capabilities: ServerCapabilities {
    ///         supports_tools: true,
    ///         supports_resources: false,
    ///         supports_prompts: false,
    ///     },
    /// };
    ///
    /// let mut categorizations = HashMap::new();
    /// categorizations.insert("create_issue".to_string(), ToolCategorization {
    ///     category: "issues".to_string(),
    ///     keywords: vec!["create".to_string(), "issue".to_string(), "new".to_string(), "bug".to_string()],
    ///     short_description: "Create a new issue".to_string(),
    /// });
    ///
    /// let code = generator.generate_with_categories(&info, &categorizations)?;
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(
        skip_all,
        fields(server_id = %server_info.id, tool_count = server_info.tools.len())
    )]
    pub fn generate_with_categories(
        &self,
        server_info: &ServerInfo,
        categorizations: &HashMap<String, ToolCategorization>,
    ) -> Result<GeneratedCode> {
        if categorizations.is_empty() {
            tracing::info!(
                "Generating progressive loading code for server: {}",
                server_info.name
            );
        } else {
            tracing::info!(
                "Generating progressive loading code with categorizations for server: {}",
                server_info.name
            );
        }

        enforce_tool_count_bound(server_info)?;

        let mut code = GeneratedCode::new();
        let mut total_bytes = 0usize;
        let server_id = server_info.id.as_str();
        let typescript_names = resolve_typescript_names(&server_info.tools);
        let mut tool_metadata = Vec::with_capacity(server_info.tools.len());

        // Generate tool files (one per tool) with categorization metadata
        for (idx, tool) in server_info.tools.iter().enumerate() {
            let tool_name = tool.name.as_str();
            let categorization = categorizations.get(tool_name);
            let typescript_name = typescript_names.get(idx).cloned().unwrap_or_default();
            let extracted_properties =
                Self::extract_property_data(&tool.input_schema).map_err(|source| {
                    Self::wrap_tool_generation_error(tool, "extract property schema", source)
                })?;
            let properties_for_context = extracted_properties
                .iter()
                .map(|(info, _)| info.clone())
                .collect();
            let tool_context = Self::create_tool_context(
                server_id,
                tool,
                categorization,
                typescript_name.clone(),
                properties_for_context,
            );
            let tool_code = self
                .engine
                .render("progressive/tool", &tool_context)
                .map_err(|source| {
                    Self::wrap_tool_generation_error(tool, "render tool template", source)
                })?;

            add_tracked(
                &mut code,
                &mut total_bytes,
                GeneratedFile {
                    path: format!("{}.ts", tool_context.typescript_name),
                    content: tool_code,
                },
            )
            .map_err(|source| {
                Self::wrap_tool_generation_error(tool, "track generated tool file", source)
            })?;

            tracing::debug!(
                "Generated tool file: {}.ts (category: {:?})",
                tool_context.typescript_name,
                categorization.map(|c| &c.category)
            );

            tool_metadata.push(Self::create_tool_metadata(
                tool,
                categorization,
                typescript_name,
                extracted_properties,
            ));
        }

        // Generate index.ts with category grouping
        let index_context =
            Self::create_index_context(server_info, Some(categorizations), &typescript_names);
        let index_code = self.engine.render("progressive/index", &index_context)?;

        add_tracked(
            &mut code,
            &mut total_bytes,
            GeneratedFile {
                path: "index.ts".to_string(),
                content: index_code,
            },
        )?;

        tracing::debug!(
            "Generated index.ts with {} categorizations",
            categorizations.len()
        );

        // Generate runtime bridge (same as non-categorized)
        let bridge_context = BridgeContext::default();
        let bridge_code = self
            .engine
            .render("progressive/runtime-bridge", &bridge_context)?;

        add_tracked(
            &mut code,
            &mut total_bytes,
            GeneratedFile {
                path: "_runtime/mcp-bridge.ts".to_string(),
                content: bridge_code,
            },
        )?;

        tracing::debug!("Generated _runtime/mcp-bridge.ts");

        // Generate package.json for ES module identification and the @types/node devDependency
        // needed for the runtime bridge to type-check (see PACKAGE_JSON doc comment)
        add_tracked(
            &mut code,
            &mut total_bytes,
            GeneratedFile {
                path: "package.json".to_string(),
                content: PACKAGE_JSON.to_string(),
            },
        )?;

        tracing::debug!("Generated package.json");

        // Generate tsconfig.json so `tsc --noEmit` accepts the `.ts`-extensioned import in
        // each tool file (see TSCONFIG_JSON doc comment)
        add_tracked(
            &mut code,
            &mut total_bytes,
            GeneratedFile {
                path: "tsconfig.json".to_string(),
                content: TSCONFIG_JSON.to_string(),
            },
        )?;

        tracing::debug!("Generated tsconfig.json");

        // Generate _meta.json sidecar with structured tool metadata
        add_tracked(
            &mut code,
            &mut total_bytes,
            Self::create_metadata_file(server_info, tool_metadata)?,
        )?;

        tracing::debug!("Generated {}", METADATA_FILE_NAME);

        if categorizations.is_empty() {
            tracing::info!(
                "Successfully generated {} files for {} (progressive loading)",
                code.file_count(),
                server_info.name
            );
        } else {
            tracing::info!(
                "Successfully generated {} files for {} with categorizations (progressive loading)",
                code.file_count(),
                server_info.name
            );
        }

        Ok(code)
    }

    /// Creates tool context from MCP tool information.
    ///
    /// Converts MCP tool schema to the format needed for template rendering.
    ///
    /// `typescript_name` must be pre-resolved via [`resolve_typescript_names`] so that
    /// collisions across a server's tools are disambiguated consistently between the tool
    /// file and its `index.ts` re-export.
    ///
    /// `properties` must come from [`extract_property_data`](Self::extract_property_data) run
    /// against `tool.input_schema`. It is taken as a parameter rather than derived internally
    /// so that callers generating both the tool context and [`ToolMetadata`] for the same tool
    /// (see [`create_tool_metadata`](Self::create_tool_metadata)) can share a single schema
    /// walk instead of parsing and sanitizing the same schema twice (issue #295).
    ///
    /// Takes no `&self`: unlike [`extract_property_data`](Self::extract_property_data), nothing
    /// here reads generator state (the `Handlebars` engine is only touched by the caller, when
    /// rendering the context this returns).
    fn create_tool_context(
        server_id: &str,
        tool: &mcp_execution_introspector::ToolInfo,
        categorization: Option<&ToolCategorization>,
        typescript_name: String,
        properties: Vec<PropertyInfo>,
    ) -> ToolContext {
        let description = sanitize_jsdoc(&tool.description, 256);
        // Falls back to the tool's own description when no LLM categorization is
        // available, so the header JSDoc always emits `@description` (issue #94).
        let short_description = categorization.map_or_else(
            || description.clone(),
            |c| sanitize_jsdoc(&c.short_description, 256),
        );

        ToolContext {
            server_id: sanitize_jsdoc(server_id, 256),
            name: sanitize_jsdoc(tool.name.as_str(), 256),
            name_literal: sanitize_ts_string_literal(tool.name.as_str()),
            server_id_literal: sanitize_ts_string_literal(server_id),
            typescript_name,
            description,
            input_schema: sanitize_schema_jsdoc_descriptions(tool.input_schema.clone()),
            properties,
            category: categorization.map(|c| sanitize_jsdoc(&c.category, 128)),
            keywords: categorization.map(|c| render_keywords_for_jsdoc(&c.keywords)),
            short_description,
        }
    }

    /// Creates index context from server information.
    ///
    /// `typescript_names` must be the same pre-resolved mapping (from
    /// [`resolve_typescript_names`]) used to generate each tool's file, so the `index.ts`
    /// re-exports reference the exact identifiers those files actually export.
    ///
    /// Takes no `&self`: this only transforms the arguments it is given, and does not touch
    /// generator state.
    fn create_index_context(
        server_info: &ServerInfo,
        categorizations: Option<&HashMap<String, ToolCategorization>>,
        typescript_names: &[String],
    ) -> IndexContext {
        let tools: Vec<ToolSummary> = server_info
            .tools
            .iter()
            .enumerate()
            .map(|(idx, tool)| {
                let tool_name = tool.name.as_str();
                let cat = categorizations.and_then(|c| c.get(tool_name));
                ToolSummary {
                    typescript_name: typescript_names.get(idx).cloned().unwrap_or_default(),
                    description: sanitize_jsdoc(&tool.description, 256),
                    category: cat.map(|c| sanitize_jsdoc(&c.category, 128)),
                    keywords: cat.map(|c| render_keywords_for_jsdoc(&c.keywords)),
                    short_description: cat.map(|c| sanitize_jsdoc(&c.short_description, 256)),
                }
            })
            .collect();

        // Build category groups if categorizations are provided and non-empty. Filtering on
        // emptiness (not just Option-ness) matters because `generate` delegates to
        // `generate_with_categories` with `Some(&HashMap::new())`: an empty-but-`Some` map must
        // behave identically to `None` (no category grouping), not synthesize a spurious
        // "uncategorized" group.
        let category_groups = categorizations.filter(|c| !c.is_empty()).map(|_| {
            let mut groups: HashMap<String, Vec<ToolSummary>> = HashMap::new();

            for tool in &tools {
                let cat_name = tool
                    .category
                    .clone()
                    .unwrap_or_else(|| "uncategorized".to_string());
                groups.entry(cat_name).or_default().push(tool.clone());
            }

            let mut result: Vec<CategoryInfo> = groups
                .into_iter()
                .map(|(name, tools)| CategoryInfo { name, tools })
                .collect();

            // Sort categories alphabetically, but keep "uncategorized" last
            result.sort_by(|a, b| {
                if a.name == "uncategorized" {
                    std::cmp::Ordering::Greater
                } else if b.name == "uncategorized" {
                    std::cmp::Ordering::Less
                } else {
                    a.name.cmp(&b.name)
                }
            });

            result
        });

        IndexContext {
            server_name: sanitize_jsdoc(&server_info.name, 256),
            server_version: sanitize_jsdoc(&server_info.version, 64),
            tool_count: server_info.tools.len(),
            tools,
            categories: category_groups,
        }
    }

    /// Wraps a per-tool codegen failure — property-schema extraction, template rendering, or
    /// output tracking — in [`Error::ScriptGenerationError`] so the failing tool's name
    /// survives past the point where `tool` goes out of scope, instead of the generic
    /// [`Error::ValidationError`]/[`Error::SerializationError`]/[`Error::ResourceLimitExceeded`]
    /// each stage raises on its own. `stage` is a short, lower-case description of the failed
    /// step (e.g. `"extract property schema"`) used to build `message`.
    ///
    /// Unlike [`TemplateEngine`]'s convention of embedding the source's `Display` text into
    /// `message` and setting `source: None`, this
    /// keeps `source` populated: `classify_exit_code` in `mcp-execution-cli` downcasts into it
    /// so a wrapped [`Error::ResourceLimitExceeded`] still maps to `SERVER_ERROR` rather than
    /// collapsing to the generic exit code for every wrapped cause.
    fn wrap_tool_generation_error(tool: &ToolInfo, stage: &str, source: Error) -> Error {
        Error::ScriptGenerationError {
            tool: tool.name.as_str().to_string(),
            message: format!("failed to {stage}"),
            source: Some(Box::new(source)),
        }
    }

    /// Extracts property information from JSON Schema, discarding raw descriptions.
    ///
    /// Converts JSON Schema properties into `PropertyInfo` structures
    /// suitable for template rendering. Sibling property names that sanitize to the same
    /// TypeScript identifier (e.g. `a-b` and `a.b` both becoming `a_b`) are disambiguated
    /// with a numeric suffix, since these become fields of the same generated `Params`
    /// interface and an undetected collision would produce a duplicate, non-compiling field.
    ///
    /// Test-only: production code calls [`extract_property_data`](Self::extract_property_data)
    /// directly and shares the single resulting `Vec` between
    /// [`create_tool_context`](Self::create_tool_context) and
    /// [`create_tool_metadata`](Self::create_tool_metadata) instead of extracting twice (issue
    /// #295). This wrapper remains for tests that only care about the sanitized half.
    ///
    /// # Errors
    ///
    /// Returns error if schema is malformed or type conversion fails.
    #[cfg(test)]
    fn extract_property_infos(schema: &serde_json::Value) -> Result<Vec<PropertyInfo>> {
        Ok(Self::extract_property_data(schema)?
            .into_iter()
            .map(|(info, _raw_description)| info)
            .collect())
    }

    /// Extracts property information from JSON Schema, alongside each property's raw
    /// (un-sanitized) description.
    ///
    /// Callers that only need the JSDoc-sanitized `PropertyInfo` for template rendering (e.g.
    /// [`create_tool_context`](Self::create_tool_context)) can discard the raw-description half.
    /// Consumers that need the description as originally authored — e.g. the `_meta.json`
    /// sidecar built by [`create_tool_metadata`](Self::create_tool_metadata), which is JSON
    /// consumed by Rust rather than text interpolated into a JS comment — use the raw half
    /// instead, so they are not subject to JSDoc-safety truncation/escaping that doesn't apply
    /// to their format (issue #141). Both are derived from a single call per tool rather than
    /// two, since re-walking and re-sanitizing the same schema twice is wasted work (issue
    /// #295).
    ///
    /// Takes no `&self`: this only walks the `schema` it is given, and does not touch
    /// generator state.
    ///
    /// # Errors
    ///
    /// Returns error if schema is malformed or type conversion fails.
    fn extract_property_data(
        schema: &serde_json::Value,
    ) -> Result<Vec<(PropertyInfo, Option<String>)>> {
        let raw_properties = extract_properties(schema);

        let mut properties = Vec::new();
        let mut used_names = HashSet::new();
        for prop in raw_properties {
            let raw_name = prop["name"]
                .as_str()
                .ok_or_else(|| Error::ValidationError {
                    field: "name".to_string(),
                    reason: "Property name is not a string".to_string(),
                })?
                .to_string();

            let typescript_type = prop["type"]
                .as_str()
                .ok_or_else(|| Error::ValidationError {
                    field: "type".to_string(),
                    reason: "Property type is not a string".to_string(),
                })?
                .to_string();

            let required = prop["required"].as_bool().unwrap_or(false);

            // Extract description if available (looked up by the raw schema key, before
            // sanitization, since that's what the input schema is actually keyed by)
            let raw_description = schema.as_object().and_then(|obj| {
                obj.get("properties")
                    .and_then(|props| props.as_object())
                    .and_then(|props| props.get(&raw_name))
                    .and_then(|prop_schema| prop_schema.as_object())
                    .and_then(|obj| obj.get("description"))
                    .and_then(|desc| desc.as_str())
                    .map(str::to_string)
            });
            let description = raw_description
                .as_deref()
                .map(|desc| sanitize_jsdoc(desc, 256));

            let base_name = sanitize_ts_identifier(&raw_name);
            properties.push((
                PropertyInfo {
                    name: disambiguate_identifier(&base_name, &mut used_names),
                    typescript_type,
                    description,
                    required,
                },
                raw_description,
            ));
        }

        Ok(properties)
    }

    /// Builds structured metadata for a single tool, for the `_meta.json` sidecar.
    ///
    /// Unlike [`create_tool_context`](Self::create_tool_context), `name`, `description`, and
    /// parameter descriptions all use the RAW, unsanitized MCP values: the sidecar is a data
    /// contract consumed by other Rust code, not interpolated into a `JSDoc` comment, so
    /// JSDoc-safety sanitization (truncation, `*/`-escaping, newline-flattening) would only
    /// lose fidelity. Parameter descriptions come from
    /// [`extract_property_data`](Self::extract_property_data)'s raw half rather than the
    /// JSDoc-sanitized `PropertyInfo` used for template rendering, which is what fully fixes
    /// the data loss described in issue #141 (the old regex-based parser could not recover
    /// parameter descriptions from the generated TypeScript at all).
    ///
    /// `properties` must come from [`extract_property_data`](Self::extract_property_data) run
    /// against `tool.input_schema` — passed in rather than derived internally so this can share
    /// a single schema walk with [`create_tool_context`](Self::create_tool_context) for the same
    /// tool instead of parsing and sanitizing the same schema twice (issue #295).
    ///
    /// Takes no `&self`: this only transforms the arguments it is given, and does not touch
    /// generator state.
    fn create_tool_metadata(
        tool: &ToolInfo,
        categorization: Option<&ToolCategorization>,
        typescript_name: String,
        properties: Vec<(PropertyInfo, Option<String>)>,
    ) -> ToolMetadata {
        let description = (!tool.description.is_empty()).then(|| tool.description.clone());
        let category = categorization.map(|c| c.category.clone());
        let keywords = categorization.map_or_else(Vec::new, |c| c.keywords.clone());

        ToolMetadata {
            name: tool.name.as_str().to_string(),
            typescript_name,
            category,
            keywords,
            description,
            parameters: properties
                .into_iter()
                .map(|(p, raw_description)| ParameterMetadata {
                    name: p.name,
                    typescript_type: p.typescript_type,
                    required: p.required,
                    description: raw_description,
                })
                .collect(),
        }
    }

    /// Builds the `_meta.json` sidecar file from per-tool metadata already collected
    /// during the tool-file generation loop.
    ///
    /// # Errors
    ///
    /// Returns error if the metadata cannot be serialized to JSON (should not happen
    /// with these plain-data types).
    fn create_metadata_file(
        server_info: &ServerInfo,
        tools: Vec<ToolMetadata>,
    ) -> Result<GeneratedFile> {
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: server_info.id.as_str().to_string(),
            server_name: server_info.name.clone(),
            server_version: server_info.version.clone(),
            tools,
        };

        let content =
            serde_json::to_string_pretty(&meta).map_err(|e| Error::SerializationError {
                message: format!("failed to serialize {METADATA_FILE_NAME}"),
                source: Some(e),
            })?;

        Ok(GeneratedFile {
            path: METADATA_FILE_NAME.to_string(),
            content,
        })
    }
}

/// Cheaply rejects an oversized `server_info.tools` list before any per-tool template
/// rendering happens (denial-of-service protection, CWE-400).
///
/// A caller reaching [`ProgressiveGenerator::generate`]/`generate_with_categories` through
/// `mcp_execution_introspector::Introspector::discover_server` already has its tool count
/// bounded upstream, but this crate's generator functions are public API and can be called
/// directly with a hand-built `ServerInfo`, so this check is not purely redundant.
///
/// # Errors
///
/// Returns [`Error::ResourceLimitExceeded`] if `server_info.tools.len()` plus the
/// [`FIXED_FILE_COUNT`] files every call emits would exceed [`MAX_GENERATED_FILES`] — the same
/// threshold [`add_tracked`] checks incrementally as each file is produced, so this
/// short-circuits exactly the inputs that check would go on to reject anyway, before any
/// template rendering happens at all.
fn enforce_tool_count_bound(server_info: &ServerInfo) -> Result<()> {
    let projected_file_count = server_info.tools.len() + FIXED_FILE_COUNT;
    if projected_file_count > MAX_GENERATED_FILES {
        return Err(Error::ResourceLimitExceeded {
            resource: format!("tool count for server '{}'", server_info.id),
            actual: server_info.tools.len(),
            limit: MAX_GENERATED_FILES - FIXED_FILE_COUNT,
        });
    }
    Ok(())
}

/// Adds `file` to `code`, tracking its contribution to `total_bytes` and bailing out
/// immediately if either the running byte total or the file count would exceed its configured
/// bound (denial-of-service protection, CWE-400).
///
/// Checked incrementally as each file is produced, rather than only after the entire
/// [`GeneratedCode`] has been built: an oversized `ServerInfo` (or one whose fixed-overhead
/// files, e.g. `_meta.json` re-embedding every tool's schema, push the total over the edge)
/// is rejected as soon as the offending file is generated, so this generator never holds the
/// full amplified output in memory before the bound is enforced (issue #198 S4).
///
/// # Errors
///
/// Returns [`Error::ResourceLimitExceeded`] if adding `file` would push the running byte total
/// past [`MAX_GENERATED_BYTES`], or the file count past [`MAX_GENERATED_FILES`]. Returns
/// [`Error::DuplicateGeneratedFilePath`] if `file.path` was already added earlier in this
/// call — see [`GeneratedCode::add_file`]; this is defense-in-depth once
/// [`resolve_typescript_names`] seeds its collision set with this module's own reserved
/// output filenames (issue #312), not a path this function expects to hit in practice.
fn add_tracked(
    code: &mut GeneratedCode,
    total_bytes: &mut usize,
    file: GeneratedFile,
) -> Result<()> {
    *total_bytes += file.content.len();
    if *total_bytes > MAX_GENERATED_BYTES {
        return Err(Error::ResourceLimitExceeded {
            resource: "generated output size".to_string(),
            actual: *total_bytes,
            limit: MAX_GENERATED_BYTES,
        });
    }

    code.add_file(file)?;

    if code.file_count() > MAX_GENERATED_FILES {
        return Err(Error::ResourceLimitExceeded {
            resource: "generated file count".to_string(),
            actual: code.file_count(),
            limit: MAX_GENERATED_FILES,
        });
    }

    Ok(())
}

/// Sanitizes a server-controlled string for safe interpolation into `JSDoc` block comments.
///
/// Neutralizes control characters (C0, DEL, C1 — everything `char::is_control` reports —
/// plus U+2028 LINE SEPARATOR/U+2029 PARAGRAPH SEPARATOR, which ECMAScript treats as line
/// terminators even though they aren't in the control-character category) by delegating to
/// the shared [`mcp_execution_core::untrusted::sanitize_untrusted_text`], which *replaces*
/// each with a space rather than deleting it — deleting would glue adjacent words together
/// (`"tab\tseparated"` -> `"tabseparated"`) and, more importantly, would let a control
/// character sitting between `*` and `/` collapse into a live `JSDoc` comment terminator once
/// removed. This neutralization runs *before* the `*/` escape step for exactly that reason:
/// escaping first would see no `*/` match (the control character still separates them), then
/// deleting/collapsing that character afterward would reopen the comment. Truncation to
/// `max_len` runs last, after escaping, so a `*/` straddling the truncation boundary is
/// already widened to `*\/` and cannot be split back into a bare `*/` by the cut.
fn sanitize_jsdoc(s: &str, max_len: usize) -> String {
    let neutralized = mcp_execution_core::untrusted::sanitize_untrusted_text(s, usize::MAX);
    let sanitized = neutralized.replace("*/", "*\\/");
    if sanitized.chars().count() > max_len {
        sanitized.chars().take(max_len).collect()
    } else {
        sanitized
    }
}

/// Joins `ToolCategorization::keywords` into the comma-separated form `JSDoc` rendering displays
/// (`@keywords foo, bar, baz`), sanitizing the joined text the same way any other JSDoc-embedded
/// value is sanitized.
fn render_keywords_for_jsdoc(keywords: &[String]) -> String {
    sanitize_jsdoc(&keywords.join(", "), 256)
}

/// Escapes a string for safe embedding inside a single-quoted TypeScript string literal.
///
/// Backslashes are escaped before quotes so the backslash introduced by quote-escaping
/// is not itself re-escaped. Carriage returns and newlines are escaped so the value
/// cannot terminate the literal by injecting a raw line break. U+2028/U+2029 are also
/// escaped: legal but unescaped inside a string literal only since ES2019, so a raw
/// occurrence would be a syntax error for consumers targeting an older ECMAScript target.
pub(crate) fn sanitize_ts_string_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// JavaScript/TypeScript reserved words that cannot be used as a function or export
/// identifier. Generated tool code is always emitted as an ES module, which is implicitly
/// strict mode, so this includes both the unconditional and strict-mode-only reserved words,
/// plus `eval`/`arguments`, which strict mode forbids as a `BindingIdentifier` (a function
/// declaration's name) even though they are not formally reserved words.
const RESERVED_WORDS: &[&str] = &[
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Output filenames (without extension) that [`ProgressiveGenerator`] itself always emits
/// alongside per-tool files, regardless of the introspected tool list. Seeded into
/// [`resolve_typescript_names`]'s case-insensitive `used_lower` collision set (unlike
/// [`RESERVED_WORDS`], which is checked case-sensitively — see that function's docs for why the
/// two differ): a tool whose sanitized name matches one of these would otherwise silently
/// overwrite this generator's own fixed output file (issue #312) instead of being disambiguated
/// like any other name collision.
const RESERVED_OUTPUT_NAMES: &[&str] = &["index"];

/// Resolves a collision-free TypeScript identifier for each tool, in tool order.
///
/// `sanitize_ts_identifier` can map distinct tool names to the same identifier (e.g.
/// `foo-bar` and `foo.bar` both become `foo_bar`), and an MCP server is not guaranteed to
/// report unique raw tool names in the first place. Since `typescript_name` doubles as the
/// generated file's basename and its `index.ts` re-export, an undetected collision would
/// silently overwrite one tool's file and produce a duplicate-export compile error.
///
/// The result is keyed by position rather than by raw tool name: two tools sharing an
/// identical raw name would otherwise collapse to a single map entry, losing one of the two
/// resolved identifiers even though both were correctly disambiguated. Callers must look up
/// entries by the tool's index in the same `tools` slice.
///
/// Collision detection combines two checks with different case sensitivity, since
/// `typescript_name` doubles as both a language-level identifier and an output filename:
///
/// - JS/TS reserved words (e.g. `delete`) are checked case-*sensitively*, an exact match
///   against the lowercase reserved word — reserved words are only reserved in their exact
///   lowercase form (`Delete`, `New`, `Import` are all legal identifiers).
/// - The fixed output filenames (e.g. `index`) and previously-resolved tool names are checked
///   case-*insensitively* (via [`disambiguate_output_filename`]'s case-folded `used_lower`
///   set), because filenames collide regardless of case on a case-insensitive filesystem (macOS
///   APFS, Windows NTFS by default, this project's primary dev platforms): a tool named `Index`
///   collides with the fixed `index.ts` output, and two tools named `getUser`/`GetUser` collide
///   with each other (issue #312 S1, N2).
///
/// The emitted identifier always preserves each tool's original case; only the collision
/// *checks* fold or preserve case as described above.
fn resolve_typescript_names(tools: &[ToolInfo]) -> Vec<String> {
    let mut used_lower: HashSet<String> = RESERVED_OUTPUT_NAMES
        .iter()
        .map(|&s| s.to_ascii_lowercase())
        .collect();
    let mut resolved = Vec::with_capacity(tools.len());

    for tool in tools {
        let base = sanitize_ts_identifier(&to_camel_case(tool.name.as_str()));
        resolved.push(disambiguate_output_filename(&base, &mut used_lower));
    }

    resolved
}

/// Disambiguates `base` against reserved JS/TS words and `used_lower`, appending a numeric
/// suffix (`_2`, `_3`, ...) — mirroring [`disambiguate_identifier`]'s suffix scheme — until a
/// candidate is found that is neither an exact (case-sensitive) match for a
/// [`RESERVED_WORDS`] entry nor a case-insensitive match in `used_lower`. The winning
/// candidate's lowercased form is then reserved in `used_lower`. The returned identifier
/// preserves `base`'s original casing; only the collision *checks* fold or preserve case (see
/// [`resolve_typescript_names`] for why the two checks differ).
///
/// A dedicated function rather than reusing [`disambiguate_identifier`]: that one is shared with
/// property-name disambiguation, which must stay case-sensitive — `name` and `Name` are
/// legitimately distinct object keys in the generated `Params` interface. Output *filenames* have
/// no such case-sensitive guarantee once a case-insensitive filesystem is in play, which is what
/// [`resolve_typescript_names`] uses this for (issue #312 S1/N2).
fn disambiguate_output_filename(base: &str, used_lower: &mut HashSet<String>) -> String {
    let mut candidate = base.to_string();
    let mut suffix = 2;
    loop {
        let is_reserved_word = RESERVED_WORDS.contains(&candidate.as_str());
        if !is_reserved_word && used_lower.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }
}

fn sanitize_schema_jsdoc_descriptions(mut value: serde_json::Value) -> serde_json::Value {
    let mut cap_hit = false;
    sanitize_schema_jsdoc_value(&mut value, 0, &mut cap_hit);
    if cap_hit {
        // Known limitation: unlike `json_schema_to_typescript` (called once per property via
        // `extract_properties`), `create_tool_context` calls this once per tool directly on
        // the whole `input_schema` — but this warning still carries no tool/server identifier
        // to correlate it back to the originating `generate_with_categories` call.
        tracing::warn!(
            max_depth = MAX_SCHEMA_RECURSION_DEPTH,
            "schema nesting exceeded MAX_SCHEMA_RECURSION_DEPTH; descriptions beyond that depth \
             were left unsanitized"
        );
    }
    value
}

/// Sanitizes every `description` field in `value`'s tree in place, recursing into nested
/// objects and arrays up to [`MAX_SCHEMA_RECURSION_DEPTH`] — see that constant's docs for what
/// this cap actually defends against (this crate's `pub` API surface, not a reachable wire-path
/// schema). `cap_hit` is set (never cleared) the first time any branch trips the cap, so the
/// public wrapper can log once per call rather than once per clipped branch.
///
/// `value` is (a clone of) a tool's `input_schema`, called on the schema's true top level with
/// no depth peeled off beforehand (unlike [`typescript::extract_properties`]'s call path into
/// `json_schema_to_typescript`) — so `depth` here always matches the schema's real JSON
/// nesting. Beyond the depth cap, remaining branches are left unsanitized rather than
/// continuing to recurse.
///
/// [`typescript::extract_properties`]: crate::common::typescript::extract_properties
fn sanitize_schema_jsdoc_value(value: &mut serde_json::Value, depth: usize, cap_hit: &mut bool) {
    if depth >= MAX_SCHEMA_RECURSION_DEPTH {
        *cap_hit = true;
        return;
    }

    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "description" {
                    if let Some(description) = child.as_str() {
                        *child = serde_json::Value::String(sanitize_jsdoc(description, 256));
                    } else {
                        *child = serde_json::Value::Null;
                    }
                } else {
                    sanitize_schema_jsdoc_value(child, depth + 1, cap_hit);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                sanitize_schema_jsdoc_value(child, depth + 1, cap_hit);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::{ServerId, ToolName};
    use mcp_execution_introspector::{ServerCapabilities, ToolInfo};
    use serde_json::json;

    fn create_test_server_info() -> ServerInfo {
        ServerInfo {
            id: ServerId::new("test-server").unwrap(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("create_issue").unwrap(),
                    description: "Creates a new issue".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Issue title"
                            },
                            "body": {
                                "type": "string",
                                "description": "Issue body"
                            }
                        },
                        "required": ["title"]
                    }),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("update_issue").unwrap(),
                    description: "Updates an existing issue".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "number"
                            }
                        },
                        "required": ["id"]
                    }),
                    output_schema: None,
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        }
    }

    #[test]
    fn test_progressive_generator_new() {
        let generator = ProgressiveGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_generate_progressive_files() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();

        // Should generate:
        // - 2 tool files
        // - 1 index.ts
        // - 1 runtime bridge
        // - 1 package.json
        // - 1 tsconfig.json
        // - 1 _meta.json
        assert_eq!(code.file_count(), 7);

        // Check tool files exist
        let tool_files: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();

        assert!(tool_files.contains(&"createIssue.ts"));
        assert!(tool_files.contains(&"updateIssue.ts"));
        assert!(tool_files.contains(&"index.ts"));
        assert!(tool_files.contains(&"_runtime/mcp-bridge.ts"));
        assert!(tool_files.contains(&"package.json"));
        assert!(tool_files.contains(&"tsconfig.json"));
        assert!(tool_files.contains(&"_meta.json"));
    }

    /// Regression guard for #279: `generate` delegates to `generate_with_categories` with an
    /// empty categorization map, so `create_index_context` must gate its category-grouping
    /// branch on emptiness, not just `Option`-ness — otherwise `Some(&HashMap::new())` produces
    /// a spurious "uncategorized" `CategoryInfo` group in `index.ts` that `generate` never
    /// produced before the delegation.
    #[test]
    fn test_generate_index_ts_has_no_category_grouping() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();
        let index_file = code.files.iter().find(|f| f.path == "index.ts").unwrap();

        assert!(
            !index_file.content.contains("uncategorized"),
            "generate()'s index.ts must not contain category grouping: {}",
            index_file.content
        );
    }

    /// Regression guard for #183: tool files import the runtime bridge with an explicit `.ts`
    /// extension, which `tsc` only accepts under `allowImportingTsExtensions`; that option in
    /// turn requires `noEmit` (or a declaration/emit setting) to be internally consistent.
    /// `types` must explicitly name `node` — TypeScript 7's automatic `@types/*` acquisition
    /// does not extend `@types/node`'s ambient globals the way TypeScript 5.x's does, so
    /// `tsc --noEmit` fails under TS 7 without this even with `@types/node` installed.
    #[test]
    fn test_generate_tsconfig_json_allows_ts_extension_imports() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();

        let tsconfig_file = code
            .files
            .iter()
            .find(|f| f.path == "tsconfig.json")
            .expect("tsconfig.json not found");

        let parsed: serde_json::Value =
            serde_json::from_str(&tsconfig_file.content).expect("tsconfig.json is not valid JSON");
        let compiler_options = &parsed["compilerOptions"];

        assert_eq!(compiler_options["allowImportingTsExtensions"], true);
        assert_eq!(compiler_options["noEmit"], true);
        assert_eq!(compiler_options["types"], serde_json::json!(["node"]));
    }

    /// Regression guard for #183: `tsconfig.json` alone does not make the generated package
    /// pass `tsc --noEmit` — the runtime bridge imports Node builtins and references ambient
    /// globals (`process`, `NodeJS`) that only resolve with `@types/node` installed. Without a
    /// declared devDependency, a consumer running `tsc --noEmit` "out of the box" still fails.
    #[test]
    fn test_generate_package_json_declares_types_node_dev_dependency() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();

        let package_json_file = code
            .files
            .iter()
            .find(|f| f.path == "package.json")
            .expect("package.json not found");

        let parsed: serde_json::Value = serde_json::from_str(&package_json_file.content)
            .expect("package.json is not valid JSON");

        assert!(
            parsed["devDependencies"]["@types/node"].is_string(),
            "package.json is missing a @types/node devDependency: {parsed}"
        );
    }

    #[test]
    fn test_generate_meta_json_preserves_parameter_descriptions() {
        // Issue #141 regression: the old regex-based skill parser could not recover
        // parameter descriptions from generated TypeScript at all. The `_meta.json`
        // sidecar must carry them through faithfully.
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();
        let meta_file = code.files.iter().find(|f| f.path == "_meta.json").unwrap();
        let meta: ServerMetadata = serde_json::from_str(&meta_file.content).unwrap();

        assert_eq!(meta.schema_version, METADATA_SCHEMA_VERSION);
        assert_eq!(meta.server_id, "test-server");
        assert_eq!(meta.server_name, "Test Server");
        assert_eq!(meta.server_version, "1.0.0");
        assert_eq!(meta.tools.len(), 2);

        let create_issue = meta
            .tools
            .iter()
            .find(|t| t.name == "create_issue")
            .unwrap();
        assert_eq!(create_issue.typescript_name, "createIssue");
        let title = create_issue
            .parameters
            .iter()
            .find(|p| p.name == "title")
            .unwrap();
        assert_eq!(title.description, Some("Issue title".to_string()));
        assert!(title.required);
    }

    #[test]
    fn test_generate_with_categories_meta_json_includes_categorization() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let mut categorizations = HashMap::new();
        categorizations.insert(
            "create_issue".to_string(),
            ToolCategorization {
                category: "issues".to_string(),
                keywords: vec!["create".to_string(), "issue".to_string(), "new".to_string()],
                short_description: "Create a new issue".to_string(),
            },
        );

        let code = generator
            .generate_with_categories(&server_info, &categorizations)
            .unwrap();
        let meta_file = code.files.iter().find(|f| f.path == "_meta.json").unwrap();
        let meta: ServerMetadata = serde_json::from_str(&meta_file.content).unwrap();

        let create_issue = meta
            .tools
            .iter()
            .find(|t| t.name == "create_issue")
            .unwrap();
        assert_eq!(create_issue.category, Some("issues".to_string()));
        assert_eq!(
            create_issue.keywords,
            vec!["create".to_string(), "issue".to_string(), "new".to_string()]
        );

        let update_issue = meta
            .tools
            .iter()
            .find(|t| t.name == "update_issue")
            .unwrap();
        assert!(update_issue.category.is_none());
        assert!(update_issue.keywords.is_empty());
    }

    #[test]
    fn test_generate_meta_json_parameter_description_is_raw_not_jsdoc_sanitized() {
        // Issue #141 regression (critic S1): the sidecar is JSON consumed by Rust, not a JS
        // comment, so its parameter descriptions must NOT go through `sanitize_jsdoc`'s
        // truncation/escaping/newline-flattening — only the `.ts` template's JSDoc comment
        // needs that treatment.
        let raw_description = format!(
            "Matches C-style /* */ comment blocks.\nSecond line follows. {}",
            "x".repeat(300)
        );
        assert!(raw_description.contains("*/"));
        assert!(raw_description.contains('\n'));
        assert!(raw_description.chars().count() > 256);

        let server_info = ServerInfo {
            id: ServerId::new("test-server").unwrap(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("send_message").unwrap(),
                description: "Sends a message".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "notes": {
                            "type": "string",
                            "description": raw_description
                        }
                    },
                    "required": []
                }),
                output_schema: None,
            }],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let generator = ProgressiveGenerator::new().unwrap();
        let code = generator.generate(&server_info).unwrap();

        // The sidecar carries the raw, untruncated, unescaped, non-flattened description.
        let meta_file = code.files.iter().find(|f| f.path == "_meta.json").unwrap();
        let meta: ServerMetadata = serde_json::from_str(&meta_file.content).unwrap();
        let send_message = meta
            .tools
            .iter()
            .find(|t| t.name == "send_message")
            .unwrap();
        let notes = send_message
            .parameters
            .iter()
            .find(|p| p.name == "notes")
            .unwrap();
        assert_eq!(notes.description, Some(raw_description.clone()));

        // The `.ts` template's JSDoc comment still uses the sanitized form, since it IS
        // embedded in a JS comment.
        let ts_file = code
            .files
            .iter()
            .find(|f| f.path == "sendMessage.ts")
            .unwrap();
        assert!(
            !ts_file.content.contains(raw_description.as_str()),
            "the .ts file must not contain the raw, un-sanitized description verbatim"
        );
        assert!(
            ts_file.content.contains("*\\/"),
            "the .ts file must escape '*/' to avoid closing the JSDoc comment early"
        );
        assert!(
            !ts_file
                .content
                .contains("Matches C-style /* */ comment blocks.\nSecond"),
            "the .ts file must flatten newlines within the description to spaces"
        );
    }

    #[test]
    fn test_create_tool_context() {
        let tool = ToolInfo {
            name: ToolName::new("send_message").unwrap(),
            description: "Sends a message".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"]
            }),
            output_schema: None,
        };

        let categorization = ToolCategorization {
            category: "messaging".to_string(),
            keywords: vec![
                "send".to_string(),
                "message".to_string(),
                "chat".to_string(),
            ],
            short_description: "Send a message".to_string(),
        };
        let properties = ProgressiveGenerator::extract_property_infos(&tool.input_schema).unwrap();
        let context = ProgressiveGenerator::create_tool_context(
            "test-server",
            &tool,
            Some(&categorization),
            "sendMessage".to_string(),
            properties,
        );

        assert_eq!(context.server_id, "test-server");
        assert_eq!(context.name, "send_message");
        assert_eq!(context.name_literal, "send_message");
        assert_eq!(context.server_id_literal, "test-server");
        assert_eq!(context.typescript_name, "sendMessage");
        assert_eq!(context.description, "Sends a message");
        assert_eq!(context.properties.len(), 1);
        assert_eq!(context.properties[0].name, "text");
        assert_eq!(context.category, Some("messaging".to_string()));
        assert_eq!(context.keywords, Some("send, message, chat".to_string()));
        assert_eq!(context.short_description, "Send a message".to_string());
    }

    #[test]
    fn test_wrap_tool_generation_error_preserves_tool_name_and_source() {
        // The property-extraction error raised deep in `extract_property_data` is generic
        // (`Error::ValidationError`) and has no `tool` field; this wrapper attributes the
        // failure back to the specific tool being generated.
        let tool = ToolInfo {
            name: ToolName::new("send_message").unwrap(),
            description: String::new(),
            input_schema: json!({}),
            output_schema: None,
        };
        let source = Error::ValidationError {
            field: "type".to_string(),
            reason: "Property type is not a string".to_string(),
        };

        let wrapped = ProgressiveGenerator::wrap_tool_generation_error(
            &tool,
            "extract property schema",
            source,
        );

        match wrapped {
            Error::ScriptGenerationError {
                tool: tool_name,
                message,
                source,
            } => {
                assert_eq!(tool_name, "send_message");
                // `message` must not duplicate the source's Display text (only one of the two
                // should carry it, per this crate's error-chain-printing convention).
                assert_eq!(message, "failed to extract property schema");
                let source = source.expect("source must be preserved for exit-code classification");
                assert!(source.to_string().contains("Property type is not a string"));
            }
            other => panic!("expected ScriptGenerationError, got {other:?}"),
        }
    }

    #[test]
    fn test_wrap_tool_generation_error_covers_render_and_tracking_stages() {
        // #185's real (reachable) attribution gap: a template-render failure or an
        // output-tracking failure that happens while processing one tool out of many must
        // still name that tool, not just the (structurally unreachable) schema-extraction path.
        let tool = ToolInfo {
            name: ToolName::new("send_message").unwrap(),
            description: String::new(),
            input_schema: json!({}),
            output_schema: None,
        };

        let render_failure = ProgressiveGenerator::wrap_tool_generation_error(
            &tool,
            "render tool template",
            Error::SerializationError {
                message: "Template rendering failed: boom".to_string(),
                source: None,
            },
        );
        assert!(render_failure.is_script_generation_error());

        let tracking_failure = ProgressiveGenerator::wrap_tool_generation_error(
            &tool,
            "track generated tool file",
            Error::ResourceLimitExceeded {
                resource: "generated output size".to_string(),
                actual: 10,
                limit: 5,
            },
        );
        match tracking_failure {
            Error::ScriptGenerationError {
                tool: tool_name,
                source,
                ..
            } => {
                assert_eq!(tool_name, "send_message");
                let source = source.expect("source must be preserved for exit-code classification");
                // The nested `ResourceLimitExceeded` must survive intact so
                // `classify_exit_code` (mcp-cli) can still recurse into it.
                assert!(
                    source
                        .downcast_ref::<Error>()
                        .unwrap()
                        .is_resource_limit_exceeded()
                );
            }
            other => panic!("expected ScriptGenerationError, got {other:?}"),
        }
    }

    #[test]
    fn test_create_tool_context_without_categorization_falls_back_to_description() {
        let generator = ProgressiveGenerator::new().unwrap();
        let tool = ToolInfo {
            name: ToolName::new("format_document").unwrap(),
            description: "Format document with language-specific rules".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"]
            }),
            output_schema: None,
        };

        let properties = ProgressiveGenerator::extract_property_infos(&tool.input_schema).unwrap();
        let context = ProgressiveGenerator::create_tool_context(
            "test-server",
            &tool,
            None,
            "formatDocument".to_string(),
            properties,
        );

        assert_eq!(
            context.short_description,
            "Format document with language-specific rules".to_string()
        );

        // The header JSDoc must emit @description even without LLM categorization.
        let rendered = generator
            .engine
            .render("progressive/tool", &context)
            .unwrap();
        assert!(rendered.contains("@description Format document with language-specific rules"));
    }

    #[test]
    fn test_create_tool_context_input_schema_is_sanitized() {
        let tool = ToolInfo {
            name: ToolName::new("send_message").unwrap(),
            description: "Sends a message".to_string(),
            input_schema: json!({
                "type": "object",
                "description": "Schema */ injected\nnext",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"]
            }),
            output_schema: None,
        };

        let properties = ProgressiveGenerator::extract_property_infos(&tool.input_schema).unwrap();
        let context = ProgressiveGenerator::create_tool_context(
            "test-server",
            &tool,
            None,
            "sendMessage".to_string(),
            properties,
        );

        let expected = sanitize_schema_jsdoc_descriptions(tool.input_schema);
        assert_eq!(context.input_schema, expected);
        assert_eq!(
            context.input_schema["description"],
            json!("Schema *\\/ injected next")
        );
    }

    #[test]
    fn test_create_index_context() {
        let server_info = create_test_server_info();
        let typescript_names = resolve_typescript_names(&server_info.tools);

        let context =
            ProgressiveGenerator::create_index_context(&server_info, None, &typescript_names);

        assert_eq!(context.server_name, "Test Server");
        assert_eq!(context.server_version, "1.0.0");
        assert_eq!(context.tool_count, 2);
        assert_eq!(context.tools.len(), 2);
        assert_eq!(context.tools[0].typescript_name, "createIssue");
        assert!(context.categories.is_none());
    }

    /// Regression guard for #312: a tool whose raw MCP name sanitizes to `index` must not
    /// collide with the always-emitted `index.ts` re-export. `resolve_typescript_names` seeds
    /// its collision set with this generator's own reserved output filenames, so the tool gets
    /// disambiguated (`index_2`) exactly like a JS/TS reserved-word collision would.
    #[test]
    fn test_tool_named_index_does_not_collide_with_index_ts() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = ServerInfo {
            id: ServerId::new("test-server").unwrap(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("index").unwrap(),
                description: "A tool literally named index".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                output_schema: None,
            }],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let code = generator.generate(&server_info).unwrap();

        let typescript_names = resolve_typescript_names(&server_info.tools);
        assert_eq!(
            typescript_names[0], "index_2",
            "a tool named `index` must be disambiguated, not collide with the fixed index.ts"
        );

        let tool_file = code
            .files
            .iter()
            .find(|f| f.path == "index_2.ts")
            .expect("the tool's own file must exist at its disambiguated path");
        assert!(
            tool_file.content.contains("A tool literally named index"),
            "the tool's own generated content must not have been lost: {}",
            tool_file.content
        );

        let index_file = code
            .files
            .iter()
            .find(|f| f.path == "index.ts")
            .expect("the fixed index.ts re-export must still exist");
        assert!(
            index_file.content.contains("index_2"),
            "index.ts must re-export the tool's disambiguated identifier: {}",
            index_file.content
        );
        assert!(
            index_file
                .content
                .contains("export { callMCPTool } from './_runtime/mcp-bridge.ts';"),
            "index.ts must be the fixed re-export (with the runtime bridge re-export), \
             not the overwritten tool file: {}",
            index_file.content
        );

        // Exactly one file at each path: the collision never happened.
        assert_eq!(
            code.files.iter().filter(|f| f.path == "index.ts").count(),
            1
        );
        assert_eq!(
            code.files.iter().filter(|f| f.path == "index_2.ts").count(),
            1
        );
    }

    /// Regression guard for #312 S1: `RESERVED_OUTPUT_NAMES` membership alone only catches an
    /// exact-case match, but `index.ts`/`Index.ts` are the same file on a case-insensitive
    /// filesystem (macOS APFS, Windows NTFS by default — this project's primary dev platforms).
    /// A tool named `Index` must be disambiguated the same way a tool literally named `index`
    /// is, via `disambiguate_output_filename`'s case-insensitive collision check.
    #[test]
    fn test_tool_named_index_with_different_case_is_disambiguated() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = ServerInfo {
            id: ServerId::new("test-server").unwrap(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("Index").unwrap(),
                description: "A tool literally named Index".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                output_schema: None,
            }],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let code = generator.generate(&server_info).unwrap();

        let typescript_names = resolve_typescript_names(&server_info.tools);
        assert_eq!(
            typescript_names[0], "Index_2",
            "a tool named `Index` must be disambiguated case-insensitively against the \
             reserved `index` output name"
        );

        assert!(
            code.files.iter().any(|f| f.path == "Index_2.ts"),
            "the tool's own file must exist at its disambiguated path: {:?}",
            code.files.iter().map(|f| &f.path).collect::<Vec<_>>()
        );
        // Exactly one file at each path: on a case-insensitive filesystem, "index.ts" and
        // "Index.ts" would otherwise be the same file.
        assert_eq!(
            code.files.iter().filter(|f| f.path == "index.ts").count(),
            1
        );
        assert!(!code.files.iter().any(|f| f.path == "Index.ts"));
    }

    /// Regression guard for #312 N2: a server exposing BOTH `Index` and `index` as distinct
    /// tools must not have their resolved output filenames collide case-insensitively with
    /// EACH OTHER, not just with the fixed `index.ts`. Both are already disambiguated away
    /// from the reserved `index` name; they must additionally be disambiguated from each
    /// other, since `Index_2`/`index_2` would themselves collide case-insensitively.
    #[test]
    fn test_tools_named_index_and_capital_index_do_not_collide_with_each_other() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = ServerInfo {
            id: ServerId::new("test-server").unwrap(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("Index").unwrap(),
                    description: "Capitalized".to_string(),
                    input_schema: json!({"type": "object", "properties": {}, "required": []}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("index").unwrap(),
                    description: "Lowercase".to_string(),
                    input_schema: json!({"type": "object", "properties": {}, "required": []}),
                    output_schema: None,
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let code = generator.generate(&server_info).unwrap();
        let typescript_names = resolve_typescript_names(&server_info.tools);

        assert_ne!(
            typescript_names[0].to_ascii_lowercase(),
            typescript_names[1].to_ascii_lowercase(),
            "the two tools' resolved names must not collide case-insensitively: {typescript_names:?}"
        );

        let paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();
        let mut lowercased_paths: Vec<String> =
            paths.iter().map(|p| p.to_ascii_lowercase()).collect();
        let before = lowercased_paths.len();
        lowercased_paths.sort();
        lowercased_paths.dedup();
        assert_eq!(
            lowercased_paths.len(),
            before,
            "no two generated file paths may be case-insensitive duplicates of each other: {paths:?}"
        );
    }

    /// Regression guard for #312 N2's side effect: two DIFFERENT tools whose sanitized names
    /// differ only by case (not involving the reserved `index` name at all) must also be
    /// disambiguated from each other, since a case-insensitive filesystem would otherwise merge
    /// their output files exactly like the `Index`/`index` case.
    #[test]
    fn test_tools_differing_only_by_case_are_disambiguated_from_each_other() {
        let server_info = ServerInfo {
            id: ServerId::new("test-server").unwrap(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("get_user").unwrap(),
                    description: "snake_case".to_string(),
                    input_schema: json!({"type": "object", "properties": {}, "required": []}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("GetUser").unwrap(),
                    description: "PascalCase".to_string(),
                    input_schema: json!({"type": "object", "properties": {}, "required": []}),
                    output_schema: None,
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let typescript_names = resolve_typescript_names(&server_info.tools);

        assert_eq!(typescript_names[0], "getUser");
        assert_ne!(
            typescript_names[0].to_ascii_lowercase(),
            typescript_names[1].to_ascii_lowercase(),
            "getUser/GetUser must not collide case-insensitively: {typescript_names:?}"
        );
    }

    #[test]
    fn test_extract_property_infos() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "User name"
                },
                "age": {
                    "type": "number"
                }
            },
            "required": ["name"]
        });

        let props = ProgressiveGenerator::extract_property_infos(&schema).unwrap();

        assert_eq!(props.len(), 2);

        // Find name property
        let name_prop = props.iter().find(|p| p.name == "name").unwrap();
        assert_eq!(name_prop.typescript_type, "string");
        assert_eq!(name_prop.description, Some("User name".to_string()));
        assert!(name_prop.required);

        // Find age property
        let age_prop = props.iter().find(|p| p.name == "age").unwrap();
        assert_eq!(age_prop.typescript_type, "number");
        assert!(!age_prop.required);
    }

    #[test]
    fn test_extract_property_infos_sanitizes_malicious_property_name() {
        let schema = json!({
            "type": "object",
            "properties": {
                "x: string }; export const pwned = 1; interface J {": {
                    "type": "string",
                    "description": "Evil property"
                }
            },
            "required": []
        });

        let props = ProgressiveGenerator::extract_property_infos(&schema).unwrap();

        assert_eq!(props.len(), 1);
        assert!(!props[0].name.contains(['{', '}', ';', ':', ' ']));
        // The description lookup must still succeed even though the property
        // name used for the lookup differs from the sanitized display name.
        assert_eq!(props[0].description, Some("Evil property".to_string()));
    }

    #[test]
    fn test_extract_property_infos_disambiguates_colliding_sibling_names() {
        // "a-b" and "a.b" both sanitize to "a_b"; since both become fields of the same
        // top-level `Params` interface, the collision must be disambiguated rather than
        // producing a duplicate, non-compiling field.
        let schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a.b": {"type": "number"}
            },
            "required": []
        });

        let props = ProgressiveGenerator::extract_property_infos(&schema).unwrap();
        let mut names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();

        assert_eq!(names, vec!["a_b", "a_b_2"]);
    }

    #[test]
    fn test_extract_property_infos_disambiguates_collision_introduced_by_collapsing() {
        // Before issue #192's collapsing fix, "a-b" -> "a_b" and "a--b" -> "a__b" were
        // distinct identifiers; collapsing consecutive invalid runs now sanitizes both to
        // "a_b", introducing a *new* collision that must still be disambiguated rather than
        // producing a duplicate, non-compiling field.
        let schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a--b": {"type": "number"}
            },
            "required": []
        });

        let props = ProgressiveGenerator::extract_property_infos(&schema).unwrap();
        let mut names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();

        assert_eq!(names, vec!["a_b", "a_b_2"]);
    }

    #[test]
    fn test_extract_property_infos_disambiguates_three_way_collision() {
        let schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a.b": {"type": "number"},
                "a b": {"type": "boolean"}
            },
            "required": []
        });

        let props = ProgressiveGenerator::extract_property_infos(&schema).unwrap();
        let mut names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();

        assert_eq!(names, vec!["a_b", "a_b_2", "a_b_3"]);
    }

    #[test]
    fn test_generate_disambiguates_colliding_top_level_params() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools[0].input_schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a.b": {"type": "number"}
            },
            "required": []
        });

        let code = generator.generate(&server_info).unwrap();
        let tool = code
            .files
            .iter()
            .find(|f| f.path == "createIssue.ts")
            .unwrap();

        assert_eq!(
            tool.content.matches("a_b:").count() + tool.content.matches("a_b?:").count(),
            1,
            "field 'a_b' must appear exactly once in the Params interface: {}",
            tool.content
        );
        assert_eq!(
            tool.content.matches("a_b_2:").count() + tool.content.matches("a_b_2?:").count(),
            1,
            "disambiguated field 'a_b_2' must appear exactly once in the Params interface: {}",
            tool.content
        );
    }

    #[test]
    fn test_generate_sanitizes_property_name_injection() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools[0].input_schema = json!({
            "type": "object",
            "properties": {
                "x: string }; export const pwned = evil(); interface J {": {"type": "string"}
            },
            "required": []
        });

        let code = generator.generate(&server_info).unwrap();
        let tool = code
            .files
            .iter()
            .find(|f| f.path == "createIssue.ts")
            .unwrap();

        assert!(
            !tool.content.contains("export const pwned"),
            "raw property name must not inject a top-level statement: {}",
            tool.content
        );
    }

    #[test]
    fn test_sanitize_jsdoc_strips_comment_terminator() {
        assert_eq!(sanitize_jsdoc("Foo */ bar", 256), "Foo *\\/ bar");
    }

    #[test]
    fn test_sanitize_jsdoc_replaces_newlines() {
        assert_eq!(
            sanitize_jsdoc("line1\nline2\r\nline3", 256),
            "line1 line2  line3"
        );
    }

    #[test]
    fn test_sanitize_jsdoc_replaces_unicode_line_terminators() {
        // U+2028 (LINE SEPARATOR) and U+2029 (PARAGRAPH SEPARATOR) are treated as line
        // terminators by ECMAScript, so they terminate a `//` line comment (e.g.
        // `index.ts.hbs`'s `// --- {{name}} ---` category header) even though they are not
        // ASCII `\r`/`\n` and were never covered by Handlebars' HTML-escaping either.
        assert_eq!(
            sanitize_jsdoc("line1\u{2028}line2\u{2029}line3", 256),
            "line1 line2 line3"
        );
    }

    #[test]
    fn test_generate_with_categories_sanitizes_unicode_line_terminator_in_category() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let mut categorizations = HashMap::new();
        categorizations.insert(
            "create_issue".to_string(),
            ToolCategorization {
                category: "issues\u{2028}export const pwned = 1;".to_string(),
                keywords: vec![],
                short_description: "Create a new issue".to_string(),
            },
        );

        let code = generator
            .generate_with_categories(&server_info, &categorizations)
            .unwrap();
        let index = code.files.iter().find(|f| f.path == "index.ts").unwrap();

        // The sanitized category text (including the literal "export const pwned = 1;"
        // substring) is expected to still appear, harmlessly, inside the `// --- ... ---`
        // comment. What must NOT happen is U+2028 terminating that comment early and
        // letting "export const pwned" start a fresh, live top-level statement.
        assert!(
            index
                .content
                .contains("// --- issues export const pwned = 1; ---"),
            "sanitized category text should remain inert inside the comment: {}",
            index.content
        );
        assert!(
            !index.content.contains("\nexport const pwned"),
            "U+2028 must not terminate the `// --- {{category}} ---` line comment and \
             inject a live top-level statement: {}",
            index.content
        );
    }

    #[test]
    fn test_sanitize_jsdoc_truncates() {
        let long = "a".repeat(300);
        assert_eq!(sanitize_jsdoc(&long, 256).chars().count(), 256);
    }

    #[test]
    fn test_sanitize_jsdoc_passthrough() {
        assert_eq!(sanitize_jsdoc("Normal string", 256), "Normal string");
    }

    #[test]
    fn test_sanitize_jsdoc_strips_ansi_escape_sequence() {
        // Issue #300: this project's documented workflow is `cat` on the generated `.ts`
        // file, so a raw ESC (`\x1b`)-led ANSI escape sequence in a tool description must
        // not survive into the JSDoc comment verbatim. The control character becomes a
        // space (not deleted), so adjacent text is not glued together.
        let payload = "Innocuous \x1b[31mred text\x1b[0m looking description";
        let sanitized = sanitize_jsdoc(payload, 256);
        assert!(!sanitized.contains('\x1b'));
        assert_eq!(sanitized, "Innocuous  [31mred text [0m looking description");
    }

    #[test]
    fn test_sanitize_jsdoc_replaces_other_c0_control_characters_with_space() {
        assert_eq!(sanitize_jsdoc("a\u{0}b\u{7}c\u{7f}d", 256), "a b c d");
    }

    /// Critic C1 regression: a control character sitting directly between `*` and `/`
    /// must not survive as a live `JSDoc` comment terminator. The old (buggy) order
    /// escaped `*/` before neutralizing control characters, so `*\u{0}/` had no `*/`
    /// substring at escape time, then the control character was deleted, reconstituting
    /// a bare `*/` that closed the comment early and let the trailing text become live
    /// top-level TypeScript.
    #[test]
    fn test_sanitize_jsdoc_control_char_between_star_slash_cannot_reopen_comment() {
        for ctrl in ['\u{0}', '\u{7f}', '\u{1b}'] {
            let payload = format!("safe *{ctrl}/ export const pwned = 1; //");
            let sanitized = sanitize_jsdoc(&payload, 256);
            assert!(
                !sanitized.contains("*/"),
                "control char {ctrl:?} must not let a bare `*/` reappear: {sanitized:?}"
            );
            assert!(
                sanitized.contains("* /"),
                "the control char should be neutralized to a space, not deleted: {sanitized:?}"
            );
        }
    }

    #[test]
    fn test_sanitize_ts_string_literal_escapes_quote_and_backslash() {
        assert_eq!(
            sanitize_ts_string_literal(r"it's a \test"),
            r"it\'s a \\test"
        );
    }

    #[test]
    fn test_sanitize_ts_string_literal_escape_order_prevents_double_escaping() {
        // A trailing backslash followed by a quote must not become `\\\'`
        // (which would re-open the string); backslash escaping happens first.
        assert_eq!(sanitize_ts_string_literal("\\'"), r"\\\'");
    }

    #[test]
    fn test_sanitize_ts_string_literal_escapes_newlines() {
        assert_eq!(
            sanitize_ts_string_literal("line1\nline2\rline3"),
            "line1\\nline2\\rline3"
        );
    }

    #[test]
    fn test_sanitize_ts_string_literal_escapes_unicode_line_terminators() {
        // U+2028/U+2029 are legal-but-unescaped inside a string literal only since ES2019;
        // a raw occurrence is a syntax error for consumers targeting an older ES target.
        assert_eq!(
            sanitize_ts_string_literal("line1\u{2028}line2\u{2029}line3"),
            "line1\\u2028line2\\u2029line3"
        );
    }

    // `sanitize_ts_identifier`'s core behavior (invalid-char replacement, leading-digit
    // and empty-string prefixing) is unit-tested in `common::typescript`, its canonical
    // home now that it's a shared `pub fn`; this test covers the passthrough case that's
    // specific to how this module uses it (already-valid camelCase tool names).
    #[test]
    fn test_sanitize_ts_identifier_passthrough_valid() {
        assert_eq!(sanitize_ts_identifier("sendMessage_1"), "sendMessage_1");
    }

    #[test]
    fn test_generate_sanitizes_call_site_string_literal_injection() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools[0].name = ToolName::new("create_issue'); alert('pwned").unwrap();

        let code = generator.generate(&server_info).unwrap();
        let tool = code
            .files
            .iter()
            .find(|f| {
                std::path::Path::new(&f.path)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
                    && f.path != "index.ts"
            })
            .unwrap();

        // Scoped to the `callMCPTool(...)` call site itself: the JSDoc header above it
        // legitimately echoes the raw tool name inside a `/** */` block comment (quotes
        // there are inert, not code), so a whole-file substring check would false-positive
        // on that comment. The security property under test is that `name_literal`
        // (escaped via `sanitize_ts_string_literal`) can't break out of the single-quoted
        // string literal actually passed to `callMCPTool`.
        // Matches the actual invocation (`return (await callMCPTool(...`) rather than any
        // line merely containing "callMCPTool(" — a future `@example` line in the JSDoc
        // above (which legitimately shows `callMCPTool(...)` usage in prose) would otherwise
        // silently redirect this assertion to the wrong line.
        let call_site_line = tool
            .content
            .lines()
            .find(|line| line.contains("return (await callMCPTool("))
            .expect("generated tool file must contain a callMCPTool(...) invocation");
        assert!(
            !call_site_line.contains("'); alert('pwned"),
            "raw quote must not break out of the callMCPTool string literal: {call_site_line}"
        );
    }

    #[test]
    fn test_resolve_typescript_names_disambiguates_collisions() {
        let tools = vec![
            ToolInfo {
                name: ToolName::new("foo-bar").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("foo.bar").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("foo bar").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
        ];

        let resolved = resolve_typescript_names(&tools);
        let mut names: Vec<&String> = resolved.iter().collect();
        names.sort();

        // All three distinct tool names must resolve to distinct identifiers.
        assert_eq!(resolved.len(), 3);
        let unique: HashSet<&String> = names.iter().copied().collect();
        assert_eq!(
            unique.len(),
            3,
            "collisions must be disambiguated: {names:?}"
        );
        assert_eq!(resolved[0], "foo_bar");
    }

    #[test]
    fn test_resolve_typescript_names_disambiguates_identical_raw_names() {
        // Two tools with the exact same raw name are invalid per the MCP spec but must
        // not be rejected upstream; each must still get a distinct resolved identifier
        // instead of one silently losing its slot in a raw-name-keyed map.
        let tools = vec![
            ToolInfo {
                name: ToolName::new("dup").unwrap(),
                description: "First".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("dup").unwrap(),
                description: "Second".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
        ];

        let resolved = resolve_typescript_names(&tools);

        assert_eq!(resolved, vec!["dup".to_string(), "dup_2".to_string()]);
    }

    #[test]
    fn test_resolve_typescript_names_disambiguates_three_way_identical_raw_names() {
        let tools: Vec<ToolInfo> = (0..3)
            .map(|_| ToolInfo {
                name: ToolName::new("dup").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            })
            .collect();

        let resolved = resolve_typescript_names(&tools);

        assert_eq!(
            resolved,
            vec!["dup".to_string(), "dup_2".to_string(), "dup_3".to_string()]
        );
    }

    #[test]
    fn test_resolve_typescript_names_disambiguates_reserved_words() {
        let reserved_tool_names = [
            "delete",
            "typeof",
            "class",
            "new",
            "import",
            "export",
            "in",
            "instanceof",
            "void",
            "enum",
            "eval",
            "arguments",
        ];

        for name in reserved_tool_names {
            let tools = vec![ToolInfo {
                name: ToolName::new(name).unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            }];

            let resolved = resolve_typescript_names(&tools);
            let typescript_name = &resolved[0];

            assert_ne!(
                typescript_name, name,
                "reserved word {name} must be disambiguated"
            );
            assert!(
                !RESERVED_WORDS.contains(&typescript_name.as_str()),
                "resolved name {typescript_name} for tool {name} must not be a reserved word"
            );
        }
    }

    #[test]
    fn test_resolve_typescript_names_reserved_word_avoids_existing_collision() {
        let tools = vec![
            ToolInfo {
                name: ToolName::new("class").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
            // Must be a hyphen, not an underscore: `to_camel_case` only acts on `_` (it
            // capitalizes the following character and drops the underscore), so a raw name
            // of "class_2" would sanitize to "class2", never colliding with the "class"
            // tool's reserved-word fallback "class_2" and making this test vacuous.
            // `sanitize_ts_identifier` replaces the hyphen in "class-2" with "_" verbatim
            // (untouched by `to_camel_case`), so it genuinely sanitizes to the literal
            // identifier "class_2", producing a real collision to test against.
            ToolInfo {
                name: ToolName::new("class-2").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
        ];

        let resolved = resolve_typescript_names(&tools);

        assert_ne!(
            resolved[0], resolved[1],
            "a reserved-word tool's fallback name must not collide with an unrelated tool that already claims it"
        );
        assert!(!RESERVED_WORDS.contains(&resolved[0].as_str()));
    }

    #[test]
    fn test_resolve_typescript_names_reserved_word_case_variant_is_not_suffixed() {
        // Issue #320: JS/TS reserved words are reserved only in their exact lowercase form —
        // `Delete`, `New`, `Import` are all legal identifiers — so a tool named `Delete` must
        // not be treated as colliding with the reserved word `delete`.
        let tools = vec![ToolInfo {
            name: ToolName::new("Delete").unwrap(),
            description: String::new(),
            input_schema: json!({}),
            output_schema: None,
        }];

        let resolved = resolve_typescript_names(&tools);

        assert_eq!(resolved[0], "Delete");
    }

    #[test]
    fn test_resolve_typescript_names_exact_reserved_word_is_still_suffixed() {
        let tools = vec![ToolInfo {
            name: ToolName::new("delete").unwrap(),
            description: String::new(),
            input_schema: json!({}),
            output_schema: None,
        }];

        let resolved = resolve_typescript_names(&tools);

        assert_ne!(resolved[0], "delete");
        assert!(!RESERVED_WORDS.contains(&resolved[0].as_str()));
    }

    #[test]
    fn test_resolve_typescript_names_delete_and_delete_case_variant_in_same_batch() {
        // Issue #320 follow-up: `delete` and `Delete` in the same tool list must both resolve
        // to distinct names regardless of order. A reserved-word candidate is rejected via
        // `&&` short-circuit before it ever claims its lowercase slot in `used_lower`
        // (`disambiguate_output_filename`), so `delete` being suffixed away must not block a
        // later `Delete` from keeping its unsuffixed name (or vice versa).
        let make_tools = |first: &str, second: &str| {
            vec![
                ToolInfo {
                    name: ToolName::new(first).unwrap(),
                    description: String::new(),
                    input_schema: json!({}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new(second).unwrap(),
                    description: String::new(),
                    input_schema: json!({}),
                    output_schema: None,
                },
            ]
        };

        let delete_first = resolve_typescript_names(&make_tools("delete", "Delete"));
        assert_ne!(delete_first[0], "delete");
        assert_eq!(delete_first[1], "Delete");
        assert_ne!(
            delete_first[0].to_ascii_lowercase(),
            delete_first[1].to_ascii_lowercase()
        );

        let delete_second = resolve_typescript_names(&make_tools("Delete", "delete"));
        assert_eq!(delete_second[0], "Delete");
        assert_ne!(delete_second[1], "delete");
        assert_ne!(
            delete_second[0].to_ascii_lowercase(),
            delete_second[1].to_ascii_lowercase()
        );
    }

    #[test]
    fn test_resolve_typescript_names_collapses_non_ascii_run() {
        // Issue #192: a non-ASCII tool name used to produce one `_` per invalid character
        // (`café_menu_日本語` -> `caf_Menu___`), losing more information than necessary.
        let tools = vec![ToolInfo {
            name: ToolName::new("café_menu_日本語").unwrap(),
            description: String::new(),
            input_schema: json!({}),
            output_schema: None,
        }];

        let resolved = resolve_typescript_names(&tools);

        assert_eq!(resolved[0], "caf_Menu_");
    }

    #[test]
    fn test_resolve_typescript_names_disambiguates_collision_introduced_by_collapsing() {
        // Same collapsing-introduced collision as
        // `test_extract_property_infos_disambiguates_collision_introduced_by_collapsing`, but
        // for tool names rather than property names: "a-b" and "a--b" used to sanitize to
        // distinct identifiers and now both sanitize to "a_b".
        let tools = vec![
            ToolInfo {
                name: ToolName::new("a-b").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("a--b").unwrap(),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
        ];

        let resolved = resolve_typescript_names(&tools);

        assert_eq!(resolved, vec!["a_b", "a_b_2"]);
    }

    #[test]
    fn test_generate_sanitizes_reserved_word_tool_name() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools = vec![ToolInfo {
            name: ToolName::new("delete").unwrap(),
            description: "Delete something".to_string(),
            input_schema: json!({}),
            output_schema: None,
        }];

        let code = generator.generate(&server_info).unwrap();
        let tool_file = code.files.iter().find(|f| f.path == "delete_2.ts").unwrap();

        assert!(!tool_file.content.contains("export async function delete("));
        assert!(
            tool_file
                .content
                .contains("export async function delete_2(")
        );
    }

    #[test]
    fn test_generate_disambiguates_colliding_tool_names() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools = vec![
            ToolInfo {
                name: ToolName::new("foo-bar").unwrap(),
                description: "First".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("foo.bar").unwrap(),
                description: "Second".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
        ];

        let code = generator.generate(&server_info).unwrap();

        // Both tools must produce distinct files: no silent overwrite.
        let tool_files: Vec<&str> = code
            .files
            .iter()
            .filter(|f| f.path == "foo_bar.ts" || f.path == "foo_bar_2.ts")
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(
            tool_files.len(),
            2,
            "colliding names must not overwrite each other's file: {tool_files:?}"
        );

        let index = code.files.iter().find(|f| f.path == "index.ts").unwrap();
        assert_eq!(
            index.content.matches("export { foo_bar,").count(),
            1,
            "index.ts must export the first tool's identifier exactly once"
        );
        assert_eq!(
            index.content.matches("export { foo_bar_2,").count(),
            1,
            "index.ts must export the disambiguated second identifier exactly once"
        );
    }

    #[test]
    fn test_generate_disambiguates_identical_raw_tool_names() {
        // An MCP server reporting two tools with the exact same raw `name` is invalid per
        // spec but is not currently rejected upstream; generation must not let the second
        // tool silently overwrite the first tool's file.
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools = vec![
            ToolInfo {
                name: ToolName::new("dup").unwrap(),
                description: "First".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("dup").unwrap(),
                description: "Second".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
        ];

        let code = generator.generate(&server_info).unwrap();

        let dup_files: Vec<&str> = code
            .files
            .iter()
            .filter(|f| f.path == "dup.ts" || f.path == "dup_2.ts")
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(
            dup_files.len(),
            2,
            "identical raw tool names must not overwrite each other's file: {dup_files:?}"
        );

        let index = code.files.iter().find(|f| f.path == "index.ts").unwrap();
        assert_eq!(
            index.content.matches("export { dup,").count(),
            1,
            "index.ts must export the first tool's identifier exactly once"
        );
        assert_eq!(
            index.content.matches("export { dup_2,").count(),
            1,
            "index.ts must export the disambiguated second identifier exactly once"
        );
    }

    #[test]
    fn test_sanitize_schema_jsdoc_drops_non_string_descriptions() {
        let sanitized = sanitize_schema_jsdoc_descriptions(json!({
            "type": "object",
            "description": {"text": "Schema */ injected\nnext"},
            "properties": {
                "title": {
                    "type": "string",
                    "description": ["Title */ injected\nnext"]
                }
            }
        }));

        assert!(sanitized["description"].is_null());
        assert!(sanitized["properties"]["title"]["description"].is_null());
    }

    #[test]
    fn test_sanitize_schema_jsdoc_recurses_into_array_items() {
        let sanitized = sanitize_schema_jsdoc_descriptions(json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": [
                        {
                            "type": "string",
                            "description": "Tag */ injected\nnext"
                        }
                    ]
                }
            }
        }));

        let description = sanitized["properties"]["tags"]["items"][0]["description"]
            .as_str()
            .unwrap();

        assert_eq!(description, "Tag *\\/ injected next");
    }

    /// Builds a schema with `depth` nested `type: "array"` levels, each carrying its own
    /// `description`, wrapping a `string` leaf that also has a `description`.
    ///
    /// Assembled directly via `serde_json::Map`/`Value` (like
    /// `typescript::tests::nested_array_schema`) so building this fixture can't itself
    /// overflow the stack, and using `"array"`/`"items"` rather than `"object"`/`"properties"`
    /// so each `depth` step costs exactly one recursion level in
    /// [`sanitize_schema_jsdoc_value`] — unlike an `"object"`/`"properties"`-nested schema,
    /// where the intermediate `properties` map itself consumes a level (see
    /// [`sanitize_schema_jsdoc_value`]'s doc comment) — making it possible to predict exactly
    /// which nesting level lands on either side of [`MAX_SCHEMA_RECURSION_DEPTH`].
    fn nested_array_schema_with_descriptions(depth: usize, description: &str) -> serde_json::Value {
        let mut schema = json!({"type": "string", "description": description});
        for _ in 0..depth {
            let mut map = serde_json::Map::new();
            map.insert(
                "type".to_string(),
                serde_json::Value::String("array".to_string()),
            );
            map.insert("items".to_string(), schema);
            map.insert(
                "description".to_string(),
                serde_json::Value::String(description.to_string()),
            );
            schema = serde_json::Value::Object(map);
        }
        schema
    }

    /// Walks `depth` `"items"` hops into `value`, returning the schema found there.
    fn nth_level(value: &serde_json::Value, depth: usize) -> serde_json::Value {
        let mut v = value.clone();
        for _ in 0..depth {
            v = v["items"].clone();
        }
        v
    }

    #[test]
    fn test_sanitize_schema_jsdoc_bounds_deeply_nested_schema() {
        // Issue #303: `input_schema` is attacker-controlled and this sanitizer used to
        // recurse into every nested object/array with no depth limit; this is
        // defense-in-depth for the pub API surface (see `MAX_SCHEMA_RECURSION_DEPTH`'s docs
        // in `typescript.rs`), not a fix for a wire-reachable schema. Asserts the cap's actual
        // observable effect rather than just "doesn't panic": a `description` one level below
        // the cap is still sanitized, while one at the cap is left untouched because the
        // function stops recursing before ever reaching it.
        const MALICIOUS: &str = "desc */ injected\nnext";
        let depth = MAX_SCHEMA_RECURSION_DEPTH + 10;
        let schema = nested_array_schema_with_descriptions(depth, MALICIOUS);

        let sanitized = sanitize_schema_jsdoc_descriptions(schema);

        let just_below_cap = nth_level(&sanitized, MAX_SCHEMA_RECURSION_DEPTH - 1);
        let below_description = just_below_cap["description"]
            .as_str()
            .expect("description below the cap must still be a string");
        assert!(
            !below_description.contains("*/"),
            "description one level below the cap must be sanitized: {below_description}"
        );

        let at_cap = nth_level(&sanitized, MAX_SCHEMA_RECURSION_DEPTH);
        let at_cap_description = at_cap["description"]
            .as_str()
            .expect("description at the cap must still be a string");
        assert_eq!(
            at_cap_description, MALICIOUS,
            "description at the cap must be left untouched — the function must stop \
             recursing before reaching it"
        );
    }

    #[test]
    fn test_sanitize_schema_jsdoc_survives_pathologically_deep_input() {
        // Issue #303: a caller of this crate's `pub` API could hand it a `Value` nested far
        // beyond any depth reachable via introspection (see `MAX_SCHEMA_RECURSION_DEPTH`'s
        // docs). Must not stack overflow at 5,000+ levels, however that `Value` was built.
        //
        // Runs on a dedicated large-stack thread because `serde_json::Value`'s own `Drop` is
        // recursive and unrelated to this fix: dropping a sufficiently deep `Value` overflows
        // the *default* thread stack merely by going out of scope, regardless of how it was
        // traversed beforehand — see `typescript::tests::run_on_large_stack` for the full
        // rationale.
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let schema = nested_array_schema_with_descriptions(5_000, "leaf");
                let sanitized = sanitize_schema_jsdoc_descriptions(schema);
                assert!(sanitized.is_object());
            })
            .expect("spawn test thread")
            .join()
            .expect("test thread panicked");
    }

    #[test]
    fn test_sanitize_jsdoc_truncation_boundary_injection() {
        let max_len = 256;
        // Place the "*/" pair straddling the max_len boundary: '*' is the
        // max_len-th character and '/' is the very next one, so a naive
        // truncate-then-check could see the split land between them.
        let payload = format!("{}*/{}", "a".repeat(max_len - 1), "trailer");

        let sanitized = sanitize_jsdoc(&payload, max_len);

        assert!(
            !sanitized.contains("*/"),
            "truncation must not re-open the JSDoc comment: {sanitized}"
        );
        assert_eq!(sanitized.chars().count(), max_len);
    }

    /// #221 item 3 — real drift guard: reads the names from
    /// `mcp_execution_core::forbidden_env_names()` directly (not a hardcoded second copy),
    /// so a future addition/removal in `command.rs`'s `FORBIDDEN_ENV_NAMES` is picked up here
    /// automatically. Passing is guaranteed by construction (`BridgeContext`'s hand-written
    /// `Default` impl renders from that same accessor), which is the point: since the rendered
    /// TypeScript literal is generated from the Rust constant rather than hand-copied, the two
    /// can no longer silently desynchronize the way a hand-maintained second copy could.
    ///
    /// This does NOT prove the validator is reachable or enforced at runtime — a
    /// `grep`-style assertion can pass even against dead code (e.g. an unreachable function,
    /// or one whose result is never checked). The actual behavioral regression guard for
    /// #201 — that a hostile `~/.claude/mcp.json` is rejected before any subprocess is
    /// spawned — lives in `crates/mcp-codegen/tests/progressive_generation.rs`
    /// (`test_runtime_bridge_rejects_forbidden_env_var_before_spawn`), which compiles and
    /// actually executes the rendered bridge under Node.
    #[test]
    fn test_generate_runtime_bridge_declares_forbidden_env_var_list() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();
        let bridge = code
            .files
            .iter()
            .find(|f| f.path == "_runtime/mcp-bridge.ts")
            .unwrap();

        for forbidden_env in mcp_execution_core::forbidden_env_names() {
            assert!(
                bridge.content.contains(&format!("'{forbidden_env}'")),
                "runtime bridge must list forbidden env var {forbidden_env}: {}",
                bridge.content
            );
        }

        for forbidden_char in mcp_execution_core::forbidden_chars() {
            let escaped = sanitize_ts_string_literal(&forbidden_char.to_string());
            assert!(
                bridge.content.contains(&format!("'{escaped}'")),
                "runtime bridge must list forbidden char {forbidden_char:?}: {}",
                bridge.content
            );
        }

        assert!(
            bridge
                .content
                .contains(mcp_execution_core::forbidden_env_prefix()),
            "runtime bridge must reference the forbidden env prefix: {}",
            bridge.content
        );
    }

    #[test]
    fn test_generate_preserves_benign_punctuation() {
        // Issue #204 regression: Handlebars' default HTML-escaping corrupted benign
        // punctuation (apostrophes, ampersands, comparison operators) in JSDoc comments,
        // turning e.g. `don't` into `don&#x27;t` or `a < b` into `a &lt; b`.
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools[0].description =
            "Compares values: a < b && b > c, or use \"quotes\" & don't forget 'em".to_string();

        let code = generator.generate(&server_info).unwrap();
        let tool = code
            .files
            .iter()
            .find(|f| f.path == "createIssue.ts")
            .unwrap();

        assert!(
            tool.content
                .contains("a < b && b > c, or use \"quotes\" & don't forget 'em"),
            "benign punctuation must survive verbatim, not be HTML-escaped: {}",
            tool.content
        );
        for entity in ["&lt;", "&gt;", "&amp;", "&quot;", "&#x27;", "&#39;"] {
            assert!(
                !tool.content.contains(entity),
                "output must not contain HTML entity {entity}: {}",
                tool.content
            );
        }
    }

    #[test]
    fn test_generate_sanitizes_jsdoc_injection() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.name = "Evil */ injection".to_string();
        server_info.version = "1.0\n<script>".to_string();

        let code = generator.generate(&server_info).unwrap();
        let index = code.files.iter().find(|f| f.path == "index.ts").unwrap();

        // Raw injected strings must not appear in the output.
        assert!(
            !index.content.contains("Evil */ injection"),
            "Server name should be sanitized in JSDoc"
        );
        assert!(
            !index.content.contains("1.0\n<script>"),
            "Server version should have newlines stripped"
        );
    }

    #[test]
    fn test_generate_sanitizes_schema_and_category_jsdoc_injection() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools[0].input_schema = json!({
            "type": "object",
            "description": "Schema */ injected\nnext",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Title */ injected\nnext"
                }
            },
            "required": ["title"]
        });

        let mut categorizations = HashMap::new();
        categorizations.insert(
            "create_issue".to_string(),
            ToolCategorization {
                category: "issues */ injected\nnext".to_string(),
                keywords: vec!["create,*/ injected\nnext".to_string()],
                short_description: "Create */ injected\nnext".to_string(),
            },
        );

        let code = generator
            .generate_with_categories(&server_info, &categorizations)
            .unwrap();
        let tool = code
            .files
            .iter()
            .find(|f| f.path == "createIssue.ts")
            .unwrap();

        for raw in [
            "Schema */ injected",
            "Title */ injected",
            "issues */ injected",
            "create,*/ injected",
            "Create */ injected",
        ] {
            assert!(
                !tool.content.contains(raw),
                "generated JSDoc should not contain raw injection text: {raw}"
            );
        }

        assert!(tool.content.contains("Schema *\\/ injected next"));
        assert!(tool.content.contains("Title *\\/ injected next"));
        assert!(tool.content.contains("issues *\\/ injected next"));
        assert!(tool.content.contains("create,*\\/ injected next"));
        assert!(tool.content.contains("Create *\\/ injected next"));
    }

    // ── Resource-exhaustion bounds (issue #198) ──────────────────────────────

    fn server_info_with_tool_count(count: usize) -> ServerInfo {
        ServerInfo {
            id: ServerId::new("bulk-server").unwrap(),
            name: "Bulk Server".to_string(),
            version: "1.0.0".to_string(),
            tools: (0..count)
                .map(|i| ToolInfo {
                    name: ToolName::new(format!("tool{i}")).unwrap(),
                    description: String::new(),
                    input_schema: json!({}),
                    output_schema: None,
                })
                .collect(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        }
    }

    #[test]
    fn test_generate_rejects_tool_count_that_would_exceed_max_generated_files() {
        let server_info = server_info_with_tool_count(MAX_GENERATED_FILES - FIXED_FILE_COUNT + 1);
        let generator = ProgressiveGenerator::new().unwrap();

        let result = generator.generate(&server_info);

        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_generate_accepts_tool_count_at_exact_max_generated_files() {
        let server_info = server_info_with_tool_count(MAX_GENERATED_FILES - FIXED_FILE_COUNT);
        let generator = ProgressiveGenerator::new().unwrap();

        let code = generator.generate(&server_info).unwrap();

        assert_eq!(code.file_count(), MAX_GENERATED_FILES);
    }

    #[test]
    fn test_generate_with_categories_rejects_tool_count_that_would_exceed_max_generated_files() {
        let server_info = server_info_with_tool_count(MAX_GENERATED_FILES - FIXED_FILE_COUNT + 1);
        let generator = ProgressiveGenerator::new().unwrap();

        let result = generator.generate_with_categories(&server_info, &HashMap::new());

        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_add_tracked_rejects_oversized_total_bytes() {
        let mut code = GeneratedCode::new();
        let mut total_bytes = 0usize;

        let result = add_tracked(
            &mut code,
            &mut total_bytes,
            GeneratedFile {
                path: "big.ts".to_string(),
                content: "a".repeat(MAX_GENERATED_BYTES + 1),
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_add_tracked_accepts_total_bytes_at_exact_max() {
        let mut code = GeneratedCode::new();
        let mut total_bytes = 0usize;

        let result = add_tracked(
            &mut code,
            &mut total_bytes,
            GeneratedFile {
                path: "big.ts".to_string(),
                content: "a".repeat(MAX_GENERATED_BYTES),
            },
        );

        assert!(result.is_ok());
    }

    /// #198 M8 — proves `generate()` itself (not just the private `add_tracked` helper)
    /// enforces the byte budget, using a `total_bytes` starting point injected just below the
    /// cap rather than materializing a real `MAX_GENERATED_BYTES`-sized (now several hundred
    /// MB, after the M1 fix ties it to `mcp_execution_introspector`'s own bounds) `ServerInfo`
    /// — which would make this test itself a slow, wasteful multi-hundred-MB allocation for
    /// every CI run without proving anything `add_tracked`'s direct boundary tests don't
    /// already cover more precisely.
    #[test]
    fn test_add_tracked_rejects_immediately_once_running_total_exceeds_max() {
        let mut code = GeneratedCode::new();
        let mut total_bytes = MAX_GENERATED_BYTES - 1;

        let result = add_tracked(
            &mut code,
            &mut total_bytes,
            GeneratedFile {
                path: "second.ts".to_string(),
                content: "ab".to_string(),
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
        // The offending file must not have been added — the caller should not be able to
        // observe a `GeneratedCode` that already exceeds the bound.
        assert_eq!(code.file_count(), 0);
    }
}
