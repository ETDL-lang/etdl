# Supplement Plugins

`etdl-cli` can dynamically load third-party supplements as sandboxed
`.wasm` modules — no rebuild of `etdl-cli` itself required, unlike the
built-in `reliability`/`discovery` extensions (compiled in via Cargo
feature flags). This page documents the wire contract for anyone writing
a plugin in a language other than Rust; a Rust author should use
[`etdl-supplement-sdk`](https://github.com/ETDL-lang/etdl/tree/main/etdl-supplement-sdk)
instead of implementing this ABI by hand.

Always compiled into `etdl-cli` (not feature-gated) — no rebuild flag is
needed to use it.

## The three reuse tiers

| Tier | Adds new semantics? | Mechanism |
|---|---|---|
| Common ETDL code (`libraries:`) | No — content reuse | Source-spliced at compile time |
| **Supplement plugin (this page)** | Yes — validation/processing logic | Sandboxed `.wasm`, loaded at runtime |
| Handler (Operation `handler:`) | N/A — compiled, not ETDL source | A function the generated code calls directly |

A supplement plugin sits in the middle: like a built-in supplement
(`reliability`, `discovery`), it can validate a document and resolve
values into it — but it's dynamically loaded, sandboxed, third-party
code, not compiled into `etdl-cli`.

## Commands

```bash
etdl install <path-or-https-url>   # loads it once to check conformance, then installs
etdl supplement list               # built-in extensions + installed plugins
etdl supplement remove <id>
```

Installed plugins live in `~/.etdl/plugins/` (a `manifest.json` plus one
`.wasm` file per plugin). A document opts a plugin in the same way it
opts into any supplement — the plugin does nothing for a document that
doesn't declare it:

```yaml
supplements:
  - id: "etdl.mycompany-audit"   # etdl.<single-segment-domain> — no extra dots (rule E-106)
    version: "1.0"
```

## Sandbox guarantees

- **No WASI, no ambient capability.** A plugin gets no filesystem,
  network, clock, or environment access. Its only interface to the world
  is the four exported functions below — everything else is the plugin's
  own private linear memory.
- **Fuel-limited execution.** Every call (`validate`, `process`, and the
  one-time `id`/`version` calls at install time) runs under a `wasmtime`
  fuel budget. A plugin that loops forever traps rather than hanging
  `etdl` — the WASM-hosting equivalent of ECEL's own "Bounded" evaluation
  guarantee (spec §6.8).
- **Untrusted by construction.** A plugin that panics, traps, returns
  malformed JSON, or is missing an expected export becomes an ordinary
  `Diagnostic` (code `PLUGIN-ERROR`), never a host crash — the same
  "untrusted input must never crash the compiler" bar `asyncapi_imports`
  resolution is already held to.

`PLUGIN-ERROR` is deliberately not one of the spec's registered E-/V-/W-
codes: dynamic plugin hosting is an `etdl-cli`-specific capability, not
new ETDL language semantics every Conforming Compiler must implement.

## Wire ABI

A conforming module exports exactly these six symbols:

```text
memory                                                          (exported linear memory)
etdl_alloc(len: u32) -> u32                                     (returns ptr)
etdl_dealloc(ptr: u32, len: u32)
etdl_supplement_id() -> u64                                     (packed ptr/len — see below)
etdl_supplement_version() -> u64                                (packed ptr/len)
etdl_supplement_validate(doc_ptr, doc_len, ctx_ptr, ctx_len: u32) -> u64  (packed ptr/len)
etdl_supplement_process(doc_ptr, doc_len, ctx_ptr, ctx_len: u32) -> u64   (packed ptr/len)
```

No other imports are permitted — a module that imports anything (WASI or
otherwise) fails to instantiate, loudly, the moment it's loaded.

### Packed return values

A WASM function can return exactly one value without needing
multi-value-return support on the host, so every "return a string/JSON
blob" export packs a pointer and length into one `u64`:

```text
packed = (ptr << 32) | len
```

The host reads `len` bytes starting at `ptr` out of the module's exported
`memory` after the call returns.

### Call sequence (host's perspective)

For `etdl_supplement_id`/`etdl_supplement_version` (zero arguments):

1. Call the export. Unpack the returned `u64` into `(ptr, len)`.
2. Read `len` bytes at `ptr` from `memory`; decode as UTF-8.

For `etdl_supplement_validate`/`etdl_supplement_process`:

1. Call `etdl_alloc(doc_json.len())` → `doc_ptr`. Write `doc_json` bytes
   into `memory` at `doc_ptr`.
2. Call `etdl_alloc(ctx_json.len())` → `ctx_ptr`. Write `ctx_json` bytes
   into `memory` at `ctx_ptr`.
3. Call `etdl_supplement_validate(doc_ptr, doc_json.len(), ctx_ptr,
   ctx_json.len())` (or `_process`). Unpack the returned `u64` into
   `(result_ptr, result_len)`.
4. Read `result_len` bytes at `result_ptr` from `memory`; decode as JSON.
5. Call `etdl_dealloc(doc_ptr, doc_json.len())`,
   `etdl_dealloc(ctx_ptr, ctx_json.len())`, and
   `etdl_dealloc(result_ptr, result_len)` to let the module free
   everything it allocated for this call.

### JSON shapes

`doc_json` is `serde_json::to_vec(&EtlDocument)` — the fully parsed and
library-expanded document, as JSON. `ctx_json` is:

```json
{ "base_dir": "/path/to/document's directory", "config": {} }
```

`etdl_supplement_validate` returns a JSON array of diagnostics:

```json
[
  { "code": "MYAUDIT-001", "severity": "warning", "message": "..." }
]
```

`severity` is `"error"` or `"warning"`. The host prefixes `message` with
`[<supplement-id>]` before surfacing it, so a plugin's own message text
should not repeat its own id.

`etdl_supplement_process` returns basic-event probability overrides —
the same shape a built-in extension's `ExtensionResult::
basic_event_overrides()` already produces:

```json
{ "overrides": [["FaultTreeId.BasicEventId", 0.0042]] }
```

### Memory ownership

`etdl_alloc(len)` reserves `len` bytes and returns a pointer the caller
(the host, for input; the module, for its own return value) may write
into. `etdl_dealloc(ptr, len)` releases a block previously returned by
`etdl_alloc` with the *same* `len` — this is a simple bump/free-list
arena per module instance, not a general allocator; every `etdl_alloc`
must be matched by exactly one `etdl_dealloc` with the same length.

A fresh WASM instance is created per top-level call — a plugin that
forgets to call/support `etdl_dealloc` correctly only leaks within that
one instance's memory, which is discarded when the call returns; it can
never leak across calls or affect the host process.
