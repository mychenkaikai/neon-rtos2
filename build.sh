#!/bin/bash
# ============================================================================
# Neon-RTOS2 构建和运行脚本
# ============================================================================
#
# 用法:
#   ./build.sh <命令> [选项]
#
# 命令:
#   test              - 运行单元测试
#   build <target>    - 构建指定目标 (cortex-m3, riscv, tests)
#   run <target>      - 构建并运行指定目标
#   clean             - 清理构建产物
#   check             - 检查代码（不生成二进制）
#   doc               - 生成文档
#   help              - 显示帮助信息
#
# 目标:
#   cortex-m3         - Cortex-M3 示例 (QEMU LM3S6965EVB)
#   riscv             - RISC-V 示例 (QEMU virt)
#   tests             - 测试示例 (Cortex-M3)
#
# 示例:
#   ./build.sh run cortex-m3      # 构建并运行 Cortex-M3 示例
#   ./build.sh run riscv          # 构建并运行 RISC-V 示例
#   ./build.sh build cortex-m3    # 仅构建 Cortex-M3 示例
#   ./build.sh test               # 运行单元测试
#
# ============================================================================

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# 项目根目录
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"

# 打印带颜色的消息
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_header() {
    echo ""
    echo -e "${CYAN}========================================${NC}"
    echo -e "${CYAN}  $1${NC}"
    echo -e "${CYAN}========================================${NC}"
    echo ""
}

# 显示帮助信息
show_help() {
    print_header "Neon-RTOS2 构建脚本帮助"
    echo "用法: ./build.sh <命令> [选项]"
    echo ""
    echo "命令:"
    echo "  test              运行单元测试"
    echo "  build <target>    构建指定目标"
    echo "  run <target>      构建并运行指定目标 (使用 QEMU)"
    echo "  clean             清理所有构建产物"
    echo "  check             检查代码 (不生成二进制)"
    echo "  doc               生成文档"
    echo "  help              显示此帮助信息"
    echo ""
    echo "目标 (用于 build/run):"
    echo "  cortex-m3         Cortex-M3 示例 (QEMU LM3S6965EVB)"
    echo "  riscv             RISC-V 示例 (QEMU virt)"
    echo "  tests             测试示例 (Cortex-M3)"
    echo ""
    echo "选项:"
    echo "  --release         使用 release 模式构建"
    echo "  --debug           使用 debug 模式构建 (默认)"
    echo ""
    echo "示例:"
    echo "  ./build.sh run cortex-m3          # 运行 Cortex-M3 示例 (debug)"
    echo "  ./build.sh run cortex-m3 --release  # 运行 Cortex-M3 示例 (release)"
    echo "  ./build.sh run riscv              # 运行 RISC-V 示例"
    echo "  ./build.sh build cortex-m3        # 仅构建 Cortex-M3"
    echo "  ./build.sh test                   # 运行单元测试"
    echo "  ./build.sh clean                  # 清理构建产物"
    echo ""
}

# 检查 QEMU 是否安装
check_qemu_arm() {
    if ! command -v qemu-system-arm &> /dev/null; then
        print_error "qemu-system-arm 未安装!"
        echo ""
        echo "请安装 QEMU:"
        echo "  macOS:  brew install qemu"
        echo "  Ubuntu: sudo apt install qemu-system-arm"
        exit 1
    fi
}

check_qemu_riscv() {
    if ! command -v qemu-system-riscv32 &> /dev/null; then
        print_error "qemu-system-riscv32 未安装!"
        echo ""
        echo "请安装 QEMU:"
        echo "  macOS:  brew install qemu"
        echo "  Ubuntu: sudo apt install qemu-system-misc"
        exit 1
    fi
}

# 检查并安装 Rust target
ensure_target() {
    local target=$1
    if ! rustup target list --installed | grep -q "$target"; then
        print_info "安装 Rust target: $target"
        rustup target add "$target"
    fi
}

