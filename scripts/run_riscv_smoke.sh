#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../examples/riscv-qemu"
out="${CARGO_TARGET_DIR:-$PWD/target}"
cargo build --locked --release --bin kernel_smoke --target riscv32imac-unknown-none-elf --target-dir "$out"
timeout 15s qemu-system-riscv32 -machine virt -cpu rv32 -m 128M -bios none -nographic \
    -icount shift=3,align=off,sleep=off -kernel "$out/riscv32imac-unknown-none-elf/release/kernel_smoke"
