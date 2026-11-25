//! Per-tool parameter and result types for one generated tool file.
//!
//! A tool's schemas are converted once ([`ToolConversions`]) and then rendered at a
//! [`DegradeStage`]: the generator steps a tool down to a more generic form when the full form
//! would exceed its share of the output budget.

use crate::common::ts_types::{ConvertedType, RenderStyle, convert_schema};
use crate::common::typescript::sanitize_jsdoc;
use crate::progressive::generator::STORED_TYPE_TEXT_MAX_BYTES;
use mcp_execution_core::helper_protocol::ResultSource;
use mcp_execution_core::metadata::{ParameterMetadata, ResultTypeOrigin};
use mcp_execution_introspector::ToolInfo;
use serde_json::Value;

/// Parameters type of a tool whose input schema is degraded or not object-like.
pub(super) const GENERIC_PARAMS_TYPE: &str = "Record<string, unknown>";

/// Result type of a tool without a representable output schema; `undefined` is an empty success.
pub(super) const GENERIC_RESULT_TYPE: &str =
    "Record<string, unknown> | unknown[] | string | undefined";

/// Result type of a tool that declares an output schema whose converted type is not used:
/// `structuredContent` may be any JSON value.
pub(super) const GENERIC_DECLARED_RESULT_TYPE: &str =
    "Record<string, unknown> | unknown[] | string | number | boolean | null | undefined";

/// Characters kept of the observed-shape hint, which is the fixed prefix plus a shape of at most
/// [`STORED_TYPE_TEXT_MAX_BYTES`] bytes.
const OBSERVED_HINT_CHARS: usize = STORED_TYPE_TEXT_MAX_BYTES + 256;

/// How far a tool has been stepped down from its full types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DegradeStage {
    /// Declared parameter and result types.
    Full,
    /// Generic result; the observed-shape hint is dropped.
    ResultGeneric,
    /// Generic result and generic parameters; no parameter list is stored.
    AllGeneric,
}

impl DegradeStage {
    /// The next, more generic stage; `None` after the last one.
    pub(super) const fn next(self) -> Option<Self> {
        match self {
            Self::Full => Some(Self::ResultGeneric),
            Self::ResultGeneric => Some(Self::AllGeneric),
            Self::AllGeneric => None,
        }
    }
}

/// The rendered types and the metadata derived from them for one tool at one [`DegradeStage`].
#[derive(Debug, Clone)]
pub(super) struct ToolTypes {
    /// Parameters type for the tool file.
    pub(super) params_declaration: String,
    /// Parameters type for the catalog.
    pub(super) params_compact: String,
    /// Result type for the tool file, ending in `| undefined`.
    pub(super) result_declaration: String,
    /// Result type for the catalog, ending in `| undefined`.
    pub(super) result_compact: String,
    /// `JSDoc` text for an observed result shape, only for tools without a declared schema.
    pub(super) observed_hint: Option<String>,
    /// How the generated tool function asks the bridge for its result.
    pub(super) result_source: ResultSource,
    /// Parameter list for `_meta.json`.
    pub(super) parameters: Vec<ParameterMetadata>,
    /// Compact result type for `_meta.json`, absent when generic or over the stored cap.
    pub(super) stored_result_type: Option<String>,
    /// Why the result type has its form.
    pub(super) origin: ResultTypeOrigin,
}

/// A tool's schemas, converted once.
#[derive(Debug)]
pub(super) struct ToolConversions<'a> {
    tool: &'a ToolInfo,
    input: ConvertedType,
    output: Option<ConvertedType>,
    observed_hint: Option<String>,
}

fn warn_diagnostics(tool: &ToolInfo, role: &str, converted: &ConvertedType) {
    let total = converted.diagnostics().len() + converted.dropped_diagnostics();
    if total == 0 {
        return;
    }
    let kinds: Vec<_> = converted
        .diagnostics()
        .iter()
        .take(4)
        .map(|d| d.kind)
        .collect();
    tracing::warn!(
        tool = ?tool.name.as_str(),
        role,
        count = total,
        kinds = ?kinds,
        "schema conversion degraded part of a type"
    );
}

