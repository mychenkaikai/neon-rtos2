#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
out="${CARGO_TARGET_DIR:-$root/target}"
cargo build --locked --release -p neon-rtos2-example-tests --bin wait_latency \
    --target thumbv7m-none-eabi --target-dir "$out"
timeout 20s qemu-system-arm -machine lm3s6965evb -cpu cortex-m3 -nographic \
    -icount shift=5,align=off,sleep=off -semihosting-config enable=on,target=native \
    -kernel "$out/thumbv7m-none-eabi/release/wait_latency"
