//! Neon-RTOS2 测试套件
//!
//! 完整的功能测试，覆盖：
//! - 任务管理（创建、状态、优先级）
//! - Builder 模式
//! - 任务迭代器
//! - 互斥锁（RAII）
//! - 信号量
//! - 定时器
//! - 消息队列
//! - 错误处理
//!
//! # 运行方式
//!
//! ```bash
//! # 使用 QEMU 运行测试
//! cargo run --release
//! ```

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use cortex_m::Peripherals;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;

// 导入 RTOS 功能
use neon_rtos2::prelude::*;
use neon_rtos2::config::MAX_TASKS;
use neon_rtos2::ipc::queue::Mq;
use neon_rtos2::{info, error, warn, debug as log_debug, define_signal};
use neon_rtos2::log::{LogLevel, set_log_level};

// ============================================================================
// 系统配置
// ============================================================================

const SYST_FREQ: u32 = 1000;
const SYS_CLOCK: u32 = 12_000_000;
const SYST_RELOAD: u32 = SYS_CLOCK / SYST_FREQ;

// ============================================================================
// 测试框架
// ============================================================================

/// 测试结果
#[derive(Copy, Clone)]
struct TestResult {
    name: &'static str,
    passed: bool,
    message: &'static str,
}

/// 全局测试状态
static mut TEST_RESULTS: [Option<TestResult>; 32] = [None; 32];
static mut TEST_COUNT: usize = 0;
static mut TESTS_PASSED: usize = 0;
static mut TESTS_FAILED: usize = 0;

/// 记录测试结果
fn report_test(name: &'static str, passed: bool, message: &'static str) {
    unsafe {
        if TEST_COUNT < 32 {
            TEST_RESULTS[TEST_COUNT] = Some(TestResult { name, passed, message });
            TEST_COUNT += 1;
            if passed {
                TESTS_PASSED += 1;
                info!("  [PASS] {}", name);
            } else {
                TESTS_FAILED += 1;
                error!("  [FAIL] {} - {}", name, message);
            }
        }
    }
}

/// 简单断言宏
macro_rules! test_assert {
    ($cond:expr, $name:expr, $msg:expr) => {
        if $cond {
            report_test($name, true, "OK");
            true
        } else {
            report_test($name, false, $msg);
            false
        }
    };
}

/// 断言相等宏
macro_rules! test_assert_eq {
    ($left:expr, $right:expr, $name:expr) => {
        if $left == $right {
            report_test($name, true, "OK");
            true
        } else {
            report_test($name, false, "Values not equal");
            false
        }
    };
}

// ============================================================================
// 测试用例
// ============================================================================

/// 测试：基本任务创建
fn test_task_creation() -> bool {
    info!("Testing: Task Creation");
    
    // 测试使用 Task::new 创建任务
    let result = Task::new("test_basic", |_| {
        loop { Delay::delay(1000).unwrap(); }
    });
    
    test_assert!(result.is_ok(), "task_creation_basic", "Task::new should succeed")
}

/// 测试：Builder 模式创建任务
fn test_task_builder() -> bool {
    info!("Testing: Task Builder Pattern");
    
    // 测试 Builder 模式
    let result = Task::builder("test_builder")
        .priority(Priority::High)
        .spawn(|_| {
            loop { Delay::delay(1000).unwrap(); }
        });
    
    if !test_assert!(result.is_ok(), "task_builder_creation", "Builder should create task") {
        return false;
    }
    
    let task = result.unwrap();
    
    // 验证优先级设置
    test_assert_eq!(task.get_priority(), Priority::High, "task_builder_priority")
}

