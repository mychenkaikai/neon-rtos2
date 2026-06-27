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
    Task::new("task1", |_| {
        loop {
            info!("Task 1 running");
            Delay::delay(1000).unwrap();
        }
    }).unwrap();
    
    Task::new("task2", |_| {
        loop {
            info!("Task 2 running");
            Delay::delay(500).unwrap();
        }
    }).unwrap();
    
    // 3. 启动调度器
    Scheduler::start();
    
    loop {}
}
```

---

## 任务管理

### 创建任务

#### 方式一：简单创建

```rust
let task = Task::new("my_task", |task_id| {
    loop {
        // 任务逻辑
        info!("Task {} running", task_id);
        Delay::delay(1000).unwrap();
    }
})?;
```

#### 方式二：Builder 模式（推荐）

```rust
let task = Task::builder("sensor_task")
    .priority(Priority::High)      // 设置优先级
    .stack_size(8192)              // 设置栈大小
    .spawn(|_| {
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
    Ready,            // 就绪
    Running,          // 运行中
    Blocked(Event),   // 阻塞（包含阻塞原因）
}

// 获取状态
let state = task.get_state();

// 状态转换
task.ready();                      // -> Ready
task.run();                        // -> Running
task.block(Event::Signal(1));      // -> Blocked
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

#### 创建互斥锁

```rust
let mutex = Mutex::new()?;
```

#### 方式一：手动加锁/解锁

```rust
mutex.lock();
// 临界区代码
mutex.unlock()?;
```

#### 方式二：RAII 守卫（推荐）

```rust
{
    let _guard = mutex.lock_guard();
    // 临界区代码
    // 离开作用域自动释放锁
}
```

#### 方式三：闭包风格

```rust
mutex.with_lock(|| {
    // 临界区代码
});
```

### 信号量 (Signal)

```rust
// 定义信号量
define_signal!(MY_SIGNAL, 0);

// 任务 A：等待信号
fn task_a(_: usize) {
    loop {
        MY_SIGNAL.wait();  // 阻塞等待
        info!("Signal received!");
    }
}

// 任务 B：发送信号
fn task_b(_: usize) {
    loop {
        Delay::delay(1000).unwrap();
        MY_SIGNAL.signal();  // 发送信号
    }
}
```

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

### 消息队列 (Mq)

静态分配的消息队列，使用 const generics 指定容量：

```rust
// 创建容量为 10 的 u32 消息队列
let mut mq: Mq<u32, 10> = Mq::new()?;

// 发送消息
mq.send(42)?;

// 接收消息
if let Some(msg) = mq.receive() {
    info!("Received: {}", msg);
}

// 检查状态
if mq.is_empty() { /* ... */ }
if mq.is_full() { /* ... */ }
let count = mq.len();
```

### 通道 (Channel)

类型安全的通道，支持动态分配：

```rust
// 创建通道
let channel = Channel::new();

// 发送数据
channel.send(SensorData { temp: 25.5 })?;

// 接收数据
let data: SensorData = channel.receive()?;
```

---

## 定时器系统

### 软件定时器

```rust
// 创建定时器（超时时间 1000ms）
let mut timer = Timer::new(1000)?;

// 启动定时器
timer.start()?;

// 停止定时器
timer.stop()?;

// 检查是否超时
if timer.is_timeout() {
    info!("Timer expired!");
}

// 删除定时器
timer.delete();
```

### 延时

```rust
// 延时 1000ms
Delay::delay(1000)?;
```

### 系统时钟

```rust
// 获取当前系统时间（tick 数）
let now = Systick::get_current_time();

// 初始化系统时钟
Systick::init();
```

---

## 异步运行时

### 创建并驱动执行器

```rust,no_run
use neon_rtos2::runtime::{Executor, sleep, yield_now};

async fn worker() {
    sleep(10).await;
    yield_now().await;
}

fn run_runtime() {
    let mut executor = Executor::new();
    executor.spawn(worker());

    while executor.poll_once() {
        // 在测试或外部驱动场景中推进执行器。
    }
}
```

### 异步休眠

```rust,no_run
use neon_rtos2::runtime::sleep;

async fn periodic() {
    loop {
        // 处理周期性工作
        sleep(100).await;
    }
}
```

- `sleep(duration_ms)` 在首次 `poll` 时向定时器子系统注册异步休眠槽位。
- `Timer::timer_check_and_send_event()` 在截止时间到达时会唤醒对应 `Waker`，执行器下一次轮询该 Future 时返回完成。
- 如果 Future 在完成前被丢弃，其 `Drop` 会注销已注册的休眠槽位，避免残留等待项。

### 当前支持范围

- 本轮优化范围仅覆盖异步休眠的注册、截止时间唤醒、重新轮询完成路径，以及执行器唤醒队列与 `run()`/`poll_once()` 的共享轮询逻辑。
- `Executor::run()` 适用于 RTOS 任务上下文；当没有可运行异步任务但仍有未完成 Future 时，它会阻塞当前 RTOS 任务并保持现有空闲语义。
- `Executor::poll_once()` 不阻塞当前 RTOS 任务，适合测试或手动驱动。
- 当前 API 仍以毫秒或 tick 整数参数为主，不提供 `Duration` 风格异步休眠接口。

---

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
    let timer = Timer::new(1000)?;
    Ok(())
}
```

#### 方式二：使用 `unwrap()`（仅用于确定不会失败的情况）

```rust
let task = Task::new("task", |_| {}).unwrap();
```

#### 方式三：模式匹配

```rust
match Task::new("task", |_| {}) {
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
// ✅ 推荐：自动释放
{
    let _guard = mutex.lock_guard();
    // 临界区
}

// ❌ 不推荐：可能忘记解锁
mutex.lock();
// 临界区
mutex.unlock()?;  // 如果上面 panic，锁不会释放
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
    let task = Task::new("task", |_| {})?;
    Ok(())
}

// ❌ 不推荐：忽略错误
let _ = Task::new("task", |_| {});
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
        Delay::delay(100).unwrap();
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
| `Mutex::new()` | 创建互斥锁 |
| `mutex.lock()` | 加锁 |
| `mutex.unlock()` | 解锁 |
| `mutex.lock_guard()` | 获取 RAII 守卫 |
| `mutex.with_lock(fn)` | 闭包风格 |
| `Signal::wait()` | 等待信号 |
| `Signal::signal()` | 发送信号 |

### 定时器

| API | 说明 |
|-----|------|
| `Timer::new(timeout)` | 创建定时器 |
| `timer.start()` | 启动 |
| `timer.stop()` | 停止 |
| `Delay::delay(ms)` | 延时 |

### IPC

| API | 说明 |
|-----|------|
| `Mq::<T, N>::new()` | 创建消息队列 |
| `mq.send(msg)` | 发送消息 |
| `mq.receive()` | 接收消息 |

---

*文档版本：v1.0*  
*最后更新：2026年6月13日*
