# ETDL Standard Library 1.0

This document describes how reusable ETDL functionality can be written in
ETDL source itself — the standard library, domain libraries, optional
libraries, and user libraries — and how a document imports and resolves
them. It is additive: nothing here changes the meaning of an ordinary
`.etdl` document that declares no `libraries:`.

## The five layers

```
                    ETDL Core
                        |
             +----------+----------+
             |                     |
       Built-in primitives     Compiler
             |
             v
      ETDL Standard Library      (this document; `std.*`, embedded, 1.0)
             |
       +-----+-----+------------+
       v           v            v
    std.events  std.logic  std.probability   (std.collections/std.units
             |                                 deferred — see below)
             v
   Generic Tree Event Supplement    (etdl.tree-event — a separate extension
             |                       mechanism, `supplements:`, not `libraries:`;
             |                       see generic-tree-event-supplement.md.
             |                       Domain-neutral: no dependency on any
             |                       domain below it)
      +------+-------+--------------+
      v              v              v
 Reliability      Safety/Security  Future tree domains
 Supplement       (future)
      |
      v
 Optional / user libraries
      |
      v
 User ETDL programs
```

| Layer | What it is | Where it lives | Ships with ETDL? |
|---|---|---|---|
| **ETDL Core** | The language, parser, compiler, and runtime primitives (`etdl-parser`, `etdl-compiler`, `etdl-core`) | This repository's crates | Always |
| **ETDL Standard Library** | Reusable ETDL source, embedded in the compiler, reserved `std.*` namespace | `etdl-compiler/stdlib/` in this repository, embedded via `include_str!` | Always (built-in) |
| **ETDL Domain Libraries** | Functionality for one engineering domain (reliability today; safety/security/performance conceivable later) | May be a compiled-in Rust `supplement`/`EtdlExtension` (like `etdl.reliability`) *or* an ETDL-source library depending on `std.*` | Depends on the domain |
| **ETDL Optional Libraries** | Installed separately, resolved from a configured search path | Anywhere on disk; never embedded | No — opt-in |
| **User Libraries** | Project-local reusable definitions | `<project>/lib/<name>/lib.etdl`, resolved relative to the importing document | No — project-specific |

**The core-vs-library rule:** if something can be expressed with existing
ETDL language constructs (a component catalog of basic events, gates,
barriers, operations), it should be a library, not a compiler feature.
Compiler/native implementation remains justified only for things that
genuinely require it — compiler semantics (parsing, type checking, fault-tree
evaluation), runtime primitives (retry, chaos, telemetry), native
performance, FFI, or platform facilities. The reliability domain library is
deliberately *not* moved into this mechanism in this version — it stays a
compiled-in Rust supplement (see "Relationship to supplements" below), and
none of evidence, estimation, `ReliabilityArtifact`, calibration, runtime
observation, or dependency/CCF analysis changes.

## Two kinds of ETDL source

An **[`EtlDocument`]** is a system: event trees, fault trees, a model of
something that runs. A **library** is not a system — it has no event trees
or fault trees of its own. It is a reusable *component catalog*: named
`components` (today, `basic_events`; `gates`/`barriers`/`operations` follow
the same shape and could be provided by a future library the same way) that
an importing document references.

A library file (conventionally `lib.etdl`) is ordinary ETDL, parsed by the
same `etdl-parser` crate, using the exact same `Components`/`BasicEvent`
types a document's own `components:` block already uses — just under a
lighter top-level schema that doesn't require an event tree to exist:

```yaml
etdl: "1.0.0"

library:
  name: std.events
  version: "1.0"
  description: "Reusable basic-event definitions for common failure mechanisms."
  # dependsOn: [{ name: some.other.lib, version: "1.0" }]   # optional

components:
  basic_events:
    NetworkTimeout:
      description: "A network call did not complete within its configured timeout."
      probability: 0.001
```

**A library containing only `.etdl` is valid.** `std.events` has no native
(Rust) component — it is exactly this one file. See
`etdl-compiler/stdlib/events/lib.etdl`.

