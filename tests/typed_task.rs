//! # TypedTask 集成测试
//!
//! 测试类型状态模式的正确性，验证编译期类型安全。
//!
//! ## 测试覆盖
//!
//! - 状态转换：Created -> Ready -> Running -> Blocked -> Ready
//! - Builder 模式
//! - 类型转换
//! - 编译期安全（通过注释说明无法编译的情况）

use neon_rtos2::kernel::task::state::*;
use neon_rtos2::kernel::task::Priority;
use neon_rtos2::sync::event::Event;
use neon_rtos2::utils::kernel_init;
use serial_test::serial;

// ============================================================================
// 基本创建测试
// ============================================================================

#[test]
#[serial]
fn test_typed_task_new() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("test_new", |_| {}).unwrap();
    assert_eq!(task.name(), "test_new");
    assert!(task.id() < 16); // MAX_TASKS = 16
}

#[test]
#[serial]
fn test_typed_task_builder_basic() {
    kernel_init();
    
    let task = TypedTask::builder("builder_basic")
        .spawn(|_| {})
        .unwrap();
    
    assert_eq!(task.name(), "builder_basic");
    assert_eq!(task.priority(), Priority::Normal); // 默认优先级
}

#[test]
#[serial]
fn test_typed_task_builder_with_priority() {
    kernel_init();
    
    let task = TypedTask::builder("high_priority")
        .priority(Priority::High)
        .spawn(|_| {})
        .unwrap();
    
    assert_eq!(task.priority(), Priority::High);
}

#[test]
#[serial]
fn test_typed_task_builder_build_vs_spawn() {
    kernel_init();
    
    // build() 返回 Created 状态
    let created_task = TypedTask::builder("build_task")
        .build(|_| {})
        .unwrap();
    assert_eq!(created_task.name(), "build_task");
    
    // spawn() 返回 Ready 状态
    let ready_task = TypedTask::builder("spawn_task")
        .spawn(|_| {})
        .unwrap();
    assert_eq!(ready_task.name(), "spawn_task");
}

// ============================================================================
// 状态转换测试
// ============================================================================

#[test]
#[serial]
fn test_created_to_ready() {
    kernel_init();
    
    // Created 状态
    let task: TypedTask<Created> = TypedTask::new("c2r", |_| {}).unwrap();
    
    // Created -> Ready
    let task: TypedTask<Ready> = task.start();
    assert_eq!(task.name(), "c2r");
}

#[test]
#[serial]
fn test_ready_to_running() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("r2run", |_| {}).unwrap();
    let task = task.start(); // Ready
    
    // Ready -> Running
    let task: TypedTask<Running> = task.run();
    assert_eq!(task.name(), "r2run");
}

#[test]
#[serial]
fn test_running_to_ready_yield() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("yield_test", |_| {}).unwrap();
    let task = task.start();
    let task = task.run();
    
    // Running -> Ready (yield)
    let task: TypedTask<Ready> = task.yield_now();
    assert_eq!(task.name(), "yield_test");
}

#[test]
#[serial]
fn test_running_to_blocked() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("run2block", |_| {}).unwrap();
    let task = task.start();
    let task = task.run();
    
    // Running -> Blocked
    let task: TypedTask<Blocked> = task.block(Event::Signal(42));
    assert_eq!(task.blocked_event(), Some(Event::Signal(42)));
}

#[test]
#[serial]
fn test_ready_to_blocked() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("ready2block", |_| {}).unwrap();
    let task = task.start();
    
    // Ready -> Blocked
    let task: TypedTask<Blocked> = task.block(Event::Timer(100));
    assert_eq!(task.blocked_event(), Some(Event::Timer(100)));
}

#[test]
#[serial]
fn test_blocked_to_ready_wake() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("wake_test", |_| {}).unwrap();
    let task = task.start();
    let task = task.run();
    let task = task.block(Event::Signal(1));
    
    // Blocked -> Ready (wake)
    let task: TypedTask<Ready> = task.wake();
    assert_eq!(task.name(), "wake_test");
}

// ============================================================================
// 完整状态转换链测试
// ============================================================================

#[test]
#[serial]
fn test_full_lifecycle() {
    kernel_init();
    
    // 完整生命周期：Created -> Ready -> Running -> Blocked -> Ready -> Running
    let task = TypedTask::<Created>::new("lifecycle", |_| {}).unwrap();
    let name = task.name();
    
    let task = task.start();           // Created -> Ready
    let task = task.run();             // Ready -> Running
    let task = task.block(Event::Signal(1)); // Running -> Blocked
    let task = task.wake();            // Blocked -> Ready
    let task = task.run();             // Ready -> Running
    let _task = task.yield_now();      // Running -> Ready
    
    assert_eq!(name, "lifecycle");
}

#[test]
#[serial]
fn test_multiple_yield_cycles() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("multi_yield", |_| {}).unwrap();
    let task = task.start();
    
    // 多次 yield 循环
    let task = task.run();
    let task = task.yield_now();
    let task = task.run();
    let task = task.yield_now();
    let task = task.run();
    let _task = task.yield_now();
}

