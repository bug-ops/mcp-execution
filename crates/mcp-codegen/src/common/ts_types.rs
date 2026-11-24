//! JSON Schema to TypeScript type conversion with bounds and diagnostics.
//!
//! [`convert_schema`] walks a schema once into a private typed tree and yields a
//! [`ConvertedType`] that renders in two styles ([`RenderStyle`]): the multi-line, commented
//! `Declaration` form used in tool files and the one-line `Compact` form used in the catalog and
//! stored texts.
//!
//! The converter never panics, never errors and never fetches a remote reference: every bound it
//! hits degrades the smallest enclosing node to `unknown` and records a `SchemaDiagnostic`.
//! Property keys that are not bare identifiers render as exact quoted literals, never renamed.

use crate::common::typescript::{
    MAX_SCHEMA_RECURSION_DEPTH, sanitize_jsdoc, ts_string_literal_exact,
};
use mcp_execution_core::untrusted::{is_bare_identifier, sanitize_untrusted_text};
use mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// Maximum number of members rendered in one union before the node degrades.
pub(crate) const MAX_UNION_MEMBERS: usize = 128;

/// Maximum number of members rendered in one intersection before the node degrades.
pub(crate) const MAX_INTERSECTION_MEMBERS: usize = 8;

/// Maximum number of `$ref` expansions performed per root schema.
pub(crate) const MAX_REF_EXPANSIONS: usize = 64;

/// Maximum number of diagnostics recorded per conversion; further ones are only counted.
pub(crate) const MAX_DIAGNOSTICS: usize = 32;

/// Maximum number of characters kept in a [`SchemaPath`].
pub(crate) const MAX_SCHEMA_PATH_CHARS: usize = 256;

/// Maximum size in bytes of a rendered `Declaration` type before the converter degrades.
pub(crate) const MAX_RENDERED_TYPE_BYTES: usize = MAX_SCHEMA_SIZE_BYTES;

/// Characters kept of a `JSDoc` description or tag value on a member.
const MEMBER_DOC_CHARS: usize = 256;

/// Bytes charged for every node slot: the widest leaf (`unknown`, `boolean`) plus separators.
const SLOT_BYTES: usize = 10;

/// Bytes charged for an index-signature line, excluding its indentation and value.
const INDEX_LINE_BYTES: usize = 24;

/// Keywords whose only effect is to narrow the accepted values; the superset type is kept.
const NARROWING_KEYWORDS: &[&str] = &[
    "not",
    "if",
    "then",
    "else",
    "patternProperties",
    "propertyNames",
    "dependentSchemas",
    "dependentRequired",
    "dependencies",
    "unevaluatedProperties",
    "unevaluatedItems",
    "contains",
    "uniqueItems",
    "multipleOf",
];

/// Keywords next to a `$ref` that never change the accepted values.
const ANNOTATION_KEYWORDS: &[&str] = &[
    "$ref",
    "$schema",
    "$id",
    "$comment",
    "$defs",
    "definitions",
    "description",
    "title",
    "default",
    "examples",
    "example",
    "deprecated",
    "readOnly",
    "writeOnly",
];

/// Scalar keywords rendered as `JSDoc` tags on a member, with the kind of value each takes.
const DOC_TAG_KEYWORDS: &[(&str, TagValue)] = &[
    ("format", TagValue::Text),
    ("default", TagValue::Json),
    ("minimum", TagValue::Number),
    ("maximum", TagValue::Number),
    ("minLength", TagValue::Number),
    ("maxLength", TagValue::Number),
    ("minItems", TagValue::Number),
    ("maxItems", TagValue::Number),
];

#[derive(Debug, Clone, Copy)]
enum TagValue {
    Text,
    Number,
    Json,
}

/// How a [`ConvertedType`] is rendered.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
/// use serde_json::json;
///
/// let converted = convert_schema(&json!({"type": "string"}));
/// assert_eq!(converted.render(RenderStyle::Compact), "string");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStyle {
    /// Multi-line with member `JSDoc`, for tool files.
    Declaration,
    /// One line without `JSDoc`, for the catalog and stored texts.
    Compact,
}

/// A JSON pointer into the converted schema, sanitized and capped at
/// [`MAX_SCHEMA_PATH_CHARS`] characters; the empty path is the schema root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaPath(pub(crate) String);

