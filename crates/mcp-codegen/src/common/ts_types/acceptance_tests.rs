//! Acceptance tests for #565 and #584: the converter matrix, diagnostics, literals and keys.

use super::*;
use crate::common::typescript::ts_string_literal_exact;
use serde_json::json;

fn compact(schema: &Value) -> String {
    convert_schema(schema).render(RenderStyle::Compact)
}

fn declaration(schema: &Value) -> String {
    convert_schema(schema).render(RenderStyle::Declaration)
}

fn kinds(converted: &ConvertedType) -> Vec<DiagnosticKind> {
    converted.diagnostics().iter().map(|d| d.kind).collect()
}

fn object_with(properties: &Value) -> Value {
    json!({"type": "object", "properties": properties})
}

fn member<'a>(converted: &'a ConvertedType, name: &str) -> &'a MemberSummary {
    converted
        .members()
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("member {name} must be representable"))
}

#[test]
fn convert_schema_enum_renders_deduplicated_literal_union_in_order() {
    let schema = json!({"type": "string", "enum": ["open", "closed", "open", "all"]});
    assert_eq!(compact(&schema), "'open' | 'closed' | 'all'");
}

#[test]
fn convert_schema_const_renders_a_literal() {
    assert_eq!(compact(&json!({"const": "fixed"})), "'fixed'");
    assert_eq!(compact(&json!({"const": 3})), "3");
    assert_eq!(compact(&json!({"const": true})), "true");
}

#[test]
fn convert_schema_type_array_with_null_renders_nullable_union() {
    assert_eq!(
        compact(&json!({"type": ["string", "null"]})),
        "string | null"
    );
}

#[test]
fn convert_schema_keeps_optional_and_nullable_distinct() {
    let converted = convert_schema(&json!({
        "type": "object",
        "properties": {
            "nullable_required": {"type": ["string", "null"]},
            "optional_plain": {"type": "string"}
        },
        "required": ["nullable_required"]
    }));

    let nullable_required = member(&converted, "nullable_required");
    assert_eq!(nullable_required.type_text, "string | null");
    assert!(nullable_required.required);
    let optional_plain = member(&converted, "optional_plain");
    assert_eq!(optional_plain.type_text, "string");
    assert!(!optional_plain.required);
}

#[test]
fn convert_schema_one_of_and_any_of_render_unions() {
    let members = json!([{"type": "string"}, {"type": "number"}]);
    assert_eq!(compact(&json!({"oneOf": members})), "string | number");
    assert_eq!(compact(&json!({"anyOf": members})), "string | number");
}

#[test]
fn convert_schema_all_of_renders_an_intersection() {
    let rendered = compact(&json!({"allOf": [
        object_with(&json!({"a": {"type": "string"}})),
        object_with(&json!({"b": {"type": "number"}}))
    ]}));

    assert!(rendered.contains(" & "), "{rendered}");
    assert!(
        rendered.contains('a') && rendered.contains('b'),
        "{rendered}"
    );
}

#[test]
fn convert_schema_all_of_over_the_bound_degrades_to_unknown() {
    let members: Vec<Value> = (0..=MAX_INTERSECTION_MEMBERS)
        .map(|_| object_with(&json!({"a": {"type": "string"}})))
        .collect();
    let converted = convert_schema(&json!({"allOf": members}));

    assert_eq!(converted.render(RenderStyle::Compact), "unknown");
    assert!(kinds(&converted).contains(&DiagnosticKind::IntersectionTooLarge));
}

#[test]
fn convert_schema_one_of_over_the_bound_degrades_to_unknown() {
    let members: Vec<Value> = (0..=MAX_UNION_MEMBERS)
        .map(|i| json!({"const": i}))
        .collect();
    let converted = convert_schema(&json!({"oneOf": members}));

    assert_eq!(converted.render(RenderStyle::Compact), "unknown");
    assert!(kinds(&converted).contains(&DiagnosticKind::UnionTooLarge));
}