**A library may also have a native-backed implementation** for cases the
compiler needs a real Rust extension for (this version does not add a new
one; the reliability supplement already demonstrates the pattern via
`EtdlExtension`). Where both exist, the ETDL API is the stable surface; the
native implementation is an optimization/implementation detail an importer
does not need to know about.

## Declaring an import

A document declares libraries the same way it already declares
supplements — a named, versioned entry in a top-level list — because a
library import *is* the same kind of thing a supplement is: a named,
versioned external capability. There is no `use std.events;` statement:
ETDL is a declarative YAML document format with no general executable
syntax (the only embedded expression language, ECEL, exists solely for
barrier branch conditions and has no import/function system of its own), so
the "ordinary ETDL" equivalent of an import statement is a declarative list
entry, exactly like `supplements:`:

```yaml
libraries:
  - name: std.events
    version: "1.0"
    # required: true   # default; false = warn instead of error if unresolved
```

A qualified reference to a library's contents is `<library-name>.<short-name>`,
used anywhere an ordinary basic-event id is used (currently: fault-tree gate
`inputs:`):

```yaml
faultTrees:
  FT:
    topEvent: { id: Top, description: "system unavailable", rootCause: G }
    gates:
      G: { type: OR, inputs: ["std.events.NetworkTimeout", "LocalDiskFull"] }
    basicEvents:
      LocalDiskFull: { description: "local disk full", probability: 0.0003 }
```

A document can always override a library default by declaring the same
qualified id itself under `basicEvents:` — the local declaration wins.

## Resolution

Resolution happens once, before structural validation, and produces a new,
*expanded* document — the original document is never mutated:

```
parsed document
      |
      v
expand_libraries()   <- resolves `libraries:` transitively, splices
      |                  referenced qualified ids into fault trees
      v
expanded document
      |
      v
validate / compile / analyze   <- completely unaware libraries exist;
                                   a qualified id is just another
                                   basic-event id to them
```

This is deliberate: the standard library is a source-expansion concern, not
a collection of compiler special cases. Type checking, fault-tree
evaluation, and code generation were not modified to support libraries —
they don't need to be.

### Where a name resolves from

| Name | Resolves from | Search order |
|---|---|---|
| `std.*` | **Only** the embedded built-in registry | N/A — never searched elsewhere |
| anything else | Optional search paths, then a user-library directory | 1. Each `--library-path` directory's `<name>/lib.etdl`, in the order given. 2. `<document-directory>/lib/<name>/lib.etdl` |

**Shadowing protection is a hard partition, not a precedence rule.** A
`std.*` name is never looked up on a search path or in a project's `lib/`
directory, even if a same-named directory happens to exist there — such a
directory is shadowed, not used, and the built-in resolves as normal. This
is checked directly, not achieved by search-order luck.

### Dependencies and cycles

A library may declare `library.dependsOn: [{name, version}, ...]`.
Resolution is a depth-first walk with an explicit "currently resolving"
stack; a name reappearing on that stack is a cyclic dependency, reported as
a diagnostic (`E-117`), not an infinite loop or a stack overflow. There is
no version-solving beyond the major-version compatibility check below — no
diamond-dependency merge logic, deliberately, per the project's
"do not overengineer" guidance for this version.

### Versioning

Four independent axes already exist or are introduced here; none is a
substitute for another:

| Axis | Example | Where it's checked |
|---|---|---|
| ETDL language version | `doc.etdl: "1.0.0"` | `validate::validate_language_version` (`E-100`) |
| Crate/implementation version | Cargo `workspace.package.version` | N/A (publishing) |
| Artifact/schema version | `etdl.reliability.artifact/1.0`, `etdl.stdlib/1.0` | Exact/prefix match on load |
| **Library version** | `library.version: "1.0"` in a library file; `libraries[].version` in an importing document | `stdlib::check_version_compatible` — major must match |

A document requesting `libraries: [{name: std.events, version: "1.0"}]` is
compatible with any installed `std.events` whose major version is `1`
(mirroring exactly how `doc.etdl` major-version compatibility and
`Supplement::version` major-version compatibility already work). A
mismatched major version is `E-114`, not a silent best-effort substitution.

