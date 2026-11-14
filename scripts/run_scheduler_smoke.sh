#!/usr/bin/env bash
# Requires the thumbv7m-none-eabi target and qemu-system-arm.
# A timeout or firmware panic is a failure; the firmware exits explicitly on success.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
SMOKE_TARGET_DIR="${CARGO_TARGET_DIR:-$PROJECT_ROOT/target}"
cd "$PROJECT_ROOT"

cargo build --locked -p neon-rtos2-example-tests --bin scheduler_smoke \
    --target thumbv7m-none-eabi --target-dir "$SMOKE_TARGET_DIR" --release

timeout 15s qemu-system-arm \
    -machine lm3s6965evb -cpu cortex-m3 -nographic \
    -semihosting-config enable=on,target=native \
    -kernel "$SMOKE_TARGET_DIR/thumbv7m-none-eabi/release/scheduler_smoke"