#[test]
fn convert_schema_enum_over_the_bound_renders_the_shared_base_primitive() {
    let values: Vec<String> = (0..=MAX_UNION_MEMBERS).map(|i| format!("v{i}")).collect();
    let converted = convert_schema(&json!({"type": "string", "enum": values}));

    assert_eq!(converted.render(RenderStyle::Compact), "string");
    assert!(kinds(&converted).contains(&DiagnosticKind::UnionTooLarge));
}

#[test]
fn convert_schema_local_ref_is_resolved_against_the_root() {
    let converted = convert_schema(&json!({
        "$defs": {"Id": {"type": "string"}},
        "type": "object",
        "properties": {"id": {"$ref": "#/$defs/Id"}}
    }));

    assert_eq!(member(&converted, "id").type_text, "string");
    assert_eq!(kinds(&converted), Vec::<DiagnosticKind>::new());
}

#[test]
fn convert_schema_cyclic_ref_is_cut_with_unknown_and_a_diagnostic() {
    let converted = convert_schema(&json!({
        "$defs": {"Node": {"type": "object", "properties": {"next": {"$ref": "#/$defs/Node"}}}},
        "$ref": "#/$defs/Node"
    }));

    assert!(kinds(&converted).contains(&DiagnosticKind::CyclicRef));
    assert!(converted.render(RenderStyle::Compact).contains("unknown"));
}

#[test]
fn convert_schema_remote_ref_is_unknown_without_fetching() {
    let converted = convert_schema(&json!({"$ref": "https://example.invalid/schema.json"}));

    assert_eq!(converted.render(RenderStyle::Compact), "unknown");
    assert_eq!(kinds(&converted), vec![DiagnosticKind::NonLocalRef]);
    assert!(converted.root_degraded());
}

#[test]
fn convert_schema_additional_properties_forms() {
    let base = |extra: Value| {
        let mut schema = object_with(&json!({"a": {"type": "string"}}));
        if let Some(value) = extra.as_object() {
            for (k, v) in value {
                schema[k] = v.clone();
            }
        }
        compact(&schema)
    };

    assert!(base(json!({})).contains("[key: string]: unknown"));
    assert!(base(json!({"additionalProperties": true})).contains("[key: string]: unknown"));
    assert!(!base(json!({"additionalProperties": false})).contains("[key:"));
    assert!(
        base(json!({"additionalProperties": {"type": "number"}})).contains("[key: string]: number")
    );
}

#[test]
fn convert_schema_narrowing_keywords_keep_the_superset_type_with_a_diagnostic() {
    let converted = convert_schema(&json!({"type": "string", "not": {"enum": ["x"]}}));

    assert_eq!(converted.render(RenderStyle::Compact), "string");
    assert!(kinds(&converted).contains(&DiagnosticKind::NarrowingKeywordIgnored));
}

#[test]
fn convert_schema_tuple_items_render_unknown_array_with_a_diagnostic() {
    for schema in [
        json!({"type": "array", "prefixItems": [{"type": "string"}, {"type": "number"}]}),
        json!({"type": "array", "items": [{"type": "string"}, {"type": "number"}]}),
    ] {
        let converted = convert_schema(&schema);
        assert_eq!(converted.render(RenderStyle::Compact), "unknown[]");
        assert!(kinds(&converted).contains(&DiagnosticKind::TupleItems));
    }
}

#[test]
fn convert_schema_emits_jsdoc_tags_in_declaration_form_only() {
    let schema = object_with(&json!({
        "n": {"type": "integer", "format": "int32", "default": 5, "minimum": 1, "description": "count"}
    }));

    let declared = declaration(&schema);
    for expected in ["/**", "@format int32", "@default 5", "@minimum 1", "count"] {
        assert!(
            declared.contains(expected),
            "missing {expected}: {declared}"
        );
    }
    let compact = compact(&schema);
    assert!(
        !compact.contains("/**") && !compact.contains('@'),
        "{compact}"
    );
}

#[test]
fn convert_schema_long_enum_string_is_not_rendered_as_a_truncated_value() {
    let long = "a".repeat(600);
    let converted = convert_schema(&json!({"type": "string", "enum": [long]}));

    assert_eq!(converted.render(RenderStyle::Compact), "string");
    assert!(kinds(&converted).contains(&DiagnosticKind::LiteralNotRepresentable));
}