/// What went wrong at one [`SchemaPath`] during conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticKind {
    /// The node is not a schema object or uses an unsupported `type`.
    UnsupportedType,
    /// Nesting exceeded `MAX_SCHEMA_RECURSION_DEPTH`.
    DepthExceeded,
    /// A union exceeded [`MAX_UNION_MEMBERS`].
    UnionTooLarge,
    /// An intersection exceeded [`MAX_INTERSECTION_MEMBERS`].
    IntersectionTooLarge,
    /// More than [`MAX_REF_EXPANSIONS`] references were expanded.
    RefExpansionLimit,
    /// The rendered type exceeded [`MAX_RENDERED_TYPE_BYTES`].
    SizeExceeded,
    /// A reference cycle was cut.
    CyclicRef,
    /// A reference does not point into the root document.
    NonLocalRef,
    /// A local reference could not be resolved.
    UnresolvedRef,
    /// Sibling keywords next to a `$ref` were ignored.
    RefSiblingsIgnored,
    /// A narrowing-only keyword was ignored, keeping the superset type.
    NarrowingKeywordIgnored,
    /// Tuple `items` or `prefixItems` rendered as `unknown[]`.
    TupleItems,
    /// A string literal could not be represented exactly.
    LiteralNotRepresentable,
    /// A property key could not be represented exactly and was omitted.
    KeyNotRepresentable,
}

/// One conversion diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaDiagnostic {
    /// Where in the schema the problem occurred.
    pub(crate) path: SchemaPath,
    /// What the problem was.
    pub(crate) kind: DiagnosticKind,
}

/// One representable member of an object-like root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemberSummary {
    /// The raw schema key.
    pub(crate) name: String,
    /// The member's type in `Compact` form.
    pub(crate) type_text: String,
    /// Whether the member is required.
    pub(crate) required: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Unknown,
    Primitive(&'static str),
    Literal(String),
    Array(Box<Self>),
    Object(ObjectNode),
    Union(Vec<Self>),
    Intersection(Vec<Self>),
}

#[derive(Debug, Clone, PartialEq)]
struct ObjectNode {
    properties: Vec<Property>,
    /// `None` is a closed object; `Some` is the value type of the index signature.
    index: Option<Box<Node>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Property {
    name: String,
    key: String,
    optional: bool,
    node: Node,
    doc: Vec<String>,
}

/// Where a node sits, which decides whether it needs parentheses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    Top,
    UnionMember,
    IntersectionMember,
    ArrayElement,
}

impl Node {
    fn write(&self, out: &mut String, style: RenderStyle, indent: usize, context: Context) {
        match self {
            Self::Unknown => out.push_str("unknown"),
            Self::Primitive(name) => out.push_str(name),
            Self::Literal(text) => out.push_str(text),
            Self::Array(element) => {
                element.write(out, style, indent, Context::ArrayElement);
                out.push_str("[]");
            }
            Self::Object(object) => object.write(out, style, indent),
            Self::Union(members) => {
                let parens = matches!(context, Context::IntersectionMember | Context::ArrayElement);
                write_joined(
                    out,
                    members,
                    " | ",
                    style,
                    indent,
                    parens,
                    Context::UnionMember,
                );
            }
            Self::Intersection(members) => {
                let parens = context == Context::ArrayElement;
                write_joined(
                    out,
                    members,
                    " & ",
                    style,
                    indent,
                    parens,
                    Context::IntersectionMember,
                );
            }
        }
    }

    fn render(&self, style: RenderStyle) -> String {
        let mut out = String::new();
        self.write(&mut out, style, 0, Context::Top);
        out
    }
}

fn write_joined(
    out: &mut String,
    members: &[Node],
    separator: &str,
    style: RenderStyle,
    indent: usize,
    parens: bool,
    context: Context,
) {
    if parens {
        out.push('(');
    }
    for (position, member) in members.iter().enumerate() {
        if position > 0 {
            out.push_str(separator);
        }
        member.write(out, style, indent, context);
    }
    if parens {
        out.push(')');
    }
}

impl ObjectNode {
    fn write(&self, out: &mut String, style: RenderStyle, indent: usize) {
        if self.properties.is_empty() && self.index.is_none() {
            out.push_str("Record<string, never>");
            return;
        }
        match style {
            RenderStyle::Declaration => self.write_multiline(out, indent),
            RenderStyle::Compact => self.write_inline(out),
        }
    }

    fn write_multiline(&self, out: &mut String, indent: usize) {
        let inner = indent + 2;
        out.push_str("{\n");
        for property in &self.properties {
            property.write_doc(out, inner);
            push_spaces(out, inner);
            property.write_signature(out, RenderStyle::Declaration, inner);
            out.push_str(";\n");
        }
        if let Some(index) = &self.index {
            push_spaces(out, inner);
            out.push_str("[key: string]: ");
            index.write(out, RenderStyle::Declaration, inner, Context::Top);
            out.push_str(";\n");
        }
        push_spaces(out, indent);
        out.push('}');
    }

    fn write_inline(&self, out: &mut String) {
        out.push_str("{ ");
        let mut first = true;
        for property in &self.properties {
            if !first {
                out.push_str("; ");
            }
            first = false;
            property.write_signature(out, RenderStyle::Compact, 0);
        }
        if let Some(index) = &self.index {
            if !first {
                out.push_str("; ");
            }
            out.push_str("[key: string]: ");
            index.write(out, RenderStyle::Compact, 0, Context::Top);
        }
        out.push_str(" }");
    }
}

