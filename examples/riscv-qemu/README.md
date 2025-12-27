# RISC-V QEMU 示例

本示例展示了 Neon-RTOS2 在 RISC-V 平台上的使用。

## 功能演示

- ✅ 内核初始化 (`kernel_init()`)
- ✅ Builder 模式创建任务 (`Task::builder()`)
- ✅ 任务优先级设置 (`Priority::High/Normal/Low/Idle`)
- ✅ 信号量同步 (`define_signal!`, `signal()`, `wait()`)
- ✅ 任务迭代��� (`Task::iter()`, `Task::ready_tasks()`)
- ✅ 延时功能 (`Delay::delay()`)
- ✅ **日志系统** (`info!`, `debug!`, `error!`, `warn!`, `trace!`)

## 环境要求

### 安装 RISC-V 目标

```bash
rustup target add riscv32imac-unknown-none-elf
```

### 安装 QEMU

**macOS:**
```bash
brew install qemu
```

**Ubuntu/Debian:**
```bash
sudo apt install qemu-system-misc
```

**Windows:**
下载并安装 [QEMU for Windows](https://www.qemu.org/download/#windows)

## 编译

```bash
# Debug 模式
cargo build

# Release 模式（推荐）
cargo build --release
```

## 运行

```bash
# 使用 QEMU 运行
qemu-system-riscv32 -machine virt -nographic -bios none \
    -kernel target/riscv32imac-unknown-none-elf/release/riscv-qemu-example

# 或者使用 cargo run（需要配置 .cargo/config.toml）
cargo run --release
```

## 预期输出

```
[INFO] 
[INFO] ================================================
[INFO]        Neon-RTOS2 RISC-V QEMU Example
[INFO] ================================================
[INFO] 
[INFO] This example demonstrates:
[INFO]   - Kernel initialization
[INFO]   - Task creation with Builder pattern
[INFO]   - Task priorities
[INFO]   - Signal synchronization
[INFO]   - Task iterators
[INFO]   - Delay functionality
[INFO]   - Log system (info!, debug!, etc.)
[INFO] 
[INFO] Kernel initialized successfully!
[INFO] 
[INFO] Creating tasks with Builder pattern...
[INFO]   Created: sensor (High Priority)
[INFO]   Created: processor (Normal Priority)
[INFO]   Created: logger (Normal Priority)
[INFO]   Created: monitor (Low Priority)
[INFO]   Created: heartbeat (Idle Priority)
[INFO] 
[INFO] All tasks created successfully!
[INFO] Starting scheduler...
[INFO] 
[INFO] ================================================
[INFO] 
[INFO] Sensor task started (High Priority)
[INFO] Processor task started (Normal Priority)
[INFO] Logger task started (Normal Priority)
[INFO] Monitor task started (Low Priority)
[INFO] Heartbeat task started (Idle Priority)
[DEBUG] Sensor: Reading #1 - sending DATA_READY signal
[DEBUG] Processor: Waiting for data...
[INFO] Processor: Got data! Processed count: 1
[INFO] Logger: Log entry #1 - Data processing completed
...
```

## 退出 QEMU

按 `Ctrl+A` 然后按 `X` 退出 QEMU。

## 项目结构

```
riscv-qemu/
├── Cargo.toml      # 项目配置和依赖
├── build.rs        # 构建脚本（复制链接脚本）
├── memory.x        # 链接脚本（riscv-rt 兼容）
├── src/
│   └── main.rs     # 主程序
└── README.md       # 本文件
```

## 依赖说明

| 依赖 | 版本 | 说明 |
|------|------|------|
| neon-rtos2 | path | RTOS 核心库（启用 riscv 特性） |
| riscv-rt | 0.12 | RISC-V 运行时（提供启动代码） |
| riscv | 0.11 | RISC-V 底层访问 |

## 注意事项

1. **不要手动实现 `_start` 函数** - `riscv-rt` 已经提供了完整的启动代码
2. **使用 `#[entry]` 宏** - 这是 `riscv-rt` 提供的入口点宏
3. **链接脚本** - 使用 `REGION_ALIAS` 格式以兼容 `riscv-rt`

## 常见问题

### Q: QEMU 启动后没有输出？

A: 确保使用了正确的 QEMU 参数，特别是 `-nographic` 和 `-bios none`。

### Q: 编译错误：找不到 riscv-rt？

A: 运行 `cargo update` 更新依赖。

### Q: 如何调试？

A: 可以使用 GDB 连接 QEMU：
```bash
# 启动 QEMU 并等待 GDB 连接
qemu-system-riscv32 -machine virt -nographic -bios none \
    -kernel target/riscv32imac-unknown-none-elf/release/riscv-qemu-example \
    -s -S

# 在另一个终端启动 GDB
riscv32-unknown-elf-gdb target/riscv32imac-unknown-none-elf/release/riscv-qemu-example
(gdb) target remote :1234
(gdb) continue
```