# 运行单元测试
run_tests() {
    print_header "运行单元测试"
    cd "$PROJECT_ROOT"
    RUSTFLAGS='--cfg test' cargo test -- --test-threads=1
    print_success "测试完成!"
}

# 构建 Cortex-M3 示例
build_cortex_m3() {
    local build_mode=$1
    print_header "构建 Cortex-M3 示例 ($build_mode)"
    
    ensure_target "thumbv7m-none-eabi"
    
    cd "$PROJECT_ROOT/examples/cortex-m3"
    
    if [ "$build_mode" = "release" ]; then
        cargo build --release
    else
        cargo build
    fi
    
    print_success "Cortex-M3 示例构建完成!"
}

# 运行 Cortex-M3 示例
run_cortex_m3() {
    local build_mode=$1
    
    check_qemu_arm
    build_cortex_m3 "$build_mode"
    
    print_header "在 QEMU 上运行 Cortex-M3 示例"
    print_info "平台: LM3S6965EVB (Cortex-M3)"
    print_info "按 Ctrl+A X 退出 QEMU"
    echo ""
    
    cd "$PROJECT_ROOT/examples/cortex-m3"
    
    if [ "$build_mode" = "release" ]; then
        BINARY="target/thumbv7m-none-eabi/release/neon-rtos2-example-cortex-m3"
    else
        BINARY="target/thumbv7m-none-eabi/debug/neon-rtos2-example-cortex-m3"
    fi
    
    qemu-system-arm \
        -cpu cortex-m3 \
        -machine lm3s6965evb \
        -nographic \
        -semihosting-config enable=on,target=native \
        -kernel "$BINARY"
}

# 构建 RISC-V 示例
build_riscv() {
    local build_mode=$1
    print_header "构建 RISC-V 示例 ($build_mode)"
    
    ensure_target "riscv32imac-unknown-none-elf"
    
    cd "$PROJECT_ROOT/examples/riscv-qemu"
    
    if [ "$build_mode" = "release" ]; then
        cargo build --release
    else
        cargo build
    fi
    
    print_success "RISC-V 示例构建完成!"
}

# 运行 RISC-V 示例
run_riscv() {
    local build_mode=$1
    
    check_qemu_riscv
    build_riscv "$build_mode"
    
    print_header "在 QEMU 上运行 RISC-V 示例"
    print_info "平台: virt (RISC-V 32-bit)"
    print_info "按 Ctrl+A X 退出 QEMU"
    echo ""
    
    cd "$PROJECT_ROOT/examples/riscv-qemu"
    
    if [ "$build_mode" = "release" ]; then
        BINARY="target/riscv32imac-unknown-none-elf/release/riscv-qemu-example"
    else
        BINARY="target/riscv32imac-unknown-none-elf/debug/riscv-qemu-example"
    fi
    
    qemu-system-riscv32 \
        -machine virt \
        -cpu rv32 \
        -m 128M \
        -bios none \
        -nographic \
        -semihosting-config enable=on,target=native \
        -kernel "$BINARY"
}

# 构建测试示例
build_tests() {
    local build_mode=$1
    print_header "构建测试示例 ($build_mode)"
    
    ensure_target "thumbv7m-none-eabi"
    
    cd "$PROJECT_ROOT/examples/tests"
    
    if [ "$build_mode" = "release" ]; then
        cargo build --release
    else
        cargo build
    fi
    
    print_success "测试示例构建完成!"
}

# 运行测试示例
run_tests_example() {
    local build_mode=$1
    
    check_qemu_arm
    build_tests "$build_mode"
    
    print_header "在 QEMU 上运行测试示例"
    print_info "平台: LM3S6965EVB (Cortex-M3)"
    print_info "按 Ctrl+A X 退出 QEMU"
    echo ""
    
    cd "$PROJECT_ROOT/examples/tests"
    
    if [ "$build_mode" = "release" ]; then
        BINARY="target/thumbv7m-none-eabi/release/mutex_test"
    else
        BINARY="target/thumbv7m-none-eabi/debug/mutex_test"
    fi
    
    qemu-system-arm \
        -cpu cortex-m3 \
        -machine lm3s6965evb \
        -nographic \
        -semihosting-config enable=on,target=native \
        -kernel "$BINARY"
}

