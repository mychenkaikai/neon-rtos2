# Neon-RTOS2 项目信息

## 📋 项目概述

**Neon-RTOS2** 是一个使用 Rust 语言开发的嵌入式实时操作系统（RTOS），专为资源受限的嵌入式系统设计。该项目采用 `no_std` 环境，支持裸机运行，提供了完整的任务调度、进程间通信、同步机制等核心功能。

## 📊 基本信息

| 项目属性 | 详情 |
|---------|------|
| **项目名称** | neon-rtos2 |
| **版本** | 0.1.0 |
| **开发语言** | Rust (Edition 2024) |
| **仓库地址** | https://github.com/mychenkaikai/neon-rtos2.git |
| **许可证** | 未指定 |
| **目标平台** | Cortex-M3, RISC-V (计划支持) |

## 🎯 项目特性

### 核心功能

1. **任务管理**
   - 支持多任务并发执行
   - 基于优先级的任务调度
   - 任务状态管理（未初始化、就绪、运行、阻塞）
   - 支持最多 10 个任务（可配置）
   - 每个任务独立的 4KB 栈空间

2. **调度器**
   - 抢占式任务调度
   - 轮转调度算法
   - 空闲任务自动管理
   - 任务切换优化

3. **同步机制**
   - 互斥锁（Mutex）
   - 信号量（Signal）
   - 事件管理（Event）
   - 支持任务间同步与互斥

4. **进程间通信（IPC）**
   - 消息队列
   - 类型安全的消息传递
   - 支持任意类型数据传输
   - 基于句柄的 IPC 管理

5. **定时器系统**
   - 系统滴答（SysTick）
   - 软件定时器
   - 延时功能（Delay）
   - 支持最多 10 个定时器

6. **日志系统**
   - 多级别日志（Trace, Debug, Info, Warn, Error）
   - 可配置日志级别
   - 支持格式化输出
   - 便捷的日志宏

7. **内存管理**
   - 动态内存分配器
   - 8KB 堆空间（可配置）
   - 支持 `embedded-alloc`
   - 内存安全保证

## 🏗️ 项目架构

### 目录结构

```
neon-rtos2/
├── src/                    # 核心源代码
│   ├── arch/              # 架构相关代码
│   │   ├── cortex_m3/    # Cortex-M3 支持
│   │   ├── cortex_m3.rs  # Cortex-M3 实现
│   │   └── test.rs       # 测试架构
│   ├── allocator.rs       # 内存分配器
│   ├── arch.rs            # 架构抽象层
│   ├── config.rs          # 系统配置
│   ├── event.rs           # 事件管理
│   ├── ipc.rs             # 进程间通信
│   ├── lib.rs             # 库入口
│   ├── log.rs             # 日志系统
│   ├── mq.rs              # 消息队列
│   ├── mutex.rs           # 互斥锁
│   ├── schedule.rs        # 任务调度器
│   ├── signal.rs          # 信号量
│   ├── systick.rs         # 系统滴答
│   ├── task.rs            # 任务管理
│   ├── timer.rs           # 定时器
│   └── utils.rs           # 工具函数
├── examples/              # 示例程序
│   ├── cortex-m3/        # Cortex-M3 示例
│   └── tests/            # 测试示例
├── tests/                 # 单元测试
├── docs/                  # 文档目录
└── target/                # 编译输出

```

### 模块说明

| 模块 | 功能描述 |
|------|---------|
| `task` | 任务创建、管理和状态控制 |
| `schedule` | 任务调度算法和调度器实现 |
| `arch` | 硬件架构抽象层，支持不同 CPU 架构 |
| `mutex` | 互斥锁实现，保护共享资源 |
| `signal` | 信号量机制，用于任务同步 |
| `event` | 事件管理，支持任务等待和唤醒 |
| `ipc` | 进程间通信，消息传递机制 |
| `timer` | 定时器和延时功能 |
| `systick` | 系统时钟滴答管理 |
| `log` | 日志输出和级别控制 |
| `allocator` | 动态内存分配 |
| `utils` | 内核初始化和工具函数 |

