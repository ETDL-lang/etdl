# `etdl context`: document export for LLM pipelines

`etdl context` is not a compiler command — it never validates a document
or generates code. It turns one or more `.etdl` documents into
representations meant to be fed to an LLM, either as retrieval units
(**RAG** — Retrieval-Augmented Generation) or as a full-context payload
(**CAG** — stuffing the whole document into a long-context prompt or
prompt cache). It has three subcommands, `dump`/`graph`/`chunks`,
covering the two use cases with three output shapes.

All three accept **multiple files**, so shell globbing does the directory
work: `etdl context dump *.etdl` exports every document in the current
directory in one call. There is no recursive directory scan and no
cross-file merging — each file is parsed and exported independently. A
file that fails to parse is reported inline (see "Error handling" below)
rather than aborting the rest of the batch, since one malformed file in a
`*.etdl` glob shouldn't blank out everything else.

**Parse-only.** None of the three subcommands run `etdl validate`'s
diagnostics or resolve fault-tree probabilities — they work directly off
a freshly parsed document, the same way `etdl-wasm::parse_for_diagram`
already does for the browser-based diagram tooling. If you need a
corpus-quality signal (e.g. to filter invalid documents out of a RAG
corpus before embedding them), run `etdl validate` separately first.

## `etdl context dump <files...>`

Full parsed AST as JSON, one array entry per file:

```bash
etdl context dump examples/safety/hazard-demo.etdl
```

```json
[
  {
    "file": "examples/safety/hazard-demo.etdl",
    "ast": { "etdl": "1.0.0", "info": { ... }, "event_trees": { ... }, "fault_trees": { ... }, ... }
  }
]
```

`"ast"` is `serde_json::to_value(&doc)` on the parsed `EtlDocument` —
every field the parser itself sees, snake_case per Rust's own field names
(not the document's own camelCase YAML spelling). Best suited for **CAG**:
paste the whole thing into a long-context prompt, or persist it as-is
in whatever store backs a cache-augmented pipeline. It is *not*
retrieval-optimized — there's no natural-language summary anywhere in it,
and a large document produces a large, deeply nested blob.

## `etdl context graph <files...>`

A unified `{nodes, edges}` graph, one array entry per file:

```bash
etdl context graph examples/safety/hazard-demo.etdl
```

```json
[
  {
    "file": "examples/safety/hazard-demo.etdl",
    "graph": {
      "nodes": [
        { "id": "OrderFulfillment/RetryBarrier", "kind": "barrier", "tree_id": "OrderFulfillment", "label": "RetryBarrier", "attributes": { "branches": [...] } }
      ],
      "edges": [
        { "from": "OrderFulfillment/RetryBarrier", "to": "OrderFulfillment/FallbackBarrier", "label": "FAILURE" }
      ]
    }
  }
]
```

Node `id`s are qualified by tree (`"<tree_id>/<node_id>"`), unique within
one document — never by array position. `kind` is one of:
`initiating_event`, `barrier`, `operation`, `consequence` (event trees);
`fault_tree_top_event`, `gate`, `basic_event`, `transfer` (fault trees);
`generic_tree_leaf`, `generic_tree_gate` (`etdl.tree-event`, when
declared). Edges carry a `label` naming the relationship (a branch
outcome, `"rootCause"`, `"input"`, `"success"`/`"failure"`,
`"transfersTo"`) except where the edge shape alone already says
everything (an initiating event's `next`, a generic tree's parent→child
edge).

Event trees, fault trees, and `etdl.tree-event` generic trees have no
shared traversal abstraction in this codebase, so `graph` is the one
place all three get projected into the same shape — useful for
graph-aware tooling (a knowledge graph, a graph database, graph-based
retrieval) that a raw AST dump doesn't serve well.

## `etdl context chunks <files...>`

RAG-ready chunks as **JSON Lines** — one compact JSON object per line, no
enclosing array, the standard ingestion shape for embedding/vector-store
pipelines:

```bash
etdl context chunks examples/safety/hazard-demo.etdl
```

```
{"file":"examples/safety/hazard-demo.etdl","chunk_id":"document","kind":"document","text":"Document 'Order Fulfillment (Safety Demo)' (v1.0.0, domain FulfillmentContext). Declares 1 event tree(s) and 1 fault tree(s); supplements: etdl.safety, etdl.live-reliability.","metadata":{...}}
{"file":"examples/safety/hazard-demo.etdl","chunk_id":"OrderFulfillment/RetryBarrier","kind":"node.barrier","text":"Barrier 'RetryBarrier' in event tree 'OrderFulfillment' has 2 branch(es): FAILURE (probability from #/faultTrees/GatewayFailure/topEvent) -> FallbackBarrier; SUCCESS (probability unspecified) -> ProcessPaymentOperation.","metadata":{...}}
...
```

Every chunk has:
- `chunk_id` — stable, unique within one document
- `kind` — see the table below
- `text` — an **auto-generated, deterministic** natural-language summary
  (templated prose from the document's structure, not limited to
  author-written `description` fields) — embed this
- `metadata` — the same information structured for filtering/display,
  not for embedding

| `kind` | One per... | Requires |
|---|---|---|
| `document` | document (always exactly one) | — |
| `event_tree` | event tree | — |
| `node.barrier` / `node.operation` / `node.consequence` | event tree node | — |
| `fault_tree` | fault tree | — |
| `gate` | fault tree gate | — |
| `basic_event` | fault tree basic event | — |
| `hazard` / `safety_barrier` | `x-safety` hazard/barrier | `etdl.safety` declared |
| `budget` / `barrier_check` | `x-performance` budget/barrier check | `etdl.performance` declared |
| `generic_tree` / `generic_tree_node` | `x-tree-event` tree/node | `etdl.tree-event` declared |

Supplement-gated kinds are simply absent (not emitted as empty/error
chunks) when the relevant supplement isn't declared — every existing
supplement parser (`safety::parse_and_validate_safety`,
`performance::parse_and_validate_performance`,
`tree_event::parse_and_validate_trees`) already returns empty data in
that case, so `chunks` calls all of them unconditionally rather than
checking `supplements:` itself. A fault tree declared live-tracked under
`x-live-reliability` gets that folded into its own `fault_tree` chunk's
`metadata` (`"live_tracked": true, "live_threshold": ...`) rather than a
separate chunk kind, since it's metadata *about* a fault tree, not a new
semantic unit.

## Error handling

A file that fails to parse does not abort the batch:

- `dump`/`graph`: that file's array entry is `{"file": "...", "error":
  "..."}` instead of `{"file": "...", "ast": ...}` / `{"..., "graph":
  ...}` — every other file in the batch still gets its normal entry.
- `chunks`: a single `{"file": "...", "error": "..."}` line takes the
  place of that file's chunks, then the batch continues to the next file.

Exit code is `1` if **any** file in the batch failed to parse, `0`
otherwise — matches every other `etdl` command's exit-code convention
(`etdl --help`: `0` = success, `1` = validation/compile failure, `2` =
usage error).

## Compiler integration

Implemented in `etdl-compiler::context` (`build_graph`/`build_chunks`),
reusing every existing supplement's own parser rather than
re-implementing supplement-specific logic — see that module's own doc
comment for the full design rationale. `etdl-cli`'s `cmd_context_dump`/
`cmd_context_graph`/`cmd_context_chunks` are thin wrappers: parse each
file, call into `etdl_compiler::context`, print JSON or JSON Lines.
