# Cortex-M3 示例

本示例展示了 Neon-RTOS2 在 Cortex-M3 平台上的完整功能。

## 功能演示

- ✅ 内核初始化 (`kernel_init()`)
- ✅ Builder 模式创建任务 (`Task::builder()`)
- ✅ 任务优先级 (`Priority::Idle/Low/Normal/High/Critical`)
- ✅ 信号量同步 (`define_signal!`, `signal()`, `wait()`)
- ✅ 互斥锁 RAII (`Mutex::lock_guard()`, `with_lock()`)
- ✅ 软件定时器 (`Timer::new()`, `start()`, `is_timeout()`)
- ✅ 任务迭代器 (`Task::iter()`, `ready_tasks()`, `blocked_tasks()`)
- ✅ 延时功能 (`Delay::delay()`)
- ✅ 日志系统 (`info!`, `debug!`, `error!` 等)

## 环境要求

### 安装 ARM 目标

```bash
rustup target add thumbv7m-none-eabi
```

### 安装 QEMU（可选，用于模拟运行）

**macOS:**
```bash
brew install qemu
```

**Ubuntu/Debian:**
```bash
sudo apt install qemu-system-arm
```

### 安装烧录工具（可选，用于实际硬件）

```bash
cargo install probe-rs --features cli
# 或者
cargo install cargo-flash
```

## 编译

```bash
# Debug 模式
cargo build

# Release 模式（推荐）
cargo build --release
```

## 运行

### 使用 QEMU 模拟

```bash
qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic \
    -semihosting-config enable=on,target=native \
    -kernel target/thumbv7m-none-eabi/release/neon-rtos2-example-cortex-m3
```

### 烧录到实际硬件

```bash
# 使用 cargo-flash（以 STM32F103 为例）
cargo flash --release --chip STM32F103C8

# 或者使用 probe-rs
probe-rs run --chip STM32F103C8 target/thumbv7m-none-eabi/release/neon-rtos2-example-cortex-m3
```

## 任务说明

本示例创建了 7 个任务，展示不同的 RTOS 功能：

| 任务名 | 优先级 | 功能 |
|--------|--------|------|
| sensor | High | 模拟传感器数据采集，发送信号 |
| processor | Normal | 等待传感器数据，处理后发送信号 |
| logger | Normal | 记录处理结果 |
| monitor | Low | 系统状态监控，展示任务迭代器 |
| mutex_demo | Normal | 互斥锁 RAII 用法演示 |
| timer_demo | Low | 软件定时器演示 |
| heartbeat | Idle | 心跳指示 |

## 预期输出

```
================================================
    Neon-RTOS2 Cortex-M3 Complete Example
================================================

This example demonstrates:
  - Task creation with Builder pattern
  - Task priorities (Idle/Low/Normal/High)
  - Signal synchronization
  - Mutex with RAII guards
  - Software timers
  - Task iterators
  - Error handling

Initializing SysTick...
SysTick initialized: 1000 Hz

Creating tasks with Builder pattern...
  Created: sensor (High Priority)
  Created: processor (Normal Priority)
  Created: logger (Normal Priority)
  Created: monitor (Low Priority)
  Created: mutex_demo (Normal Priority)
  Created: timer_demo (Low Priority)
  Created: heartbeat (Idle Priority)

All 7 tasks created successfully!

Starting scheduler...
================================================

[INFO] Sensor task started (High Priority)
[INFO] Processor task started (Normal Priority)
[INFO] Logger task started (Normal Priority)
[INFO] Monitor task started (Low Priority)
[INFO] Mutex demo task started (Normal Priority)
[INFO] Timer demo task started (Low Priority)
[INFO] Heartbeat task started (Idle Priority)

[DEBUG] Sensor: new reading = 7
[DEBUG] Processor: waiting for sensor data...
[INFO] Processor: processed value 7 -> 114
[INFO] Logger: entry #1 - total processed: 1
...
```

## 项目结构

```
cortex-m3/
├── Cargo.toml      # 项目配置和依赖
├── build.rs        # 构建脚本
├── memory.x        # 链接脚本
├── src/
│   └── main.rs     # 主程序
└── README.md       # 本文件
```

## 依赖说明

| 依赖 | 版本 | 说明 |
|------|------|------|
| neon-rtos2 | path | RTOS 核心库（启用 cortex_m3 特性） |
| cortex-m | 0.7.7 | Cortex-M 底层访问 |
| cortex-m-rt | 0.7.3 | Cortex-M 运行时 |
| cortex-m-semihosting | 0.5.0 | 半主机调试支持 |

## 自定义配置

### 修改系统时钟

在 `main.rs` 中修改以下常量：

```rust
const SYS_CLOCK: u32 = 12_000_000;  // 修改为实际时钟频率
const SYST_FREQ: u32 = 1000;        // SysTick 频率（Hz）
```

### 修改日志级别

```rust
set_log_level(LogLevel::Debug);  // 可选：Trace, Debug, Info, Warn, Error
```

## 常见问题

### Q: 编译错误：找不到 memory.x？

A: 确保 `build.rs` 正确配置，并且 `memory.x` 文件存在。

### Q: 烧录失败？

A: 检查：
1. 硬件连接是否正确
2. 调试器驱动是否安装
3. 芯片型号是否匹配

### Q: 任务不运行？

A: 检查：
1. SysTick 是否正确初始化
2. 调度器是否启动 (`Scheduler::start()`)
3. 任务创建是否成功（检查返回值）

## 扩展阅读

- [Neon-RTOS2 API 指南](../../docs/API_GUIDE.md)
- [项目信息](../../PROJECT_INFO.md)
- [Cortex-M 编程指南](https://docs.rust-embedded.org/book/)