## 🔧 技术栈

### 核心依赖

- **cortex-m** (0.7.7) - Cortex-M 处理器支持
- **cortex-m-rt** (0.7.3) - Cortex-M 运行时
- **embedded-alloc** (0.5.1) - 嵌入式内存分配器
- **spin** (0.9.8) - 自旋锁实现
- **critical-section** (1.2) - 临界区保护
- **cortex-m-semihosting** (0.5.0) - 半主机调试支持
- **paste** (1.0) - 宏辅助工具

### 构建工具

- **cc** (1.0) - C/C++ 编译器集成
- **Rust Edition 2024** - 最新 Rust 版本特性

## 🚀 快速开始

### 环境准备

1. **安装 Rust 工具链**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **安装目标平台支持**
   ```bash
   # Cortex-M3 支持
   rustup target add thumbv7m-none-eabi
   
   # RISC-V 支持
   rustup target add riscv32imac-unknown-none-elf
   ```

3. **安装 QEMU 模拟器**
   ```bash
   # macOS
   brew install qemu
   
   # Ubuntu/Debian
   sudo apt install qemu-system-arm qemu-system-misc
   ```

### 构建和运行

项目提供了统一的 `build.sh` 脚本来管理构建和运行：

```bash
# 查看帮助
./build.sh help

# ========== 运行示例 ==========

# 运行 Cortex-M3 示例 (QEMU)
./build.sh run cortex-m3

# 运行 RISC-V 示例 (QEMU)
./build.sh run riscv

# 运行测试示例
./build.sh run tests

# 使用 release 模式运行
./build.sh run cortex-m3 --release

# ========== 仅构建 ==========

# 构建 Cortex-M3 示例
./build.sh build cortex-m3

# 构建 RISC-V 示例
./build.sh build riscv

# ========== 其他命令 ==========

# 运行单元测试
./build.sh test

# 检查代码
./build.sh check

# 生成文档
./build.sh doc

# 清理构建产物
./build.sh clean
```

### 使用 Cargo 直接运行

每个示例目录都配置了 `.cargo/config.toml`，支持直接使用 `cargo run`：

```bash
# Cortex-M3 示例
cd examples/cortex-m3
cargo run           # debug 模式
cargo run --release # release 模式

# RISC-V 示例
cd examples/riscv-qemu
cargo run           # debug 模式
cargo run --release # release 模式

# 测试示例
cd examples/tests
cargo run           # 运行 mutex_test
```

**注意**: 运行时按 `Ctrl+A X` 退出 QEMU。

### 系统配置

在 `src/config.rs` 中可以配置系统参数：

```rust
pub const STACK_SIZE: usize = 4096;      // 任务栈大小：4KB
pub const MAX_TASKS: usize = 10;         // 最大任务数：10
pub const MAX_SIGNALS: usize = 10;       // 最大信号量数：10
pub const MAX_TIMERS: usize = 10;        // 最大定时器数：10
pub const MAX_MUTEXES: usize = 10;       // 最大互斥锁数：10
pub const MAX_MQS: usize = 10;           // 最大消息队列数：10
pub const HEAP_SIZE: usize = 8 * 1024;   // 堆大小：8KB
```

### 示例代码

```rust
use neon_rtos2::task::Task;
use neon_rtos2::schedule::Scheduler;
use neon_rtos2::timer::Delay;
use neon_rtos2::utils::kernel_init;

fn main() -> ! {
    // 初始化内核
    kernel_init();
    
    // 创建任务
    let task1 = Task::new("task1", |_| {
        loop {
            debug!("task1 running");
            Delay::delay(1000);
        }
    });
    
    let task2 = Task::new("task2", |_| {
        loop {
            debug!("task2 running");
            Delay::delay(1000);
        }
    });
    
    // 启动调度器
    Scheduler::start();
    
    loop {}
}
```

