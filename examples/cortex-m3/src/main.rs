//! Cortex-M3 完整示例
//!
//! 展示 Neon-RTOS2 的主要功能：
//! - **用户配置日志输出**（可选半主机或 UART）
//! - Builder 模式创建任务
//! - 任务优先级设置
//! - 互斥锁（RAII 风格）
//! - 信号量同步
//! - 消息队列
//! - 定时器
//! - 任务迭代器
//! - 错误处理最佳实践
//!
//! # 硬件要求
//!
//! - Cortex-M3 处理器
//! - 12MHz 系统时钟（可根据实际硬件调整）
//!
//! # 运行方式
//!
//! ```bash
//! # 使用 QEMU 运行（半主机模式）
//! cargo run --release
//!
//! # 或者烧录到实际硬件（建议使用 UART 模式）
//! cargo flash --release --chip STM32F103C8
//! ```

#![no_std]
#![no_main]


use cortex_m::Peripherals;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::entry;

// 导入 RTOS prelude - 包含所有常用类型
use neon_rtos2::prelude::*;
use neon_rtos2::{debug, error, info, warn, trace, define_signal};

// ============================================================================
// 用户配置：日志输出方式
// ============================================================================

/// 日志输出配置
/// 
/// 根据你的使用场景选择合适的输出方式：
/// 
/// ## 选项 1: 半主机输出（调试用，需要连接调试器）
/// ```rust
/// static LOG_OUTPUT: SemihostOutput = SemihostOutput;
/// ```
/// 
/// ## 选项 2: UART 输出（生产环境推荐）
/// ```rust
/// const UART_BASE: usize = 0x4001_3804; // STM32F1 USART1 DR 寄存器
/// static LOG_OUTPUT: UartOutput<UART_BASE> = UartOutput::new();
/// ```
/// 
/// ## 选项 3: 禁用日志输出
/// ```rust
/// static LOG_OUTPUT: NullOutput = NullOutput;
/// ```

// 当前使用半主机输出（适合 QEMU 调试）
// 如果要烧录到实际硬件，请改用 UART 输出
static LOG_OUTPUT: SemihostOutput = SemihostOutput;

// ============================================================================
// 系统配置
// ============================================================================

/// 系统时钟频率 (Hz)
const SYS_CLOCK: u32 = 12_000_000;

/// SysTick 频率 (Hz) - 1000Hz = 1ms tick
const SYST_FREQ: u32 = 1000;

/// SysTick 重载值
const SYST_RELOAD: u32 = SYS_CLOCK / SYST_FREQ;

// ============================================================================
// 信号量定义
// ============================================================================

/// 传感器数据就绪信号
define_signal!(SENSOR_DATA_READY);

/// 处理完成信号
define_signal!(PROCESSING_DONE);

/// 日志同步信号
define_signal!(LOG_SYNC);

// ============================================================================
// 共享数据（使用互斥锁保护）
// ============================================================================

/// 模拟的传感器数据
static mut SENSOR_VALUE: u32 = 0;

/// 处理计数器
static mut PROCESS_COUNT: u32 = 0;

// ============================================================================
// 任务函数
// ============================================================================

/// 传感器任务 - 高优先级
///
/// 模拟传感器数据采集，展示：
/// - 高优先级任务
/// - 信号量发送
/// - 延时功能
fn sensor_task(_: usize) {
    info!("Sensor task started (High Priority)");
    
    let mut reading = 0u32;
    
    loop {
        // 模拟传感器读取
        reading = reading.wrapping_add(7); // 模拟变化的传感器值
        
        // 更新共享数据（实际应用中应使用互斥锁）
        unsafe {
            SENSOR_VALUE = reading;
        }
        
        debug!("Sensor: new reading = {}", reading);
        
        // 发送数据就绪信号
        SENSOR_DATA_READY().send();
        
        // 采样间隔 500ms
        Delay::delay(500).unwrap();
    }
}

/// 数据处理任务 - 普通优先级
///
/// 等待传感器数据并处理，展示：
/// - 信号量等待
/// - 信号量发送（链式通知）
fn processor_task(_: usize) {
    info!("Processor task started (Normal Priority)");
    
    loop {
        // 等待传感器数据就绪
        debug!("Processor: waiting for sensor data...");
        SENSOR_DATA_READY().wait();
        
        // 读取并处理数据
        let value = unsafe { SENSOR_VALUE };
        let processed = value * 2 + 100; // 简单的处理逻辑
        
        unsafe {
            PROCESS_COUNT = PROCESS_COUNT.wrapping_add(1);
        }
        
        info!("Processor: processed value {} -> {}", value, processed);
        
        // 通知处理完成
        PROCESSING_DONE().send();
    }
}