## Diagnostics

Library diagnostics extend the existing `E-1xx`/`W-4xx` families (the same
families `asyncapi_imports` and `supplements` already use) rather than
inventing a new prefix:

| Code | Severity | Meaning |
|---|---|---|
| `E-113` | error | Invalid library name (must be dotted lowercase segments, e.g. `std.events`) |
| `E-114` | error | Incompatible or unparseable library version |
| `E-115` | error | Invalid library manifest (malformed YAML, or the library's declared name doesn't match how it was resolved) |
| `E-116` | error | A `required: true` (default) library could not be resolved, or a `std.*` name was shadowed and thus unresolved |
| `E-117` | error | Cyclic library dependency |
| `W-409` | warning | A `required: false` library could not be resolved; its definitions simply aren't available (any reference to them then fails as an ordinary undefined-basic-event error) |

No internal filesystem path is exposed in these messages beyond what the
user themselves configured (a `--library-path` value, or the document's own
directory) — the built-in registry's error text never mentions where inside
the compiler binary the embedded source lives.

## Built-in distribution

The standard library's `.etdl` source is embedded directly into the
compiler binary via `include_str!` at compile time (`etdl-compiler/src/stdlib.rs`,
reading from `etdl-compiler/stdlib/`, a sibling of `src/` within the crate —
not the repository root, so the crate stays self-contained when packaged
for crates.io). This means:

- Normal compilation works **offline** — no network access, no manual
  copying of stdlib files, no install step. `etdl compile` and `std.events`
  work the moment you have the `etdl` binary.
- The embedded source flows through automatically to **every** consumer
  of `etdl-compiler` — the CLI, the compiler library itself, tests, and
  `etdl-wasm` (WASM has no filesystem, so this matters: a filesystem-search
  approach would have required a WASM-specific code path, exactly as
  `asyncapi_imports` needed `load_from_content` as its WASM-safe
  alternative to `load`; embedding avoids that entirely for the built-in
  case). Optional/user libraries, which *do* use the filesystem, simply
  aren't available inside a WASM host — this is explicit, not a silent gap.
- There is no existing `include_dir!`-style precedent in this repository;
  this is the first embedded-non-Rust-source pattern here, and it is kept
  to plain `include_str!` per file (no new dependency) since the standard
  library is, deliberately, still small.

## Reproducibility and provenance

`etdl compile` writes `etdl-stdlib-manifest.json` next to the generated
code whenever at least one library was actually resolved (schema
`etdl.stdlib/1.0`, listing each resolved library's name/version/kind) —
independent of the `reliability` feature and of the existing
`etdl-build-manifest.json` (which stays exactly as it was; nothing about
reliability provenance changed). Two builds of the same document, with the
same compiler version and the same libraries installed, resolve the same
libraries the same way — resolution has no hidden state (no network calls,
no timestamps, no environment-dependent search beyond the paths the caller
explicitly configured).

## Module reference

Every public stdlib module documents its purpose, public constructs,
examples, limitations, and stability in its own source file — this is the
project's convention for "understand `std.events` without reading compiler
code" (a user reads `etdl-compiler/stdlib/events/lib.etdl`, not
`etdl-compiler/src/`).
This section is the index.

### `std.events` — reusable event identities

**Purpose:** generic, reusable named occurrences, and (kept for
non-regression) a small set of illustrative failure-mechanism basic events
predating the stricter neutrality guidance below.

**Public constructs** (`components.basic_events`):
- Generic identity (no probability/failure_rate/mission_time): `Occurred`,
  `StateChanged`, `ConditionMet`, `SignalReceived`.
- Illustrative failure mechanisms (probability-bearing): `NetworkTimeout`,
  `ConnectionRefused`, `ProcessCrash`, `DiskFull`, `ConfigurationMissing`.

**Example:** see [`examples/standard-library/service.etdl`](../../examples/standard-library/service.etdl).