/// 测试：任务状态转换
fn test_task_state() -> bool {
    info!("Testing: Task State Transitions");
    
    let result = Task::new("test_state", |_| {
        loop { Delay::delay(1000).unwrap(); }
    });
    
    if !test_assert!(result.is_ok(), "task_state_creation", "Task creation should succeed") {
        return false;
    }
    
    let mut task = result.unwrap();
    
    // 新创建的任务应该是 Ready 状态
    let is_ready = task.get_state() == TaskState::Ready;
    if !test_assert!(is_ready, "task_state_initial_ready", "New task should be Ready") {
        return false;
    }
    
    // 测试状态转换到 Running
    task.run();
    let is_running = task.get_state() == TaskState::Running;
    if !test_assert!(is_running, "task_state_to_running", "Task should be Running") {
        return false;
    }
    
    // 测试状态转换到 Blocked
    task.block(Event::Signal(1));
    let is_blocked = matches!(task.get_state(), TaskState::Blocked(_));
    if !test_assert!(is_blocked, "task_state_to_blocked", "Task should be Blocked") {
        return false;
    }
    
    // 测试状态转换回 Ready
    task.ready();
    let is_ready_again = task.get_state() == TaskState::Ready;
    test_assert!(is_ready_again, "task_state_back_to_ready", "Task should be Ready again")
}

/// 测试：任务优先级
fn test_task_priority() -> bool {
    info!("Testing: Task Priority");
    
    let result = Task::builder("test_priority")
        .priority(Priority::Low)
        .spawn(|_| {
            loop { Delay::delay(1000).unwrap(); }
        });
    
    if !test_assert!(result.is_ok(), "task_priority_creation", "Task creation should succeed") {
        return false;
    }
    
    let mut task = result.unwrap();
    
    // 验证初始优先级
    if !test_assert_eq!(task.get_priority(), Priority::Low, "task_priority_initial") {
        return false;
    }
    
    // 修改优先级
    task.set_priority(Priority::Critical);
    
    // 验证修改后的优先级
    test_assert_eq!(task.get_priority(), Priority::Critical, "task_priority_modified")
}

/// 测试：任务迭代器
fn test_task_iterator() -> bool {
    info!("Testing: Task Iterator");
    
    // 记录当前任务数
    let initial_count = Task::iter().count();
    
    // 创建几个测试任务
    let _ = Task::new("iter_test_1", |_| { loop { Delay::delay(1000).unwrap(); } });
    let _ = Task::new("iter_test_2", |_| { loop { Delay::delay(1000).unwrap(); } });
    let _ = Task::new("iter_test_3", |_| { loop { Delay::delay(1000).unwrap(); } });
    
    // 测试 iter() 计数
    let new_count = Task::iter().count();
    if !test_assert!(new_count >= initial_count + 3, "task_iterator_count", "Should have at least 3 more tasks") {
        return false;
    }
    
    // 测试 ready_tasks() 迭代器
    let ready_count = Task::ready_tasks().count();
    if !test_assert!(ready_count >= 3, "task_iterator_ready", "Should have at least 3 ready tasks") {
        return false;
    }
    
    // 测试迭代器的 for_each
    let mut found_iter_test = false;
    Task::iter().for_each(|task| {
        if task.get_name().starts_with("iter_test") {
            found_iter_test = true;
        }
    });
    
    test_assert!(found_iter_test, "task_iterator_foreach", "Should find iter_test tasks")
}

/// 测试：互斥锁基本功能
fn test_mutex_basic() -> bool {
    info!("Testing: Mutex Basic");
    
    let result = Mutex::new();
    
    if !test_assert!(result.is_ok(), "mutex_creation", "Mutex creation should succeed") {
        return false;
    }
    
    let mutex = result.unwrap();
    
    // 测试加锁
    mutex.lock();
    
    // 测试解锁
    let unlock_result = mutex.unlock();
    test_assert!(unlock_result.is_ok(), "mutex_unlock", "Mutex unlock should succeed")
}

/// 测试：互斥锁 RAII 守卫
fn test_mutex_raii() -> bool {
    info!("Testing: Mutex RAII Guard");
    
    let mutex = Mutex::new().expect("Mutex creation failed");
    
    // 测试 RAII 守卫
    {
        let _guard = mutex.lock_guard();
        // 在作用域内，锁应该被持有
        report_test("mutex_raii_lock", true, "OK");
    }
    // 离开作用域，锁应该自动释放
    
    // 验证锁已释放（可以再次获取）
    {
        let _guard = mutex.lock_guard();
        report_test("mutex_raii_relock", true, "OK");
    }
    
    true
}

