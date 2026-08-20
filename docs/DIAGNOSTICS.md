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
