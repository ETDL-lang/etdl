# Worked examples: ETDL Standard Library 1.0

Four examples, each proving a different part of the architecture:

| File | Proves |
|---|---|
| `service.etdl` (this README) | ETDL source -> module -> import -> user ETDL code, the basic path |
| `generic-composition.etdl` | `std.events` + `std.logic` together, generic composition, no reliability at all |
| `units-limitation.md` | why `std.units` belongs in stdlib once it exists — and why it doesn't yet |
| `future-domain-sketch/` | a future *domain* library (`acme.signals`, optional, not built-in) depending on `std.events` |

## `service.etdl`: the basic path

`service.etdl` declares one library:

```yaml
libraries:
  - name: std.events
    version: "1.0"
```

and references one of its basic events, `std.events.NetworkTimeout`,
directly as a fault-tree gate input — alongside `LocalDiskFull`, an
ordinary, locally-declared basic event:

```yaml
gates:
  G:
    type: OR
    inputs: ["std.events.NetworkTimeout", "LocalDiskFull"]
basicEvents:
  LocalDiskFull:
    description: "the gateway's local disk is full"
    probability: 0.0003
```

Nothing else in the document, and nothing in `etdl-parser`'s type checker,
`etdl-compiler`'s fault-tree evaluator, or the Rust code generator, knows
`std.events.NetworkTimeout` came from a library rather than being declared
right here. That's the point: library resolution runs once, before
validation, and produces an ordinary document with that id already present.

## Running it

```bash
etdl library resolve service.etdl
etdl validate service.etdl
etdl analyze service.etdl
etdl compile service.etdl --out-dir out
```

## Output (captured verbatim)

```text
$ etdl library resolve service.etdl
  std.events requested 1.0 -> resolved 1.0 (built-in) [optional]
```

(`[optional]` reflects `required` defaulting to `false` — the same default
`supplements:` already uses. Set `required: true` to make an unresolvable
library a hard error, `E-116`, instead of a warning, `W-409`.)

```text
$ etdl validate service.etdl
document 'service.etdl' is valid (0 errors, 0 warnings)

$ etdl analyze service.etdl
document: service.etdl
event trees: 1
fault trees: 1
  GatewayUnavailable: topEvent probability = 0.001300
```

`0.001300` is `1 - (1 - 0.001) * (1 - 0.0003)` — the OR of the library's
`NetworkTimeout` (`0.001`, see `../../etdl-compiler/stdlib/events/lib.etdl`) and the
locally-declared `LocalDiskFull` (`0.0003`), evaluated by the *existing*
fault-tree evaluator, completely unmodified for this feature.

```text
$ etdl --verbose compile service.etdl --out-dir out
standard-library manifest written to out/etdl-stdlib-manifest.json
compiled 'service.etdl' to 'out/service.rs' (0 errors, 0 warnings)

$ cat out/etdl-stdlib-manifest.json
{
  "libraries": [
    { "kind": "built-in", "name": "std.events", "version": "1.0" }
  ],
  "schema": "etdl.stdlib/1.0"
}
```

## Trying it offline

Disconnect from the network (or just note that nothing here ever touches
it) — `std.events` resolves because its source is embedded in the `etdl`
binary itself, not fetched or copied from anywhere. Delete this directory's
`api.yaml` and compilation still fails for an unrelated reason (a missing
AsyncAPI import) rather than a missing library, demonstrating the two
"named external thing" mechanisms — `asyncapi_imports` and `libraries` —
are resolved independently.

## Trying an override

Add a local `NetworkTimeout` under a different qualified id, or simply
declare `std.events.NetworkTimeout` directly under this document's own
`basicEvents:` with a different probability — the local declaration wins;
the library's default is only used when the document doesn't supply one
itself. See `docs/reference/standard-library.md` for the full resolution
rules, versioning, and diagnostics reference.

## `generic-composition.etdl`: `std.events` + `std.logic`, no reliability

```bash
etdl analyze generic-composition.etdl
```

```text
document: generic-composition.etdl
event trees: 1
fault trees: 1
  ReadinessComposition: topEvent probability = 0.354000
```

This document declares `libraries: [std.events, std.logic]`, uses
`std.logic.AnyOf` (`OR(SignalA, SignalB, SignalC)`) as its fault tree's
`rootCause` directly (no local wrapper gate needed — a qualified id
resolves as a gate reference exactly like a local one), and overrides all
three placeholder signals with concrete probabilities. `0.354000` is
`1 - (1-0.2)(1-0.15)(1-0.05)`. There is no `supplements:` block anywhere in
this file — the reliability supplement is not involved at all.

## `future-domain-sketch/`: a domain library depending on `std.events`

```bash
cd future-domain-sketch
etdl analyze device-monitor.etdl --library-path .
```

See `future-domain-sketch/README.md` for the full walkthrough: an
illustrative *optional* library, `acme.signals`, declares
`dependsOn: [{name: std.events, ...}]` and composes `std.events`' generic
identities into its own gate — proving a future domain library can build
on the standard library without this repository implementing that domain.

## `units-limitation.md`: why `std.units` isn't built yet

Not a runnable example — a direct demonstration of the silent
unit-confusion problem `std.units` would exist to solve, and why shipping
named constants without real unit checking would be exactly the "unsafe
implicit behavior" this task says not to build. See the file itself.
