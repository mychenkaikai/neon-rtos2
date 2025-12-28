//! RISC-V QEMU 示例
//!
//! 展示 Neon-RTOS2 在 RISC-V 平台上的使用。
//!
//! # 功能演示
//!
//! - **用户配置日志输出**（UART 地址由用户指定）
//! - 内核初始化
//! - Builder 模式创建任务
//! - 任务优先级设置
//! - **同步原语**（支持闭包传递，无需全局变量）
//!   - Signal: 信号量
//!   - Mutex: 互斥锁（RAII 风格）
//! - 任务迭代器
//! - 延时功能
//! - 日志系统（使用 `info!`, `debug!` 等宏）
//!
//! # 同步原语的优势
//!
//! 基于 Arc 的设计，可以在局部创建并通过闭包传递：
//! ```rust,ignore
//! let signal = Signal::new();  // 局部创建
//! let signal_clone = signal.clone();
//! Task::builder("task").spawn(move |_| {
//!     signal_clone.wait().unwrap();  // 通过闭包捕获
//! });
//! ```
//!
//! # 运行方式
//!
//! ```bash
//! # 安装目标
//! rustup target add riscv32imac-unknown-none-elf
//!
//! # 编译
//! cargo build --release
//!
//! # 使用 QEMU 运行
//! qemu-system-riscv32 -machine virt -nographic \
//!     -bios none \
//!     -kernel target/riscv32imac-unknown-none-elf/release/riscv-qemu-example
//! ```

#![no_std]
#![no_main]

use riscv_rt::entry;
use core::panic::PanicInfo;

// 导入 RTOS 功能
use neon_rtos2::prelude::*;
// 导入同步原语（支持闭包传递）
use neon_rtos2::sync::{Signal, Mutex};
use neon_rtos2::{info, debug, error, trace};

// ============================================================================
// 用户配置：日志输出
// ============================================================================

/// QEMU RISC-V virt 平台的 UART 基地址
/// 
/// 不同的芯片/平台需要修改这个地址：
/// - QEMU virt: 0x1000_0000
/// - K210: 0x3800_0000
/// - 其他芯片请查阅数据手册
const UART_BASE_ADDR: usize = 0x1000_0000;

/// 使用 UartOutput 并指定地址
static UART_OUTPUT: UartOutput<UART_BASE_ADDR> = UartOutput::new();

// ============================================================================
// 主函数
// ============================================================================

