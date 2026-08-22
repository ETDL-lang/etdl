# Architecture

How an `.etdl` document becomes running code.

## Pipeline

```mermaid
flowchart LR
    S[".etdl document"] --> P["etdl-parser"]
    AA["AsyncAPI 3.0<br/>YAML/JSON"] --> P
    P --> V["etdl-compiler<br/>validate"]
    V --> FT["fault tree<br/>resolution"]
    FT --> CG["code generation<br/>--target rust|java|python|go|dotnet"]
    CG --> GEN["generated code<br/>(Rust, or a thin binding<br/>in another language)"]
    GEN --> CORE["etdl-core runtime<br/>(directly for Rust; via<br/>etdl-runtime-ffi's C ABI otherwise)"]
```

### 1. Parse (`etdl-parser`)

- The document AST is deserialized with serde; a manual `Deserialize` implementation normalizes legacy field names (`eventTree` → `eventTrees`) and preserves `x-*` extension fields.
- ECEL conditions are parsed with nom into `Condition::Comparison` / `Condition::Default`.
- `asyncapi_imports` are loaded and registered; message/channel references (`orders_api#/components/messages/OrderPlaced`) are resolved with RFC 6901 JSON Pointer.

### 2. Validate (`etdl-compiler`)

Structural and semantic checks, grouped by diagnostic class:

- **E-1xx** — document structure, version, required fields
- **V-1xx** — info/document integrity
- **V-2xx** — type checking (ECEL against AsyncAPI schemas), probability ranges, barrier branch sums
- **V-3xx** — event tree topology (initiating event reachability, node references)
- **V-4xx** — fault tree correctness (gate arity, cycles, probability computation)
- **V-5xx** — code generation preconditions (handler existence, channel/message existence)
- **W-4xx** — warnings (e.g. unlinked fault trees)

### 3. Resolve fault trees

`resolve_fault_trees` topologically sorts gates, computes each basic event's probability (`probability` or exponential `failureRate`/`missionTime`), evaluates gates, and returns `FaultTreeProbabilities` — a map of tree → top-event probability.

### 4. Generate code (`--target`)

`--target` selects one or more [`CodeGenerator`](architecture/targets.md)
implementations; `rust` (always available) is the default. `java`,
`python`, `go`, and `dotnet` are also implemented — each generates a thin
binding to `etdl-runtime-ffi`, a stable C ABI over `etdl-core`, rather
than reimplementing ETDL semantics in that language. Every target
consumes the *same* validated document and resolved fault-tree
probabilities from steps 1–3 above — only this last step differs per
target. See **[Target Architecture](architecture/targets.md)** for the
full trait, the `etdl-cli` target registry, the `etdl-runtime-ffi`
boundary, and how `--target java,python,go,dotnet` (or any combination)
works.

`RustCodeGenerator` (the `rust` target, unchanged by the above) emits:

- one `pub async fn handle_<initiating_event>` per event tree,
- a `const` per linked fault-tree top event (the build-time probability),
- `BranchMonitor` instantiation per barrier,
- `RetryPolicy` construction from `retryPolicy`,
- ECEL conditions compiled to wildcard `iter().all(...)` checks,
- channel publishes for consequences,
- failure-path recording with the resolved probability.

### 5. Run (`etdl-core`)

Rust-target generated code depends only on `etdl-core` and the message types from your AsyncAPI-generated crates. There is no engine, no server, no interpreter — the tree is the code. Other targets depend on that same `etdl-core`, reached through `etdl-runtime-ffi`'s C ABI plus a thin language-specific binding (see [Target Architecture](architecture/targets.md)) — never a separate reimplementation.

## The codegen contract

Rust-target generated functions follow a fixed contract so they compose with any message consumer:

```rust
pub async fn handle_<id>(message: <MessageType>) -> Result<(), WorkflowError>
```

- Return `Ok(())` when all consequences are handled.
- Return `Err` only for non-retryable infrastructure failures in publishing.
- All probabilistic accounting goes through `BranchMonitor`, so SLA and chaos components observe a single source of truth.

Each target defines its own idiomatic equivalent of this contract — see
[Target Architecture](architecture/targets.md#what-each-target-generates)
for the Java/Python/Go/.NET targets' interface-based versions, all backed
by the same `etdl-runtime-ffi`-bound `BranchMonitor`/`RetryPolicy`.

## Extension points

| Concern | Extension point |
|---|---|
| New code-generation targets | `codegen::CodeGenerator` trait + `etdl-cli`'s target registry — see [Target Architecture](architecture/targets.md) |
| New gate types | `ast::GateType` + `compute_gate_probability` |
| New condition operators | `ecel::Comparator` + parser + typeck |
| Runtime behavior | `etdl-core` modules (monitor, retry, sla, chaos, telemetry) |
