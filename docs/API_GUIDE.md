# Neon-RTOS2 API 使用指南

> 最后更新：2026年6月13日

本文档提供 Neon-RTOS2 的 API 使用指南和最佳实践。

---

## 📋 目录

- [快速开始](#快速开始)
- [任务管理](#任务管理)
- [同步原语](#同步原语)
- [进程间通信](#进程间通信)
- [定时器系统](#定时器系统)
- [异步运行时](#异步运行时)
- [日志系统](#日志系统)
- [错误处理](#错误处理)
- [最佳实践](#最佳实践)

---

## 快速开始

### 导入 Prelude

推荐使用 `prelude` 模块一次性导入所有常用类型：

```rust
use neon_rtos2::prelude::*;
```

### 最小示例

```rust
#![no_std]
#![no_main]

use neon_rtos2::prelude::*;

#[entry]
fn main() -> ! {
    // 1. 初始化内核
    kernel_init();
    
    // 2. 创建任务
    Task::new("task1", || {
        loop {
            info!("Task 1 running");
            Delay::delay(Ticks(1000)).unwrap();
        }
    }).unwrap();
    
    Task::new("task2", || {
        loop {
            info!("Task 2 running");
            Delay::delay(Ticks(500)).unwrap();
        }
    }).unwrap();
    
    // 3. 启动调度器
    Scheduler::start();
    
    loop {}
}
```

---

## 任务管理

任务入口闭包返回时，局部变量正常析构，调度器随后把任务标记为 `Finished` 并唤醒所有等待者。在另一个运行中的任务里调用 `child.join()?` 即可等待退出。任务已经结束时立即返回，槽位复用也不会让旧句柄等到新任务。丢弃普通 `Task` 观察句柄后，任务继续执行。

`TypedTask::<Created>::new(...)?` 返回预留任务槽的唯一所有者。`start()` 消费它并发布任务。提前返回或丢弃它会释放闭包和槽位，并唤醒等待该任务退出的调用者。

### 创建任务

#### 方式一：简单创建

```rust
let task = Task::new("my_task", || {
    let task_id = Scheduler::get_current_task().get_taskid();
    loop {
        // 任务逻辑
        info!("Task {} running", task_id);
        Delay::delay(Ticks(1000)).unwrap();
    }
})?;
```

#### 方式二：Builder 模式（推荐）

```rust
let task = Task::builder("sensor_task")
    .priority(Priority::High)      // 设置优先级
    .spawn(|| {
        loop {
            // 高优先级任务逻辑
        }
    })?;
```

### 优先级

```rust
pub enum Priority {
    Idle = 0,      // 空闲优先级（最低）
    Low = 1,       // 低优先级
    Normal = 2,    // 普通优先级（默认）
    High = 3,      // 高优先级
    Critical = 4,  // 关键优先级（最高）
}

// 获取优先级
let priority = task.get_priority();

// 设置优先级
task.set_priority(Priority::High);
```

### 任务状态

```rust
pub enum TaskState {
    Uninit,           // 未初始化
    Created,          // 已保留槽位，等待启动
    Finished,         // 任务结束或句柄已失效
    Ready,            // 就绪
    Running,          // 运行中
    Blocked(Event),   // 阻塞（包含阻塞原因）
}

// 获取状态
let state = task.get_state();

// 任务由调度器选中时进入 Running。需要主动让出 CPU 时调用 Scheduler::task_switch()。
// 显式挂起尚未运行的就绪任务，并按相同事件唤醒。
if task.block_if_ready(Event::Signal(1)) {
    assert!(task.wake_if(Event::Signal(1)));
}
```

### 任务迭代器

```rust
// 遍历所有任务
Task::iter().for_each(|task| {
    info!("Task: {} (ID: {})", task.get_name(), task.get_taskid());
});

// 统计就绪任务数量
let ready_count = Task::iter()
    .filter(|t| t.get_state() == TaskState::Ready)
    .count();

// 获取所有就绪任务
for task in Task::ready_tasks() {
    info!("Ready: {}", task.get_name());
}

// 获取所有阻塞任务
for task in Task::blocked_tasks() {
    info!("Blocked: {}", task.get_name());
}

// 获取最高优先级的就绪任务
let highest = Task::ready_tasks()
    .max_by_key(|t| t.get_priority());
```

---

## 同步原语

### 互斥锁 (Mutex)

锁的生命周期由守卫管理，同步和异步获取都返回守卫。需要跨所有权边界传递时使用 `lock_owned()`。

```rust
let counter = Mutex::new(0u32);
counter.with_lock(|value| *value += 1)?;
let value = *counter.lock()?;
```

### 条件变量与信号量

`wait_while` 和 `wait_while_async` 在持锁状态下检查条件。进入等待时释放守卫，成功返回时重新持有同一把锁。丢弃等待中的 Future 会注销等待。借用守卫跨过 await 的任务在 `LocalExecutor` 上运行；可在线程间移动的 `Executor` 接受 Send Future。

```rust
let guard = ready.lock_async().await?;
let guard = changed.wait_while_async(guard, |flag| !*flag).await?;
```

`try_lock()`、`try_wait()` 立即返回；有截止时间的操作使用 `lock_timeout(duration)`、`wait_timeout(duration)`。计数事件使用 `Signal::send()` 和 `wait()`。占用有限资源使用 `Semaphore::acquire_permit()` 或 `acquire_permit_async().await`，作用域结束时归还。合并拥有所有权的许可使用 `try_merge(other)`，失败时从错误中取回原许可。

### 事件 (Event)

事件用于标识任务阻塞的原因：

```rust
pub enum Event {
    Signal(usize),   // 等待信号量
    Mutex(usize),    // 等待互斥锁
    Timer(usize),    // 等待定时器
    Mq(usize),       // 等待消息队列
}

// 阻塞任务
task.block(Event::Signal(signal_id));

// 唤醒等待特定事件的任务
Event::wake_task(Event::Signal(signal_id));
```

---

## 进程间通信

### 类型化消息通道

`bounded::<T, N>()?` 创建有界通道，缓冲区在堆上预留。发送端可克隆，接收端独占接收权。两端都支持立即尝试、RTOS 任务阻塞等待、Future 异步等待。

| 操作 | 立即尝试 | RTOS 任务 | Future |
|---|---|---|---|
| 发送 | `tx.try_send(value)` | `tx.send_blocking(value)` | `tx.send(value).await` |
| 接收 | `rx.try_recv()` | `rx.recv_blocking()` | `rx.recv().await` |

`try_send` 失败时通过 `TrySendError::Full(value)` 或 `Closed(value)` 交还原值。等待发送只在关闭时返回 `SendError(value)`。最后一个发送端析构后，接收方读完缓冲消息会收到 `None`。接收端析构会唤醒等待的发送方。

```rust
use neon_rtos2::prelude::*;

let (tx, rx) = bounded::<u32, 4>()?;
Task::builder("producer").spawn(move || {
    tx.send_blocking(42).unwrap();
})?;
while let Some(sample) = rx.recv_blocking() {
    info!("sample {}", sample);
}
```

需要多个任务共享出队操作时可使用 `Mq<T, N>`。动态 IPC 使用 `IpcHandle`，接收时检查类型，由调用方销毁队列。

---

## 定时器系统

### 软件定时器

```rust
let mut timer = Timer::after(Ticks(1000))?;
timer.wait()?;     // 从创建时刻算起，到期后立即返回
timer.restart()?;  // 从现在重新计时，复用原槽位
timer.cancel();    // 提前取消并释放槽位；离开作用域也会自动取消
```

### 延时

```rust
// 延时 1000ms
Delay::delay(Ticks(1000))?;
```

### 系统时钟

```rust
let now = Systick::now();
let period = Systick::tick_period();
```

板级代码在 `kernel_init()` 后配置硬件和内核时间单位。Cortex-M3 使用 `hal::cortex_m3::configure_systick(syst, core_hz, period)`，RISC-V virt 使用 `hal::riscv::configure_systick(period)`。Cortex-M3 参数无效时，错误携带原外设，可调整参数重试。

`hal::with_interrupts_masked(|cs| ...)` 将中断屏蔽局限于闭包，嵌套调用和提前返回都会恢复进入时的状态。

---

## 异步运行时

### 创建并驱动执行器

```rust,no_run
use neon_rtos2::runtime::{Executor, sleep, yield_now};
use neon_rtos2::prelude::Ticks;

async fn worker() {
    sleep(Ticks(10)).await;
    yield_now().await;
}

fn run_runtime() {
    let mut executor = Executor::new();
    executor.spawn(worker());

    executor.run();
}
```

需要返回值或控制子任务生命周期时使用 `spawn_joinable(future)`。`handle.join().await` 返回 `Some(output)`；执行器提前析构则返回 `None`。`join()` 可变借用句柄，结果只能取一次，取完后再次等待返回 `None`。在 `select!` 中放弃本次等待后，仍可用原句柄继续等待。显式调用 `handle.cancel()` 或丢弃未完成句柄会唤醒执行器，在下一次轮询该任务时释放 Future 和等待登记。独立运行的任务使用 `spawn(future)`。

`runtime::timeout(duration, future)` 从构造时开始计时。到期会释放传入的 Future 及其等待登记，返回 `Err(RtosError::Timeout)`。同一次轮询中操作和截止时间都已就绪时，优先返回操作结果。比如 `timeout(Ticks(10), handle.join()).await` 超时后，可以继续等待同一个子任务。

`signal_pair()`、`mpsc_signal()` 和 `broadcast_signal()` 按方向跟踪端点。最后一个发送端析构会唤醒所有接收者，已发送的计数仍可消费，耗尽后等待返回 `SignalClosed`。最后一个接收端析构后，发送端可通过 `is_closed()` 检查断开。显式 `close()` 会立即终止等待。


外部事件循环使用 `poll_once()` 单步驱动。返回 `RunState::Runnable` 时还有就绪任务，`Waiting` 表示需要外部唤醒，`Finished` 表示全部结束。

静态异步信号使用 `AsyncSignal::new()`，生产者调用 `send()` 或 `send_many(n)`，消费者调用 `wait_async().await`。

### 异步休眠

```rust,no_run
use neon_rtos2::runtime::sleep;

async fn periodic() {
    loop {
        // 处理周期性工作
        sleep(Duration::from_millis(100)).await;
    }
}
```

- `sleep(duration)` 在首次 `poll` 时向定时器子系统注册异步休眠槽位。
- `Timer::timer_check_and_send_event()` 在截止时间到达时会唤醒对应 `Waker`，执行器下一次轮询该 Future 时返回完成。
- 如果 Future 在完成前被丢弃，其 `Drop` 会注销已注册的休眠槽位，避免残留等待项。

### 当前支持范围

- 本轮优化范围仅覆盖异步休眠的注册、截止时间唤醒、重新轮询完成路径，以及执行器唤醒队列与 `run()`/`poll_once()` 的共享轮询逻辑。
- `Executor::run()` 适用于 RTOS 任务上下文；当没有可运行异步任务但仍有未完成 Future 时，它会阻塞当前 RTOS 任务并保持现有空闲语义。
- `Executor::poll_once()` 不阻塞当前 RTOS 任务，适合测试或手动驱动。
- 延时与同步等待统一接受 `Duration::from_millis(10)` 或 `Ticks(10)`。原先按 tick 传入的整数迁移为 `Ticks(n)`。Duration 按板级 tick 周期向上取整，最大值为 usize::MAX 个 tick。默认周期为 1 ms。板级时钟初始化同时记录实际周期，已登记的截止点保留原 tick 计数。

---

### DMA 传输所有权

板级后端实现 `hal::DmaChannel`，一个通道值代表独占且已配置传输方向的外设 DMA 通道。`channel.transfer(buffer)` 消费通道和 `Box<[u8]>`，首次 poll 时启动。完成结果携带通道、原缓冲区和硬件状态；`transfer.cancel()` 停止并取回两项资源。直接丢弃活动传输会同步停止硬件，再释放缓冲区。STM32F1 UART 示例缺少寄存器后端，对非空硬件读写返回 `DeviceError::Unsupported`。

## 驱动接口

`Device::into_initialized()` 适用于创建后立即初始化的设备。已有设备可以用 `init(&mut self)` 原地初始化。UART、SPI 和 I²C 驱动通过 `Configure` 实现配置一次；UART 波特率由 `SerialPort` 提供。

```rust
use neon_rtos2::drivers::{Configure, Device, Read, SerialConfig};
use neon_rtos2::drivers::examples::MockUart;

let mut uart = MockUart::new().into_initialized().unwrap();
uart.configure(SerialConfig::default()).unwrap();
uart.mock_receive(&[42]);
assert_eq!(uart.read_byte().unwrap(), 42);
```

`read`、`write` 返回实际传输数量。`read_exact`、`write_all` 及其异步版本在零进展时返回 `TransferError::NoProgress { completed }`；设备自身的错误保留在 `TransferError::Device` 中。定时器使用 `set_period(Duration)`，硬件分辨率与范围由驱动校验。

## 日志系统

### 日志级别

```rust
trace!("Trace message");   // 最详细
debug!("Debug message");   // 调试信息
info!("Info message");     // 一般信息
warn!("Warning message");  // 警告
error!("Error message");   // 错误
```

### 格式化输出

```rust
let task_id = 1;
let value = 42;

info!("Task {} value: {}", task_id, value);
debug!("State: {:?}", task.get_state());
```

---

## 错误处理

### 错误类型

```rust
pub enum RtosError {
    // 任务相关
    TaskNotFound,
    TaskSlotsFull,
    InvalidTaskState,
    
    // 同步相关
    MutexNotOwned,
    MutexSlotsFull,
    SignalSlotsFull,
    
    // IPC 相关
    QueueFull,
    QueueEmpty,
    InvalidHandle,
    TypeMismatch,
    
    // 定时器相关
    TimerSlotsFull,
    TimerNotFound,
    
    // 内存相关
    OutOfMemory,
}
```

### 错误处理方式

#### 方式一：使用 `?` 操作符

```rust
fn init_system() -> Result<()> {
    let task1 = Task::new("task1", task_fn)?;
    let mutex = Mutex::new()?;
    let timer = Timer::after(Ticks(1000))?;
    Ok(())
}
```

#### 方式二：使用 `unwrap()`（仅用于确定不会失败的情况）

```rust
let task = Task::new("task", || {}).unwrap();
```

#### 方式三：模式匹配

```rust
match Task::new("task", || {}) {
    Ok(task) => info!("Task created: {}", task.get_name()),
    Err(RtosError::TaskSlotsFull) => error!("No free task slots!"),
    Err(e) => error!("Failed to create task: {:?}", e),
}
```

---

## 最佳实践

### 1. 使用 Builder 模式创建任务

```rust
// ✅ 推荐
let task = Task::builder("my_task")
    .priority(Priority::High)
    .spawn(task_fn)?;

// ❌ 不推荐（无法设置优先级）
let task = Task::new("my_task", task_fn)?;
```

### 2. 使用 RAII 管理互斥锁

```rust
let mut guard = mutex.lock()?;
*guard += 1;
```

### 3. 使用迭代器而非手动遍历

```rust
// ✅ 推荐
let ready_count = Task::ready_tasks().count();

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
// ✅ 推荐：传播错误
fn init() -> Result<()> {
    let task = Task::new("task", || {})?;
    Ok(())
}

// ❌ 不推荐：忽略错误
let _ = Task::new("task", || {});
```

### 5. 使用 prelude 简化导入

```rust
// ✅ 推荐
use neon_rtos2::prelude::*;

// ❌ 不推荐
use neon_rtos2::kernel::task::{Task, TaskBuilder, Priority};
use neon_rtos2::kernel::scheduler::Scheduler;
use neon_rtos2::sync::{Mutex, Signal};
// ...
```

### 6. 任务函数保持简洁

```rust
// ✅ 推荐：任务函数调用其他函数
fn sensor_task(_: usize) {
    loop {
        let data = read_sensor();
        process_data(data);
        Delay::delay(Ticks(100)).unwrap();
    }
}

// ❌ 不推荐：所有逻辑都在任务函数中
fn sensor_task(_: usize) {
    loop {
        // 100 行代码...
    }
}
```

---

## API 速查表

### 任务

| API | 说明 |
|-----|------|
| `Task::new(name, fn)` | 创建任务 |
| `Task::builder(name)` | 创建任务构建器 |
| `task.get_state()` | 获取状态 |
| `task.get_priority()` | 获取优先级 |
| `task.set_priority(p)` | 设置优先级 |
| `Task::iter()` | 任务迭代器 |
| `Task::ready_tasks()` | 就绪任务迭代器 |

### 调度器

| API | 说明 |
|-----|------|
| `Scheduler::start()` | 启动调度器 |
| `Scheduler::stop()` | 停止调度器 |
| `Scheduler::get_current_task()` | 获取当前任务 |

### 同步

| API | 说明 |
|-----|------|
| `Mutex::new(value)` | 创建互斥锁 |
| `mutex.lock()` | 加锁 |
| `mutex.lock_async().await` | 异步获取守卫 |
| `mutex.with_lock(fn)` | 闭包风格 |
| `Signal::wait()` | 等待信号 |
| `Signal::send()` | 发送信号 |

### 定时器

| API | 说明 |
|-----|------|
| `Timer::after(timeout)` | 创建并开始计时 |
| `timer.wait()` | 等待到期 |
| `timer.cancel()` | 消费句柄，取消并释放槽位 |
| `Delay::delay(Duration::from_millis(ms))` | 延时 |

### IPC

| API | 说明 |
|-----|------|
| `bounded::<T, N>()?` | 创建有界类型化通道 |
| `tx.try_send(msg)` | 立即尝试，失败退回消息 |
| `rx.recv_blocking()` | 等待消息或通道结束 |

---

*文档版本：v1.0*  
*最后更新：2026年6月13日*
