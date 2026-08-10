---
aliases:
  - ADR-447
  - single display-form tool-name lookup
tags:
  - sdd
  - decision
  - security
  - server
created: 2026-08-11
status: accepted
review-date: 2027-02-11
related:
  - "[[../constitution]]"
  - "[[../server/spec]]"
  - "[[../core/spec]]"
  - "[[../README]]"
---

# ADR-447: Simplify `display_forms`/`display_tool_name` to a single display-form lookup

## 1. Context

Issue #307 gave `validate_categorized_tools` a display-name→raw-name lookup so a caller could
echo back the *display* form of a tool name `introspect_server` showed it, not the raw one.
`display_forms` computed **two** plausible display keys per raw tool name — the fully
entity-escaped form (`display_tool_name`) and the same text with `&`/`<`/`>` entities decoded —
because at the time a raw name could still contain `&`/`<`/`>` or control characters, and
`wrap_untrusted_block`'s preamble explicitly invites the LLM reader to decode escaped entities
back to their literal form.

Issue #433 introduced a Unicode-identifier allowlist in `ToolName::new`
(`first_disallowed_identifier_char`, [[../core/spec]]) that rejects every character
`display_tool_name` ever transforms: control characters, line terminators, and `&`/`<`/`>`. As a
result, for any valid `ToolName` under `sanitize_untrusted_text`'s truncation point, a raw tool
name and its display form are now always identical — `display_forms` returns a single-element
vector in every reachable case. The dual-form branch became structurally dead: still compiled,
still tested (five near-identical tests, ~350 lines, each carrying "originally about X, closed by
construction" archaeology), but never exercised by anything a caller could actually construct.

## 2. Options Considered

### (a) Keep `display_forms` as defense-in-depth

Leave the two-form lookup in place in case `ToolName`'s allowlist is ever loosened.

**Pros:** zero migration effort; no risk of removing something that becomes needed later.
**Cons:** the trigger for this branch is not reachable at all today — `ToolName`'s inner field is
private and every construction path (including `Deserialize`, via `#[serde(try_from = "String")]`)
routes through `ToolName::new`, so there is no way to hold a `ToolName` whose value contains
`&`/`<`/`>` or a control character. Code with zero reachable trigger is not defense-in-depth, it
is dead weight: it still has to be read, tested, and reasoned about on every change to this
function, for a scenario that cannot occur without an unrelated, independent change
(`ToolName::new`'s validation itself) that would need its own review anyway.

### (b) Simplify: single `display_tool_name` key, drop `display_forms`

Collapse the lookup to key on `display_tool_name(raw)` alone; drop the second (decoded) form.

**Pros:** removes genuinely dead code and the test archaeology that accumulated around it;
`display_tool_name` becomes the identity function for any valid `ToolName`, which is easy to state
and verify; the S3 ambiguity guard (two distinct raw names colliding on one display key) is kept,
since it is still reachable via `sanitize_untrusted_text`'s truncation at `MAX_UNTRUSTED_FIELD_LEN`
— a real, if narrow, live branch.
**Cons:** if `ToolName::new`'s allowlist is ever loosened to re-admit `&`/`<`/`>` or a control
character, the removed second form would need to be re-added. Concretely, without it a caller
that echoes back the entity-*decoded* literal form of a name (instead of the escaped form
`display_tool_name` produces) would get a hard "not found" error — the exact #307 S2 symptom
`display_forms` originally existed to avoid. Judged acceptable: that allowlist change would
itself be a deliberate, reviewed modification to `first_disallowed_identifier_char`, which is
exactly the moment to re-evaluate this lookup too, not a silent regression a test could catch
today.

### (c) Bound `ToolName`'s length instead

Give `ToolName::new` its own length cap, on the theory that the S3 collision (truncation at
`MAX_UNTRUSTED_FIELD_LEN`) could be prevented at the source instead of guarded against downstream.

**Pros:** would shrink the space of truncation-driven collisions the S3 guard exists for.
**Cons:** rejected on three independent grounds. First, it inverts [[../core/spec]]'s and
[[../README]]'s "resource bounds cascade downward by value" cross-block contract: root
length bounds live at `mcp-introspector::MAX_TOOL_NAME_LEN` (256 bytes) and cascade upward by
value into `mcp-codegen`/`mcp-files`/`mcp-server`; a bound added directly to `ToolName::new` in
`mcp-core` would be a second, independently-chosen root rather than a derived one, and would let a
lower layer (`mcp-core`) reject data a higher layer (`mcp-introspector`) still accepts. Second, it
is a breaking change to `ToolName`'s `Deserialize` behavior — anything holding a `ToolName`
constructed from persisted data (e.g. `_meta.json` sidecars via `mcp_execution_core::metadata`)
could start failing to deserialize on a value that was previously valid. Third, it would make the
S3 ambiguity guard itself untestable at the `mcp-core` layer: the existing regression test
(`test_save_categorized_tools_rejects_ambiguous_display_name_instead_of_misattributing`) directly
constructs two `ToolName`s that differ only past the 500-char truncation point specifically to
exercise S3; a length bound below 500 chars on `ToolName::new` would make that construction itself
fail, deleting the coverage rather than narrowing it.

**Decision: (b).**

## 3. Consequences

- `display_forms` is removed; `display_tool_name` is retained unchanged behaviorally (still
  applies `&`/`<`/`>` escaping) as a mirror of `wrap_untrusted_block`'s own transformation, even
  though it is the identity function for every value reachable today. The escaping itself is not
  a fail-closed guard against misattribution (it's injective); what *is* fail-closed on future
  allowlist drift is having removed `display_forms`'s second lookup key — see (b)'s Cons above.
- The S3 ambiguity guard in `validate_categorized_tools` is kept, narrowed in its doc comment to
  record that its only live trigger today is `sanitize_untrusted_text` truncation, additionally
  masked in production by `mcp-introspector::MAX_TOOL_NAME_LEN` (256 bytes) and the independent
  128-byte `MAX_CATEGORIZED_TOOL_NAME_LEN` cap on a submitted `categorized_tools` entry's `name`
  — and that its real future trigger is drift in
  `first_disallowed_identifier_char`/`sanitize_untrusted_text` toward *collapsing* input (e.g.
  readmitting a control character or bidi control) rather than truncating it; `&`/`<`/`>`
  escaping is injective and so is not itself a source of this collision.
- Five near-identical regression tests pinning "display key == raw name" under different
  historical labels are collapsed into one end-to-end test plus one direct
  `assert_eq!(display_tool_name("a_b"), "a_b")` unit assertion.
- No change to `mcp-core` or `mcp-introspector`; [[../core/spec]] gained one sentence recording
  that `ToolName` deliberately carries no length bound, so option (c) is not re-proposed without
  re-litigating this ADR.

## See Also

- [[../server/spec]] — `save_categorized_tools` / `validate_categorized_tools` contract
- [[../core/spec]] — `ToolName` validation and the no-length-bound note
- [[../README]] — "resource bounds cascade downward by value" cross-block contract
