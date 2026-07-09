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
//! let server_id = ServerId::new("github");
//! let config = ServerConfig::builder().command("/path/to/server".to_string()).build();
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
    disambiguate_identifier, extract_properties, sanitize_ts_identifier, to_camel_case,
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

impl<'a> ProgressiveGenerator<'a> {
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
    ///     id: ServerId::new("github"),
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
    /// // - one file per tool
    /// println!("Generated {} files", code.file_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate(&self, server_info: &ServerInfo) -> Result<GeneratedCode> {
        tracing::info!(
            "Generating progressive loading code for server: {}",
            server_info.name
        );

        let mut code = GeneratedCode::new();
        let server_id = server_info.id.as_str();
        let typescript_names = resolve_typescript_names(&server_info.tools);
        let mut tool_metadata = Vec::with_capacity(server_info.tools.len());

        // Generate tool files (one per tool)
        for (idx, tool) in server_info.tools.iter().enumerate() {
            let typescript_name = typescript_names.get(idx).cloned().unwrap_or_default();
            let tool_context =
                self.create_tool_context(server_id, tool, None, typescript_name.clone())?;
            let tool_code = self.engine.render("progressive/tool", &tool_context)?;

            code.add_file(GeneratedFile {
                path: format!("{}.ts", tool_context.typescript_name),
                content: tool_code,
            });

            tracing::debug!("Generated tool file: {}.ts", tool_context.typescript_name);

            tool_metadata.push(self.create_tool_metadata(tool, None, typescript_name)?);
        }

        // Generate index.ts
        let index_context = self.create_index_context(server_info, None, &typescript_names)?;
        let index_code = self.engine.render("progressive/index", &index_context)?;

        code.add_file(GeneratedFile {
            path: "index.ts".to_string(),
            content: index_code,
        });

        tracing::debug!("Generated index.ts");

        // Generate runtime bridge
        let bridge_context = BridgeContext::default();
        let bridge_code = self
            .engine
            .render("progressive/runtime-bridge", &bridge_context)?;

        code.add_file(GeneratedFile {
            path: "_runtime/mcp-bridge.ts".to_string(),
            content: bridge_code,
        });

        tracing::debug!("Generated _runtime/mcp-bridge.ts");

        // Generate package.json for ES module identification
        code.add_file(GeneratedFile {
            path: "package.json".to_string(),
            content: "{\"type\":\"module\"}\n".to_string(),
        });

        tracing::debug!("Generated package.json");

        // Generate _meta.json sidecar with structured tool metadata
        code.add_file(Self::create_metadata_file(server_info, tool_metadata)?);

        tracing::debug!("Generated {}", METADATA_FILE_NAME);

        tracing::info!(
            "Successfully generated {} files for {} (progressive loading)",
            code.file_count(),
            server_info.name
        );