**Limitations:** an "event" here is exactly what `BasicEvent` already is —
a named leaf with optional numeric attributes. There is no distinct event
*type system* (occurrence vs. state-transition vs. signal is convention/
naming only, not a checked type), no event *relationship* graph (no
"caused-by"/"related-to" edges — the only relationship the language
expresses is "referenced as a gate input"), and no event *grouping*
construct beyond referencing several qualified ids from the same gate. See
"Future tree-event domains" below for why these are deferred rather than
faked.

**Stability:** 1.0. Existing entries are additive-only going forward — see
the non-regression rule.

### `std.logic` — reusable named boolean composition

**Purpose:** named, documented composition patterns (`AnyOf`, `AllOf`,
`MajorityOf`, `ExactlyOneOf`) built from ETDL's native `AND`/`OR`/`VOTING`/
`XOR` gate types — not a reimplementation of them (see "Core vs. library"
below).

**Public constructs:**
- `components.basic_events`: `SignalA`, `SignalB`, `SignalC` — placeholder
  inputs, no probability. `ExactlyOneOf` (XOR, exactly 2 inputs by ETDL's
  own gate rule) uses only `SignalA`/`SignalB`.
- `components.gates`: `AnyOf` (OR), `AllOf` (AND), `MajorityOf` (VOTING,
  k=2), `ExactlyOneOf` (XOR).

**Example:** see [`examples/standard-library/service.etdl`](../../examples/standard-library/service.etdl),
which imports `std.logic`, overrides all three signals with real
probabilities, and uses `std.logic.AnyOf` as a gate input.

