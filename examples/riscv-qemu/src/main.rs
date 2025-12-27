//! RISC-V QEMU 示例
//!
//! 展示 Neon-RTOS2 在 RISC-V 平台上的使用。
//!
//! # 功能演示
//!
//! - 内核初始化
//! - Builder 模式创建任务
//! - 任务优先级设置
//! - 信号量同步
//! - 任务迭代器
//! - 延时功能
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
use neon_rtos2::{info, debug, warn, error, define_signal};

// ============================================================================
// 信号量定义
// ============================================================================

/// 任务同步信号量
define_signal!(TASK_SYNC);

/// 数据就绪信号量
define_signal!(DATA_READY);

// ============================================================================
// UART 输出（QEMU virt 平台）
// ============================================================================

/// QEMU virt 平台 UART 基地址
const UART_BASE: usize = 0x1000_0000;

/// 通过 UART 输出单个字符
fn uart_putc(c: u8) {
    unsafe {
        core::ptr::write_volatile(UART_BASE as *mut u8, c);
    }
}

/// 输出字符串
fn print(s: &str) {
    for byte in s.bytes() {
        uart_putc(byte);
    }
}

/// 输出带换行的字符串
fn println(s: &str) {
    print(s);
    uart_putc(b'\n');
}

/// 输出数字
fn print_num(mut n: u32) {
    if n == 0 {
        uart_putc(b'0');
        return;
    }
    
    let mut buf = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        uart_putc(buf[i]);
    }
}

// ============================================================================
// 任务函数
// ============================================================================

/// 传感器任务 - 高优先级
/// 
/// 模拟传感器数据采集，周期性发送数据就绪信号
fn sensor_task(_: usize) {
    println("[Sensor] Task started (High Priority)");
    let mut reading = 0u32;
    
    loop {
        // 模拟传感器读取
        reading = reading.wrapping_add(1);
        
        print("[Sensor] Reading #");
        print_num(reading);
        println(" - sending DATA_READY signal");
        
        // 发送数据就绪信号
        DATA_READY.signal();
        
        // 延时 2 秒
        Delay::delay(2000).unwrap();
    }
}

/// 处理器任务 - 普通优先级
/// 
/// 等待传感器数据，处理后发送同步信号
fn processor_task(_: usize) {
    println("[Processor] Task started (Normal Priority)");
    let mut processed = 0u32;
    
    loop {
        // 等待数据就绪
        print("[Processor] Waiting for data...");
        DATA_READY.wait();
        
        processed = processed.wrapping_add(1);
        print(" Got it! Processed count: ");
        print_num(processed);
        println("");
        
        // 处理完成，发送同步信号
        TASK_SYNC.signal();
    }
}

/// 日志任务 - 普通优先级
/// 
/// 等待处理完成信号，记录日志
fn logger_task(_: usize) {
    println("[Logger] Task started (Normal Priority)");
    let mut log_count = 0u32;
    
    loop {
        // 等待同步信号
        TASK_SYNC.wait();
        
        log_count = log_count.wrapping_add(1);
        print("[Logger] Log entry #");
        print_num(log_count);
        println(" - Data processing completed");
    }
}

/// 监控任务 - 低优先级
/// 
/// 周期性监控系统状态，展示任务迭代器的使用
fn monitor_task(_: usize) {
    println("[Monitor] Task started (Low Priority)");
    let mut tick = 0u32;
    
    loop {
        tick = tick.wrapping_add(1);
        
        println("");
        print("========== System Monitor (tick ");
        print_num(tick);
        println(") ==========");
        
        // 使用迭代器统计任务状态
        let total = Task::iter().count();
        let ready = Task::ready_tasks().count();
        let blocked = Task::blocked_tasks().count();
        
        print("Total tasks: ");
        print_num(total as u32);
        println("");
        
        print("Ready tasks: ");
        print_num(ready as u32);
        println("");
        
        print("Blocked tasks: ");
        print_num(blocked as u32);
        println("");
        
        // 遍历所有任务并显示状态
        println("Task list:");
        Task::iter().for_each(|task| {
            print("  - ");
            print(task.get_name());
            print(" (ID: ");
            print_num(task.get_taskid() as u32);
            print(", State: ");
            match task.get_state() {
                TaskState::Uninit => print("Uninit"),
                TaskState::Ready => print("Ready"),
                TaskState::Running => print("Running"),
                TaskState::Blocked(_) => print("Blocked"),
            }
            println(")");
        });
        
        println("==========================================");
        println("");
        
        // 延时 5 秒
        Delay::delay(5000).unwrap();
    }
}

/// 心跳任务 - 最低优先级
/// 
/// 简单的心跳指示，证明系统在运行
fn heartbeat_task(_: usize) {
    println("[Heartbeat] Task started (Idle Priority)");
    let mut beat = 0u32;
    
    loop {
        beat = beat.wrapping_add(1);
        print("[Heartbeat] ");
        print_num(beat);
        println("");
        
        // 延时 3 秒
        Delay::delay(3000).unwrap();
    }
}

// ============================================================================
// 主函数
// ============================================================================

#[entry]
fn main() -> ! {
    // 打印欢迎信息
    println("");
    println("================================================");
    println("       Neon-RTOS2 RISC-V QEMU Example");
    println("================================================");
    println("");
    println("This example demonstrates:");
    println("  - Kernel initialization");
    println("  - Task creation with Builder pattern");
    println("  - Task priorities");
    println("  - Signal synchronization");
    println("  - Task iterators");
    println("  - Delay functionality");
    println("");
    
    // 初始化内核
    println("[Main] Initializing kernel...");
    kernel_init();
    println("[Main] Kernel initialized successfully!");
    println("");
    
    // 创建任务 - 使用 Builder 模式（推荐方式）
    println("[Main] Creating tasks with Builder pattern...");
    
    // 高优先级传感器任务
    Task::builder("sensor")
        .priority(Priority::High)
        .spawn(sensor_task)
        .expect("Failed to create sensor task");
    println("[Main] Created: sensor (High Priority)");
    
    // 普通优先级处理器任务
    Task::builder("processor")
        .priority(Priority::Normal)
        .spawn(processor_task)
        .expect("Failed to create processor task");
    println("[Main] Created: processor (Normal Priority)");
    
    // 普通优先级日志任务
    Task::builder("logger")
        .priority(Priority::Normal)
        .spawn(logger_task)
        .expect("Failed to create logger task");
    println("[Main] Created: logger (Normal Priority)");
    
    // 低优先级监控任务
    Task::builder("monitor")
        .priority(Priority::Low)
        .spawn(monitor_task)
        .expect("Failed to create monitor task");
    println("[Main] Created: monitor (Low Priority)");
    
    // 空闲优先级心跳任务
    Task::builder("heartbeat")
        .priority(Priority::Idle)
        .spawn(heartbeat_task)
        .expect("Failed to create heartbeat task");
    println("[Main] Created: heartbeat (Idle Priority)");
    
    println("");
    println("[Main] All tasks created successfully!");
    println("[Main] Starting scheduler...");
    println("");
    println("================================================");
    println("");
    
    // 启动调度器
    Scheduler::start();
    
    // 不应该到达这里
    println("[Main] ERROR: Scheduler returned unexpectedly!");
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

// ============================================================================
// Panic 处理
// ============================================================================

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println("");
    println("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    println("                   PANIC!");
    println("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    
    if let Some(location) = info.location() {
        print("Location: ");
        println(location.file());
        print("Line: ");
        print_num(location.line());
        println("");
    }
    
    if let Some(message) = info.message().as_str() {
        print("Message: ");
        println(message);
    }
    
    println("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
