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
    fn init_task_stack(stack_top: &mut usize, entry: fn(usize), arg: usize);

    /// 触发上下文切换
    ///
    /// 通常通过触发 PendSV 中断来实现
    fn trigger_switch();

    /// 启动第一个任务
    ///
    /// 从调度器启动第一个任务，开始多任务执行
    fn start_first_task();
}

/// 系统时钟 trait
///
/// 定义了系统时钟（SysTick）的基本操作
pub trait SysTickTrait {
    /// 初始化系统时钟
    ///
    /// # 参数
    /// - `frequency`: 时钟频率（Hz）
    fn init(frequency: u32);

    /// 获取当前时间
    ///
    /// # 返回值
    /// 当前的 tick 计数
    fn get_current_time() -> usize;

    /// 增加当前时间
    ///
    /// 主要用于测试环境模拟时间流逝
    ///
    /// # 参数
    /// - `ticks`: 要增加的 tick 数
    fn add_current_time(ticks: usize);

    /// 时钟中断处理
    ///
    /// 在 SysTick 中断中调用，处理定时器和任务调度
    fn tick_handler();

    /// 暂停系统时钟（用于 Tickless Idle）
    ///
    /// # 参数
    /// - `ticks`: 预计暂停的 tick 数量
    fn suspend(ticks: usize);

    /// 恢复系统时钟
    ///
    /// # 返回值
    /// 实际经过的 tick 数量
    fn resume() -> usize;
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

/// 临界区 trait
///
/// 定义了进入和退出临界区的操作
pub trait CriticalSectionTrait {
    /// 临界区令牌类型
    ///
    /// 用于保存进入临界区前的状态
    type Token;

    /// 进入临界区
    ///
    /// 禁用中断并返回之前的中断状态
    ///
    /// # 返回值
    /// 临界区令牌，用于退出时恢复状态
    fn enter() -> Self::Token;

    /// 退出临界区
    ///
    /// 恢复进入临界区前的中断状态
    ///
    /// # 参数
    /// - `token`: 进入临界区时获得的令牌
    fn exit(token: Self::Token);
}

/// 中断控制 trait
///
/// 定义了中断的基本控制操作
pub trait InterruptControl {
    /// 全局禁用中断
    fn disable_interrupts();

    /// 全局启用中断
    fn enable_interrupts();

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

    /// 启用特定中断
    ///
    /// # 参数
    /// - `irq`: 中断号
    fn enable_irq(irq: u32);

    /// 禁用特定中断
    ///
    /// # 参数
    /// - `irq`: 中断号
    fn disable_irq(irq: u32);

    /// 注册中断处理函数
    ///
    /// # 参数
    /// - `irq`: 中断号
    /// - `handler`: 中断处理函数
    fn register_handler(irq: u32, handler: fn());

    /// 注销中断处理函数
    ///
    /// # 参数
    /// - `irq`: 中断号
    fn unregister_handler(irq: u32);
}

/// DMA 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    /// 内存到外设
    MemoryToPeripheral,
    /// 外设到内存
    PeripheralToMemory,
    /// 内存到内存
    MemoryToMemory,
    /// 外设到外设
    PeripheralToPeripheral,
}

/// DMA 传输配置
#[derive(Debug, Clone)]
pub struct DmaConfig {
    /// 源地址
    pub src_addr: usize,
    /// 目标地址
    pub dst_addr: usize,
    /// 传输数据长度
    pub length: usize,
    /// 传输方向
    pub direction: DmaDirection,
}

/// DMA 控制器 trait
///
/// 抽象平台底层的直接内存访问（DMA）引擎
pub trait DmaController {
    /// DMA 错误类型
    type Error;

    /// 启动异步 DMA 传输
    ///
    /// # 参数
    /// - `channel`: DMA 通道号
    /// - `config`: 传输配置
    async fn start_transfer_async(&mut self, channel: u8, config: DmaConfig) -> Result<(), Self::Error>;

    /// 停止 DMA 传输
    fn stop_transfer(&mut self, channel: u8) -> Result<(), Self::Error>;

    /// 获取 DMA 通道传输剩余长度
    fn remaining_length(&self, channel: u8) -> usize;
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