**Reuse pattern:** override every placeholder input a gate uses (declare
the same qualified id, e.g. `std.logic.SignalA`, under the importing
fault tree's own `basicEvents:`) to repurpose a named pattern. A gate
input that resolves to a library placeholder still lacking a probability
fails the *existing* `V-503` check ("supplies neither probability nor
failureRate") — the same rule that already applies to any other
underspecified basic event; nothing new was added to enforce this.

**Limitations:** inputs are NOT parameterizable — `std.logic.AnyOf` always
means "OR of SignalA, SignalB, SignalC," never "OR of whatever the caller
passes." True template parameterization is a proposed core primitive, not
implemented — see below.

**Stability:** 1.0.

### `std.probability` — domain-neutral probability foundation

**Purpose:** `Probability`/`Rate` types, explicit composition (complement,
independent AND/OR, mutually-exclusive OR, conditional probability, Bayes'
rule), and five foundational distributions (Bernoulli, Binomial, Beta,
Exponential, Normal) — the mathematical layer beneath the reliability
domain (and any future safety/security/risk domain), never the reverse.

**Public constructs:** `components.basic_events`: `Certain` (P=1),
`Impossible` (P=0), `EvenOdds` (P=0.5) — the pure-ETDL half. The
compositional math and distributions are the `etdl-probability-core` Rust
crate, consumed directly by Rust code (compiler extensions, domain
libraries), not by ETDL YAML — ETDL has no expression syntax to call them.

Full reference — types, every operation's formula, all five
distributions, numerical tolerance policy, determinism/sampling scope,
units, provenance, the reliability adapter, and future predictive-
reliability hooks — is its own document:
[`standard-probability-library.md`](standard-probability-library.md).

**Stability:** 1.0.

### `std.collections` — not implemented (deferred, with reasoning)

ETDL's type system, as parsed by `etdl-parser::ast`, has no generic or
user-definable structural type: a library's `components:` can only
instantiate the fixed set of types the parser already knows (`Barrier`,
`Operation`, `Gate`, `BasicEvent`). There is no way to define, in ETDL
source, something like "a sequence of T" or "a map of K to V" as a reusable
abstraction distinct from what a Rust struct in `etdl-parser` already
hard-codes. Implementing `std.collections` today would mean either (a)
misusing an existing type to *simulate* a collection (dishonest — it would
look like a collection but enforce none of the semantics a real one would),
or (b) adding new core AST types for generic containers.

**Proposed core primitive** (not implemented): a generic record/list value
type usable inside `components:` (something like a `components.values:
Option<BTreeMap<String, serde_yaml::Value>>` catch-all, or, more ambitiously,
real generics in the type system). This clears the "broadly useful /
semantically fundamental / hard to express otherwise" bar in the abstract,
but is a real language-design undertaking — a full generics story affects
parsing, validation, and codegen — not something to bolt on inside a
standard-library task. Deferred to a future, dedicated language-design
task rather than forced in here.

### `std.units` — not implemented (deferred, with reasoning)

ETDL's core language has no unit-of-measure primitive at all: probabilities,
failure rates, and mission times are raw `f64`. (`TimeBasis` — `per-request`,
`per-hour`, ... — exists, but only in `etdl-reliability-core`, a domain
crate the core language and this stdlib module do not and must not depend
on; see "Dependency direction" below.) Beyond that, `Components` has no
component *kind* for "a named number" at all — `basic_events`/`gates`/
`barriers`/`operations` are all structured, multi-field records, none of
which represents a bare constant. There is no honest way to express "one
hour, as a reusable named quantity" in the current schema.

**Proposed core primitive** (not implemented): a `components.constants:
Option<BTreeMap<String, f64>>` kind (a named numeric value, nothing more)
would be enough to let a future `std.units` provide e.g. `HOUR_IN_SECONDS`.
It is small and broadly useful outside any one domain (any library wanting
reusable named numbers benefits), so it is a reasonable future proposal —
but ETDL still has no unit *type* (nothing stops mixing seconds and hours
arithmetically), so **unit safety would remain the caller's responsibility
even with this primitive** — exactly per this task's instruction not to
implement unsafe implicit conversion. Deferred rather than forced in.

## Public vs. internal stdlib API convention

Every name under `components:` in a library file (a `basic_events` or
`gates` key) is public — it is exactly what an importing document can
reference by qualified id, and what `etdl library list`/`etdl library
resolve` report. There is no internal/private construct inside a library
file today: `LibraryDocument` has no visibility modifier, and nothing
prevents importing any name a library declares.

The convention this version establishes, in the absence of a language-level
visibility mechanism: **an internal implementation detail does not appear
under `components:` at all.** A library's own doc comments (the `#`-prefixed
YAML comments, as in `etdl-compiler/stdlib/events/lib.etdl` and
`etdl-compiler/stdlib/logic/lib.etdl`)
are the only place non-public reasoning belongs; anything that must be
resolvable by a qualified id is, by construction, public. Naming
conventions like `__internal_*` are unnecessary and are not used — if
something needs to be hidden, don't put it under `components:`.

## Dependency direction

```
ETDL Core
   |
   v
ETDL Standard Library (std.*)
   |
   v
Reliability Supplement (and other domains)  -- MAY depend on std.*
```

Enforced today by ordinary crate boundaries, not a special check:
`etdl-compiler::stdlib` has no dependency on `etdl-reliability` or
`etdl-reliability-core` (confirmed: neither appears in
`etdl-compiler/Cargo.toml`'s non-dev dependencies beyond the existing,
unrelated optional reliability-supplement wiring), and nothing in
`etdl-compiler/stdlib/events/lib.etdl` or
`etdl-compiler/stdlib/logic/lib.etdl` references anything
reliability-specific (no `probability` source types, no `x-reliability`
extensions, no supplement declarations). The reliability crates, in turn,
do not depend on `etdl-compiler::stdlib` either — the two mechanisms
(`supplements:` for compiled-in Rust extensions, `libraries:` for ETDL
source) remain independent, by design, in this version. A future
reliability-domain library built in ETDL source (not required by this
task, not implemented) would depend on `std.*`, never the reverse.

## Future tree-event domains

`std.events` and `std.logic` are deliberately structured so a future
tree-event supplement could sit between them and a domain:

```
ETDL Core
   |
std.events    (generic occurrence identity)
   |
std.logic     (generic boolean composition)
   |
Tree Event Supplement      (NOT implemented — a future generic notion of
   |                         "compose named events into a tree, evaluate
   |                         it" that isn't specific to failure)
   |
   +-- Reliability Tree     (the existing fault-tree/event-tree machinery,
   |                          reframed as one instantiation)
   +-- Safety Tree
   +-- Other Tree Domains
```

Nothing about this version's `std.events`/`std.logic` design assumes
failure/fault/hazard semantics (see "No reliability leakage" below), which
is what keeps this door open. The Tree Event Supplement itself is out of
scope here — this section documents the shape the architecture supports,
not new implementation.

## No reliability leakage

`std.events` and `std.logic` do not define, reference, or depend on:
failure probability as a first-class concept (the *generic* `Occurred`/
`StateChanged`/`ConditionMet`/`SignalReceived` entries carry no probability
field at all — only the pre-existing, unchanged failure-mechanism entries
do, and those predate this task), MTBF, hazard rate, failure mode, or
reliability block. `std.logic`'s placeholder signals (`SignalA`/`SignalB`/
`SignalC`) are named for their role in boolean composition, not for any
assumed outcome.