impl Property {
    fn write_signature(&self, out: &mut String, style: RenderStyle, indent: usize) {
        out.push_str(&self.key);
        if self.optional {
            out.push('?');
        }
        out.push_str(": ");
        self.node.write(out, style, indent, Context::Top);
    }

    fn write_doc(&self, out: &mut String, indent: usize) {
        if self.doc.is_empty() {
            return;
        }
        push_spaces(out, indent);
        out.push_str("/**\n");
        for line in &self.doc {
            push_spaces(out, indent);
            out.push_str(" * ");
            out.push_str(line);
            out.push('\n');
        }
        push_spaces(out, indent);
        out.push_str(" */\n");
    }
}

fn push_spaces(out: &mut String, count: usize) {
    out.extend(std::iter::repeat_n(' ', count));
}

/// The result of converting one JSON Schema to TypeScript.
///
/// Opaque: obtain one from [`convert_schema`] and render it with [`ConvertedType::render`].
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
/// use serde_json::json;
///
/// let converted = convert_schema(&json!({"type": "boolean"}));
/// assert_eq!(converted.render(RenderStyle::Declaration), "boolean");
/// ```
#[derive(Debug, Clone)]
pub struct ConvertedType {
    node: Node,
    diagnostics: Vec<SchemaDiagnostic>,
    dropped_diagnostics: usize,
    members: Option<Vec<MemberSummary>>,
}

impl ConvertedType {
    /// Renders the converted type in the requested style.
    ///
    /// `Compact` output has no newline and no `/**`, and is never longer than `Declaration`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
    /// use serde_json::json;
    ///
    /// let converted = convert_schema(&json!({
    ///     "type": "object",
    ///     "properties": {"id": {"type": "integer"}},
    ///     "required": ["id"],
    ///     "additionalProperties": false
    /// }));
    /// assert_eq!(converted.render(RenderStyle::Compact), "{ id: number }");
    /// assert_eq!(converted.render(RenderStyle::Declaration), "{\n  id: number;\n}");
    /// ```
    #[must_use]
    pub fn render(&self, style: RenderStyle) -> String {
        self.node.render(style)
    }

    /// Diagnostics recorded during conversion, at most [`MAX_DIAGNOSTICS`].
    pub(crate) fn diagnostics(&self) -> &[SchemaDiagnostic] {
        &self.diagnostics
    }

    /// Number of diagnostics beyond [`MAX_DIAGNOSTICS`] that were counted but not recorded.
    pub(crate) const fn dropped_diagnostics(&self) -> usize {
        self.dropped_diagnostics
    }

    /// Whether the root itself degraded: it became `unknown` and a diagnostic exists at the
    /// empty path. Ignored narrowing keywords and `$ref` siblings keep the declared type.
    pub(crate) fn root_degraded(&self) -> bool {
        self.node == Node::Unknown && self.diagnostics.iter().any(|d| d.path.0.is_empty())
    }

    /// Whether the root is an object, or a union or intersection of object-like members.
    pub(crate) const fn is_object_like(&self) -> bool {
        self.members.is_some()
    }

    /// The root's representable members in declared order.
    pub(crate) fn members(&self) -> &[MemberSummary] {
        self.members.as_deref().unwrap_or_default()
    }
}

/// Converts a JSON Schema to a [`ConvertedType`].
///
/// Never panics, never errors and never fetches a remote reference; the output is
/// deterministic.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};
/// use serde_json::json;
///
/// let converted = convert_schema(&json!({"type": "number"}));
/// assert_eq!(converted.render(RenderStyle::Compact), "number");
///
/// let status = convert_schema(&json!({"enum": ["open", "closed"]}));
/// assert_eq!(status.render(RenderStyle::Compact), "'open' | 'closed'");
/// ```
#[must_use]
pub fn convert_schema(schema: &Value) -> ConvertedType {
    let mut converter = Converter::new(schema);
    let node = converter.node(schema, 0, 0);
    let members = summarize(&node);
    ConvertedType {
        node,
        diagnostics: converter.diagnostics,
        dropped_diagnostics: converter.dropped,
        members,
    }
}

#[derive(Debug, Clone, Copy)]
enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

struct Converter<'a> {
    root: &'a Value,
    diagnostics: Vec<SchemaDiagnostic>,
    dropped: usize,
    ref_expansions: usize,
    active_refs: Vec<&'a str>,
    size: usize,
    size_exceeded: bool,
    path: Vec<Segment<'a>>,
}

impl<'a> Converter<'a> {
    const fn new(root: &'a Value) -> Self {
        Self {
            root,
            diagnostics: Vec::new(),
            dropped: 0,
            ref_expansions: 0,
            active_refs: Vec::new(),
            size: 0,
            size_exceeded: false,
            path: Vec::new(),
        }
    }

