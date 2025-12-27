#!/bin/bash
# Run Cortex-M3 QEMU example
#
# Usage:
#   ./scripts/run_cortex_m_qemu.sh [debug|release]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
EXAMPLE_DIR="$PROJECT_ROOT/examples/cortex-m3"

# Default to release build
BUILD_TYPE="${1:-release}"

echo "=========================================="
echo "  Neon-RTOS2 Cortex-M3 QEMU Runner"
echo "=========================================="
echo ""

# Check if QEMU is installed
if ! command -v qemu-system-arm &> /dev/null; then
    echo "Error: qemu-system-arm not found!"
    echo ""
    echo "Please install QEMU:"
    echo "  macOS:  brew install qemu"
    echo "  Ubuntu: sudo apt install qemu-system-arm"
    exit 1
fi

# Check if target is installed
if ! rustup target list --installed | grep -q "thumbv7m-none-eabi"; then
    echo "Installing Cortex-M3 target..."
    rustup target add thumbv7m-none-eabi
fi

# Build the example
echo "Building Cortex-M3 example ($BUILD_TYPE)..."
cd "$EXAMPLE_DIR"

if [ "$BUILD_TYPE" = "debug" ]; then
    cargo build
    BINARY="$EXAMPLE_DIR/target/thumbv7m-none-eabi/debug/cortex-m3-example"
else
    cargo build --release
    BINARY="$EXAMPLE_DIR/target/thumbv7m-none-eabi/release/cortex-m3-example"
fi

echo ""
echo "Running on QEMU (LM3S6965EVB)..."
echo "=========================================="
echo ""

# Run QEMU with LM3S6965 evaluation board
qemu-system-arm \
    -machine lm3s6965evb \
    -cpu cortex-m3 \
    -nographic \
    -semihosting-config enable=on,target=native \
    -kernel "$BINARY"

echo ""
echo "=========================================="
echo "  QEMU execution completed"
echo "=========================================="

