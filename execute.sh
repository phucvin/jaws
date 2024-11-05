#!/usr/bin/env bash
set -o pipefail

# Parse options
CARGO_RUN=0
USE_NODE=0
while [[ $# -gt 0 ]]; do
  case $1 in
  --cargo-run)
    CARGO_RUN=1
    shift
    ;;
  --node)
    USE_NODE=1
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

generate_wasm() {
  wasm-tools parse $JS2WASM_DIR/wat/generated.wat -o $JS2WASM_DIR/wasm/generated.wasm
  # && \
  #   wasm-tools component embed --all-features $JS2WASM_DIR/wit --world js2wasm $JS2WASM_DIR/wat/generated.wat -t -o wasm/generated.core.wasm && \
  #   wasm-tools component new $JS2WASM_DIR/wasm/generated.core.wasm -o $JS2WASM_DIR/wasm/generated.component.wasm
}

run_wasm() {

  if [ $USE_NODE -eq 1 ]; then
    node run.js $JS2WASM_DIR/wasm/generated.wasm
  else
    wasmedge run --enable-gc --enable-exception-handling $JS2WASM_DIR/wasm/generated.wasm | tail -n +2
  fi
}

# Convert WAT to WASM
if ! generate_wasm; then
  exit 100
fi

# Run the WASM file
if ! run_wasm; then
  exit 101
fi
