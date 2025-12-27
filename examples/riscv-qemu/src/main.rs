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
//! - 信号量同步
//! - 任务迭代器
//! - 延时功能
//! - 日志系统（使用 `info!`, `debug!` 等宏）
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
use neon_rtos2::{info, debug, warn, error, trace, define_signal};

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
// 信号量定义
// ============================================================================

/// 任务同步信号量
define_signal!(TASK_SYNC);

/// 数据就绪信号量
define_signal!(DATA_READY);

// ============================================================================
// 任务函数
// ============================================================================

/// 传感器任务 - 高优先级
/// 
/// 模拟传感器数据采集，周期性发送数据就绪信号
fn sensor_task(_: usize) {
    info!("Sensor task started (High Priority)");
    let mut reading = 0u32;
    
    loop {
        // 模拟传感器读取
        reading = reading.wrapping_add(1);
        
        debug!("Sensor: Reading #{} - sending DATA_READY signal", reading);
        
        // 发送数据就绪信号
        DATA_READY().send();
        
        // 延时 2 秒
        Delay::delay(2000).unwrap();
    }
}

/// 处理器任务 - 普通优先级
/// 
/// 等待传感器数据，处理后发送同步信号
fn processor_task(_: usize) {
    info!("Processor task started (Normal Priority)");
    let mut processed = 0u32;
    
    loop {
        // 等待数据就绪
        debug!("Processor: Waiting for data...");
        DATA_READY().wait();
        
        processed = processed.wrapping_add(1);
        info!("Processor: Got data! Processed count: {}", processed);
        
        // 处理完成，发送同步信号
        TASK_SYNC().send();
    }
}

/// 日志任务 - 普通优先级
/// 
/// 等待处理完成信号，记录日志
fn logger_task(_: usize) {
    info!("Logger task started (Normal Priority)");
    let mut log_count = 0u32;
    
    loop {
        // 等待同步信号
        TASK_SYNC().wait();
        
        log_count = log_count.wrapping_add(1);
        info!("Logger: Log entry #{} - Data processing completed", log_count);
    }
}

/// 监控任务 - 低优先级
/// 
/// 周期性监控系统状态，展示任务迭代器的使用
fn monitor_task(_: usize) {
    info!("Monitor task started (Low Priority)");
    let mut tick = 0u32;
    
    loop {
        tick = tick.wrapping_add(1);
        
        info!("");
        info!("========== System Monitor (tick {}) ==========", tick);
        
        // 使用迭代器统计任务状态
        let total = Task::iter().count();
        let ready = Task::ready_tasks().count();
        let blocked = Task::blocked_tasks().count();
        
        info!("Total tasks: {}", total);
        info!("Ready tasks: {}", ready);
        info!("Blocked tasks: {}", blocked);
        
        // 遍历所有任务并显示状态
        info!("Task list:");
        Task::iter().for_each(|task| {
            let state_str = match task.get_state() {
                TaskState::Uninit => "Uninit",
                TaskState::Ready => "Ready",
                TaskState::Running => "Running",
                TaskState::Blocked(_) => "Blocked",
            };
            debug!("  - {} (ID: {}, State: {})", 
                   task.get_name(), 
                   task.get_taskid(),
                   state_str);
        });
        
        info!("==========================================");
        info!("");
        
        // 延时 5 秒
        Delay::delay(5000).unwrap();
    }
}

/// 心跳任务 - 最低优先级
/// 
/// 简单的心跳指示，证明系统在运行
fn heartbeat_task(_: usize) {
    info!("Heartbeat task started (Idle Priority)");
    let mut beat = 0u32;
    
    loop {
        beat = beat.wrapping_add(1);
        trace!("Heartbeat: {}", beat);
        
        // 延时 3 秒
        Delay::delay(3000).unwrap();
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
    // 可以是 UART、RTT、或其他���定义实现
    set_log_output(&UART_OUTPUT);
    
    // 设置日志级别
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
    info!("  - Signal synchronization");
    info!("  - Task iterators");
    info!("  - Delay functionality");
    info!("  - Log system (info!, debug!, etc.)");
    info!("");
    
    info!("Kernel initialized successfully!");
    info!("");
    
    // 创建任务 - 使用 Builder 模式（推荐方式）
    info!("Creating tasks with Builder pattern...");
    
    // 高优先级传感器任务
    Task::builder("sensor")
        .priority(Priority::High)
        .spawn(sensor_task)
        .expect("Failed to create sensor task");
    info!("  Created: sensor (High Priority)");
    
    // 普通优先级处理器任务
    Task::builder("processor")
        .priority(Priority::Normal)
        .spawn(processor_task)
        .expect("Failed to create processor task");
    info!("  Created: processor (Normal Priority)");
    
    // 普通优先级日志任务
    Task::builder("logger")
        .priority(Priority::Normal)
        .spawn(logger_task)
        .expect("Failed to create logger task");
    info!("  Created: logger (Normal Priority)");
    
    // 低优先级监控任务
    Task::builder("monitor")
        .priority(Priority::Low)
        .spawn(monitor_task)
        .expect("Failed to create monitor task");
    info!("  Created: monitor (Low Priority)");
    
    // 空闲优先级心跳任务
    Task::builder("heartbeat")
        .priority(Priority::Idle)
        .spawn(heartbeat_task)
        .expect("Failed to create heartbeat task");
    info!("  Created: heartbeat (Idle Priority)");
    
    info!("");
    info!("All tasks created successfully!");
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
