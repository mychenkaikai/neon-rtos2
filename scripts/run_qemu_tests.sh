#!/bin/bash
# Run all QEMU tests for CI
#
# Usage:
#   ./scripts/run_qemu_tests.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "=========================================="
echo "  Neon-RTOS2 QEMU Test Suite"
echo "=========================================="
echo ""

PASSED=0
FAILED=0

# Function to run a test
run_test() {
    local name="$1"
    local script="$2"
    
    echo "Running: $name"
    echo "----------------------------------------"
    
    if timeout 60 "$script" release; then
        echo "Result: PASSED"
        ((PASSED++))
    else
        echo "Result: FAILED"
        ((FAILED++))
    fi
    echo ""
}

# Run RISC-V tests
if command -v qemu-system-riscv32 &> /dev/null; then
    run_test "RISC-V QEMU Test" "$SCRIPT_DIR/run_riscv_qemu.sh"
else
    echo "Skipping RISC-V tests (qemu-system-riscv32 not found)"
    echo ""
fi

# Run Cortex-M tests
if command -v qemu-system-arm &> /dev/null; then
    run_test "Cortex-M3 QEMU Test" "$SCRIPT_DIR/run_cortex_m_qemu.sh"
else
    echo "Skipping Cortex-M3 tests (qemu-system-arm not found)"
    echo ""
fi

# Summary
echo "=========================================="
echo "  Test Summary"
echo "=========================================="
echo "  Passed: $PASSED"
echo "  Failed: $FAILED"
echo "=========================================="

if [ $FAILED -gt 0 ]; then
    exit 1
fi

exit 0

