//! Cortex-M3 完整示例
//!
//! 展示 Neon-RTOS2 的主要功能：
//! - **用户配置日志输出**（可选半主机或 UART）
//! - Builder 模式创建任务
//! - 任务优先级设置
//! - **V2 同步原语**（支持闭包传递，无需全局变量）
//!   - SignalV2: 信号量
//!   - MutexV2: 互斥锁（RAII 风格）
//!   - SemaphoreV2: 计数信号量
//! - 定时器
//! - 任务迭代器
//! - 错误处理最佳实践
//!
//! # V2 同步原语的优势
//!
//! 传统方式需要使用全局变量或宏：
//! ```rust,ignore
//! define_signal!(MY_SIGNAL);  // 必须是全局的
//! ```
//!
//! V2 版本可以在局部创建并通过闭包传递：
//! ```rust,ignore
//! let signal = SignalV2::new();  // 局部创建
//! let signal_clone = signal.clone();
//! Task::builder("task").spawn(move |_| {
//!     signal_clone.wait().unwrap();  // 通过闭包捕获
//! });
//! ```
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
// 导入 V2 同步原语（支持闭包传递）
use neon_rtos2::sync::{SignalV2, MutexV2, SemaphoreV2, signal_pair};
use neon_rtos2::{debug, error, info, trace};

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
// 主函数
// ============================================================================

