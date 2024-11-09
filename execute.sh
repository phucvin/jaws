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

# Set JAWS_DIR only if JAWS_DIR is not already set
: ${JAWS_DIR:=.}

# Determine how to run the compiler
if [ $CARGO_RUN -eq 1 ]; then
  COMPILER="cargo run"
else
  if [ -n "$JAWS_BINARY" ]; then
    COMPILER="$JAWS_BINARY"
  else
    COMPILER="$JAWS_DIR/target/release/jaws"
  fi
fi

# Run the compiler
if ! cat $1 | $COMPILER; then
  exit 100
fi

generate_wasm() {
  wasm-tools parse $JAWS_DIR/wat/generated.wat -o $JAWS_DIR/wasm/generated.wasm
  # && \
  #   wasm-tools component embed --all-features $JAWS_DIR/wit --world jaws $JAWS_DIR/wat/generated.wat -t -o wasm/generated.core.wasm && \
  #   wasm-tools component new $JAWS_DIR/wasm/generated.core.wasm -o $JAWS_DIR/wasm/generated.component.wasm
}

run_wasm() {
  node run.js $JAWS_DIR/wasm/generated.wasm
}

# Convert WAT to WASM
if ! generate_wasm; then
  exit 100
fi

# Run the WASM file
if ! run_wasm; then
  exit 101
fi
