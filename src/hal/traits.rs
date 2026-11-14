#![allow(async_fn_in_trait)]

//! 硬件抽象层 Trait 定义
//!
//! 这些 trait 定义了 RTOS 与底层硬件交互的接口，
//! 不同架构（Cortex-M3, RISC-V 等）需要实现这些 trait。

/// 上下文切换 trait
///
/// 定义了任务上下文切换所需的基本操作
pub trait ContextSwitch {
    /// 初始化任务栈
    ///
    /// 在任务栈上构建初始上下文，使任务可以被调度执行
    ///
    /// # 参数
    /// - `stack_top`: 栈顶指针（会被修改为初始化后的栈顶）
    /// - `entry`: 任务入口函数
    /// - `arg`: 传递给任务的参数
    ///
    /// # Safety
    /// stack_top must refer to the exclusive writable stack of the required
    /// size and alignment for this port.
    unsafe fn init_task_stack(stack_top: &mut usize, entry: fn(usize), arg: usize);

    /// 触发上下文切换
    ///
    /// 通常通过触发 PendSV 中断来实现
    fn trigger_switch();

    /// 启动第一个任务
    ///
    /// 从调度器启动第一个任务，开始多任务执行
    ///
    /// # Safety
    /// The scheduler and first task frame must be fully initialized.
    unsafe fn start_first_task();
}

/// 空闲任务 trait
///
/// 定义了空闲任务的初始化和执行
pub trait IdleTaskTrait {
    /// 初始化空闲任务
    ///
    /// 创建系统空闲任务，当没有其他任务可运行时执行
    fn init_idle_task();

    /// 空闲任务执行体
    ///
    /// 空闲任务的主循环，通常执行低功耗等待
    fn idle_loop() -> !;
}

/// Board-specific interrupt routing. Global masking uses hal::with_interrupts_masked.
pub trait InterruptControl {
    /// 检查中断是否启用
    ///
    /// # 返回值
    /// `true` 如果中断已启用
    fn is_interrupts_enabled() -> bool;

    /// 设置中断优先级
    ///
    /// # 参数
    /// - `irq`: 中断号
    /// - `priority`: 优先级值
    fn set_priority(irq: u32, priority: u8);

    /// Platform-specific registration error.
    type Error;
    /// An exclusive registration. Its Drop implementation must mask the IRQ
    /// and remove this handler before releasing the slot for reuse.
    type Registration;

    /// Install and enable the handler atomically. An occupied IRQ returns an
    /// error, retaining the existing registration.
    fn register_handler(irq: u32, handler: fn()) -> Result<Self::Registration, Self::Error>;
}

/// 处理器控制 trait
///
/// 定义了处理器级别的控制操作
pub trait ProcessorControl {
    /// 等待中断（低功耗模式）
    ///
    /// 使处理器进入低功耗等待状态，直到有中断发生
    fn wait_for_interrupt();

    /// 等待事件
    ///
    /// 使处理器等待事件发生
    fn wait_for_event();

    /// 发送事件
    ///
    /// 向其他处理器核心发送事件（多核系统）
    fn send_event();

    /// 数据同步屏障
    ///
    /// 确保之前的所有内存访问完成
    fn data_sync_barrier();

    /// 指令同步屏障
    ///
    /// 确保之前的所有指令执行完成
    fn instruction_sync_barrier();

    /// 数据内存屏障
    ///
    /// 确保内存访问的顺序性
    fn data_memory_barrier();
}

/// 架构信息 trait
///
/// 提供架构相关的信息查询
pub trait ArchInfo {
    /// 获取架构名称
    ///
    /// # 返回值
    /// 架构名称字符串，如 "Cortex-M3", "RISC-V" 等
    fn arch_name() -> &'static str;

    /// 获取字长（位数）
    ///
    /// # 返回值
    /// 处理器字长，如 32 或 64
    fn word_size() -> usize;

    /// 获取栈对齐要求
    ///
    /// # 返回值
    /// 栈对齐字节数，通常为 4 或 8
    fn stack_alignment() -> usize;
}

/// 平台/SoC 抽象 trait
///
/// 定义了统一的平台生命周期和硬件资源管理接口。
/// 具体开发板或芯片需要实现此 trait 来提供平台特定的初始化。
pub trait Platform {
    /// 平台特定的错误类型
    type Error;

    /// 获取平台名称
    fn platform_name() -> &'static str;

    /// 平台早期初始化
    ///
    /// 在内核和调度器启动前调用，用于初始化核心时钟、内存控制器等基础硬件。
    fn early_init() -> Result<(), Self::Error>;

    /// 平台设备初始化
    ///
    /// 在调度器初始化后调用，用于注册和初始化板载外设驱动（如 UART, SPI, 定时器）。
    fn device_init() -> Result<(), Self::Error>;
}