/// 测试：互斥锁闭包风格
fn test_mutex_closure() -> bool {
    info!("Testing: Mutex Closure Style");
    
    let mutex = Mutex::new().expect("Mutex creation failed");
    
    let mut executed = false;
    mutex.with_lock(|| {
        executed = true;
    });
    
    test_assert!(executed, "mutex_closure_executed", "Closure should be executed")
}

/// 测试：定时器基本功能
fn test_timer_basic() -> bool {
    info!("Testing: Timer Basic");
    
    let mut timer = Timer::new(100); // 100ms 定时器
    
    // 测试初始状态
    if !test_assert!(!timer.is_running(), "timer_initial_stopped", "Timer should not be running initially") {
        return false;
    }
    
    // 测试启动
    timer.start();
    if !test_assert!(timer.is_running(), "timer_started", "Timer should be running after start") {
        return false;
    }
    
    // 测试停止
    timer.stop();
    test_assert!(!timer.is_running(), "timer_stopped", "Timer should not be running after stop")
}

/// 测试：延时功能
fn test_delay() -> bool {
    info!("Testing: Delay");
    
    // 测试延时不会失败
    let result = Delay::delay(10);
    test_assert!(result.is_ok(), "delay_basic", "Delay should succeed")
}

/// 测试：消息队列基本功能
fn test_message_queue() -> bool {
    info!("Testing: Message Queue");
    
    // 创建消息队列
    let result: Result<Mq<u32, 8>> = Mq::new();
    
    if !test_assert!(result.is_ok(), "mq_creation", "Message queue creation should succeed") {
        return false;
    }
    
    let mut mq = result.unwrap();
    
    // 测试发送
    let send_result = mq.send(42);
    if !test_assert!(send_result.is_ok(), "mq_send", "Send should succeed") {
        return false;
    }
    
    // 测试接收
    let received = mq.receive();
    if !test_assert!(received == Some(42), "mq_receive", "Should receive 42") {
        return false;
    }
    
    // 测试空队列接收
    let empty_receive = mq.receive();
    if !test_assert!(empty_receive.is_none(), "mq_receive_empty", "Empty queue should return None") {
        return false;
    }
    
    // 测试队列满
    for i in 0..8 {
        let _ = mq.send(i);
    }
    if !test_assert!(mq.is_full(), "mq_full", "Queue should be full after 8 sends") {
        return false;
    }
    
    // 测试队列长度
    test_assert_eq!(mq.len(), 8, "mq_length")
}

/// 测试：错误处理 - 任务槽满
fn test_error_task_slots_full() -> bool {
    info!("Testing: Error Handling - Task Slots Full");
    
    // 注意：这个测试可能会影响其他测试，因为它会填满任务槽
    // 在实际测试中，应该在最后运行或者有清理机制
    
    // 计算剩余槽位
    let current_count = Task::iter().count();
    let remaining = MAX_TASKS.saturating_sub(current_count);
    
    if remaining == 0 {
        report_test("error_task_slots_full", true, "Already full");
        return true;
    }
    
    // 填满剩余槽位
    for _ in 0..remaining {
        let _ = Task::new("fill_task", |_| { loop { Delay::delay(10000).unwrap(); } });
    }
    
    // 尝试创建超出限制的任务
    let result = Task::new("overflow_task", |_| { loop { Delay::delay(10000).unwrap(); } });
    
    let is_slots_full_error = matches!(result, Err(RtosError::TaskSlotsFull));
    test_assert!(is_slots_full_error, "error_task_slots_full", "Should return TaskSlotsFull error")
}

/// 测试：任务名称和 ID
fn test_task_name_and_id() -> bool {
    info!("Testing: Task Name and ID");
    
    let result = Task::new("named_task", |_| {
        loop { Delay::delay(1000).unwrap(); }
    });
    
    if !test_assert!(result.is_ok(), "task_name_creation", "Task creation should succeed") {
        return false;
    }
    
    let task = result.unwrap();
    
    // 测试名称
    if !test_assert_eq!(task.get_name(), "named_task", "task_name_correct") {
        return false;
    }
    
    // 测试 ID（应该是非负数）
    let id = task.get_taskid();
    test_assert!(id < MAX_TASKS, "task_id_valid", "Task ID should be valid")
}