## 🎨 特色亮点

1. **类型安全** - 充分利用 Rust 的类型系统，编译期保证内存安全
2. **零成本抽象** - 高级抽象不带来运行时开销
3. **无标准库** - 完全 `no_std` 环境，适合资源受限设备
4. **闭包支持** - 任务可以使用闭包定义，代码更简洁
5. **宏辅助** - 提供便捷的宏简化开发（如 `define_signal!`）
6. **模块化设计** - 清晰的模块划分，易于扩展和维护

## 📈 开发状态

### 已完成功能 ✅

- [x] 基础任务管理
- [x] 任务调度器
- [x] 互斥锁
- [x] 信号量
- [x] 事件系统
- [x] IPC 消息传递
- [x] 定时器系统
- [x] 日志系统
- [x] 内存分配器
- [x] Cortex-M3 支持
- [x] 闭包任务支持

### 计划功能 📝

- [ ] RISC-V 架构支持
- [ ] 更多同步原语
- [ ] 电源管理
- [ ] 文件系统支持
- [ ] 网络协议栈
- [ ] 更完善的文档

## 🔬 测试与示例

项目包含多个示例和测试：

- **examples/cortex-m3** - Cortex-M3 平台完整示例
- **examples/tests** - 功能测试示例
- **tests/** - 单元测试

## 🛠️ 构建与编译

### 编译配置

项目支持多种编译配置：

- **Debug 模式** - 优化级别 0，包含调试信息
- **Release 模式** - 优化级别 3，性能优化
- **Test 模式** - 优化级别 0，用于测试

### 特性标志

- `cortex_m3` (默认) - 启用 Cortex-M3 支持
- `riscv` - 启用 RISC-V 支持（开发中）

## 📝 最近更新

- 删除多余代码，优化项目结构
- 完善闭包支持，提升开发体验
- 添加 IPC 进程间通信功能
- 实现内存分配器
- 完善日志系统
- 添加空闲任务支持
- 优化信号量机制

## 🔮 未来愿景与发展方向

### 项目定位

Neon-RTOS2 是一个**探索性质**的 RTOS 项目，核心目标不是追求极致性能，而是：

- ✨ **充分展现 Rust 语言特性**：利用 Rust 的类型系统、所有权、trait 等特性
- 🚀 **提升开发效率**：相比传统 C 语言 RTOS，提供更高层次的抽象和更好的开发体验
- 🔬 **技术探索**：尝试将现代编程范式应用到嵌入式 RTOS 领域
- 📚 **学习价值**：为 Rust 嵌入式开发者提供参考和灵感

### 核心改进方向

#### 1. 异步编程支持 ⭐⭐⭐⭐⭐

引入 async/await 异步编程模型，这是 Rust 最强大的特性之一：

```rust
// 异步信号量
pub async fn wait_async(&self) {
    SignalFuture::new(self.id).await
}

// 异步任务
async fn sensor_task() {
    loop {
        let data = read_sensor().await;
        CHANNEL.send(data).await;
        Timer::sleep(Duration::from_millis(100)).await;
    }
}

// 同时等待多个事件
select! {
    data = sensor_rx.recv() => handle_sensor(data),
    cmd = command_rx.recv() => handle_command(cmd),
    _ = timer.tick() => handle_timeout(),
}
```

**优势：**
- 零成本抽象的协程
- 避免回调地狱
- 更直观的异步逻辑表达
- 与 Rust 生态无缝集成

#### 2. Builder 模式与链式 API ⭐⭐⭐⭐

提供更优雅的 API 设计：

```rust
// 任务创建
Task::builder()
    .name("sensor_reader")
    .priority(Priority::High)
    .stack_size(8192)
    .spawn(|| {
        // 任务逻辑
    })?;

// 定时任务
Task::builder()
    .name("periodic")
    .every(Duration::from_millis(100))
    .run(|| {
        // 周期性执行
    });

// 消息通道
let (tx, rx) = channel::<SensorData>()
    .capacity(16)
    .overflow_policy(OverflowPolicy::DropOldest)
    .build();
```

#### 3. 类型状态模式（Type State Pattern）⭐⭐⭐⭐⭐

利用类型系统在编译期保证状态转换的正确性：

```rust
// 不同状态是不同类型
pub struct Task<State> {
    id: usize,
    _state: PhantomData<State>,
}

pub struct Created;
pub struct Ready;
pub struct Running;

impl Task<Created> {
    pub fn start(self) -> Task<Ready> { ... }
}

impl Task<Ready> {
    pub fn run(self) -> Task<Running> { ... }
}

// 编译期保证状态转换正确
let task = Task::new("test")
    .start()  // Created -> Ready
    .run();   // Ready -> Running
// task.start(); // 编译错误！
```

#### 4. RAII 资源管理 ⭐⭐⭐⭐

充分利用 Rust 的 RAII 和 Drop trait：

```rust
// 自动释放的互斥锁
let mutex = Mutex::new(SharedData::default());
{
    let mut guard = mutex.lock(); // 获取 MutexGuard
    guard.modify();
    // 离开作用域自动释放，即使 panic 也安全
}

// 闭包风格
mutex.with_lock(|data| {
    data.modify();
});

// DMA 安全传输
let buffer = DmaBuffer::new([0u8; 256]);
let transfer = dma.start_transfer(&buffer);
// transfer 被 drop 时自动等待 DMA 完成
```

#### 5. 强大的错误处理 ⭐⭐⭐⭐⭐

使用 Result 和 Option 替代错误码：

```rust
// 返回 Result
pub fn send(&self, data: T) -> Result<(), SendError> {
    if self.is_full() {
        return Err(SendError::QueueFull);
    }
    // ...
    Ok(())
}

// 使用 ? 操作符链式处理
pub fn complex_operation() -> Result<(), Error> {
    let data = read_data()?;
    let processed = process_data(data)?;
    send_result(processed)?;
    Ok(())
}

// 自定义错误类型
#[derive(Debug)]
pub enum RtosError {
    TaskNotFound,
    QueueFull,
    Timeout,
    InvalidState,
}
```

#### 6. 迭代器与函数式编程 ⭐⭐⭐⭐

提供迭代器接口，支持函数式编程风格：

```rust
// 迭代器风格的任务遍历
Task::iter()
    .filter(|task| task.is_ready())
    .take(5)
    .for_each(|task| task.schedule());

// 收集结果
let ready_tasks: Vec<_> = Task::iter()
    .filter(|t| t.state() == TaskState::Ready)
    .collect();

// 链式数据处理
let average = sensors
    .iter()
    .map(|s| s.read())
    .filter(|v| v.is_valid())
    .map(|v| v.value)
    .sum::<u32>() / sensors.len();
```

#### 7. 过程宏与声明式编程 ⭐⭐⭐⭐

扩展宏系统，提供更强大的代码生成能力：

```rust
// 任务定义宏
task! {
    name: "sensor_task",
    priority: High,
    stack: 4096,
    async fn run() {
        loop {
            let data = read_sensor().await;
            process(data);
        }
    }
}

// 状态机宏
state_machine! {
    enum DeviceState {
        Idle => {
            on_event(PowerOn) => Active,
        },
        Active => {
            on_event(PowerOff) => Idle,
            on_event(Error) => ErrorState,
        },
        ErrorState => {
            on_event(Reset) => Idle,
        }
    }
}

// 设备驱动宏
device_driver! {
    name: Uart0,
    base_addr: 0x4000_0000,
    registers: {
        data: RW<u32> @ 0x00,
        status: RO<u32> @ 0x04,
        control: RW<u32> @ 0x08,
    }
}
```

#### 8. Const Generics 编译期优化 ⭐⭐⭐⭐

利用常量泛型在编译期确定大小：

```rust
// 编译期确定大小的队列
pub struct StaticQueue<T, const N: usize> {
    buffer: [Option<T>; N],
    head: usize,
    tail: usize,
}

// 不同大小是不同类型，零运行时开销
let small: StaticQueue<u32, 8> = StaticQueue::new();
let large: StaticQueue<u32, 256> = StaticQueue::new();

// 编译期配置
pub struct Task<const STACK_SIZE: usize, const PRIORITY: u8> {
    // 编译期就知道栈大小和优先级
}
```

#### 9. Trait 系统与多态 ⭐⭐⭐⭐

使用 trait 实现灵活的抽象：

```rust
// 设备驱动 trait
pub trait Device {
    type Error;
    
    fn init(&mut self) -> Result<(), Self::Error>;
    fn read(&self) -> Result<u32, Self::Error>;
    fn write(&mut self, data: u32) -> Result<(), Self::Error>;
}

// 泛型设备管理
pub struct DeviceManager<D: Device> {
    device: D,
}

impl<D: Device> DeviceManager<D> {
    pub fn new(device: D) -> Self {
        Self { device }
    }
}

// 或者使用 trait 对象实现动态分发
pub struct DynamicDeviceManager {
    devices: Vec<Box<dyn Device<Error = DeviceError>>>,
}
```

#### 10. 零大小类型（ZST）优化 ⭐⭐⭐

利用 ZST 实现零成本的类型标记：

```rust
// 优先级标记
pub struct HighPriority;
pub struct LowPriority;

pub struct Task<P> {
    id: usize,
    _priority: PhantomData<P>,
}

impl Task<HighPriority> {
    pub fn preempt(&self) { ... }
}

// PhantomData 不占用任何空间
assert_eq!(
    size_of::<Task<HighPriority>>(), 
    size_of::<usize>()
);
```

### 架构演进方向

#### 分层架构

```
neon-rtos2/
├── src/
│   ├── hal/              # 硬件抽象层
│   │   ├── traits.rs     # HAL trait 定义
│   │   ├── cortex_m/     # Cortex-M 实现
│   │   └── riscv/        # RISC-V 实现
│   ├── kernel/           # 内核核心
│   │   ├── scheduler/    # 调度器
│   │   ├── task/         # 任务管理
│   │   └── sync/         # 同步原语
│   ├── runtime/          # 异步运行时
│   │   ├── executor.rs   # 执行器
│   │   ├── future.rs     # Future 实现
│   │   └── waker.rs      # Waker 机制
│   ├── drivers/          # 设备驱动
│   │   ├── gpio.rs
│   │   ├── uart.rs
│   │   └── timer.rs
│   └── std/              # 标准库替代
│       ├── collections/  # 集合类型
│       └── sync/         # 同步原语
```

#### 编译期任务分析

使用过程宏在编译期分析任务依赖关系：

```rust
#[task_graph]
mod tasks {
    #[task(depends_on = [])]
    async fn sensor_read() -> SensorData { ... }
    
    #[task(depends_on = [sensor_read])]
    async fn data_process(data: SensorData) -> ProcessedData { ... }
    
    #[task(depends_on = [data_process])]
    async fn send_result(data: ProcessedData) { ... }
}

// 编译器自动生成最优调度代码
```

#### 嵌入式友好的 std 替代

```rust
// 提供类似标准库的 API
pub mod std {
    pub use core::*;
    
    pub mod collections {
        pub use heapless::Vec;
        pub use heapless::String;
    }
    
    pub mod sync {
        pub use crate::mutex::Mutex;
        pub use crate::channel::channel;
    }
}

// 用户代码可以像写普通 Rust 一样
use std::collections::Vec;
use std::sync::Mutex;
```

### 创新探索方向

#### 1. 形式化验证支持

集成形式化验证工具（如 Prusti、Creusot）：

```rust
#[requires(self.is_locked())]
#[ensures(!self.is_locked())]
pub fn unlock(&mut self) {
    self.locked = false;
}
```

#### 2. 编译期内存分析

在编译期分析内存使用，避免运行时溢出：

```rust
#[max_stack_usage(2048)]
fn complex_function() {
    // 编译器检查栈使用不超过 2KB
}
```

#### 3. 类型级编程

使用类型级编程实现更强的编译期保证：

```rust
// 编译期检查任务优先级
type_assert!(Priority<Task1> > Priority<Task2>);
```

### 示例：完整的异步信号量实现

```rust
use core::task::{Context, Poll, Waker};
use core::pin::Pin;
use core::future::Future;
use alloc::collections::VecDeque;
use spin::Mutex;

pub struct AsyncSignal {
    id: usize,
    waiters: Mutex<VecDeque<Waker>>,
}

impl AsyncSignal {
    pub const fn new(id: usize) -> Self {
        Self {
            id,
            waiters: Mutex::new(VecDeque::new()),
        }
    }
    
    pub fn signal(&self) {
        let mut waiters = self.waiters.lock();
        if let Some(waker) = waiters.pop_front() {
            waker.wake();
        }
    }
    
    pub fn wait(&self) -> SignalFuture<'_> {
        SignalFuture { signal: self }
    }
}

pub struct SignalFuture<'a> {
    signal: &'a AsyncSignal,
}

impl<'a> Future for SignalFuture<'a> {
    type Output = ();
    
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut waiters = self.signal.waiters.lock();
        
        // 检查是否有信号
        if waiters.is_empty() {
            // 没有信号，注册 waker
            waiters.push_back(cx.waker().clone());
            Poll::Pending
        } else {
            // 有信号，返回 Ready
            Poll::Ready(())
        }
    }
}

// 使用示例
async fn task1() {
    loop {
        SIGNAL.wait().await;
        debug!("Signal received!");
    }
}

async fn task2() {
    loop {
        Timer::sleep(Duration::from_secs(1)).await;
        SIGNAL.signal();
    }
}
```

### 开发路线图

#### 短期目标（1-3个月）

- [ ] 实现 Builder 模式 API
- [ ] 改进错误处理，全面使用 Result
- [ ] 添加迭代器支持
- [ ] 完善宏系统
- [ ] 增加更多示例和文档

#### 中期目标（3-6个月）

- [ ] 实现基础的异步运行时
- [ ] 支持 async/await
- [ ] 实现类型状态模式
- [ ] 添加更多 trait 抽象
- [ ] RISC-V 架构支持

#### 长期目标（6-12个月）

- [ ] 完整的异步生态
- [ ] 编译期任务分析
- [ ] 形式化验证集成
- [ ] 完善的驱动框架
- [ ] 网络协议栈

### 设计哲学

1. **安全第一**：利用 Rust 类型系统在编译期捕获错误
2. **零成本抽象**：高层抽象不应带来运行时开销
3. **人体工程学**：API 设计注重开发体验
4. **可组合性**：模块之间松耦合，易于组合
5. **渐进式**：保持向后兼容，平滑演进

### 与传统 C RTOS 的对比

| 特性 | 传统 C RTOS | Neon-RTOS2 |
|------|------------|------------|
| 内存安全 | 运行时检查 | 编译期保证 |
| 错误处理 | 错误码 | Result/Option |
| 资源管理 | 手动管理 | RAII 自动管理 |
| 并发安全 | 依赖开发者 | 类型系统保证 |
| 代码复用 | 宏/函数指针 | Trait/泛型 |
| 异步编程 | 回调/状态机 | async/await |
| 开发效率 | 较低 | 高 |
| 学习曲线 | 平缓 | 陡峭但值得 |

## 👥 贡献

欢迎提交 Issue 和 Pull Request！

特别欢迎以下方面的贡献：
- 新的 Rust 特性应用探索
- API 设计改进建议
- 示例代码和教程
- 文档完善
- Bug 修复

## 📧 联系方式

- GitHub: https://github.com/mychenkaikai/neon-rtos2

---

*最后更新时间：2025年12月26日*

