# Neon-RTOS2 架构设计文档

> 最后更新：2026年6月13日

本文档详细介绍 Neon-RTOS2 的架构设计、模块组织和核心实现原理。

---

## 📋 目录

- [设计理念](#设计理念)
- [整体架构](#整体架构)
- [模块详解](#模块详解)
- [内存布局](#内存布局)
- [调度机制](#调度机制)
- [同步原语](#同步原语)
- [硬件抽象层](#硬件抽象层)
- [异步运行时](#异步运行时)

---

## 设计理念

### 核心原则

1. **安全第一** - 利用 Rust 类型系统在编译期捕获错误
2. **零成本抽象** - 高层抽象不带来运行时开销
3. **模块化设计** - 清晰的模块划分，易于扩展和维护
4. **可移植性** - 通过 HAL 层支持多种硬件架构

### Rust 特性应用

| 特性 | 应用场景 |
|------|----------|
| 所有权系统 | 资源管理、防止数据竞争 |
| Trait | 设备驱动抽象、HAL 接口 |
| 泛型 | 类型安全的消息队列、通道 |
| 宏 | 信号量定义、设备驱动生成 |
| async/await | 异步任务、非阻塞 I/O |
| const generics | 编译期确定的缓冲区大小 |

---

## 整体架构

### 分层架构图

```
┌─────────────────────────────────────────────────────────────┐
│                      Application Layer                       │
│                    (用户任务和应用逻辑)                        │
├─────────────────────────────────────────────────────────────┤
│                       API Layer                              │
│              prelude.rs (统一导出接口)                        │
├─────────────────────────────────────────────────────────────┤
│                      Kernel Layer                            │
│  ┌─────────┐  ┌──────────┐  ┌────────┐  ┌─────────────┐    │
│  │  Task   │  │Scheduler │  │  Time  │  │   Power     │    │
│  │ Manager │  │          │  │ System │  │  Manager    │    │
│  └─────────┘  └──────────┘  └────────┘  └─────────────┘    │
├─────────────────────────────────────────────────────────────┤
│                   Synchronization Layer                      │
│  ┌─────────┐  ┌──────────┐  ┌────────┐  ┌─────────────┐    │
│  │  Mutex  │  │  Signal  │  │ Event  │  │   Guard     │    │
│  └─────────┘  └──────────┘  └────────┘  └─────────────┘    │
├─────────────────────────────────────────────────────────────┤
│                       IPC Layer                              │
│  ┌─────────────────┐  ┌─────────────────────────────┐      │
│  │  Message Queue  │  │         Channel             │      │
│  └─────────────────┘  └─────────────────────────────┘      │
├─────────────────────────────────────────────────────────────┤
│                    Runtime Layer                             │
│  ┌──────────┐  ┌────────┐  ┌───────┐  ┌────────────┐       │
│  │ Executor │  │ Future │  │ Waker │  │   Select   │       │
│  └──────────┘  └────────┘  └───────┘  └────────────┘       │
├─────────────────────────────────────────────────────────────┤
│                       HAL Layer                              │
│  ┌─────────────────┐  ┌─────────────────────────────┐      │
│  │   Cortex-M3     │  │         RISC-V              │      │
│  │  (ARM Thumb)    │  │      (RV32IMAC)             │      │
│  └─────────────────┘  └─────────────────────────────┘      │
├─────────────────────────────────────────────────────────────┤
│                     Driver Layer                             │
│  ┌────────┐  ┌──────┐  ┌─────┐  ┌─────┐  ┌───────┐        │
│  │  UART  │  │ GPIO │  │ SPI │  │ I2C │  │ Timer │        │
│  └────────┘  └──────┘  └─────┘  └─────┘  └───────┘        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │    Hardware     │
                    └─────────────────┘
```

### 目录结构

```
src/
├── kernel/                 # 内核核心
│   ├── task/              # 任务管理
│   │   ├── mod.rs         # Task 结构体和方法
│   │   ├── builder.rs     # TaskBuilder 构建器
│   │   ├── priority.rs    # Priority 优先级
│   │   └── state.rs       # TypedTask 类型状态
│   ├── scheduler/         # 调度器
│   │   └── mod.rs         # Scheduler 实现
│   ├── time/              # 时间管理
│   │   ├── mod.rs
│   │   ├── systick.rs     # 系统滴答
│   │   └── timer.rs       # 软件定时器
│   ├── power.rs           # 电源管理
│   └── mod.rs
├── sync/                   # 同步原语
│   ├── mutex.rs           # 互斥锁
│   ├── signal.rs          # 信号量
│   ├── event.rs           # 事件
│   └── guard.rs           # RAII 守卫
├── ipc/                    # 进程间通信
│   ├── queue.rs           # 消息队列
│   └── channel.rs         # 类型安全通道
├── runtime/                # 异步运行时
│   ├── executor.rs        # 执行器
│   ├── future.rs          # Future 实现
│   ├── waker.rs           # Waker 机制
│   ├── channel.rs         # 异步通道
│   └── select.rs          # select! 宏
├── hal/                    # 硬件抽象层
│   ├── traits.rs          # HAL trait 定义
│   ├── cortex_m3/         # Cortex-M3 实现
│   │   ├── mod.rs
│   │   └── asm/context.s  # 上下文切换汇编
│   └── riscv/             # RISC-V 实现
│       ├── mod.rs
│       └── asm/context.s
├── drivers/                # 设备驱动框架
│   ├── traits.rs          # 驱动 trait 定义
│   ├── macros.rs          # 驱���宏
│   └── examples/          # 示例驱动
├── error/                  # 错误类型
├── config/                 # 系统配置
├── mem/                    # 内存管理
├── log/                    # 日志系统
├── lib.rs                  # 库入口
├── prelude.rs              # 预导入模块
└── utils.rs                # 工具函数
```

---

## 模块详解

### 1. 任务管理 (kernel/task)

#### Task 结构

```rust
pub struct Task {
    /// 任务 ID
    id: usize,
    /// 任务名称
    name: &'static str,
    /// 任务状态
    state: TaskState,
    /// 任务优先级
    priority: Priority,
    /// 栈指针
    stack_ptr: *mut usize,
    /// 栈空间
    stack: [u8; STACK_SIZE],
    /// 任务入口函数
    entry: fn(usize),
}
```

#### 任务状态机

```
                    ┌─────────────┐
                    │   Uninit    │
                    └──────┬──────┘
                           │ init()
                           ▼
                    ┌─────────────┐
         ┌─────────│    Ready    │◄────────┐
         │         └──────┬──────┘         │
         │                │ schedule()     │
         │                ▼                │
         │         ┌─────────────┐         │
         │         │   Running   │         │
         │         └──────┬──────┘         │
         │                │                │
         │    ┌───────────┼───────────┐    │
         │    │           │           │    │
         │    ▼           ▼           ▼    │
         │ yield()    block()     完成     │
         │    │           │           │    │
         │    │           ▼           │    │
         │    │    ┌─────────────┐    │    │
         │    │    │   Blocked   │    │    │
         │    │    └──────┬──────┘    │    │
         │    │           │ wake()    │    │
         │    └───────────┴───────────┘    │
         │                                 │
         └─────────────────────────────────┘
```

#### 优先级

```rust
pub enum Priority {
    Idle = 0,      // 空闲优先级（最低）
    Low = 1,       // 低���先级
    Normal = 2,    // 普通优先级（默认）
    High = 3,      // 高优先级
    Critical = 4,  // 关键优先级（最高）
}
```

### 2. 调度器 (kernel/scheduler)

#### 调度算法

Neon-RTOS2 使用**优先级抢占式调度**结合**时间片轮转**：

1. **优先级抢占**：高优先级任务可以抢占低优先级任务
2. **时间片轮转**：同优先级任务按时间片轮转执行
3. **空闲任务**：当没有就绪任务时执行空闲任务

```rust
pub struct Scheduler {
    /// 当前运行任务 ID
    current_task: AtomicUsize,
    /// 是否正在运行
    running: AtomicBool,
    /// 时间片计数
    tick_count: AtomicUsize,
}

impl Scheduler {
    /// 选择下一个任务
    fn select_next_task() -> Option<usize> {
        // 1. 查找最高优先级的就绪任务
        Task::ready_tasks()
            .max_by_key(|t| t.get_priority())
            .map(|t| t.get_taskid())
    }
    
    /// 调度（在 SysTick 中断中调用）
    pub fn schedule() {
        if let Some(next) = Self::select_next_task() {
            let current = Self::current_task_id();
            if next != current {
                Self::context_switch(current, next);
            }
        }
    }
}
```

### 3. 同步原语 (sync)

#### Mutex 实现

```rust
pub struct Mutex {
    /// 互斥锁 ID
    id: usize,
    /// 是否被锁定
    locked: AtomicBool,
    /// 持有者任务 ID
    owner: AtomicUsize,
}

impl Mutex {
    /// 获取锁（阻塞）
    pub fn lock(&self) {
        loop {
            if self.try_lock() {
                return;
            }
            // 阻塞当前任务
            Task::current().block(Event::Mutex(self.id));
            Scheduler::schedule();
        }
    }
    
    /// RAII 守卫
    pub fn lock_guard(&self) -> MutexGuard<'_> {
        self.lock();
        MutexGuard { mutex: self }
    }
}

/// RAII 守卫，离开作用域自动释放锁
pub struct MutexGuard<'a> {
    mutex: &'a Mutex,
}

impl Drop for MutexGuard<'_> {
    fn drop(&mut self) {
        self.mutex.unlock().ok();
    }
}
```

#### Signal 实现

```rust
pub struct Signal {
    /// 信号量 ID
    id: usize,
    /// 信号计数
    count: AtomicUsize,
}

impl Signal {
    /// 等待信号
    pub fn wait(&self) {
        loop {
            let count = self.count.load(Ordering::Acquire);
            if count > 0 {
                if self.count.compare_exchange(
                    count, count - 1,
                    Ordering::AcqRel, Ordering::Relaxed
                ).is_ok() {
                    return;
                }
            } else {
                // 阻塞当前任务
                Task::current().block(Event::Signal(self.id));
                Scheduler::schedule();
            }
        }
    }
    
    /// 发送信号
    pub fn send(&self) {
        self.count.fetch_add(1, Ordering::Release);
        // 唤醒等待的任务
        Event::wake_task(Event::Signal(self.id));
    }
}
```

### 4. 硬件抽象层 (hal)

#### HAL Trait 定义

```rust
/// 架构相关操作 trait
pub trait ArchOps {
    /// 初始化任务栈
    fn init_task_stack(stack: &mut [u8], entry: fn(usize), arg: usize) -> *mut usize;
    
    /// 上下文切换
    fn context_switch(from: *mut usize, to: *mut usize);
    
    /// 启动第一个任务
    fn start_first_task(sp: *mut usize) -> !;
    
    /// 使能中断
    fn enable_interrupts();
    
    /// 禁用中断
    fn disable_interrupts();
    
    /// 进入低功耗模式
    fn enter_low_power();
}
```

#### Cortex-M3 实现

```rust
// 上下文切换（通过 PendSV 中断）
#[naked]
#[no_mangle]
pub unsafe extern "C" fn PendSV() {
    asm!(
        // 保存当前上下文
        "mrs r0, psp",
        "stmdb r0!, {{r4-r11}}",
        
        // 保存 PSP 到当前任务
        "ldr r1, =CURRENT_TASK_SP",
        "str r0, [r1]",
        
        // 加载下一个任务的 SP
        "ldr r1, =NEXT_TASK_SP",
        "ldr r0, [r1]",
        
        // 恢复上下文
        "ldmia r0!, {{r4-r11}}",
        "msr psp, r0",
        
        // 返回
        "bx lr",
        options(noreturn)
    );
}
```

#### RISC-V 实现

```rust
// 上下文切换
#[naked]
#[no_mangle]
pub unsafe extern "C" fn context_switch() {
    asm!(
        // 保存当前上下文
        "addi sp, sp, -64",
        "sw ra, 0(sp)",
        "sw s0, 4(sp)",
        "sw s1, 8(sp)",
        // ... 保存其他寄存器
        
        // 保存 SP 到当前任务
        "la t0, CURRENT_TASK_SP",
        "sw sp, 0(t0)",
        
        // 加载下一个任务的 SP
        "la t0, NEXT_TASK_SP",
        "lw sp, 0(t0)",
        
        // 恢复上下文
        "lw ra, 0(sp)",
        "lw s0, 4(sp)",
        // ... 恢复其他寄存器
        "addi sp, sp, 64",
        
        "ret",
        options(noreturn)
    );
}
```

---

## 内存布局

### 任务栈布局

```
高地址
┌──────────��──────────┐
│    栈顶标记         │  ← 用于栈溢出检测
├─────────────────────┤
│                     │
│    用户栈空间       │
│                     │
├─────────────────────┤
│    保存的寄存器     │  ← 上下文切换时保存
│    (R4-R11/s0-s11)  │
├─────────────────────┤
│    异常帧           │  ← 硬件自动保存
│    (R0-R3,R12,LR,   │
│     PC,xPSR)        │
├─────────────────────┤
│    栈底标记         │  ← 用于栈溢出检测
└─────────────────────┘
低地址
```

### 系统内存布局

```
┌─────────────────────┐  高地址
│                     │
│    堆空间           │  ← 动态内存分配
│    (HEAP_SIZE)      │
│                     │
├─────────────────────┤
│                     │
│    任务栈池         │  ← 每个任务 STACK_SIZE
│    (MAX_TASKS *     │
│     STACK_SIZE)     │
│                     │
├─────────────────────┤
│    全局数据         │  ← 任务管理器、调度器等
│    (.data/.bss)     │
├─────────────────────┤
│    代码段           │
│    (.text)          │
└─────────────────────┘  低地址
```

---

## 调度机制

### 调度时机

1. **时间片到期** - SysTick 中断触发
2. **任务阻塞** - 等待信号量、互斥锁等
3. **任务唤醒** - 信号量发送、互斥锁释放
4. **主动让出** - 调用 `yield()`

### 调度流程

```
SysTick 中断
     │
     ▼
┌─────────────┐
│ 更新系统时间 │
└──────┬──────┘
       │
       ▼
┌─────────────┐
│ 检查定时器   │
└──────┬──────┘
       │
       ▼
┌─────────────┐     否
│ 需要调度？   │────────► 返回
└──────┬──────┘
       │ 是
       ▼
┌─────────────┐
│ 选择下一任务 │
└──────┬──────┘
       ��
       ▼
┌─────────────┐
│ 触发 PendSV  │
└──────┬──────┘
       │
       ▼
┌─────────────┐
│ 上下文切换   │
└─────────────┘
```

---

## 异步运行时

### 执行器 (Executor)

```rust
pub struct Executor {
    tasks: Vec<Option<AsyncTask>>,
    woken_state: Arc<Mutex<RefCell<WokenState>>>,
    next_task_id: usize,
    task_count: usize,
}

impl Executor {
    pub fn run(&mut self) { /* 阻塞式驱动 */ }
    pub fn poll_once(&mut self) -> bool { /* 非阻塞驱动 */ }
}
```

- `spawn()` 会把新 Future 包装为 `AsyncTask`，并立即把任务 ID 推入唤醒队列，保证至少被轮询一次。
- `run()` 与 `poll_once()` 都复用 `poll_next_woken_task()` 这一核心路径，只处理被唤醒的 Future，而不是每轮遍历所有任务。
- `run()` 在没有新唤醒任务但执行器仍有未完成任务时，会把当前 RTOS 任务阻塞在 `Event::Async(...)` 上，再交回 RTOS 调度器，保留当前空闲阻塞语义。
- `poll_once()` 共享同一轮询逻辑，但不会阻塞当前 RTOS 任务，更适合测试或外部手动驱动。
- `TaskWaker` 在唤醒 Future 时会同时把 Future ID 放回执行器队列，并通过 `Event::wake_task_by_id()` 唤醒承载执行器的 RTOS 任务。

### 异步休眠 (Sleep)

```rust
pub struct Sleep {
    deadline: usize,
    registered: bool,
    waiter_id: Option<usize>,
}
```

- `runtime::sleep(duration_ms)` 返回 `Sleep` Future；首次 `poll` 时通过 `Timer::register_async_sleep()` 注册一个异步休眠槽位。
- `registered` 和 `waiter_id` 保证同一个 `Sleep` 只注册一次，不再依赖占位式 TODO 或仅克隆 `Waker` 的临时实现。
- `Timer::timer_check_and_send_event()` 会在截止时间到达时取出对应 `Waker`、清理槽位，并触发唤醒。
- 被唤醒的任务再次进入执行器后，`Sleep::poll()` 检查当前时间达到 `deadline`，随后返回 `Poll::Ready(())`。
- 如果 `Sleep` 在完成前被丢弃，`Drop` 会调用 `Timer::unregister_async_sleep()` 清理未使用的槽位。

### 本轮优化范围

- 本轮只覆盖异步休眠的注册、截止时间唤醒、重新轮询完成路径，以及执行器的唤醒队列与共享轮询逻辑。
- 本轮没有扩展为 `Duration` 风格时间接口，也没有引入公平调度、多核执行或新的定时器后端。

### Select 宏

```rust
/// 同时等待多个 Future，返回第一个完成的
#[macro_export]
macro_rules! select {
    ($($fut:expr => $handler:expr),+ $(,)?) => {{
        // 创建 Select Future
        let select_future = select2($($fut),+);
        
        // 等待结果
        match select_future.await {
            Either::First(result) => { $handler }
            Either::Second(result) => { $handler }
            // ...
        }
    }};
}
```

---

## 设备驱动框架

### Trait 层次

```
Device (基础设备)
   │
   ├── Read (可读设备)
   │      │
   │      └── ReadWrite = Read + Write
   │
   ├── Write (可写设备)
   │
   ├── GpioPin (GPIO 引脚)
   │      ├── InputPin
   │      └── OutputPin
   │
   ├── Uart (串口)
   │
   ├── Spi (SPI 总线)
   │
   ├── I2c (I2C 总线)
   │
   └── TimerDevice (定时器)
```

### 驱动实现示例

```rust
use neon_rtos2::drivers::{Device, Uart, Read, Write};

pub struct MyUart {
    base_addr: usize,
    baudrate: u32,
}

impl Device for MyUart {
    type Error = UartError;
    
    fn init(&mut self) -> Result<(), Self::Error> {
        // 初始化硬件
        unsafe {
            let cr = (self.base_addr + 0x0C) as *mut u32;
            cr.write_volatile(0x01); // 使能 UART
        }
        Ok(())
    }
    
    fn name(&self) -> &'static str {
        "UART0"
    }
}

impl Read for MyUart {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // 从 UART 读取数据
        Ok(buf.len())
    }
}

impl Write for MyUart {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // 向 UART 写入数据
        for &byte in buf {
            unsafe {
                let dr = (self.base_addr + 0x00) as *mut u32;
                dr.write_volatile(byte as u32);
            }
        }
        Ok(buf.len())
    }
    
    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
```

---

## 电源管理

### 电源状态

```rust
pub enum PowerState {
    Active,     // 全速运行
    Idle,       // 空闲（WFI）
    Sleep,      // 睡眠模式
    DeepSleep,  // 深度睡眠
}
```

### 唤醒源

```rust
pub enum WakeupSource {
    Interrupt,      // 任意中断
    Timer,          // 定时器
    Gpio,           // GPIO 事件
    Uart,           // UART 接收
    Rtc,            // RTC 闹钟
    External,       // 外部事件
}
```

---

## 总结

Neon-RTOS2 通过精心设计的分层架构，实现了：

1. **高度模块化** - 各模块职责清晰，易于维护和扩展
2. **类型安全** - 利用 Rust 类型系统防止常见错误
3. **可移植性** - HAL 层抽象使得支持新架构变得简���
4. **现代化 API** - Builder 模式、RAII、async/await 等现代编程范式

---

*文档版本：v1.0*  
*最后更新：2026年6月13日*
