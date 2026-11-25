---
tags:
  - sdd
  - delta
  - server
created: 2026-11-26
status: merged
related:
  - "[[../../spec]]"
---

# Delta: device names and pre-1.0 hardening (server)

Issues: #702. Merged into [[../../spec]].

## ADDED

No new requirement ids; additions are inside the modified sections below.

## MODIFIED

### `rmcp` log cap (#421, #702)

Old:

> `main.rs`'s `log_filter(env)` adds the directive of
> `mcp_execution_core::cli::RmcpLogCap::Info` (`rmcp=info`; the one statement of the cap policy, also used by
> `mcp-cli`, [[../core/spec]], #608, #678), which closes this specific path — *debug-level, raw-line*
> logging — by `add_directive`-ing `rmcp=info` onto the *result* of
> `EnvFilter::try_from_default_env().unwrap_or_else(...)`, not folded into only the fallback string —
> a directive added solely to the fallback is dead code whenever `RUST_LOG` parses successfully,
> which is exactly the case this exists to cover.
>
> The cap is level-based, not a content filter, so it does not cover every untrusted-data-into-log
> site: `rmcp`'s `service.rs` also logs a `Debug`-formatted peer notification at `info`
> (`tracing::info!(?notification, "received notification")`), which `rmcp=info` does not and cannot
> suppress — `info` is exactly the level this cap still allows. That site fires under this binary's
> *default* filter with no `RUST_LOG` set at all. It is mitigated, not eliminated: `?` renders via
> `Debug`, which escapes control characters, so it lacks the newline/ANSI-forging vector the
> `message`-field `debug!` sites carry — but the full notification content still reaches the log
> verbatim otherwise. Closing that site (if ever warranted) would require a content-level
> sanitization of `rmcp`'s own event, not a level directive.
>
> `tracing_subscriber` orders directives by target specificity, so this `rmcp=info` directive (more
> specific than a bare global `debug`) wins over it; an operator who explicitly sets a *more
> specific* directive, e.g. `RUST_LOG=rmcp::transport=debug`, still wins over this one —
> intentional, since the goal is closing the accidental broad-`debug` case, not blocking a
> deliberate request for `rmcp` transport debug logs, and it is the escape hatch for an operator who
> needs one. Specificity is not the same axis as level, though: an *equally* specific
> `RUST_LOG=rmcp=debug` (same `rmcp` target this cap sets, different level) is not merged with this
> cap's `rmcp=info` — `tracing_subscriber`'s `Directive` ordering does not compare level, so
> `EnvFilter::add_directive` on a same-target directive *replaces* the existing entry, silently
> downgrading an operator's explicit `rmcp=debug` to `info`. Both the escape hatch and the
> replace-not-merge behavior are pinned by dedicated tests rather than assumed, per the original
> audit's request. `log_filter` is a pure function (not env-mutating), so it is unit-tested
> directly with a scoped subscriber rather than by setting `RUST_LOG` in-process (parallel test
> threads share one process).

New:

> `main.rs`'s `log_filter(env) -> Result<(EnvFilter, Targets)>` returns the base filter
> (`EnvFilter::try_from_default_env()`, default `info,mcp_execution_server=debug`) and the cap built from
> `mcp_execution_core::cli::RmcpLogCap::Info` (the one statement of the cap policy, also used by `mcp-cli`,
> [[../core/spec]], #608, #678). The subscriber applies base, then cap, then the format layer. The cap applies whether
> or not `RUST_LOG` is set.
>
> **AND filter (#702).** An event is emitted only if the base filter enables it **and** the cap does. The cap is a
> `Targets` filter parsed from `cap_filter_directives(&base.to_string())`: `rmcp` at `info`, plus the base filter's
> `rmcp::` target directives (without span or field parts) as exemptions. `cap_filter_directives` returns a `trace`
> default, which the binary replaces with the base filter's max level hint. Consequences:
>
> | `RUST_LOG` | `rmcp` events emitted |
> |---|---|
> | `error` | errors only |
> | `debug` | up to `info` |
> | `rmcp=debug` | up to `info` |
> | `rmcp=error` | errors only |
> | `rmcp::transport=debug` | `rmcp::transport` up to `debug`, nothing else under `rmcp` |
>
> The last row is the escape hatch: a directive naming an `rmcp::` submodule is a deliberate request and is not capped.
> Directives with a span or field part are not exempt.
>
> The cap is level-based, not a content filter, so it does not cover every untrusted-data-into-log
> site: `rmcp`'s `service.rs` also logs a `Debug`-formatted peer notification at `info`
> (`tracing::info!(?notification, "received notification")`), which the cap does not suppress:
> `info` is exactly the level it allows. That site fires under this binary's *default* filter with no
> `RUST_LOG` set at all. It is mitigated, not eliminated: `?` renders via `Debug`, which escapes control
> characters, so it lacks the newline/ANSI-forging vector the `message`-field `debug!` sites carry, but
> the full notification content still reaches the log verbatim otherwise. Closing that site (if ever
> warranted) would require a content-level sanitization of `rmcp`'s own event, not a level directive.
>
> `log_filter` is a pure function (not env-mutating), so it is unit-tested directly with a scoped
> subscriber rather than by setting `RUST_LOG` in-process (parallel test threads share one process).

## REMOVED

None.

## Deviations

- The binary rebuilds the parsed cap filter to replace its `trace` default with the base filter's max level hint.