/// 日志任务 - 普通优先级
///
/// 记录处理结果，展示：
/// - 多个任务间的信号量同步
fn logger_task(_: usize) {
    info!("Logger task started (Normal Priority)");
    
    let mut log_count = 0u32;
    
    loop {
        // 等待处理完成
        PROCESSING_DONE().wait();
        
        log_count = log_count.wrapping_add(1);
        let count = unsafe { PROCESS_COUNT };
        
        info!("Logger: entry #{} - total processed: {}", log_count, count);
        
        // 发送日志同步信号（可用于其他任务）
        LOG_SYNC().send();
    }
}

/// 监控任务 - 低优先级
///
/// 系统状态监控，展示：
/// - 任务迭代器 (Task::iter())
/// - 就绪任务迭代器 (Task::ready_tasks())
/// - 阻塞任务迭代器 (Task::blocked_tasks())
fn monitor_task(_: usize) {
    info!("Monitor task started (Low Priority)");
    
    let mut tick = 0u32;
    
    loop {
        tick = tick.wrapping_add(1);
        
        info!("========== System Monitor (tick {}) ==========", tick);
        
        // 使用迭代器统计任务状态
        let total_tasks = Task::iter().count();
        let ready_count = Task::ready_tasks().count();
        let blocked_count = Task::blocked_tasks().count();
        
        info!("Task Statistics:");
        info!("  Total tasks:   {}", total_tasks);
        info!("  Ready tasks:   {}", ready_count);
        info!("  Blocked tasks: {}", blocked_count);
        
        // 遍历所有任务并显示详细信息
        info!("Task Details:");
        Task::iter().for_each(|task| {
            let state_str = match task.get_state() {
                TaskState::Uninit => "Uninit",
                TaskState::Ready => "Ready",
                TaskState::Running => "Running",
                TaskState::Blocked(_) => "Blocked",
            };
            let priority_str = match task.get_priority() {
                Priority::Idle => "Idle",
                Priority::Low => "Low",
                Priority::Normal => "Normal",
                Priority::High => "High",
                Priority::Critical => "Critical",
            };
            debug!("  - {} [ID:{}, Pri:{}, State:{}]", 
                   task.get_name(), 
                   task.get_taskid(),
                   priority_str,
                   state_str);
        });
        
        // 显示处理统计
        let process_count = unsafe { PROCESS_COUNT };
        info!("Processing Statistics:");
        info!("  Total processed: {}", process_count);
        
        info!("================================================");
        
        // 监控间隔 5 秒
        Delay::delay(5000).unwrap();
    }
}

/// 互斥锁演示任务 - 普通优先级
///
/// 展示互斥锁的 RAII 用法：
/// - lock_guard() 方法
/// - 自动释放锁
fn mutex_demo_task(_: usize) {
    info!("Mutex demo task started (Normal Priority)");
    
    // 创建互斥锁
    let mutex = match Mutex::new() {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to create mutex: {:?}", e);
            loop { Delay::delay(1000).unwrap(); }
        }
    };
    
    let mut counter = 0u32;
    
    loop {
        counter = counter.wrapping_add(1);
        
        // 方式1：使用 RAII 守卫（推荐）
        {
            let _guard = mutex.lock_guard();
            // 临界区开始
            debug!("Mutex demo #{}: in critical section (RAII)", counter);
            // 模拟临界区操作
            for _ in 0..100 {
                core::hint::spin_loop();
            }
            // 临界区结束 - 离开作用域时自动释放锁
        }
        
        // 方式2：使用闭包风格
        mutex.with_lock(|| {
            debug!("Mutex demo #{}: in critical section (closure)", counter);
            // 临界区操作
        });
        
        // 演示间隔 2 秒
        Delay::delay(2000).unwrap();
    }
}

/// 定时器演示任务 - 低优先级
///
/// 展示软件定时器的使用：
/// - Timer::new() 创建定时器
/// - timer.start() 启动
/// - timer.is_timeout() 检查超时
fn timer_demo_task(_: usize) {
    info!("Timer demo task started (Low Priority)");
    
    // 创建 3 秒定时器
    let mut timer = Timer::new(3000).expect("Failed to create timer");
    timer.start().unwrap();
    
    let mut timeout_count = 0u32;
    
    loop {
        if timer.is_timeout() {
            timeout_count = timeout_count.wrapping_add(1);
            info!("Timer demo: timeout #{} (3 seconds elapsed)", timeout_count);
            
            // 重启定时器
            timer.start().unwrap();
        }
        
        // 检查间隔 100ms
        Delay::delay(100).unwrap();
    }
}