fn observed_hint(shape: &Value) -> Option<String> {
    let compact = convert_schema(shape).render(RenderStyle::Compact);
    (compact.len() <= STORED_TYPE_TEXT_MAX_BYTES).then(|| {
        sanitize_jsdoc(
            &format!("Result shape observed in a past call, not declared by the server: {compact}"),
            OBSERVED_HINT_CHARS,
        )
    })
}

impl<'a> ToolConversions<'a> {
    /// Converts the tool's schemas and its observed result shape, warning once per role that
    /// lost information.
    pub(super) fn new(tool: &'a ToolInfo, observed_shape: Option<&Value>) -> Self {
        let input = convert_schema(&tool.input_schema);
        warn_diagnostics(tool, "input", &input);
        if !input.is_object_like() {
            tracing::warn!(
                tool = ?tool.name.as_str(),
                "input schema is not an object; parameters are generic"
            );
        }
        let output = tool.output_schema.as_ref().map(convert_schema);
        if let Some(output) = &output {
            warn_diagnostics(tool, "output", output);
        }
        let observed_hint = observed_shape
            .filter(|_| tool.output_schema.is_none())
            .and_then(observed_hint);
        Self {
            tool,
            input,
            output,
            observed_hint,
        }
    }

    /// Renders the tool's types at `stage`.
    pub(super) fn build(&self, stage: DegradeStage) -> ToolTypes {
        let object_params = stage != DegradeStage::AllGeneric && self.input.is_object_like();
        let (params_declaration, params_compact) = if object_params {
            (
                self.input.render(RenderStyle::Declaration),
                self.input.render(RenderStyle::Compact),
            )
        } else {
            (
                GENERIC_PARAMS_TYPE.to_string(),
                GENERIC_PARAMS_TYPE.to_string(),
            )
        };
        let declared = self
            .output
            .as_ref()
            .filter(|output| stage == DegradeStage::Full && !output.root_degraded());
        let generic_result = if self.output.is_some() {
            GENERIC_DECLARED_RESULT_TYPE
        } else {
            GENERIC_RESULT_TYPE
        };
        let (result_declaration, result_compact, stored_result_type) = declared.map_or_else(
            || (generic_result.to_string(), generic_result.to_string(), None),
            |output| {
                let compact = output.render(RenderStyle::Compact);
                let stored = (compact.len() <= STORED_TYPE_TEXT_MAX_BYTES).then(|| compact.clone());
                (
                    format!("{} | undefined", output.render(RenderStyle::Declaration)),
                    format!("{compact} | undefined"),
                    stored,
                )
            },
        );
        let origin = match (&self.output, declared) {
            (None, _) => ResultTypeOrigin::Undeclared,
            (Some(_), Some(_)) => ResultTypeOrigin::Declared,
            (Some(_), None) => ResultTypeOrigin::Degraded,
        };
        ToolTypes {
            params_declaration,
            params_compact,
            result_declaration,
            result_compact,
            observed_hint: self
                .observed_hint
                .clone()
                .filter(|_| stage == DegradeStage::Full),
            result_source: if self.output.is_some() {
                ResultSource::Structured
            } else {
                ResultSource::Content
            },
            parameters: if object_params {
                self.parameters()
            } else {
                Vec::new()
            },
            stored_result_type,
            origin,
        }
    }

    fn parameters(&self) -> Vec<ParameterMetadata> {
        self.input
            .members()
            .iter()
            .map(|member| ParameterMetadata {
                name: member.name.clone(),
                typescript_type: if member.type_text.len() <= STORED_TYPE_TEXT_MAX_BYTES {
                    member.type_text.clone()
                } else {
                    "unknown".to_string()
                },
                required: member.required,
                description: self.raw_description(&member.name),
            })
            .collect()
    }

