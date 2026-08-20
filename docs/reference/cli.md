# CLI Reference

`etdl` is the command-line interface to the ETDL compiler. Install with:

```bash
cargo install etdl-cli
```

## Global usage

```
Usage: etdl <COMMAND>

Commands:
  compile   Compile an .etdl document to a target language
  validate  Validate an .etdl document
  help      Print this message or the help of the given subcommand
```

## etdl validate

Validate an `.etdl` document without generating code.

```
Usage: etdl validate <FILE>

Arguments:
  <FILE>  Path to the .etdl document
```

Exit code is 0 when the document is valid (warnings are non-fatal).

Example:

```bash
$ etdl validate order-fulfillment.etdl
valid: order-fulfillment.etdl (3 diagnostics cleared)
```

## etdl compile

Compile an `.etdl` document to a target language.

```
Usage: etdl compile --target <TARGET> --out-dir <OUT_DIR> <FILE>

Arguments:
  <FILE>  Path to the .etdl document

Options:
  --target <TARGET>  Code generation target (currently: rust)
  --out-dir <OUT_DIR>  Output directory for generated files
  -h, --help         Print help
```

Example:

```bash
$ etdl compile order-fulfillment.etdl --target rust --out-dir ./generated
compiled 'order-fulfillment.etdl' to './generated/order-fulfillment.rs' (0 errors, 0 warnings)
```

### Diagnostics

The compiler reports three severity classes:

| Class | Meaning |
|---|---|
| `E-1xx` | Structural errors — malformed document, unresolved imports |
| `V-1xx`–`V-5xx` | Semantic validation errors — bad probabilities, type errors, gate cycles, undefined handlers, invalid references |
| `W-4xx` | Warnings — suspicious but non-fatal patterns |

Full diagnostic codes are enumerated in the [specification](https://github.com/usamassem/etdl-specification).