    fn pointer(&self) -> String {
        let mut raw = String::new();
        for segment in &self.path {
            raw.push('/');
            match segment {
                Segment::Key(key) => raw.push_str(&key.replace('~', "~0").replace('/', "~1")),
                Segment::Index(index) => raw.push_str(&index.to_string()),
            }
        }
        sanitize_untrusted_text(&raw, MAX_SCHEMA_PATH_CHARS)
    }

    fn report(&mut self, kind: DiagnosticKind) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.dropped += 1;
            return;
        }
        let path = SchemaPath(self.pointer());
        self.diagnostics.push(SchemaDiagnostic { path, kind });
    }

    /// Accounts `bytes` of rendered output; `false` once the size bound is hit.
    fn charge(&mut self, bytes: usize) -> bool {
        if self.size_exceeded {
            return false;
        }
        let next = self.size.saturating_add(bytes);
        if next > MAX_RENDERED_TYPE_BYTES {
            self.size_exceeded = true;
            self.report(DiagnosticKind::SizeExceeded);
            return false;
        }
        self.size = next;
        true
    }

    fn unknown_with(&mut self, kind: DiagnosticKind) -> Node {
        self.report(kind);
        Node::Unknown
    }

    fn child(
        &mut self,
        segments: &[Segment<'a>],
        schema: &'a Value,
        depth: usize,
        indent: usize,
    ) -> Node {
        let mark = self.path.len();
        self.path.extend_from_slice(segments);
        let node = self.node(schema, depth + 1, indent);
        self.path.truncate(mark);
        node
    }

    fn node(&mut self, schema: &'a Value, depth: usize, indent: usize) -> Node {
        if depth >= MAX_SCHEMA_RECURSION_DEPTH {
            return self.unknown_with(DiagnosticKind::DepthExceeded);
        }
        if !self.charge(SLOT_BYTES) {
            return Node::Unknown;
        }
        let object = match schema {
            Value::Object(object) => object,
            Value::Bool(true) => return Node::Unknown,
            _ => return self.unknown_with(DiagnosticKind::UnsupportedType),
        };
        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            return self.reference(reference, object, depth, indent);
        }
        self.schema_object(object, depth, indent)
    }

    fn reference(
        &mut self,
        reference: &'a str,
        object: &'a Map<String, Value>,
        depth: usize,
        indent: usize,
    ) -> Node {
        if object
            .keys()
            .any(|key| !ANNOTATION_KEYWORDS.contains(&key.as_str()))
        {
            self.report(DiagnosticKind::RefSiblingsIgnored);
        }
        let Some(fragment) = reference.strip_prefix('#') else {
            return self.unknown_with(DiagnosticKind::NonLocalRef);
        };
        if self.active_refs.contains(&reference) {
            return self.unknown_with(DiagnosticKind::CyclicRef);
        }
        if self.ref_expansions >= MAX_REF_EXPANSIONS {
            return self.unknown_with(DiagnosticKind::RefExpansionLimit);
        }
        let Some(target) = self.root.pointer(fragment) else {
            return self.unknown_with(DiagnosticKind::UnresolvedRef);
        };
        self.ref_expansions += 1;
        self.active_refs.push(reference);
        let node = self.child(&[], target, depth, indent);
        self.active_refs.pop();
        node
    }

    fn schema_object(
        &mut self,
        object: &'a Map<String, Value>,
        depth: usize,
        indent: usize,
    ) -> Node {
        if NARROWING_KEYWORDS.iter().any(|k| object.contains_key(*k)) {
            self.report(DiagnosticKind::NarrowingKeywordIgnored);
        }
        let mut parts = Vec::new();
        if let Some(base) = self.base(object, depth, indent) {
            parts.push(base);
        }
        for keyword in ["oneOf", "anyOf"] {
            if let Some(Value::Array(members)) = object.get(keyword) {
                parts.push(self.union_of(keyword, members, depth, indent));
            }
        }
        if let Some(Value::Array(members)) = object.get("allOf") {
            if members.len() > MAX_INTERSECTION_MEMBERS {
                return self.unknown_with(DiagnosticKind::IntersectionTooLarge);
            }
            for (position, member) in members.iter().enumerate() {
                let segments = [Segment::Key("allOf"), Segment::Index(position)];
                parts.push(self.child(&segments, member, depth, indent));
            }
        }
        self.intersect(parts)
    }

    fn intersect(&mut self, parts: Vec<Node>) -> Node {
        let mut parts: Vec<Node> = parts
            .into_iter()
            .filter(|part| *part != Node::Unknown)
            .collect();
        if parts.len() > MAX_INTERSECTION_MEMBERS {
            return self.unknown_with(DiagnosticKind::IntersectionTooLarge);
        }
        match parts.len() {
            0 => Node::Unknown,
            1 => parts.remove(0),
            _ => Node::Intersection(parts),
        }
    }

    fn union_of(
        &mut self,
        keyword: &'static str,
        members: &'a [Value],
        depth: usize,
        indent: usize,
    ) -> Node {
        if members.len() > MAX_UNION_MEMBERS {
            return self.unknown_with(DiagnosticKind::UnionTooLarge);
        }
        let nodes = members
            .iter()
            .enumerate()
            .map(|(position, member)| {
                let segments = [Segment::Key(keyword), Segment::Index(position)];
                self.child(&segments, member, depth, indent)
            })
            .collect();
        self.union(nodes)
    }

    /// A union of `nodes`: member unions flattened, deduplicated in order, `unknown` when any
    /// member is `unknown` or the flattened union exceeds [`MAX_UNION_MEMBERS`].
    fn union(&mut self, nodes: Vec<Node>) -> Node {
        match merge_union_members(nodes) {
            UnionMerge::Unknown => Node::Unknown,
            UnionMerge::TooLarge => self.unknown_with(DiagnosticKind::UnionTooLarge),
            UnionMerge::Members(mut unique) => match unique.len() {
                0 => Node::Unknown,
                1 => unique.remove(0),
                _ => Node::Union(unique),
            },
        }
    }

    fn base(
        &mut self,
        object: &'a Map<String, Value>,
        depth: usize,
        indent: usize,
    ) -> Option<Node> {
        if let Some(Value::Array(values)) = object.get("enum") {
            return Some(self.literals(values));
        }
        if let Some(value) = object.get("const") {
            return Some(self.literals(std::slice::from_ref(value)));
        }
        match object.get("type") {
            Some(Value::String(name)) => Some(self.typed(name, object, depth, indent)),
            Some(Value::Array(names)) => Some(self.typed_union(names, object, depth, indent)),
            _ if object.contains_key("properties") => {
                Some(self.typed("object", object, depth, indent))
            }
            _ if object.contains_key("items") || object.contains_key("prefixItems") => {
                Some(self.typed("array", object, depth, indent))
            }
            _ => None,
        }
    }

    fn typed_union(
        &mut self,
        names: &'a [Value],
        object: &'a Map<String, Value>,
        depth: usize,
        indent: usize,
    ) -> Node {
        if names.len() > MAX_UNION_MEMBERS {
            return self.unknown_with(DiagnosticKind::UnionTooLarge);
        }
        let nodes = names
            .iter()
            .map(|name| match name.as_str() {
                Some(name) => self.typed(name, object, depth, indent),
                None => self.unknown_with(DiagnosticKind::UnsupportedType),
            })
            .collect();
        self.union(nodes)
    }

    fn typed(
        &mut self,
        name: &str,
        object: &'a Map<String, Value>,
        depth: usize,
        indent: usize,
    ) -> Node {
        match name {
            "string" => Node::Primitive("string"),
            "number" | "integer" => Node::Primitive("number"),
            "boolean" => Node::Primitive("boolean"),
            "null" => Node::Primitive("null"),
            "object" => self.object(object, depth, indent),
            "array" => self.array(object, depth, indent),
            _ => self.unknown_with(DiagnosticKind::UnsupportedType),
        }
    }

    /// A union of the literal `values`, or their shared base primitive when a member cannot be
    /// represented exactly or the union is too large.
    fn literals(&mut self, values: &[Value]) -> Node {
        if values.is_empty() {
            return self.unknown_with(DiagnosticKind::UnsupportedType);
        }
        if !self.charge(values.len()) {
            return Node::Unknown;
        }
        let mut base: Option<&'static str> = None;
        let mut mixed = false;
        for value in values {
            let kind = literal_kind(value);
            match base {
                None => base = Some(kind),
                Some(first) if first != kind => mixed = true,
                Some(_) => {}
            }
        }
        let fallback = match (base, mixed) {
            (Some(kind @ ("string" | "number" | "boolean" | "null")), false) => {
                Node::Primitive(kind)
            }
            _ => Node::Unknown,
        };
        if values.len() > MAX_UNION_MEMBERS {
            self.report(DiagnosticKind::UnionTooLarge);
            return fallback;
        }
        let mut seen = HashSet::new();
        let mut literals: Vec<Node> = Vec::new();
        for value in values {
            let Some(text) = literal_text(value) else {
                self.report(DiagnosticKind::LiteralNotRepresentable);
                return fallback;
            };
            if seen.insert(text.clone()) {
                literals.push(Node::Literal(text));
            }
        }
        let bytes: usize = literals
            .iter()
            .map(|node| node.render(RenderStyle::Compact).len() + 3)
            .sum();
        if !self.charge(bytes) {
            return fallback;
        }
        self.union(literals)
    }

    fn array(&mut self, object: &'a Map<String, Value>, depth: usize, indent: usize) -> Node {
        let is_tuple = object.contains_key("prefixItems")
            || matches!(object.get("items"), Some(Value::Array(_)));
        if is_tuple {
            self.report(DiagnosticKind::TupleItems);
            return Node::Array(Box::new(Node::Unknown));
        }
        let element = object.get("items").map_or(Node::Unknown, |items| {
            self.child(&[Segment::Key("items")], items, depth, indent)
        });
        Node::Array(Box::new(element))
    }

    fn object(&mut self, object: &'a Map<String, Value>, depth: usize, indent: usize) -> Node {
        let reserved = 2 * indent + 4 + INDEX_LINE_BYTES;
        if !self.charge(reserved) {
            return Node::Unknown;
        }
        let required: Vec<&str> = object
            .get("required")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut properties = Vec::new();
        let mut omitted_key = false;
        let mut truncated = false;
        if let Some(Value::Object(declared)) = object.get("properties") {
            for (key, schema) in declared {
                let Some(rendered_key) = self.property_key(key) else {
                    omitted_key = true;
                    continue;
                };
                let line = indent + 2 + rendered_key.len() + 5;
                if !self.charge(line) {
                    truncated = true;
                    break;
                }
                let segments = [Segment::Key("properties"), Segment::Key(key)];
                let node = self.child(&segments, schema, depth, indent + 2);
                let doc = self.member_doc(schema, indent + 2);
                properties.push(Property {
                    name: key.clone(),
                    key: rendered_key,
                    optional: !required.contains(&key.as_str()),
                    node,
                    doc,
                });
            }
        }
        let index = self.index_type(object, depth, indent, omitted_key || truncated);
        self.assemble_object(properties, index, indent)
    }

    fn property_key(&mut self, key: &'a str) -> Option<String> {
        if is_bare_identifier(key) {
            return Some(key.to_string());
        }
        if let Some(body) = ts_string_literal_exact(key) {
            return Some(format!("'{body}'"));
        }
        self.path
            .extend([Segment::Key("properties"), Segment::Key(key)]);
        self.report(DiagnosticKind::KeyNotRepresentable);
        self.path.truncate(self.path.len() - 2);
        None
    }

    fn index_type(
        &mut self,
        object: &'a Map<String, Value>,
        depth: usize,
        indent: usize,
        force_unknown: bool,
    ) -> Option<Node> {
        match object.get("additionalProperties") {
            Some(Value::Bool(false)) if !force_unknown => None,
            Some(schema @ Value::Object(_)) if !force_unknown => Some(self.child(
                &[Segment::Key("additionalProperties")],
                schema,
                depth,
                indent + 2,
            )),
            _ => Some(Node::Unknown),
        }
    }

    /// Builds the object. TypeScript requires every property of an object with an index
    /// signature to match the signature's value type, so a typed index signature next to
    /// properties takes the union of its own type and every property type, plus `undefined`
    /// when a property is optional (`unknown` when that union is too large, contains
    /// `unknown`, or does not fit the size bound). `tsc` rejects an optional property under an
    /// index type without `undefined`.
    fn assemble_object(
        &mut self,
        properties: Vec<Property>,
        index: Option<Node>,
        indent: usize,
    ) -> Node {
        let index = match index {
            Some(index) if index != Node::Unknown && !properties.is_empty() => {
                Some(self.index_union(&index, &properties, indent))
            }
            other => other,
        };
        Node::Object(ObjectNode {
            properties,
            index: index.map(Box::new),
        })
    }

    /// The flattened, deduplicated union for a typed index signature. Every copied subtree is charged
    /// before it is cloned, so nested objects cannot double the tree at each level.
    fn index_union(&mut self, index: &Node, properties: &[Property], indent: usize) -> Node {
        let optional = properties.iter().any(|property| property.optional);
        let mut seen = HashSet::new();
        let mut unique: Vec<&Node> = Vec::new();
        let mut bytes = 0usize;
        for node in std::iter::once(index).chain(properties.iter().map(|p| &p.node)) {
            if *node == Node::Unknown {
                return Node::Unknown;
            }
            let members = match node {
                Node::Union(inner) => inner.as_slice(),
                other => std::slice::from_ref(other),
            };
            for member in members {
                if seen.insert(member.render(RenderStyle::Compact)) {
                    let declaration = member.render(RenderStyle::Declaration);
                    let lines = declaration.matches('\n').count();
                    bytes += declaration.len() + lines * (indent + 2) + 3;
                    unique.push(member);
                }
                if unique.len() > MAX_UNION_MEMBERS || bytes > MAX_RENDERED_TYPE_BYTES {
                    return Node::Unknown;
                }
            }
        }
        if !self.charge(bytes) {
            return Node::Unknown;
        }
        let mut members: Vec<Node> = unique.into_iter().cloned().collect();
        if optional {
            members.push(Node::Primitive("undefined"));
        }
        self.union(members)
    }

    /// `JSDoc` lines for a member: its description and the tags of [`DOC_TAG_KEYWORDS`].
    fn member_doc(&mut self, schema: &Value, indent: usize) -> Vec<String> {
        let Some(object) = schema.as_object() else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        if let Some(description) = object.get("description").and_then(Value::as_str) {
            lines.push(sanitize_jsdoc(description, MEMBER_DOC_CHARS));
        }
        for (keyword, kind) in DOC_TAG_KEYWORDS {
            if let Some(text) = object.get(*keyword).and_then(|v| tag_text(v, *kind)) {
                lines.push(format!(
                    "@{keyword} {}",
                    sanitize_jsdoc(&text, MEMBER_DOC_CHARS)
                ));
            }
        }
        let bytes: usize = lines.iter().map(|line| indent + line.len() + 4).sum();
        let framing = if lines.is_empty() {
            0
        } else {
            2 * (indent + 5)
        };
        if self.charge(bytes + framing) {
            lines
        } else {
            Vec::new()
        }
    }
}