/// 心跳任务 - 空闲优先级
///
/// 简单的心跳指示，证明系统在运行
fn heartbeat_task(_: usize) {
    info!("Heartbeat task started (Idle Priority)");
    
    let mut beat = 0u32;
    
    loop {
        beat = beat.wrapping_add(1);
        trace!("Heartbeat: {}", beat);
        
        // 心跳间隔 1 秒
        Delay::delay(1000).unwrap();
    }
}

// ============================================================================
// 主函数
// ============================================================================

#[entry]
fn main() -> ! {
    // ========================================
    // 1. 配置日志输出（必须最先执行！）
    // ========================================
    // 用户在这里指定日志输出方式
    // 可以是半主机、UART、或其他自定义实现
    set_log_output(&LOG_OUTPUT);
    
    // 设置日志级别
    set_log_level(LogLevel::Debug);
    
    // ========================================
    // 2. 内核初始化
    // ========================================
    kernel_init();
    
    info!("================================================");
    info!("    Neon-RTOS2 Cortex-M3 Complete Example");
    info!("================================================");
    info!("");
    info!("This example demonstrates:");
    info!("  - Task creation with Builder pattern");
    info!("  - Task priorities (Idle/Low/Normal/High)");
    info!("  - Signal synchronization");
    info!("  - Mutex with RAII guards");
    info!("  - Software timers");
    info!("  - Task iterators");
    info!("  - Error handling");
    info!("");
    
    // ========================================
    // 2. 初始化 SysTick
    // ========================================
    info!("Initializing SysTick...");
    let p = Peripherals::take().unwrap();
    let mut syst = p.SYST;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(SYST_RELOAD);
    syst.enable_counter();
    syst.enable_interrupt();
    info!("SysTick initialized: {} Hz", SYST_FREQ);
    info!("");
    
    // ========================================
    // 3. 创建任务 - 使用 Builder 模式
    // ========================================
    info!("Creating tasks with Builder pattern...");
    
    // 高优先级：传感器任务
    Task::builder("sensor")
        .priority(Priority::High)
        .spawn(sensor_task)
        .expect("Failed to create sensor task");
    info!("  Created: sensor (High Priority)");
    
    // 普通优先级：处理器任务
    Task::builder("processor")
        .priority(Priority::Normal)
        .spawn(processor_task)
        .expect("Failed to create processor task");
    info!("  Created: processor (Normal Priority)");
    
    // 普通优先级：日志任务
    Task::builder("logger")
        .priority(Priority::Normal)
        .spawn(logger_task)
        .expect("Failed to create logger task");
    info!("  Created: logger (Normal Priority)");
    
    // 低优先级：监控任务
    Task::builder("monitor")
        .priority(Priority::Low)
        .spawn(monitor_task)
        .expect("Failed to create monitor task");
    info!("  Created: monitor (Low Priority)");
    
    // 普通优先级：互斥锁演示任务
    Task::builder("mutex_demo")
        .priority(Priority::Normal)
        .spawn(mutex_demo_task)
        .expect("Failed to create mutex demo task");
    info!("  Created: mutex_demo (Normal Priority)");
    
    // 低优先级：定时器演示任务
    Task::builder("timer_demo")
        .priority(Priority::Low)
        .spawn(timer_demo_task)
        .expect("Failed to create timer demo task");
    info!("  Created: timer_demo (Low Priority)");
    
    // 空闲优先级：心跳任务
    Task::builder("heartbeat")
        .priority(Priority::Idle)
        .spawn(heartbeat_task)
        .expect("Failed to create heartbeat task");
    info!("  Created: heartbeat (Idle Priority)");
    
    info!("");
    info!("All {} tasks created successfully!", Task::iter().count());
    info!("");
    
    // ========================================
    // 4. 启动调度器
    // ========================================
    info!("Starting scheduler...");
    info!("================================================");
    info!("");
    
    Scheduler::start();
    
    // 不应该到达这里
    error!("Scheduler returned unexpectedly!");
    loop {}
}

// 使用库提供的默认 panic handler
// 如果需要自定义 panic 行为，可以删除此行并自己实现 #[panic_handler]
neon_rtos2::default_panic_handler!();