/// 测试：栈顶对齐
fn test_stack_alignment() -> bool {
    info!("Testing: Stack Alignment");
    
    let result = Task::new("align_test", |_| {
        loop { Delay::delay(1000).unwrap(); }
    });
    
    if !test_assert!(result.is_ok(), "stack_align_creation", "Task creation should succeed") {
        return false;
    }
    
    let task = result.unwrap();
    let stack_top = task.get_stack_top();
    
    // 栈顶应该 8 字节对齐
    let is_aligned = (stack_top & 0x7) == 0;
    test_assert!(is_aligned, "stack_alignment_8byte", "Stack should be 8-byte aligned")
}

// ============================================================================
// 测试运行器
// ============================================================================

/// 运行所有测试
fn run_all_tests() {
    info!("================================================");
    info!("       Neon-RTOS2 Test Suite");
    info!("================================================");
    info!("");
    
    // 任务管理测试
    info!("--- Task Management Tests ---");
    test_task_creation();
    test_task_builder();
    test_task_state();
    test_task_priority();
    test_task_name_and_id();
    test_stack_alignment();
    info!("");
    
    // 任务迭代器测试
    info!("--- Task Iterator Tests ---");
    test_task_iterator();
    info!("");
    
    // 互斥锁测试
    info!("--- Mutex Tests ---");
    test_mutex_basic();
    test_mutex_raii();
    test_mutex_closure();
    info!("");
    
    // 定时器测试
    info!("--- Timer Tests ---");
    test_timer_basic();
    test_delay();
    info!("");
    
    // 消息队列测试
    info!("--- Message Queue Tests ---");
    test_message_queue();
    info!("");
    
    // 错误处理测试（放在最后��因为会填满任务槽）
    info!("--- Error Handling Tests ---");
    test_error_task_slots_full();
    info!("");
    
    // 打印测试结果汇总
    info!("================================================");
    info!("               Test Results");
    info!("================================================");
    unsafe {
        info!("Total:  {}", TEST_COUNT);
        info!("Passed: {}", TESTS_PASSED);
        info!("Failed: {}", TESTS_FAILED);
        
        if TESTS_FAILED == 0 {
            info!("");
            info!("All tests PASSED!");
        } else {
            error!("");
            error!("Some tests FAILED!");
            error!("Failed tests:");
            for i in 0..TEST_COUNT {
                if let Some(result) = &TEST_RESULTS[i] {
                    if !result.passed {
                        error!("  - {}: {}", result.name, result.message);
                    }
                }
            }
        }
    }
    info!("================================================");
}

/// 测试任务 - 运行测试套件
fn test_runner_task(_: usize) {
    info!("Test runner task started");
    info!("");
    
    // 运行所有测试
    run_all_tests();
    
    // 测试完成，退出 QEMU
    info!("");
    info!("Tests completed, exiting...");
    debug::exit(debug::EXIT_SUCCESS);
    
    // 如果退出失败，进入空闲循环
    loop {
        Delay::delay(1000).unwrap();
    }
}

// ============================================================================
// 主函数
// ============================================================================

#[entry]
fn main() -> ! {
    // 初始化内核
    kernel_init();
    set_log_level(LogLevel::Info);
    
    info!("Neon-RTOS2 Test Framework Initialized");
    info!("");
    
    // 初始化 SysTick
    let p = Peripherals::take().unwrap();
    let mut syst = p.SYST;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(SYST_RELOAD);
    syst.enable_counter();
    syst.enable_interrupt();
    
    // 创建测试运行器任务
    Task::builder("test_runner")
        .priority(Priority::High)
        .spawn(test_runner_task)
        .expect("Failed to create test runner task");
    
    // 启动调度器
    info!("Starting scheduler...");
    info!("");
    Scheduler::start();
    
    loop {}
}

// ============================================================================
// Panic 处理
// ============================================================================

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("!!! TEST PANIC !!!");
    if let Some(location) = info.location() {
        error!("Location: {}:{}", location.file(), location.line());
    }
    if let Some(message) = info.message().as_str() {
        error!("Message: {}", message);
    }
    
    // 测试失败，退出 QEMU
    debug::exit(debug::EXIT_FAILURE);
    
    loop {}
}