        Ok(code)
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
    ///     id: ServerId::new("github"),
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
    ///     keywords: "create,issue,new,bug".to_string(),
    ///     short_description: "Create a new issue".to_string(),
    /// });
    ///
    /// let code = generator.generate_with_categories(&info, &categorizations)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_with_categories(
        &self,
        server_info: &ServerInfo,
        categorizations: &HashMap<String, ToolCategorization>,
    ) -> Result<GeneratedCode> {
        tracing::info!(
            "Generating progressive loading code with categorizations for server: {}",
            server_info.name
        );

        let mut code = GeneratedCode::new();
        let server_id = server_info.id.as_str();
        let typescript_names = resolve_typescript_names(&server_info.tools);
        let mut tool_metadata = Vec::with_capacity(server_info.tools.len());

        // Generate tool files (one per tool) with categorization metadata
        for (idx, tool) in server_info.tools.iter().enumerate() {
            let tool_name = tool.name.as_str();
            let categorization = categorizations.get(tool_name);
            let typescript_name = typescript_names.get(idx).cloned().unwrap_or_default();
            let tool_context =
                self.create_tool_context(server_id, tool, categorization, typescript_name.clone())?;
            let tool_code = self.engine.render("progressive/tool", &tool_context)?;

            code.add_file(GeneratedFile {
                path: format!("{}.ts", tool_context.typescript_name),
                content: tool_code,
            });

            tracing::debug!(
                "Generated tool file: {}.ts (category: {:?})",
                tool_context.typescript_name,
                categorization.map(|c| &c.category)
            );

            tool_metadata.push(self.create_tool_metadata(tool, categorization, typescript_name)?);
        }

        // Generate index.ts with category grouping
        let index_context =
            self.create_index_context(server_info, Some(categorizations), &typescript_names)?;
        let index_code = self.engine.render("progressive/index", &index_context)?;

        code.add_file(GeneratedFile {
            path: "index.ts".to_string(),
            content: index_code,
        });

        tracing::debug!(
            "Generated index.ts with {} categorizations",
            categorizations.len()
        );

        // Generate runtime bridge (same as non-categorized)
        let bridge_context = BridgeContext::default();
        let bridge_code = self
            .engine
            .render("progressive/runtime-bridge", &bridge_context)?;

        code.add_file(GeneratedFile {
            path: "_runtime/mcp-bridge.ts".to_string(),
            content: bridge_code,
        });

        tracing::debug!("Generated _runtime/mcp-bridge.ts");

        // Generate package.json for ES module identification
        code.add_file(GeneratedFile {
            path: "package.json".to_string(),
            content: "{\"type\":\"module\"}\n".to_string(),
        });

        tracing::debug!("Generated package.json");

        // Generate _meta.json sidecar with structured tool metadata
        code.add_file(Self::create_metadata_file(server_info, tool_metadata)?);

        tracing::debug!("Generated {}", METADATA_FILE_NAME);

        tracing::info!(
            "Successfully generated {} files for {} with categorizations (progressive loading)",
            code.file_count(),
            server_info.name
        );

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
    /// # Errors
    ///
    /// Returns error if schema conversion fails.
    fn create_tool_context(
        &self,
        server_id: &str,
        tool: &mcp_execution_introspector::ToolInfo,
        categorization: Option<&ToolCategorization>,
        typescript_name: String,
    ) -> Result<ToolContext> {
        // Extract properties from input schema
        let properties = self.extract_property_infos(&tool.input_schema)?;

        let description = sanitize_jsdoc(&tool.description, 256);
        // Falls back to the tool's own description when no LLM categorization is
        // available, so the header JSDoc always emits `@description` (issue #94).
        let short_description = Some(categorization.map_or_else(
            || description.clone(),
            |c| sanitize_jsdoc(&c.short_description, 256),
        ));

        Ok(ToolContext {
            server_id: sanitize_jsdoc(server_id, 256),
            name: sanitize_jsdoc(tool.name.as_str(), 256),
            name_literal: sanitize_ts_string_literal(tool.name.as_str()),
            server_id_literal: sanitize_ts_string_literal(server_id),
            typescript_name,
            description,
            input_schema: sanitize_schema_jsdoc_descriptions(tool.input_schema.clone()),
            properties,
            category: categorization.map(|c| sanitize_jsdoc(&c.category, 128)),
            keywords: categorization.map(|c| sanitize_jsdoc(&c.keywords, 256)),
            short_description,
        })
    }

    /// Creates index context from server information.
    ///
    /// `typescript_names` must be the same pre-resolved mapping (from
    /// [`resolve_typescript_names`]) used to generate each tool's file, so the `index.ts`
    /// re-exports reference the exact identifiers those files actually export.
    fn create_index_context(
        &self,
        server_info: &ServerInfo,
        categorizations: Option<&HashMap<String, ToolCategorization>>,
        typescript_names: &[String],
    ) -> Result<IndexContext> {
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
                    keywords: cat.map(|c| sanitize_jsdoc(&c.keywords, 256)),
                    short_description: cat.map(|c| sanitize_jsdoc(&c.short_description, 256)),
                }
            })
            .collect();

        // Build category groups if categorizations are provided
        let category_groups = categorizations.map(|_| {
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

        Ok(IndexContext {
            server_name: sanitize_jsdoc(&server_info.name, 256),
            server_version: sanitize_jsdoc(&server_info.version, 64),
            tool_count: server_info.tools.len(),
            tools,
            categories: category_groups,
        })
    }

    /// Extracts property information from JSON Schema.
    ///
    /// Converts JSON Schema properties into `PropertyInfo` structures
    /// suitable for template rendering. Sibling property names that sanitize to the same
    /// TypeScript identifier (e.g. `a-b` and `a.b` both becoming `a_b`) are disambiguated
    /// with a numeric suffix, since these become fields of the same generated `Params`
    /// interface and an undetected collision would produce a duplicate, non-compiling field.
    ///
    /// # Errors
    ///
    /// Returns error if schema is malformed or type conversion fails.
    fn extract_property_infos(&self, schema: &serde_json::Value) -> Result<Vec<PropertyInfo>> {
        Ok(self
            .extract_property_data(schema)?
            .into_iter()
            .map(|(info, _raw_description)| info)
            .collect())
    }

    /// Extracts property information from JSON Schema, alongside each property's raw
    /// (un-sanitized) description.
    ///
    /// Shares the extraction logic with [`extract_property_infos`](Self::extract_property_infos),
    /// which only needs the JSDoc-sanitized `PropertyInfo` for template rendering. Consumers
    /// that need the description as originally authored — e.g. the `_meta.json` sidecar, which
    /// is JSON consumed by Rust rather than text interpolated into a JS comment — should use
    /// this method instead, so they are not subject to JSDoc-safety truncation/escaping that
    /// doesn't apply to their format (issue #141).
    ///
    /// # Errors
    ///
    /// Returns error if schema is malformed or type conversion fails.
    fn extract_property_data(
        &self,
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
            let raw_description = if let Some(obj) = schema.as_object() {
                obj.get("properties")
                    .and_then(|props| props.as_object())
                    .and_then(|props| props.get(&raw_name))
                    .and_then(|prop_schema| prop_schema.as_object())
                    .and_then(|obj| obj.get("description"))
                    .and_then(|desc| desc.as_str())
                    .map(str::to_string)
            } else {
                None
            };
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
    /// contract consumed by other Rust code, not interpolated into a JSDoc comment, so
    /// JSDoc-safety sanitization (truncation, `*/`-escaping, newline-flattening) would only
    /// lose fidelity. Parameter descriptions come from
    /// [`extract_property_data`](Self::extract_property_data)'s raw half rather than the
    /// JSDoc-sanitized `PropertyInfo` used for template rendering, which is what fully fixes
    /// the data loss described in issue #141 (the old regex-based parser could not recover
    /// parameter descriptions from the generated TypeScript at all).
    ///
    /// # Errors
    ///
    /// Returns error if schema conversion fails.
    fn create_tool_metadata(
        &self,
        tool: &ToolInfo,
        categorization: Option<&ToolCategorization>,
        typescript_name: String,
    ) -> Result<ToolMetadata> {
        let properties = self.extract_property_data(&tool.input_schema)?;

        let description = (!tool.description.is_empty()).then(|| tool.description.clone());
        let category = categorization.map(|c| c.category.clone());
        let keywords = categorization.map_or_else(Vec::new, |c| {
            c.keywords
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        });

        Ok(ToolMetadata {
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
        })
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

/// Sanitizes a server-controlled string for safe interpolation into JSDoc block comments.
///
/// Prevents JSDoc comment terminator injection by replacing `*/` sequences,
/// stripping newlines, and truncating to a safe maximum length.
fn sanitize_jsdoc(s: &str, max_len: usize) -> String {
    let sanitized = s.replace("*/", "*\\/").replace(['\r', '\n'], " ");
    if sanitized.chars().count() > max_len {
        sanitized.chars().take(max_len).collect()
    } else {
        sanitized
    }
}

/// Escapes a string for safe embedding inside a single-quoted TypeScript string literal.
///
/// Backslashes are escaped before quotes so the backslash introduced by quote-escaping
/// is not itself re-escaped. Carriage returns and newlines are escaped so the value
/// cannot terminate the literal by injecting a raw line break.
fn sanitize_ts_string_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
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
/// `used` is seeded with [`RESERVED_WORDS`] before any tool is processed, so a sanitized name
/// that exactly matches a JS/TS reserved word (e.g. a tool literally named `delete`) is treated
/// as already taken by [`disambiguate_identifier`] and gets the same numeric-suffix
/// disambiguation as a collision: `export async function delete(...)` is a hard syntax error,
/// so it becomes `delete_2` instead.
fn resolve_typescript_names(tools: &[ToolInfo]) -> Vec<String> {
    let mut used: HashSet<String> = RESERVED_WORDS.iter().map(|&s| s.to_string()).collect();
    let mut resolved = Vec::with_capacity(tools.len());

    for tool in tools {
        let base = sanitize_ts_identifier(&to_camel_case(tool.name.as_str()));
        resolved.push(disambiguate_identifier(&base, &mut used));
    }

    resolved
}

fn sanitize_schema_jsdoc_descriptions(mut value: serde_json::Value) -> serde_json::Value {
    sanitize_schema_jsdoc_value(&mut value);
    value
}

fn sanitize_schema_jsdoc_value(value: &mut serde_json::Value) {
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
                    sanitize_schema_jsdoc_value(child);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                sanitize_schema_jsdoc_value(child);
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
            id: ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("create_issue"),
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
                    name: ToolName::new("update_issue"),
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
        // - 1 _meta.json
        assert_eq!(code.file_count(), 6);

        // Check tool files exist
        let tool_files: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();

        assert!(tool_files.contains(&"createIssue.ts"));
        assert!(tool_files.contains(&"updateIssue.ts"));
        assert!(tool_files.contains(&"index.ts"));
        assert!(tool_files.contains(&"_runtime/mcp-bridge.ts"));
        assert!(tool_files.contains(&"package.json"));
        assert!(tool_files.contains(&"_meta.json"));
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
                keywords: "create, issue , new".to_string(),
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
            id: ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("send_message"),
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
        let generator = ProgressiveGenerator::new().unwrap();
        let tool = ToolInfo {
            name: ToolName::new("send_message"),
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
            keywords: "send,message,chat".to_string(),
            short_description: "Send a message".to_string(),
        };
        let context = generator
            .create_tool_context(
                "test-server",
                &tool,
                Some(&categorization),
                "sendMessage".to_string(),
            )
            .unwrap();

        assert_eq!(context.server_id, "test-server");
        assert_eq!(context.name, "send_message");
        assert_eq!(context.name_literal, "send_message");
        assert_eq!(context.server_id_literal, "test-server");
        assert_eq!(context.typescript_name, "sendMessage");
        assert_eq!(context.description, "Sends a message");
        assert_eq!(context.properties.len(), 1);
        assert_eq!(context.properties[0].name, "text");
        assert_eq!(context.category, Some("messaging".to_string()));
        assert_eq!(context.keywords, Some("send,message,chat".to_string()));
        assert_eq!(
            context.short_description,
            Some("Send a message".to_string())
        );
    }

    #[test]
    fn test_create_tool_context_without_categorization_falls_back_to_description() {
        let generator = ProgressiveGenerator::new().unwrap();
        let tool = ToolInfo {
            name: ToolName::new("format_document"),
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

        let context = generator
            .create_tool_context("test-server", &tool, None, "formatDocument".to_string())
            .unwrap();

        assert_eq!(
            context.short_description,
            Some("Format document with language-specific rules".to_string())
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
        let generator = ProgressiveGenerator::new().unwrap();
        let tool = ToolInfo {
            name: ToolName::new("send_message"),
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

        let context = generator
            .create_tool_context("test-server", &tool, None, "sendMessage".to_string())
            .unwrap();

        let expected = sanitize_schema_jsdoc_descriptions(tool.input_schema);
        assert_eq!(context.input_schema, expected);
        assert_eq!(
            context.input_schema["description"],
            json!("Schema *\\/ injected next")
        );
    }

    #[test]
    fn test_create_index_context() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();
        let typescript_names = resolve_typescript_names(&server_info.tools);

        let context = generator
            .create_index_context(&server_info, None, &typescript_names)
            .unwrap();

        assert_eq!(context.server_name, "Test Server");
        assert_eq!(context.server_version, "1.0.0");
        assert_eq!(context.tool_count, 2);
        assert_eq!(context.tools.len(), 2);
        assert_eq!(context.tools[0].typescript_name, "createIssue");
        assert!(context.categories.is_none());
    }

    #[test]
    fn test_extract_property_infos() {
        let generator = ProgressiveGenerator::new().unwrap();
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

        let props = generator.extract_property_infos(&schema).unwrap();

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
        let generator = ProgressiveGenerator::new().unwrap();
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

        let props = generator.extract_property_infos(&schema).unwrap();

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
        let generator = ProgressiveGenerator::new().unwrap();
        let schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a.b": {"type": "number"}
            },
            "required": []
        });

        let props = generator.extract_property_infos(&schema).unwrap();
        let mut names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();

        assert_eq!(names, vec!["a_b", "a_b_2"]);
    }

    #[test]
    fn test_extract_property_infos_disambiguates_three_way_collision() {
        let generator = ProgressiveGenerator::new().unwrap();
        let schema = json!({
            "type": "object",
            "properties": {
                "a-b": {"type": "string"},
                "a.b": {"type": "number"},
                "a b": {"type": "boolean"}
            },
            "required": []
        });

        let props = generator.extract_property_infos(&schema).unwrap();
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
    fn test_sanitize_jsdoc_truncates() {
        let long = "a".repeat(300);
        assert_eq!(sanitize_jsdoc(&long, 256).chars().count(), 256);
    }

    #[test]
    fn test_sanitize_jsdoc_passthrough() {
        assert_eq!(sanitize_jsdoc("Normal string", 256), "Normal string");
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
        server_info.tools[0].name = ToolName::new("create_issue'); alert('pwned");

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

        assert!(
            !tool.content.contains("'); alert('pwned"),
            "raw quote must not break out of the callMCPTool string literal: {}",
            tool.content
        );
    }

    #[test]
    fn test_resolve_typescript_names_disambiguates_collisions() {
        let tools = vec![
            ToolInfo {
                name: ToolName::new("foo-bar"),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("foo.bar"),
                description: String::new(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("foo bar"),
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
                name: ToolName::new("dup"),
                description: "First".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("dup"),
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
                name: ToolName::new("dup"),
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
                name: ToolName::new(name),
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
                name: ToolName::new("class"),
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
                name: ToolName::new("class-2"),
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
    fn test_generate_sanitizes_reserved_word_tool_name() {
        let generator = ProgressiveGenerator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools = vec![ToolInfo {
            name: ToolName::new("delete"),
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
                name: ToolName::new("foo-bar"),
                description: "First".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("foo.bar"),
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
                name: ToolName::new("dup"),
                description: "First".to_string(),
                input_schema: json!({}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("dup"),
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
                keywords: "create,*/ injected\nnext".to_string(),
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
}