#[entry]
fn main() -> ! {
    // ========================================
    // 1. 配置日志输出（必须最先执行！）
    // ========================================
    set_log_output(&UART_OUTPUT);
    set_log_level(LogLevel::Debug);
    
    // ========================================
    // 2. 初始化内核
    // ========================================
    kernel_init();
    
    // 打印欢迎信息
    info!("");
    info!("================================================");
    info!("       Neon-RTOS2 RISC-V QEMU Example");
    info!("================================================");
    info!("");
    info!("This example demonstrates:");
    info!("  - Kernel initialization");
    info!("  - Task creation with Builder pattern");
    info!("  - Task priorities");
    info!("  - Signal synchronization (Arc-based, no globals!)");
    info!("  - Mutex for shared data protection");
    info!("  - Task iterators");
    info!("  - Delay functionality");
    info!("  - Log system (info!, debug!, etc.)");
    info!("");
    
    info!("Kernel initialized successfully!");
    info!("");
    
    // ========================================
    // 3. 创建同步原语（局部变量，无需全局！）
    // ========================================
    info!("Creating sync primitives (local, no globals!)...");
    
    // Signal: 数据就绪信号
    let data_ready = Signal::new();
    info!("  Created: Signal for data ready notification");
    
    // Signal: 任务同步信号
    let task_sync = Signal::new();
    info!("  Created: Signal for task synchronization");
    
    // Mutex: 保护共享计数器
    let counter = Mutex::new(0u32);
    info!("  Created: Mutex<u32> for shared counter");
    
    info!("");
    
    // ========================================
    // 4. 创建任务 - 使用闭包捕获同步原语
    // ========================================
    info!("Creating tasks with sync primitives...");
    
    // ----- 传感器任务 - 高优先级 -----
    let data_ready_sender = data_ready.clone();
    let counter_sensor = counter.clone();
    Task::builder("sensor")
        .priority(Priority::High)
        .spawn(move |_| {
            info!("[Sensor] Started (High Priority) with Signal + Mutex");
            
            loop {
                // 使用 Mutex 保护共享数据
                {
                    let mut guard = counter_sensor.lock().unwrap();
                    *guard += 1;
                    debug!("[Sensor] Reading #{} - sending data ready signal", *guard);
                } // guard 离开作用域，自动释放锁
                
                // 发送数据就绪信号
                data_ready_sender.send();
                
                // 延时 2 秒
                Delay::delay(2000).unwrap();
            }
        })
        .expect("Failed to create sensor task");
    info!("  Created: sensor (High Priority) - uses Signal + Mutex");
    
    // ----- 处理器任务 - 普通优先级 -----
    let data_ready_receiver = data_ready.clone();
    let task_sync_sender = task_sync.clone();
    let counter_processor = counter.clone();
    Task::builder("processor")
        .priority(Priority::Normal)
        .spawn(move |_| {
            info!("[Processor] Started (Normal Priority)");
            
            loop {
                // 等待数据就绪
                debug!("[Processor] Waiting for data...");
                if data_ready_receiver.wait().is_ok() {
                    // 读取共享数据
                    let value = {
                        let guard = counter_processor.lock().unwrap();
                        *guard
                    };
                    info!("[Processor] Got data! Processed reading #{}", value);
                    
                    // 处理完成，发送同步信号
                    task_sync_sender.send();
                }
            }
        })
        .expect("Failed to create processor task");
    info!("  Created: processor (Normal Priority) - waits on Signal");
    
    // ----- 日志任务 - 普通优先级 -----
    let task_sync_receiver = task_sync.clone();
    Task::builder("logger")
        .priority(Priority::Normal)
        .spawn(move |_| {
            info!("[Logger] Started (Normal Priority)");
            let mut log_count = 0u32;
            
            loop {
                // 等待同步信号
                if task_sync_receiver.wait().is_ok() {
                    log_count += 1;
                    info!("[Logger] Log entry #{} - Data processing completed", log_count);
                }
            }
        })
        .expect("Failed to create logger task");
    info!("  Created: logger (Normal Priority) - waits on Signal");
    
    // ----- 监控任务 - 低优先级 -----
    Task::builder("monitor")
        .priority(Priority::Low)
        .spawn(|_| {
            info!("[Monitor] Started (Low Priority)");
            let mut tick = 0u32;
            
            loop {
                tick += 1;
                
                info!("");
                info!("========== System Monitor (tick {}) ==========", tick);
                
                // 使用迭代器统计任务状态
                let total = Task::iter().count();
                let ready = Task::ready_tasks().count();
                let blocked = Task::blocked_tasks().count();
                
                info!("Tasks: total={}, ready={}, blocked={}", total, ready, blocked);
                
                // 遍历所有任务并显示状态
                Task::iter().for_each(|task| {
                    let state_str = match task.get_state() {
                        TaskState::Uninit => "Uninit",
                        TaskState::Ready => "Ready",
                        TaskState::Running => "Running",
                        TaskState::Blocked(_) => "Blocked",
                    };
                    debug!("  {} [{}]", task.get_name(), state_str);
                });
                
                info!("==========================================");
                info!("");
                
                // 延时 5 秒
                Delay::delay(5000).unwrap();
            }
        })
        .expect("Failed to create monitor task");
    info!("  Created: monitor (Low Priority)");
    
    // ----- 心跳任务 - 最低优先级 -----
    Task::builder("heartbeat")
        .priority(Priority::Idle)
        .spawn(|_| {
            info!("[Heartbeat] Started (Idle Priority)");
            let mut beat = 0u32;
            
            loop {
                beat += 1;
                trace!("Heartbeat: {}", beat);
                
                // 延时 3 秒
                Delay::delay(3000).unwrap();
            }
        })
        .expect("Failed to create heartbeat task");
    info!("  Created: heartbeat (Idle Priority)");
    
    info!("");
    info!("All {} tasks created successfully!", Task::iter().count());
    info!("Starting scheduler...");
    info!("");
    info!("================================================");
    info!("");
    
    // 启动调度器
    Scheduler::start();
    
    // 不应该到达这里
    error!("Scheduler returned unexpectedly!");
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

// ============================================================================
// RISC-V 异常处理函数 (riscv-rt 0.12 需要)
// ============================================================================

/// 默认中断处理函数
#[export_name = "DefaultHandler"]
fn default_handler() {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// 异常处理函数
#[export_name = "ExceptionHandler"]
fn exception_handler(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// 指令未对齐异常
#[export_name = "InstructionMisaligned"]
fn instruction_misaligned(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 指令访问错误
#[export_name = "InstructionFault"]
fn instruction_fault(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 非法指令
#[export_name = "IllegalInstruction"]
fn illegal_instruction(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 断点
#[export_name = "Breakpoint"]
fn breakpoint(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 加载未对齐
#[export_name = "LoadMisaligned"]
fn load_misaligned(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 加载访问错误
#[export_name = "LoadFault"]
fn load_fault(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 存储未对齐
#[export_name = "StoreMisaligned"]
fn store_misaligned(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 存储访问错误
#[export_name = "StoreFault"]
fn store_fault(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 用户态环境调用
#[export_name = "UserEnvCall"]
fn user_env_call(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 监管态环境调用
#[export_name = "SupervisorEnvCall"]
fn supervisor_env_call(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 机器态环境调用
#[export_name = "MachineEnvCall"]
fn machine_env_call(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 指令页错误
#[export_name = "InstructionPageFault"]
fn instruction_page_fault(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 加载页错误
#[export_name = "LoadPageFault"]
fn load_page_fault(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

/// 存储页错误
#[export_name = "StorePageFault"]
fn store_page_fault(_trap_frame: &riscv_rt::TrapFrame) -> ! {
    loop { unsafe { core::arch::asm!("wfi") }; }
}

// ============================================================================
// Panic 处理
// ============================================================================

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("");
    error!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    error!("                   PANIC!");
    error!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    
    if let Some(location) = info.location() {
        error!("Location: {}:{}", location.file(), location.line());
    }
    
    if let Some(message) = info.message().as_str() {
        error!("Message: {}", message);
    }
    
    error!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
