# ETDL Diagnostics

Every ETDL diagnostic has a stable code, a severity, a message, and (when the
span index is available) a precise source location. Codes are stable within a
MAJOR version; new codes are added, never reused.

## Severity classes

| Prefix | Severity | Meaning |
|---|---|---|
| `E` | Error | parse/reference error; compilation blocked |
| `V` | Error | semantic validation error; compilation blocked |
| `W` | Warning | advisory; non-fatal |

## Reference (E-1xx)

| Code | Condition | Suggestion |
|---|---|---|
| E-100 | invalid language version, or unimplemented future MAJOR | use `etdl: "1.x.y"` |
| E-101 | reference matches neither External nor Internal grammar; malformed import; or a bare channel name used while `asyncapi_imports` is non-empty (spec Section 5.3.5) | fix the reference string, or use an External Reference for `channel` |
| E-102 | document contains a BOM | save as UTF-8 without BOM |
| E-103 | import alias unknown, or alias has invalid characters | declare it in `asyncapi_imports` |
| E-104 | JSON Pointer does not resolve in the AsyncAPI document | fix the pointer / schema |
| E-105 | internal reference does not match one of the resolved shapes (`#/faultTrees/<id>/topEvent`, `#/faultTrees/<id>/gates/<gate-id>`, `#/faultTrees/<id>/basicEvents/<event-id>`, `#/components/<kind>/<id>` — including `#/components/messages/<id>` for a Message Reference), or `<id>` is unknown | fix the pointer |
| E-106 | a `supplements:` entry's `id` is not a valid supplement identifier (must be `etdl.<domain>`) | fix the id |
| E-107 | a `supplements:` entry's `version` is not valid SemVer, or its MAJOR is newer than this compiler supports | fix the version, or upgrade the compiler |
| E-108 | a `supplements:` entry declares `required: true` but is not implemented by this compiler | remove `required: true`, or use a build that implements it |
| E-109 | code generation failed for a reason validation didn't already catch (e.g. a Barrier using `reliability.in_range`/`performance.in_budget` whose link to a live-tracked fault tree/Budget doesn't resolve — see the Live Reliability/Performance Supplement docs) — should be unreachable if the document is otherwise valid; a codegen-level defensive check, not a normal document-authoring mistake | see the message for which check failed and why |

## Event-tree structure (V-1xx)

| Code | Condition | Suggestion |
|---|---|---|
| V-101 | `next`/`onFailure`/`initiatingEvent.next` does not resolve | create the referenced node |
| V-102 | cycle among nodes | break the cycle |
| V-103 | a node is unreachable from the initiating event | connect it or remove it |
| V-104 | a path does not terminate in a Consequence | end every path in a Consequence |

## Barrier & branch (V-2xx)

| Code | Condition | Suggestion |
|---|---|---|
| V-201 | barrier has < 2 branches | add a branch |
| V-202 | more than one `default`, or `default` not last | keep at most one `default`, last |
| V-203 | branch probability outside [0,1], or sibling sum ≠ 1.0 ± 0.0001 | adjust probabilities |
| V-204 | ECEL condition type mismatch or missing schema field | fix the condition / schema |

## Operation & consequence (V-3xx)

| Code | Condition | Suggestion |
|---|---|---|
| V-301 | operation `handler` is not a valid identifier | use an identifier |
| V-302 | `send` consequence omits `channel` or `message` | add both |

## Fault-tree structure (V-4xx)

| Code | Condition | Suggestion |
|---|---|---|
| V-401 | `rootCause`/gate input does not resolve, or probability cannot be computed | fix the reference / probability |
| V-402 | gate and basic event share an ID | rename one |
| V-403 | cycle among gates | break the cycle |
| V-404 | gate/basic event unreachable from `rootCause` | connect it or remove it |

## Gate & basic event (V-5xx)

| Code | Condition | Suggestion |
|---|---|---|
| V-501 | gate arity violates its type | add/remove inputs |
| V-502 | VOTING `k` not in `1..=n` | fix `k` |
| V-503 | basic event supplies both or neither probability/failureRate | supply exactly one |
| V-504 | `failureRate` without `missionTime` | add `missionTime` |
| V-505 | INHIBIT without `inhibitCondition` | add `inhibitCondition` |
| V-506 | transfer target not `#/faultTrees/<id>/...` or references a missing tree | fix target |
| V-507 | basic event `probability`/`failureRate`/`missionTime` is non-finite (NaN/infinity), a rate or duration is negative, or `probability` is outside `[0, 1]` | fix the numeric value |