#[test]
fn convert_schema_neutralizes_a_comment_terminator_in_a_description() {
    let schema = object_with(&json!({
        "a": {"type": "string", "description": "first */ export const injected = 1; /*"}
    }));

    let declared = declaration(&schema);

    assert!(!declared.contains("*/ export const injected"), "{declared}");
    assert!(
        declared.contains("/**"),
        "descriptions still render as JSDoc: {declared}"
    );
}

#[test]
fn convert_schema_is_byte_identical_and_keeps_declared_order() {
    let schema = object_with(&json!({
        "zeta": {"type": "string"},
        "alpha": {"enum": ["b", "a"]},
        "mid": {"type": ["number", "null"]}
    }));

    let first = declaration(&schema);
    assert_eq!(first, declaration(&schema));
    let order: Vec<usize> = ["zeta", "alpha", "mid"]
        .iter()
        .map(|k| first.find(k).unwrap_or(usize::MAX))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{first}");
    assert!(
        first.find("'b'").unwrap() < first.find("'a'").unwrap(),
        "enum order: {first}"
    );
}

fn amplifying_schema(levels: usize) -> Value {
    let mut defs = serde_json::Map::new();
    for level in 0..levels {
        let next = format!("#/$defs/L{}", level + 1);
        defs.insert(
            format!("L{level}"),
            json!({"type": "object", "properties": {
                "left": {"$ref": next}, "right": {"$ref": next}
            }}),
        );
    }
    defs.insert(format!("L{levels}"), json!({"type": "string"}));
    json!({"$defs": defs, "$ref": "#/$defs/L0"})
}

#[test]
fn convert_schema_ref_amplification_stays_within_the_size_bound() {
    let converted = convert_schema(&amplifying_schema(40));

    let rendered = converted.render(RenderStyle::Declaration);
    assert!(
        rendered.len() <= MAX_RENDERED_TYPE_BYTES + 64,
        "{} bytes",
        rendered.len()
    );
    assert!(
        kinds(&converted).iter().any(|k| matches!(
            k,
            DiagnosticKind::RefExpansionLimit | DiagnosticKind::SizeExceeded
        )),
        "a bound hit must be reported: {:?}",
        converted.diagnostics()
    );
}

#[test]
fn convert_schema_deep_nesting_degrades_with_a_depth_diagnostic() {
    let mut schema = json!({"type": "string"});
    for _ in 0..200 {
        schema = json!({"type": "array", "items": schema});
    }

    let converted = convert_schema(&schema);

    assert!(kinds(&converted).contains(&DiagnosticKind::DepthExceeded));
}

#[test]
fn convert_schema_caps_the_number_of_diagnostics() {
    let properties: serde_json::Map<String, Value> = (0..4 * MAX_DIAGNOSTICS)
        .map(|i| {
            (
                format!("p{i}"),
                json!({"$ref": format!("https://example.invalid/{i}")}),
            )
        })
        .collect();
    let converted = convert_schema(&object_with(&Value::Object(properties)));

    assert_ne!(kinds(&converted), Vec::<DiagnosticKind>::new());
    assert!(converted.diagnostics().len() <= MAX_DIAGNOSTICS);
}

#[test]
fn convert_schema_non_object_root_is_unknown_with_unsupported_type() {
    let converted = convert_schema(&json!("not a schema"));

    assert_eq!(converted.render(RenderStyle::Compact), "unknown");
    assert_eq!(kinds(&converted), vec![DiagnosticKind::UnsupportedType]);
    assert!(converted.root_degraded());
}

#[test]
fn convert_schema_compact_has_no_newline_or_jsdoc_and_is_not_longer() {
    let schemas = [
        object_with(&json!({
            "a": {"type": "string", "description": "d"},
            "b": {"enum": ["x", "y"], "default": "x"}
        })),
        json!({"oneOf": [object_with(&json!({"a": {"type": "number"}})), {"type": "string"}]}),
        json!({"type": "array", "items": object_with(&json!({"k": {"type": "boolean"}}))}),
    ];

    for schema in &schemas {
        let converted = convert_schema(schema);
        let short = converted.render(RenderStyle::Compact);
        let long = converted.render(RenderStyle::Declaration);
        assert!(!short.contains('\n') && !short.contains("/**"), "{short}");
        assert!(short.len() <= long.len());
    }
    assert!(declaration(&schemas[0]).contains('\n'));
}