# 清理构建产物
clean_all() {
    print_header "清理构建产物"
    
    print_info "清理主项目..."
    cd "$PROJECT_ROOT"
    cargo clean
    
    print_info "清理 cortex-m3 示例..."
    cd "$PROJECT_ROOT/examples/cortex-m3"
    cargo clean 2>/dev/null || true
    
    print_info "清理 riscv-qemu 示例..."
    cd "$PROJECT_ROOT/examples/riscv-qemu"
    cargo clean 2>/dev/null || true
    
    print_info "清理 tests 示例..."
    cd "$PROJECT_ROOT/examples/tests"
    cargo clean 2>/dev/null || true
    
    print_success "清理完成!"
}

# 检查代码
check_code() {
    print_header "检查代码"
    
    cd "$PROJECT_ROOT"
    
    print_info "检查主库 (默认 features)..."
    cargo check
    
    print_info "检查主库 (cortex_m3 feature)..."
    cargo check --features cortex_m3
    
    print_info "检查主库 (riscv feature)..."
    cargo check --features riscv
    
    print_info "检查 cortex-m3 示例..."
    cd "$PROJECT_ROOT/examples/cortex-m3"
    cargo check
    
    print_info "检查 riscv-qemu 示例..."
    cd "$PROJECT_ROOT/examples/riscv-qemu"
    cargo check
    
    print_success "代码检查完成!"
}

# 生成文档
generate_docs() {
    print_header "生成文档"
    
    cd "$PROJECT_ROOT"
    cargo doc --no-deps --features cortex_m3
    
    print_success "文档生成完成!"
    print_info "文档位置: $PROJECT_ROOT/target/doc/neon_rtos2/index.html"
}

# 解析构建模式
parse_build_mode() {
    local mode="debug"
    for arg in "$@"; do
        case $arg in
            --release)
                mode="release"
                ;;
            --debug)
                mode="debug"
                ;;
        esac
    done
    echo "$mode"
}

# 主函数
main() {
    if [ $# -eq 0 ]; then
        show_help
        exit 0
    fi
    
    local command=$1
    shift
    
    case $command in
        test)
            run_tests
            ;;
        build)
            if [ $# -eq 0 ]; then
                print_error "请指定构建目标: cortex-m3, riscv, tests"
                exit 1
            fi
            local target=$1
            shift
            local mode=$(parse_build_mode "$@")
            
            case $target in
                cortex-m3|cortex_m3|cm3)
                    build_cortex_m3 "$mode"
                    ;;
                riscv|rv32)
                    build_riscv "$mode"
                    ;;
                tests)
                    build_tests "$mode"
                    ;;
                *)
                    print_error "未知目标: $target"
                    print_info "可用目标: cortex-m3, riscv, tests"
                    exit 1
                    ;;
            esac
            ;;
        run)
            if [ $# -eq 0 ]; then
                print_error "请指定运行目标: cortex-m3, riscv, tests"
                exit 1
            fi
            local target=$1
            shift
            local mode=$(parse_build_mode "$@")
            
            case $target in
                cortex-m3|cortex_m3|cm3)
                    run_cortex_m3 "$mode"
                    ;;
                riscv|rv32)
                    run_riscv "$mode"
                    ;;
                tests)
                    run_tests_example "$mode"
                    ;;
                *)
                    print_error "未知目标: $target"
                    print_info "可用目标: cortex-m3, riscv, tests"
                    exit 1
                    ;;
            esac
            ;;
        clean)
            clean_all
            ;;
        check)
            check_code
            ;;
        doc|docs)
            generate_docs
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            print_error "未知命令: $command"
            show_help
            exit 1
            ;;
    esac
}

# 运行主函数
main "$@"