## Advisory (W-4xx)

| Code | Condition |
|---|---|
| W-401 | operation has no `onFailure` path |
| W-402 | cached branch probability drifted from computed fault-tree value |
| W-405 | transfer has an empty label |
| W-406 | house event declares a probability/failureRate (boundary condition) |
| W-001 | duplicate id under `nodes`/`gates`/`basicEvents` (via span detection) |

## Security Supplement (E-14x / W-411 / W-416)

`etdl.security` (`docs/reference/security-supplement.md`) diagnostics, only
produced when a document declares `supplements: [{id: etdl.security, ...}]`
(and depends on `etdl.tree-event` also being declared — see that doc's §5).
Like Safety, a Control's declared `maxBypassProbability` is **verified
against the document's own resolved numbers**, not just checked for
self-consistency — see that doc's §7.

| Code | Condition | Suggestion |
|---|---|---|
| E-140 | a Threat Model's `leafCategories` value isn't a STRIDE category, `treeRef` doesn't resolve, `threatModels` failed to deserialize, or a duplicate Threat Model id was declared | fix the value / reference / id |
| E-141 | a `leafCategories` key or `mitigates` entry isn't a leaf of the relevant tree, a Control's `nodeRef` doesn't resolve to a Barrier, `mitigates` is empty, exactly one of `bypassOutcome`/`maxBypassProbability` is declared, a `bypassOutcome` doesn't match a branch outcome, `controls` failed to deserialize, or a duplicate Control id was declared | fix the reference / value / id |
| E-142 | a Control's `bypassOutcome` branch resolves to a probability exceeding its declared `maxBypassProbability` | raise the ceiling, or fix whatever is driving that probability |
| E-143 | a branch condition uses the `security.*` ECEL path root without the document declaring `etdl.security`, without also declaring `etdl.live-reliability`, or the path isn't exactly `security.control_effective` | declare both supplements, or fix the path/shape |
| W-411 | a `mitigates` entry is a genuine leaf but no declared Threat Model categorizes it | add a `leafCategories` entry, or accept it's uncategorized |
| W-416 | a Threat Model categorizes a leaf that zero declared Controls' `mitigates` targets | add a mitigating Control, or accept the gap |

## Diagnostics Supplement (E-15x / W-412)

`etdl.diagnostics` (`docs/reference/diagnostics-supplement.md`) diagnostics,
only produced when a document declares `supplements: [{id: etdl.diagnostics,
...}]`:

| Code | Condition | Suggestion |
|---|---|---|
| E-150 | a Correlation's `causeRef`, or an Anomaly Rule's `monitors`, doesn't resolve; or `correlations`/`anomalyRules` failed to deserialize | fix the reference |
| E-151 | two Correlations, or two Anomaly Rules, declare the same id | rename one |
| E-152 | a Correlation's `spanAttribute` is exactly `"etdl.node.id"` but `spanValue` doesn't name a real node anywhere in the document | fix `spanValue`, or a different `spanAttribute` |
| W-412 | a monitored Operation has no correlated cause on record | declare a Correlation targeting the Operation's Fault Tree, or accept the gap |

## Safety Supplement (E-13x / W-410)

`etdl.safety` (`docs/reference/safety-supplement.md`) diagnostics, only
produced when a document declares `supplements: [{id: etdl.safety, ...}]`.
Like Performance/Live Reliability below, a declared `sil` and a declared
`independentOf` claim are **verified against the document's own resolved
numbers**, not just checked for self-consistency — see that doc's
Section 6.

| Code | Condition | Suggestion |
|---|---|---|
| E-130 | a Hazard's `severity`/`likelihood`/`riskIndex`, or a Safety Barrier's `sil`, is invalid; `hazards`/`barriers` failed to deserialize; or a duplicate id was declared | fix the value / id |
| E-131 | a Hazard's `consequenceRef`, or a Safety Barrier's `nodeRef`, doesn't resolve to a node of the required kind; or a Safety Barrier's `failureOutcome` doesn't match one of that Barrier node's own branch outcomes | fix the reference / `failureOutcome` |
| E-132 | two Safety Barriers mutually claim `independentOf` each other while sharing a non-empty `commonCauseGroup` | remove the contradiction (drop the mutual claim, or use distinct groups) |
| E-133 | a Safety Barrier's `failureOutcome` branch resolves to a probability outside the IEC 61508 PFD band its declared `sil` implies | adjust `sil` to match the resolved probability, or fix whatever is driving that probability |
| E-134 | two Safety Barriers declare `independentOf` each other (one-directional) while their `failureOutcome` branches' Fault Trees share a basic event, per minimal-cut-set analysis | remove the `independentOf` claim, or eliminate the shared basic event |
| E-135 | a branch condition uses the `safety.*` ECEL path root without the document declaring `etdl.safety`, without also declaring `etdl.live-reliability`, or the path isn't exactly `safety.sil_maintained` | declare both supplements, or fix the path/shape |
| W-410 | a Hazard's declared `riskIndex` doesn't match the Section 4.1 risk matrix for its severity/likelihood | adjust `riskIndex` to match, or document why it's intentionally more conservative |

## Performance Supplement (E-16x / W-41x)

`etdl.performance` (`docs/reference/performance-supplement.md`) diagnostics,
only produced when a document declares `supplements: [{id: etdl.performance,
...}]`. Like Live Reliability below, this supplement's requirements are
**structurally enforced and observed** by generated code, not just
validated — see that doc's Section 6.

| Code | Condition | Suggestion |
|---|---|---|
| E-160 | a Budget's `nodeRef` doesn't resolve to an Event Tree or Operation node; a percentile/`maxConcurrency`/`expectedRatePerSecond` value is non-positive or non-finite; `budgets` failed to deserialize; or a duplicate budget `id` was declared | fix the reference / value / id |
| E-161 | a Budget's percentile ordering is violated (`p50Ms > p95Ms` or `p95Ms > p99Ms`) | reorder the percentiles |
| E-162 | a `barrierChecks` entry's `nodeRef` doesn't resolve to a Barrier node, its `budgetRef` doesn't resolve to a declared Budget `id`, `barrierChecks` failed to deserialize, or a duplicate `barrierChecks` id was declared | fix the reference / id |
| E-163 | a branch condition uses the `performance.*` path root without the document declaring `etdl.performance`, or the path isn't exactly `performance.in_budget` | declare the supplement, or fix the path |
| W-413 | two Budgets declare the same `nodeRef` | keep one authoritative Budget per node |
| W-415 | two `barrierChecks` entries declare the same `nodeRef` | keep one authoritative Barrier Check per barrier |

## Live Reliability Supplement (E-17x / W-414)

`etdl.live-reliability` (`docs/reference/live-reliability.md`) diagnostics,
only produced when a document declares `supplements: [{id:
etdl.live-reliability, ...}]`. Unlike every other supplement here, a
document declaring this one gets **authoritative** runtime behavior, not
just extra validation — see that doc for the "runtime never changes
compiled probabilities" exception this supplement deliberately is.

| Code | Condition | Suggestion |
|---|---|---|
| E-170 | a `faultTrees` entry's `threshold` or a basic event's `priorStrength` is non-positive/non-finite (`priorStrength`) or negative (`threshold`); a basic event's `source` isn't `local`/`inbound`; or `faultTrees` failed to deserialize | fix the value |
| E-171 | a `faultTrees` entry's `id` doesn't resolve to a declared fault tree, or a `basicEvents` entry's `id` isn't a basic event of that fault tree | fix the reference |
| E-172 | two `faultTrees` entries declare the same `id`, or two `basicEvents` entries (within one fault tree) declare the same `id` | rename or remove the duplicate |
| W-414 | a `local` basic event's `priorStrength` is below 1.0 (a single observation will dominate the declared probability almost immediately) | raise `priorStrength`, or accept the fast-moving estimate |
| E-173 | a branch condition uses the `reliability.*` path root without the document declaring `etdl.live-reliability`, or the path isn't exactly `reliability.in_range` | declare the supplement, or fix the path |

## Example quality target

```
E-103 / V-203: Branch probabilities must sum to 1.0

Current: 0.95 + 0.10 = 1.05
Expected: 1.00
Suggestion: adjust one or more branch probabilities.
```

## Regression coverage

Diagnostics are asserted in `etdl-cli/tests/integration_test.rs` and the unit
suites. Adding a new code must include a regression test that triggers it.