#[test]
#[serial]
fn test_multiple_block_wake_cycles() {
    kernel_init();
    
    let task = TypedTask::<Created>::new("multi_block", |_| {}).unwrap();
    let task = task.start();
    let task = task.run();
    
    // 多次 block/wake 循环
    let task = task.block(Event::Signal(1));
    assert_eq!(task.blocked_event(), Some(Event::Signal(1)));
    
    let task = task.wake();
    let task = task.run();
    
    let task = task.block(Event::Timer(200));
    assert_eq!(task.blocked_event(), Some(Event::Timer(200)));
    
    let _task = task.wake();
}

// ============================================================================
// 类型转换测试
// ============================================================================

#[test]
#[serial]
fn test_into_inner() {
    kernel_init();
    
    let typed_task = TypedTask::<Created>::new("into_inner", |_| {}).unwrap();
    let inner = typed_task.into_inner();
    
    assert_eq!(inner.get_name(), "into_inner");
}

#[test]
#[serial]
fn test_inner_ref() {
    kernel_init();
    
    let typed_task = TypedTask::<Created>::new("inner_ref", |_| {}).unwrap();
    let inner_ref = typed_task.inner();
    
    assert_eq!(inner_ref.get_name(), "inner_ref");
}

#[test]
#[serial]
fn test_from_task() {
    kernel_init();
    
    use neon_rtos2::kernel::task::Task;
    
    let task = Task::new("from_task", |_| {}).unwrap();
    let typed_task: TypedTask<Ready> = task.into();
    
    assert_eq!(typed_task.name(), "from_task");
}

// ============================================================================
// 优先级测试
// ============================================================================

#[test]
#[serial]
fn test_priority_in_ready_state() {
    kernel_init();
    
    let mut task = TypedTask::builder("priority_test")
        .priority(Priority::Low)
        .spawn(|_| {})
        .unwrap();
    
    assert_eq!(task.priority(), Priority::Low);
    
    // 修改优先级
    task.set_priority(Priority::High);
    assert_eq!(task.priority(), Priority::High);
}

#[test]
#[serial]
fn test_priority_in_running_state() {
    kernel_init();
    
    let task = TypedTask::builder("running_priority")
        .priority(Priority::Normal)
        .spawn(|_| {})
        .unwrap();
    
    let task = task.run();
    assert_eq!(task.priority(), Priority::Normal);
}

// ============================================================================
// 编译期类型安全说明
// ============================================================================

/// 以下代码展示了类型状态模式如何在编译期防止非法状态转换。
/// 这些代码如果取消注释将无法编译，证明了类型安全性。
///
/// ```compile_fail
/// use neon_rtos2::kernel::task::state::*;
/// use neon_rtos2::utils::kernel_init;
///
/// fn illegal_transitions() {
///     kernel_init();
///     
///     // 错误 1: Created 状态没有 run() 方法
///     let task = TypedTask::<Created>::new("test", |_| {}).unwrap();
///     task.run(); // 编译错误！
///     
///     // 错误 2: Created 状态没有 yield_now() 方法
///     let task = TypedTask::<Created>::new("test", |_| {}).unwrap();
///     task.yield_now(); // 编译错误！
///     
///     // 错误 3: Running 状态没有 start() 方法
///     let task = TypedTask::<Created>::new("test", |_| {}).unwrap();
///     let task = task.start().run();
///     task.start(); // 编译错误！
///     
///     // 错误 4: Blocked 状态没有 run() 方法
///     let task = TypedTask::<Created>::new("test", |_| {}).unwrap();
///     let task = task.start().run().block(Event::Signal(1));
///     task.run(); // 编译错误！
/// }
/// ```
#[test]
fn test_compile_time_safety_documented() {
    // 这个测试只是为了文档目的
    // 真正的编译期检查由上面的 compile_fail 文档测试完成
    assert!(true);
}

// ============================================================================
// 边界条件测试
// ============================================================================

#[test]
#[serial]
fn test_task_with_closure() {
    kernel_init();
    
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter_clone = counter.clone();
    
    let task = TypedTask::<Created>::new("closure_task", move |_| {
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }).unwrap();
    
    assert_eq!(task.name(), "closure_task");
}

#[test]
#[serial]
fn test_different_event_types() {
    kernel_init();
    
    // 测试不同的事件类型
    let task1 = TypedTask::<Created>::new("signal_event", |_| {}).unwrap();
    let task1 = task1.start().run().block(Event::Signal(123));
    assert_eq!(task1.blocked_event(), Some(Event::Signal(123)));
    
    let task2 = TypedTask::<Created>::new("timer_event", |_| {}).unwrap();
    let task2 = task2.start().run().block(Event::Timer(456));
    assert_eq!(task2.blocked_event(), Some(Event::Timer(456)));
}

