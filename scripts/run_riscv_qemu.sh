#!/bin/bash
# Run RISC-V QEMU example
#
# Usage:
#   ./scripts/run_riscv_qemu.sh [debug|release]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
EXAMPLE_DIR="$PROJECT_ROOT/examples/riscv-qemu"

# Default to release build
BUILD_TYPE="${1:-release}"

echo "=========================================="
echo "  Neon-RTOS2 RISC-V QEMU Runner"
echo "=========================================="
echo ""

# Check if QEMU is installed
if ! command -v qemu-system-riscv32 &> /dev/null; then
    echo "Error: qemu-system-riscv32 not found!"
    echo ""
    echo "Please install QEMU:"
    echo "  macOS:  brew install qemu"
    echo "  Ubuntu: sudo apt install qemu-system-misc"
    exit 1
fi

# Check if target is installed
if ! rustup target list --installed | grep -q "riscv32imac-unknown-none-elf"; then
    echo "Installing RISC-V target..."
    rustup target add riscv32imac-unknown-none-elf
fi

# Build the example
echo "Building RISC-V example ($BUILD_TYPE)..."
cd "$EXAMPLE_DIR"

if [ "$BUILD_TYPE" = "debug" ]; then
    cargo build
    BINARY="$EXAMPLE_DIR/target/riscv32imac-unknown-none-elf/debug/riscv-qemu-example"
else
    cargo build --release
    BINARY="$EXAMPLE_DIR/target/riscv32imac-unknown-none-elf/release/riscv-qemu-example"
fi

echo ""
echo "Running on QEMU..."
echo "=========================================="
echo ""

# Run QEMU
qemu-system-riscv32 \
    -machine virt \
    -cpu rv32 \
    -nographic \
    -semihosting-config enable=on,target=native \
    -kernel "$BINARY"

echo ""
echo "=========================================="
echo "  QEMU execution completed"
echo "=========================================="

