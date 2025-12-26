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