/// The JSON kind of `value`, as the primitive name its enum members share.
const fn literal_kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// The TypeScript literal type text for `value`; `None` when it cannot be written exactly.
fn literal_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => ts_string_literal_exact(text).map(|body| format!("'{body}'")),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Null => Some("null".to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn tag_text(value: &Value, kind: TagValue) -> Option<String> {
    match (kind, value) {
        (TagValue::Text, Value::String(text)) => Some(text.clone()),
        (TagValue::Number, Value::Number(number)) => Some(number.to_string()),
        (TagValue::Json, other) => serde_json::to_string(other).ok(),
        _ => None,
    }
}

/// Outcome of [`merge_union_members`].
#[derive(Debug, PartialEq)]
enum UnionMerge {
    Unknown,
    TooLarge,
    Members(Vec<Node>),
}

/// Flattens member unions of `nodes` and deduplicates in order.
///
/// Stops at the first `unknown` member or as soon as more than [`MAX_UNION_MEMBERS`] distinct
/// members exist, so the work stays bounded by the cap however many members the schema lists.
/// One level of flattening is enough: every `Node::Union` is built from this function, so its
/// members are never unions themselves.
fn merge_union_members(nodes: impl IntoIterator<Item = Node>) -> UnionMerge {
    let mut unique: Vec<Node> = Vec::new();
    for node in nodes {
        let members = match node {
            Node::Union(inner) => inner,
            other => vec![other],
        };
        for member in members {
            if member == Node::Unknown {
                return UnionMerge::Unknown;
            }
            if !unique.contains(&member) {
                if unique.len() == MAX_UNION_MEMBERS {
                    return UnionMerge::TooLarge;
                }
                unique.push(member);
            }
        }
    }
    UnionMerge::Members(unique)
}

/// The members of an object-like `node`, `None` when it is not object-like.
fn summarize(node: &Node) -> Option<Vec<MemberSummary>> {
    match node {
        Node::Object(object) => Some(
            object
                .properties
                .iter()
                .map(|property| MemberSummary {
                    name: property.name.clone(),
                    type_text: property.node.render(RenderStyle::Compact),
                    required: !property.optional,
                })
                .collect(),
        ),
        Node::Intersection(parts) => merge_members(parts, false),
        Node::Union(parts) => merge_members(parts, true),
        _ => None,
    }
}

/// Merges the members of object-like `parts`; a member is required when it is required in every
/// part of a union, or in any part of an intersection.
fn merge_members(parts: &[Node], required_in_all: bool) -> Option<Vec<MemberSummary>> {
    let summaries: Vec<Vec<MemberSummary>> =
        parts.iter().map(summarize).collect::<Option<Vec<_>>>()?;
    let mut merged: Vec<MemberSummary> = Vec::new();
    for summary in &summaries {
        for member in summary {
            if merged.iter().any(|m| m.name == member.name) {
                continue;
            }
            let required = if required_in_all {
                summaries
                    .iter()
                    .all(|s| s.iter().any(|m| m.name == member.name && m.required))
            } else {
                summaries
                    .iter()
                    .any(|s| s.iter().any(|m| m.name == member.name && m.required))
            };
            merged.push(MemberSummary {
                required,
                ..member.clone()
            });
        }
    }
    Some(merged)
}

#[cfg(test)]
mod acceptance_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn compact(schema: &Value) -> String {
        convert_schema(schema).render(RenderStyle::Compact)
    }

    #[test]
    fn typed_index_signature_flattens_member_unions() {
        let schema = json!({
            "type": "object",
            "properties": {"a": {"type": ["number", "null"]}},
            "additionalProperties": {"type": "number"}
        });
        assert_eq!(
            compact(&schema),
            "{ a?: number | null; [key: string]: number | null | undefined }"
        );
    }

    #[test]
    fn nested_unions_are_flattened_and_deduplicated() {
        let schema = json!({"anyOf": [{"type": ["string", "null"]}, {"type": "string"}, {"type": "number"}]});
        assert_eq!(compact(&schema), "string | null | number");
    }

    #[test]
    fn merge_union_members_stops_pulling_nodes_once_over_the_cap() {
        let total = 128;
        let pulled = std::cell::Cell::new(0usize);
        let nodes = (0..total).map(|group| {
            pulled.set(pulled.get() + 1);
            Node::Union(
                (0..80)
                    .map(|i| Node::Literal(format!("{}", group * 80 + i)))
                    .collect(),
            )
        });

        assert_eq!(merge_union_members(nodes), UnionMerge::TooLarge);
        assert!(
            pulled.get() <= 3,
            "pulled {} of {total} nodes",
            pulled.get()
        );
    }

    #[test]
    fn array_of_union_is_parenthesized() {
        let schema = json!({"type": "array", "items": {"enum": ["a", "b"]}});
        assert_eq!(compact(&schema), "('a' | 'b')[]");
    }

    #[test]
    fn union_inside_intersection_is_parenthesized() {
        let schema = json!({
            "type": "object",
            "properties": {"a": {"type": "string"}},
            "additionalProperties": false,
            "oneOf": [{"type": "object"}, {"type": "null"}]
        });
        assert!(compact(&schema).contains(" & ("), "{}", compact(&schema));
    }

    #[test]
    fn typed_additional_properties_next_to_properties_share_one_index_union() {
        let schema = json!({
            "type": "object",
            "properties": {"a": {"type": "string"}},
            "additionalProperties": {"type": "number"}
        });
        assert_eq!(
            compact(&schema),
            "{ a?: string; [key: string]: number | string | undefined }"
        );
    }

    #[test]
    fn narrowing_keyword_at_the_root_keeps_the_type_representable() {
        let converted = convert_schema(&json!({
            "type": "array", "items": {"type": "string"}, "uniqueItems": true
        }));
        assert_eq!(converted.render(RenderStyle::Compact), "string[]");
        assert!(!converted.root_degraded());
        assert_eq!(
            converted.diagnostics()[0].kind,
            DiagnosticKind::NarrowingKeywordIgnored
        );
    }

    #[test]
    fn oversized_enum_is_bounded_before_any_deduplication() {
        let values: Vec<Value> = (0..20_000).map(|i| json!(format!("v{}", i % 3))).collect();
        let converted = convert_schema(&json!({"enum": values}));
        assert_eq!(converted.render(RenderStyle::Compact), "string");
        assert_eq!(
            converted.diagnostics()[0].kind,
            DiagnosticKind::UnionTooLarge
        );
    }

    #[test]
    fn duplicate_enum_members_collapse_in_declared_order() {
        let converted = convert_schema(&json!({"enum": ["b", "a", "b", 1, 1, null, "a"]}));
        assert_eq!(
            converted.render(RenderStyle::Compact),
            "'b' | 'a' | 1 | null"
        );
    }

    #[test]
    fn repeated_references_to_a_huge_enum_stop_at_the_size_bound() {
        let values: Vec<Value> = (0..5_000).map(|i| json!(format!("v{i}"))).collect();
        let properties: Map<String, Value> = (0..MAX_REF_EXPANSIONS)
            .map(|i| (format!("p{i}"), json!({"$ref": "#/$defs/E"})))
            .collect();
        let converted = convert_schema(&json!({
            "$defs": {"E": {"enum": values}},
            "type": "object",
            "properties": properties
        }));
        assert!(
            converted
                .diagnostics()
                .iter()
                .any(|d| d.kind == DiagnosticKind::SizeExceeded),
            "{:?}",
            converted.diagnostics()
        );
    }

    #[test]
    fn typed_index_unions_do_not_double_the_tree_per_nesting_level() {
        let mut schema = json!({"type": "string"});
        for _ in 0..12 {
            schema = json!({
                "type": "object",
                "properties": {"a": schema},
                "additionalProperties": {"type": "number"}
            });
        }

        let converted = convert_schema(&schema);

        let declaration = converted.render(RenderStyle::Declaration);
        assert!(
            declaration.len() <= MAX_RENDERED_TYPE_BYTES + 64,
            "{} B",
            declaration.len()
        );
        assert!(converted.render(RenderStyle::Compact).len() <= declaration.len());
    }

    #[test]
    fn closed_empty_object_is_a_record_of_never() {
        let schema = json!({"type": "object", "additionalProperties": false});
        assert_eq!(compact(&schema), "Record<string, never>");
    }

    #[test]
    fn dropped_diagnostics_are_counted() {
        let properties: Map<String, Value> = (0..2 * MAX_DIAGNOSTICS)
            .map(|i| {
                (
                    format!("p{i}"),
                    json!({"$ref": "https://example.invalid/x"}),
                )
            })
            .collect();
        let converted = convert_schema(&json!({"type": "object", "properties": properties}));
        assert_eq!(converted.diagnostics().len(), MAX_DIAGNOSTICS);
        assert_eq!(converted.dropped_diagnostics(), MAX_DIAGNOSTICS);
    }
}