    fn raw_description(&self, name: &str) -> Option<String> {
        self.tool
            .input_schema
            .get("properties")
            .and_then(|properties| properties.get(name))
            .and_then(|property| property.get("description"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::ToolName;
    use serde_json::json;

    fn tool(input: &Value, output: Option<Value>) -> ToolInfo {
        ToolInfo {
            name: ToolName::new("t").unwrap(),
            description: String::new(),
            input_schema: input.clone(),
            output_schema: output,
            annotations: None,
        }
    }

    #[test]
    fn stages_step_down_in_order() {
        assert_eq!(DegradeStage::Full.next(), Some(DegradeStage::ResultGeneric));
        assert_eq!(
            DegradeStage::ResultGeneric.next(),
            Some(DegradeStage::AllGeneric)
        );
        assert_eq!(DegradeStage::AllGeneric.next(), None);
    }

    #[test]
    fn all_generic_drops_parameters_and_declared_result() {
        let tool = tool(
            &json!({"type": "object", "properties": {"a": {"type": "string"}}}),
            Some(json!({"type": "number"})),
        );
        let conversions = ToolConversions::new(&tool, None);

        let full = conversions.build(DegradeStage::Full);
        assert_eq!(full.parameters.len(), 1);
        assert_eq!(full.origin, ResultTypeOrigin::Declared);
        assert_eq!(full.stored_result_type.as_deref(), Some("number"));

        let generic = conversions.build(DegradeStage::AllGeneric);
        assert_eq!(generic.parameters.len(), 0);
        assert_eq!(generic.params_declaration, GENERIC_PARAMS_TYPE);
        assert_eq!(generic.result_declaration, GENERIC_DECLARED_RESULT_TYPE);
        assert_eq!(generic.result_source, ResultSource::Structured);
        assert_eq!(generic.origin, ResultTypeOrigin::Degraded);
        assert_eq!(generic.stored_result_type, None);
    }

    #[test]
    fn degraded_declared_result_is_widened_but_keeps_the_structured_source() {
        let unrepresentable = tool(&json!({"type": "object"}), Some(json!("not a schema")));
        let undeclared = tool(&json!({"type": "object"}), None);

        let root_degraded = ToolConversions::new(&unrepresentable, None).build(DegradeStage::Full);
        let stage_a = ToolConversions::new(
            &tool(&json!({"type": "object"}), Some(json!({"type": "number"}))),
            None,
        )
        .build(DegradeStage::ResultGeneric);
        let stage_b = ToolConversions::new(
            &tool(&json!({"type": "object"}), Some(json!({"type": "number"}))),
            None,
        )
        .build(DegradeStage::AllGeneric);
        for types in [root_degraded, stage_a, stage_b] {
            assert_eq!(types.result_declaration, GENERIC_DECLARED_RESULT_TYPE);
            assert_eq!(types.result_compact, GENERIC_DECLARED_RESULT_TYPE);
            assert_eq!(types.result_source, ResultSource::Structured);
        }
        let plain = ToolConversions::new(&undeclared, None).build(DegradeStage::Full);
        assert_eq!(plain.result_declaration, GENERIC_RESULT_TYPE);
        assert_eq!(plain.result_source, ResultSource::Content);
    }

    #[test]
    fn observed_hint_only_for_undeclared_tools_at_full_stage() {
        let shape = json!({"type": "object", "properties": {"id": {"type": "string"}}});
        let undeclared = tool(&json!({"type": "object"}), None);
        let declared = tool(&json!({"type": "object"}), Some(json!({"type": "string"})));

        let hinted = ToolConversions::new(&undeclared, Some(&shape));
        assert!(hinted.build(DegradeStage::Full).observed_hint.is_some());
        assert!(
            hinted
                .build(DegradeStage::ResultGeneric)
                .observed_hint
                .is_none()
        );
        let typed = ToolConversions::new(&declared, Some(&shape));
        assert!(typed.build(DegradeStage::Full).observed_hint.is_none());
    }
}
