# ETDL Compiler — Proposal for Language Server / IntelliSense Support

**Author:** ETDL VS Code extension maintainer
**Context:** The `etdl-language` VS Code extension now ships a language server (completions, go-to-definition, find references, hover, outline, validation). To make those features accurate and to remove fragile client-side re-parsing, we request the following additions to the ETDL compiler's WASM API (`etdl-wasm`).

## Current state

The `etdl-wasm` crate exposes four functions (wasm-bindgen, nodejs target):

```ts
validate_etdl(etdl_content: string, asyncapi_files_json: string): string // JSON
parse_for_diagram(etdl_content: string): string                          // JSON AST
parse_for_raaml(etdl_content: string): string                            // JSON AST
version(): string
```

Two gaps block high-quality IDE support:

1. **The AST carries no source positions.** Every node has semantic values only (`id`, `type`, `branches`, ...) — no line/column/offset. The extension therefore re-parses the document with an indentation scanner to obtain spans.
2. **`validate_etdl` diagnostics mostly return `line: null, column: null`.** Some messages embed positions in prose ("... at line 30 column 7") but the structured fields are empty, forcing heuristic anchoring of squiggles.

---

## Request 1 (highest value, lowest effort): structured positions in diagnostics

Populate `line`, `column` (and ideally `end_line`, `end_column`) for **every** diagnostic returned by `validate_etdl`. Example target shape:

```json
{
  "code": "E-104",
  "severity": "error",
  "message": "initiatingEvent.message: JSON Pointer '#/components/messages/OrderPlaced' does not resolve in AsyncAPI document 'orders_api'",
  "line": 12,
  "column": 15,
  "end_line": 12,
  "end_column": 55
}
```

This alone removes the current heuristic anchoring for errors/warnings.

## Request 2: span-aware AST (`parse_with_spans`)

Add a new function that returns the same semantic tree as `parse_for_diagram`/`parse_for_raaml` but attaches a `span` to every element:

```
parse_with_spans(etdl_content: string): string   // JSON
```

Span shape (0-based, consistent with LSP):

```json
{
  "span": {
    "start": 142,          // absolute character offset
    "end": 190,            // absolute character offset (exclusive)
    "line": 12,            // 0-based
    "column": 15,          // 0-based
    "end_line": 12,
    "end_column": 53
  }
}
```

Spans are requested on:
- sections (`etdl`, `info`, `asyncapi_imports`, `eventTrees`, `faultTrees`, `components`)
- tree names and tree-level fields
- `initiatingEvent`, `nodes`, node names + each field (including `branches` list items and branch fields)
- `topEvent`, `gates`, gate names + fields (`type`, `inputs`, `k`, `inhibitCondition`, ...)
- `basicEvents`, basic-event names + fields
- **identifier references**: the value tokens of `next`, `onFailure`, `rootCause`, `inputs` entries, `onFailureProbabilitySource`, and AsyncAPI refs (`message`, `channel`, `emits`) — the extension currently finds these by scanning text.

Provide spans **both** in the AST and — most usefully — a dedicated lookup:

```
find_span(etdl_content: string, offset: number): string   // JSON, see below
```

Return the deepest element containing the offset:

```json
{
  "kind": "reference",        // or "definition" | "field" | "section" | ...
  "name": "ProcessPaymentOperation",
  "field": "next",
  "tree": "OrderFulfillment",
  "span": { "start": 142, "end": 190, "line": 12, "column": 15, "end_line": 12, "end_column": 53 }
}
```

This powers go-to-definition, find-references, hover, and completion context without re-parsing.

## Request 3: duplicate-id validation

Currently duplicate ids are **silently accepted** (the last one wins). Add a validation diagnostic, e.g.:

```json
{
  "code": "V-001",
  "severity": "warning",
  "message": "duplicate node id 'InventoryCheckBarrier' in tree 'OrderFulfillment'",
  "line": 20,
  "column": 6,
  "end_line": 20,
  "end_column": 26
}
```

Duplicate detection per (tree, kind): node ids under `nodes`, gate ids under `gates`, basic-event ids under `basicEvents`.

## Request 4 (optional, phase 2): semantic endpoints

If the compiler team prefers to centralize IDE logic (rather than having the extension implement it), these endpoints would remove the need for most client-side logic. All take `etdl_content` (+ `asyncapi_files_json` where relevant) and an optional `offset`/`position`.

```
complete(etdl_content: string, offset: number): string
hover(etdl_content: string, offset: number): string
goto_definition(etdl_content: string, offset: number): string
find_references(etdl_content: string, offset: number): string
document_symbols(etdl_content: string): string
format(etdl_content: string): string
```

Each returns a JSON payload modeled on the LSP types. These are **optional** — Requests 1–3 are sufficient for the current extension, which implements the above features with its own lightweight scanner in the meantime.

---

## Reference material

- Compiler repo: `https://github.com/usamassem/etdl.git` (build via `scripts/build-wasm.sh` in the extension repo).
- Current extension consumption: `src/etdlWasm.ts` (`validate_etdl`, `parse_for_diagram`, `parse_for_raaml`, `version`), `src/etdlParser.ts` (client-side scanner), `src/server/*` (LSP handlers).

## Compatibility notes

- Keep existing function signatures/return shapes unchanged (backward compatible) and add the new functions alongside.
- Use the wasm-bindgen `--target nodejs` output (as today) so the same `pkg/` bundle continues to load via `require()` in both the extension and the language-server process.
- Line/column numbering: **0-based** (LSP convention) to avoid off-by-one bugs.
- `start`/`end` offsets must be **character offsets in the original string**, not UTF-16 code units, so they map cleanly to LSP `Position` via the editor's own text model.
