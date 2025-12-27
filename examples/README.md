# Neon-RTOS2 示例程序

本目录包含 Neon-RTOS2 的示例程序，展示了 RTOS 的各种功能和最佳实践。

## 📁 示例列表

| 示例 | 平台 | 说明 |
|------|------|------|
| [cortex-m3](./cortex-m3/) | ARM Cortex-M3 | 完整功能演示 |
| [riscv-qemu](./riscv-qemu/) | RISC-V (QEMU) | RISC-V 平台演示 |
| [tests](./tests/) | ARM Cortex-M3 | 功能测试套件 |

## 🚀 快速开始

### 1. 安装目标平台

```bash
# Cortex-M3
rustup target add thumbv7m-none-eabi

# RISC-V
rustup target add riscv32imac-unknown-none-elf
```

### 2. 安装 QEMU（用于模拟运行）

**macOS:**
```bash
brew install qemu
```

**Ubuntu/Debian:**
```bash
sudo apt install qemu-system-arm qemu-system-misc
```

### 3. 运行示例

```bash
# Cortex-M3 示例
cd examples/cortex-m3
cargo build --release
qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic \
    -semihosting-config enable=on,target=native \
    -kernel target/thumbv7m-none-eabi/release/neon-rtos2-example-cortex-m3

# RISC-V 示例
cd examples/riscv-qemu
cargo build --release
qemu-system-riscv32 -machine virt -nographic -bios none \
    -kernel target/riscv32imac-unknown-none-elf/release/riscv-qemu-example

# 测试套件
cd examples/tests
cargo build --release
qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic \
    -semihosting-config enable=on,target=native \
    -kernel target/thumbv7m-none-eabi/release/mutex_test
```

## 📚 功能覆盖

### Cortex-M3 示例

| 功能 | 状态 | 说明 |
|------|------|------|
| 任务创建 | ✅ | `Task::new()` 和 `Task::builder()` |
| 任务优先级 | ✅ | 5 级优先级 (Idle/Low/Normal/High/Critical) |
| 信号量 | ✅ | `define_signal!`, `signal()`, `wait()` |
| 互斥锁 | ✅ | RAII 守卫和闭包风格 |
| 定时器 | ✅ | 软件定时器 |
| 延时 | ✅ | `Delay::delay()` |
| 任务迭代器 | ✅ | `Task::iter()`, `ready_tasks()`, `blocked_tasks()` |
| 日志系统 | ✅ | 多级别日志 |

### RISC-V 示例

| 功能 | 状态 | 说明 |
|------|------|------|
| 任务创建 | ✅ | Builder 模式 |
| 任务优先级 | ✅ | 多级优先级 |
| 信号量 | ✅ | 任务间同步 |
| 任务迭代器 | ✅ | 系统监控 |
| 延时 | ✅ | 周期性任务 |
| UART 输出 | ✅ | QEMU virt 平台 |

### 测试套件

| 测试类别 | 测试数量 | 说明 |
|----------|----------|------|
| 任务管理 | 16 | 创建、状态、优先级、迭代器 |
| 互斥锁 | 5 | 基本操作、RAII、闭包 |
| 定时器 | 4 | 创建、启动、停止、延时 |
| 消息队列 | 6 | 发送、接收、满/空检测 |
| 错误处理 | 1 | 任务槽满错误 |

## 🎯 最佳实践

### 1. 使用 Builder 模式创建任务

```rust
// ✅ 推荐
Task::builder("my_task")
    .priority(Priority::High)
    .spawn(task_fn)
    .expect("Failed to create task");

// ❌ 不推荐（无法设置优先级）
Task::new("my_task", task_fn);
```

### 2. 使用 RAII 管理互斥锁

```rust
// ✅ 推荐：自动释放
{
    let _guard = mutex.lock_guard();
    // 临界区
}

// ✅ 也推荐：闭包风格
mutex.with_lock(|| {
    // 临界区
});

// ❌ 不推荐：手动管理
mutex.lock();
// 临界区
mutex.unlock();  // 可能忘记调用
```

### 3. 使用迭代器遍历任务

```rust
// ✅ 推荐
let ready_count = Task::ready_tasks().count();

Task::iter()
    .filter(|t| t.get_priority() == Priority::High)
    .for_each(|t| info!("High priority: {}", t.get_name()));

// ❌ 不推荐
let mut count = 0;
Task::for_each(|task, _| {
    if task.get_state() == TaskState::Ready {
        count += 1;
    }
});
```

### 4. 正确处理错误

```rust
// ✅ 推荐：使用 ? 或 expect
let task = Task::new("task", |_| {})?;
let task = Task::new("task", |_| {}).expect("Failed to create task");

// ✅ 推荐：模式匹配
match Task::new("task", |_| {}) {
    Ok(task) => { /* ... */ }
    Err(RtosError::TaskSlotsFull) => { /* 处理错误 */ }
    Err(e) => { /* 其他错误 */ }
}

// ❌ 不推荐：忽略错误
let _ = Task::new("task", |_| {});
```

## 📖 更多文档

- [API 使用指南](../docs/API_GUIDE.md)
- [项目信息](../PROJECT_INFO.md)
- [示例优化建议](../docs/EXAMPLES_OPTIMIZATION.md)

## 🔧 故障排除

### QEMU 无输出

1. 检查 `-nographic` 参数
2. 检查 `-semihosting-config` 参数（Cortex-M）
3. 确保二进制文件路径正确

### 编译错误

1. 确保安装了正确的目标：`rustup target list --installed`
2. 运行 `cargo update` 更新依赖
3. 检查 Cargo.toml 中的 features 配置

### 任务不运行

1. 检查 `kernel_init()` 是否调用
2. 检查 `Scheduler::start()` 是否调用
3. 检查 SysTick 是否正确初始化

## 📝 贡献

欢迎提交新的示例程序！请确保：

1. 代码有详细注释
2. 包含 README.md 说明
3. 展示 RTOS 的特定功能
4. 遵循最佳实践

