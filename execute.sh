#!/usr/bin/env bash
set -o pipefail

# Parse options
CARGO_RUN=0
while [[ $# -gt 0 ]]; do
  case $1 in
  --cargo-run)
    CARGO_RUN=1
    shift
    ;;
  *)
    break
    ;;
  esac
done

# Set JS2WASM_DIR only if JS2WASM_DIR is not already set
: ${JS2WASM_DIR:=/Users/drogus/code/js2wasm}

# Determine how to run the compiler
if [ $CARGO_RUN -eq 1 ]; then
  COMPILER="cargo run"
else
  if [ -n "$JS2WASM_BINARY" ]; then
    COMPILER="$JS2WASM_BINARY"
  else
    COMPILER="$JS2WASM_DIR/target/release/js2wasm"
  fi
fi

# Run the compiler
if ! cat $1 | $COMPILER; then
    exit 100
fi

# Convert WAT to WASM
if ! wasm-tools parse $JS2WASM_DIR/wat/generated.wat -o $JS2WASM_DIR/wasm/generated.wasm; then
    exit 100
fi

# Run the WASM file
if ! wasmedge run --enable-gc --enable-exception-handling $JS2WASM_DIR/wasm/generated.wasm | tail -n +2; then
    exit 101
fi