## Relationship to the Generic Tree Event Supplement

`etdl.tree-event` (the domain-neutral tree-of-events structural model —
nodes, gates, validation, traversal) is declared like the reliability
supplement, via `supplements:`, not `libraries:` — it is registered
unconditionally in the compiler's extension registry (built-in, not
gated behind any Cargo feature), since it is core-adjacent infrastructure
rather than an optional reliability capability. Full specification:
[generic-tree-event-supplement.md](generic-tree-event-supplement.md).

## Relationship to the reliability supplement

The reliability supplement (`etdl.reliability`) is declared via
`supplements:`, not `libraries:`, and continues to resolve to a compiled-in
Rust `EtdlExtension` exactly as before — this version does not move it,
rewrite it, or make it depend on the standard library. The two mechanisms
are independent and can be used together in the same document. A *future*
domain library is free to declare `dependsOn: [{name: std.events, ...}]`
without requiring any change to the reliability crates; nothing in
`etdl-reliability`/`etdl-reliability-core` depends on `etdl-compiler::stdlib`,
and nothing in `stdlib` depends on the reliability crates.

## CLI

```bash
etdl library list                        # built-in standard library modules
etdl library resolve <file.etdl>         # how a document's libraries: resolve
etdl compile <file.etdl> --library-path <dir>   # add an optional-library search path
etdl capabilities                        # reports standard_library: { available, schema, builtin_libraries }
```

`etdl validate`/`etdl analyze`/`etdl compile` resolve `libraries:`
automatically — no flag is needed for the built-in standard library or for
a project-local `lib/<name>/lib.etdl`; `--library-path` is only needed to
add an *optional* library search directory.

## What is intentionally not implemented in this version

Per the project's explicit "do not overengineer" scope for 1.0:

- No package registry, no remote/network package downloads.
- No dependency-version solver beyond major-version compatibility (no
  diamond-dependency merging, no SAT solving).
- No automatic native-component compilation, no dynamic loading, no plugin
  marketplace.
- `std.probability` — deliberately out of scope; a dedicated future task
  ("ETDL Standard Probability Library"). Nothing in this version's design
  (generic, probability-free event identities; gate composition kept
  separate from any numeric semantics) prevents probability values,
  probability expressions, distributions, or rates from being layered on
  later.
- `std.collections`, `std.units` — deferred with reasoning; see "Module
  reference" above.
- Library-provided `barriers`/`operations` remain representable (same
  `Components` type as `gates`/`basic_events`, which this version's
  splicing *does* cover transitively) but are not yet spliced — no library
  module in this version needs them, so extending the mechanism to them is
  deferred until a real use case exists, per "do not force an abstraction
  in" (the same discipline applied to `std.collections`/`std.units`).
- No generic reusable event-tree node/subtree abstraction, no Tree Event
  Supplement — `components:` already exists in the language and is reused
  here as-is; nothing new was added to event-tree or fault-tree
  *semantics*. See "Future tree-event domains" above for the shape a later
  task could fill in.
- No event *relationship* or *grouping* construct beyond gate composition
  and qualified-id references — see `std.events`' documented limitations
  above.
