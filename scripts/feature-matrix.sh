#!/usr/bin/env bash
# Feature matrix check for the ETDL reliability architecture.
#
# Verifies that every documented feature combination builds. The matrix:
#
#   A  default ETDL build            -p etdl-compiler --no-default-features
#   B  ETDL + built-in reliability   -p etdl-compiler                  (default = reliability)
#   C  ETDL + optional reliability library  -p etdl-reliability
#   D  ETDL + failure discovery      -p etdl-cli --no-default-features --features discovery
#   E  ETDL + ontology               -p etdl-reliability-ontology
#   F  all features                  --workspace --all-features
#   G  WASM-compatible feature set   -p etdl-wasm (wasm32-unknown-unknown)
#   H  etdl-conformance, lean        -p etdl-conformance --no-default-features
#   I  etdl-cli, fully lean          -p etdl-cli --no-default-features
#   J  all observability exporters   -p etdl-core --features exporter-prometheus,exporter-loki,exporter-otlp
#   K  same, via the non-Rust C ABI  -p etdl-runtime-ffi --no-default-features --features exporter-prometheus,exporter-loki,exporter-otlp
#   L  live reliability engine       -p etdl-core --features live-reliability
#
# Usage: scripts/feature-matrix.sh [--check]
#   --check : run `cargo check` only (fast). Default: check.

set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:---check}"

run() {
  echo "==> $*"
  "$@"
}

if [ "$MODE" = "--check" ]; then
  CARGO_CMD=(cargo check)
else
  CARGO_CMD=(cargo build)
fi

echo "========================================"
echo " A: default ETDL build (no features)"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-compiler --no-default-features

echo
echo "========================================"
echo " B: ETDL + built-in reliability (default features)"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-compiler

echo
echo "========================================"
echo " C: ETDL + optional reliability library"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-reliability

echo
echo "========================================"
echo " D: ETDL + failure discovery (CLI discovery feature)"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-cli --no-default-features --features discovery

echo
echo "========================================"
echo " E: ETDL + ontology"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-reliability-ontology

echo
echo "========================================"
echo " F: all features (workspace)"
echo "========================================"
run "${CARGO_CMD[@]}" --workspace --all-features

echo
echo "========================================"
echo " G: WASM-compatible feature set"
echo "========================================"
if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  run "${CARGO_CMD[@]}" -p etdl-wasm --target wasm32-unknown-unknown
else
  echo "wasm32-unknown-unknown target not installed; skipping G."
fi

echo
echo "========================================"
echo " H: etdl-conformance, lean (no reliability)"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-conformance --no-default-features

echo
echo "========================================"
echo " I: etdl-cli, fully lean (no features at all)"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-cli --no-default-features

echo
echo "========================================"
echo " J: etdl-core, all observability exporters at once"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-core --features exporter-prometheus,exporter-loki,exporter-otlp

echo
echo "========================================"
echo " K: etdl-runtime-ffi, all observability exporters at once"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-runtime-ffi --no-default-features --features exporter-prometheus,exporter-loki,exporter-otlp

echo
echo "========================================"
echo " L: etdl-core, live reliability engine"
echo "========================================"
run "${CARGO_CMD[@]}" -p etdl-core --features live-reliability

echo
echo "All feature combinations OK."