#[entry]
fn main() -> ! {
    // ========================================
    // 1. 配置日志输出（必须最先执行！）
    // ========================================
    set_log_output(&LOG_OUTPUT);
    set_log_level(LogLevel::Debug);
    
    // ========================================
    // 2. 内核初始化
    // ========================================
    kernel_init();
    
    info!("================================================");
    info!("    Neon-RTOS2 V2 Sync Primitives Example");
    info!("================================================");
    info!("");
    info!("This example demonstrates V2 sync primitives:");
    info!("  - SignalV2: closures can capture and pass");
    info!("  - MutexV2: RAII style mutex with guards");
    info!("  - SemaphoreV2: counting semaphore");
    info!("  - signal_pair(): sender/receiver pattern");
    info!("");
    
    // ========================================
    // 3. 初始化 SysTick
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
    // 4. 创建 V2 同步原语（局部变量，无需全局！）
    // ========================================
    info!("Creating V2 sync primitives (local, no globals!)...");
    
    // SignalV2: 生产者-消费者信号
    let producer_signal = SignalV2::new();
    let consumer_signal = producer_signal.clone();  // clone 共享同一个信号
    info!("  Created: SignalV2 for producer-consumer");
    
    // MutexV2: 保护共享计数器
    let counter_mutex = MutexV2::new(0u32);
    let counter_mutex_clone = counter_mutex.clone();
    info!("  Created: MutexV2<u32> for shared counter");
    
    // signal_pair(): 更清晰的发送/接收语义
    let (pair_sender, pair_receiver) = signal_pair();
    info!("  Created: signal_pair() for sender/receiver pattern");
    
    // SemaphoreV2: 限制并发访问（最多 2 个任务同时工作）
    let work_semaphore = SemaphoreV2::new(2);
    let work_sem_1 = work_semaphore.clone();
    let work_sem_2 = work_semaphore.clone();
    let work_sem_3 = work_semaphore.clone();
    info!("  Created: SemaphoreV2(2) for resource pool");
    
    info!("");
    
    // ========================================
    // 5. 创建任务 - 使用闭包捕获 V2 同步原语
    // ========================================
    info!("Creating tasks with V2 sync primitives...");
    
    // ----- 生产者任务 -----
    // 使用 SignalV2 和 MutexV2，通过 move 闭包捕获
    let producer_counter = counter_mutex.clone();
    Task::builder("producer")
        .priority(Priority::High)
        .spawn(move |_| {
            info!("[Producer] Started with SignalV2 + MutexV2");
            
            loop {
                // 使用 MutexV2 保护共享数据
                {
                    let mut guard = producer_counter.lock().unwrap();
                    *guard += 1;
                    info!("[Producer] Produced item #{}", *guard);
                } // guard 离开作用域，自动释放锁
                
                // 发送信号通知消费者
                producer_signal.send();
                
                Delay::delay(500).unwrap();
            }
        })
        .expect("Failed to create producer task");
    info!("  Created: producer (High Priority) - uses SignalV2 + MutexV2");
    
    // ----- 消费者任务 -----
    Task::builder("consumer")
        .priority(Priority::Normal)
        .spawn(move |_| {
            info!("[Consumer] Started, waiting for SignalV2...");
            
            loop {
                // 等待生产者信号
                if consumer_signal.wait().is_ok() {
                    // 读取共享数据
                    let value = {
                        let guard = counter_mutex_clone.lock().unwrap();
                        *guard
                    };
                    info!("[Consumer] Consumed item #{}", value);
                }
            }
        })
        .expect("Failed to create consumer task");
    info!("  Created: consumer (Normal Priority) - waits on SignalV2");
    
    // ----- signal_pair 发送者任务 -----
    Task::builder("pair_tx")
        .priority(Priority::Normal)
        .spawn(move |_| {
            info!("[PairTx] Started with SignalSender");
            let mut count = 0u32;
            
            loop {
                count += 1;
                debug!("[PairTx] Sending signal #{}", count);
                pair_sender.send();
                Delay::delay(1000).unwrap();
            }
        })
        .expect("Failed to create pair_tx task");
    info!("  Created: pair_tx (Normal Priority) - uses SignalSender");
    
    // ----- signal_pair 接收者任务 -----
    Task::builder("pair_rx")
        .priority(Priority::Normal)
        .spawn(move |_| {
            info!("[PairRx] Started with SignalReceiver");
            let mut count = 0u32;
            
            loop {
                if pair_receiver.wait().is_ok() {
                    count += 1;
                    info!("[PairRx] Received signal #{}", count);
                }
            }
        })
        .expect("Failed to create pair_rx task");
    info!("  Created: pair_rx (Normal Priority) - uses SignalReceiver");
    
    // ----- SemaphoreV2 工作者任务 1 -----
    Task::builder("worker1")
        .priority(Priority::Low)
        .spawn(move |_| {
            info!("[Worker1] Started, using SemaphoreV2");
            
            loop {
                debug!("[Worker1] Waiting for permit...");
                if work_sem_1.acquire().is_ok() {
                    info!("[Worker1] Got permit, working...");
                    Delay::delay(800).unwrap();
                    info!("[Worker1] Done, releasing permit");
                    let _ = work_sem_1.release();
                }
                Delay::delay(200).unwrap();
            }
        })
        .expect("Failed to create worker1 task");
    info!("  Created: worker1 (Low Priority) - uses SemaphoreV2");
    
    // ----- SemaphoreV2 工作者任务 2 -----
    Task::builder("worker2")
        .priority(Priority::Low)
        .spawn(move |_| {
            info!("[Worker2] Started, using SemaphoreV2");
            
            loop {
                debug!("[Worker2] Waiting for permit...");
                if work_sem_2.acquire().is_ok() {
                    info!("[Worker2] Got permit, working...");
                    Delay::delay(600).unwrap();
                    info!("[Worker2] Done, releasing permit");
                    let _ = work_sem_2.release();
                }
                Delay::delay(300).unwrap();
            }
        })
        .expect("Failed to create worker2 task");
    info!("  Created: worker2 (Low Priority) - uses SemaphoreV2");
    
    // ----- SemaphoreV2 工作者任务 3 -----
    Task::builder("worker3")
        .priority(Priority::Low)
        .spawn(move |_| {
            info!("[Worker3] Started, using SemaphoreV2");
            
            loop {
                debug!("[Worker3] Waiting for permit...");
                // 使用 try_acquire 非阻塞尝试
                match work_sem_3.try_acquire() {
                    Ok(true) => {
                        info!("[Worker3] Got permit (try), working...");
                        Delay::delay(500).unwrap();
                        info!("[Worker3] Done, releasing permit");
                        let _ = work_sem_3.release();
                    }
                    Ok(false) => {
                        debug!("[Worker3] No permit available, will retry");
                    }
                    Err(_) => {
                        debug!("[Worker3] Semaphore error");
                    }
                }
                Delay::delay(400).unwrap();
            }
        })
        .expect("Failed to create worker3 task");
    info!("  Created: worker3 (Low Priority) - uses SemaphoreV2::try_acquire");
    
    // ----- 监控任务 -----
    Task::builder("monitor")
        .priority(Priority::Low)
        .spawn(|_| {
            info!("[Monitor] Started");
            let mut tick = 0u32;
            
            loop {
                tick += 1;
                
                info!("========== System Monitor (tick {}) ==========", tick);
                
                let total = Task::iter().count();
                let ready = Task::ready_tasks().count();
                let blocked = Task::blocked_tasks().count();
                
                info!("Tasks: total={}, ready={}, blocked={}", total, ready, blocked);
                
                Task::iter().for_each(|task| {
                    let state = match task.get_state() {
                        TaskState::Uninit => "Uninit",
                        TaskState::Ready => "Ready",
                        TaskState::Running => "Running",
                        TaskState::Blocked(_) => "Blocked",
                    };
                    debug!("  {} [{}]", task.get_name(), state);
                });
                
                info!("================================================");
                
                Delay::delay(5000).unwrap();
            }
        })
        .expect("Failed to create monitor task");
    info!("  Created: monitor (Low Priority)");
    
    // ----- 心跳任务 -----
    Task::builder("heartbeat")
        .priority(Priority::Idle)
        .spawn(|_| {
            info!("[Heartbeat] Started");
            let mut beat = 0u32;
            
            loop {
                beat += 1;
                trace!("Heartbeat: {}", beat);
                Delay::delay(1000).unwrap();
            }
        })
        .expect("Failed to create heartbeat task");
    info!("  Created: heartbeat (Idle Priority)");
    
    info!("");
    info!("All {} tasks created successfully!", Task::iter().count());
    info!("");
    
    // ========================================
    // 6. 启动调度器
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
neon_rtos2::default_panic_handler!();