#[test]
fn convert_schema_non_identifier_keys_render_quoted_with_their_raw_value() {
    let converted = convert_schema(&object_with(&json!({
        "per-page": {"type": "integer"},
        "filter.name": {"type": "string"},
        "1st": {"type": "string"},
        "plain_key": {"type": "string"}
    })));
    let declared = converted.render(RenderStyle::Declaration);

    for key in ["'per-page'?:", "'filter.name'?:", "'1st'?:"] {
        assert!(declared.contains(key), "missing {key}: {declared}");
    }
    assert!(
        declared.contains("plain_key?:"),
        "identifier keys stay bare: {declared}"
    );
    assert_eq!(member(&converted, "per-page").type_text, "number");
}

#[test]
fn convert_schema_colliding_sanitized_keys_both_survive_without_suffix() {
    let converted = convert_schema(&object_with(&json!({
        "a-b": {"type": "string"},
        "a.b": {"type": "number"}
    })));
    let declared = converted.render(RenderStyle::Declaration);

    assert!(
        declared.contains("'a-b'?: string") && declared.contains("'a.b'?: number"),
        "{declared}"
    );
    assert!(!declared.contains("_2"), "{declared}");
    assert_eq!(converted.members().len(), 2);
}

#[test]
fn convert_schema_unrepresentable_key_becomes_index_signature_with_diagnostic() {
    let converted = convert_schema(&object_with(&json!({
        "safe": {"type": "string"},
        "evil\u{202e}key": {"type": "string"}
    })));
    let declared = converted.render(RenderStyle::Declaration);

    assert!(!declared.contains('\u{202e}'), "{declared:?}");
    assert!(declared.contains("[key: string]: unknown"), "{declared}");
    assert!(kinds(&converted).contains(&DiagnosticKind::KeyNotRepresentable));
}

#[test]
fn convert_schema_members_report_raw_names_requiredness_and_object_likeness() {
    let converted = convert_schema(&json!({
        "type": "object",
        "properties": {"per-page": {"type": "integer"}, "q": {"type": "string"}},
        "required": ["q"]
    }));

    assert!(converted.is_object_like());
    let names: Vec<&str> = converted
        .members()
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["per-page", "q"]);
    assert!(member(&converted, "q").required);
    assert!(!member(&converted, "per-page").required);
    assert!(!convert_schema(&json!({"type": "string"})).is_object_like());
}

#[test]
fn convert_schema_union_of_objects_is_object_like() {
    let converted = convert_schema(&json!({"oneOf": [
        object_with(&json!({"a": {"type": "string"}})),
        object_with(&json!({"b": {"type": "string"}}))
    ]}));

    assert!(converted.is_object_like());
}

fn decode_js_single_quoted(body: &str) -> String {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next().expect("dangling escape") {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
            }
            other => out.push(other),
        }
    }
    out
}

#[test]
fn ts_string_literal_exact_round_trips_representable_text() {
    assert_eq!(ts_string_literal_exact("open").as_deref(), Some("open"));
    for raw in ["it's", "back\\slash", "tab\there", "line\nbreak", "ünïcode"] {
        let body = ts_string_literal_exact(raw)
            .unwrap_or_else(|| panic!("{raw:?} has a lossless literal form"));
        assert!(
            !body.contains('\n'),
            "no raw newline inside a literal: {body:?}"
        );
        assert!(
            body.match_indices('\'')
                .all(|(i, _)| body[..i].ends_with('\\')),
            "every quote is escaped: {body:?}"
        );
        assert_eq!(decode_js_single_quoted(&body), raw);
    }
}

#[test]
fn ts_string_literal_exact_rejects_what_it_cannot_keep_exactly() {
    let at_limit = "a".repeat(500);
    assert!(ts_string_literal_exact(&at_limit).is_some());
    assert_eq!(ts_string_literal_exact(&"a".repeat(501)), None);
    assert_eq!(ts_string_literal_exact("a\u{202e}b"), None);
}
