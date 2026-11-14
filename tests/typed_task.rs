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
    
    let task = TypedTask::<Created>::new("test_new", || {}).unwrap();
    assert_eq!(task.name(), "test_new");
    assert!(task.id() < 16); // MAX_TASKS = 16
}

#[test]
#[serial]
fn test_typed_task_builder_basic() {
    kernel_init();
    
    let task = TypedTask::builder("builder_basic")
        .spawn(|| {})
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
        .spawn(|| {})
        .unwrap();
    
    assert_eq!(task.priority(), Priority::High);
}

#[test]
#[serial]
fn test_typed_task_builder_build_vs_spawn() {
    kernel_init();
    
    // build() 返回 Created 状态
    let created_task = TypedTask::builder("build_task")
        .build(|| {})
        .unwrap();
    assert_eq!(created_task.name(), "build_task");
    
    // spawn() 返回 Ready 状态
    let ready_task = TypedTask::builder("spawn_task")
        .spawn(|| {})
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
    let task: TypedTask<Created> = TypedTask::new("c2r", || {}).unwrap();
    
    // Created -> Ready
    let task: TypedTask<Ready> = task.start().unwrap();
    assert_eq!(task.name(), "c2r");
}

#[test]
#[serial]
fn scheduler_owns_dispatch_and_checks_wake_event() {
    use neon_rtos2::prelude::{Scheduler, TaskState};
    kernel_init();
    let task = TypedTask::<Created>::new("dispatch", || {}).unwrap().start().unwrap().into_inner();
    assert!(task.block_if_ready(Event::Signal(1)));
    assert!(TypedTask::<Ready>::try_from(task).is_err());
    assert!(!task.wake_if(Event::Timer(1)));
    assert_eq!(task.get_state(), TaskState::Blocked(Event::Signal(1)));
    assert!(task.wake_if(Event::Signal(1)));
    Scheduler::start();
    Scheduler::task_switch();
    assert_eq!(Scheduler::get_current_task(), task);
    assert!(task.into_typed().unwrap().into_running().is_some());
    assert!(!task.block_if_ready(Event::Signal(1)));
}

#[test]
#[serial]
fn created_snapshot_cannot_restart_a_dispatched_task() {
    use neon_rtos2::prelude::Scheduler;
    kernel_init();
    let created = TypedTask::<Created>::new("once", || {}).unwrap();
    let alias = created.inner().into_typed().unwrap().into_created().unwrap();
    let ready = created.start().unwrap();
    Scheduler::start();
    Scheduler::task_switch();
    assert_eq!(alias, *ready.inner());
    assert!(ready.inner().into_typed().unwrap().into_running().is_some());
}

// ============================================================================
// 类型转换测试
// ============================================================================

#[test]
#[serial]
fn test_into_inner() {
    kernel_init();
    
    let typed_task = TypedTask::<Created>::new("into_inner", || {}).unwrap();
    let inner = typed_task.start().unwrap().into_inner();
    
    assert_eq!(inner.get_name(), "into_inner");
}

#[test]
#[serial]
fn test_inner_ref() {
    kernel_init();
    
    let typed_task = TypedTask::<Created>::new("inner_ref", || {}).unwrap();
    let inner_ref = typed_task.inner();
    
    assert_eq!(inner_ref.get_name(), "inner_ref");
}

#[test]
#[serial]
fn test_from_task() {
    kernel_init();
    
    use neon_rtos2::kernel::task::Task;
    
    let task = Task::new("from_task", || {}).unwrap();
    let typed_task: TypedTask<Ready> = task.try_into().unwrap();
    
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
        .spawn(|| {})
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
        .spawn(|| {})
        .unwrap();
    
    neon_rtos2::prelude::Scheduler::start();
    neon_rtos2::prelude::Scheduler::task_switch();
    let task = task.into_inner().into_typed().unwrap().into_running().unwrap();
    assert_eq!(task.priority(), Priority::Normal);
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
    
    let task = TypedTask::<Created>::new("closure_task", move || {
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }).unwrap();
    
    assert_eq!(task.name(), "closure_task");
}

#[test]
#[serial]
fn test_different_event_types() {
    kernel_init();
    
    // 测试不同的事件类型
    let task1 = TypedTask::<Created>::new("signal_event", || {}).unwrap();
    let task1 = task1.start().unwrap().into_inner();
    assert!(task1.block_if_ready(Event::Signal(123)));
    let task1 = task1.into_typed().unwrap().into_blocked().unwrap();
    assert_eq!(task1.blocked_event(), Some(Event::Signal(123)));
    
    let task2 = TypedTask::<Created>::new("timer_event", || {}).unwrap();
    let task2 = task2.start().unwrap().into_inner();
    assert!(task2.block_if_ready(Event::Timer(456)));
    let task2 = task2.into_typed().unwrap().into_blocked().unwrap();
    assert_eq!(task2.blocked_event(), Some(Event::Timer(456)));
}


#[test]
#[serial]
fn an_unstarted_job_never_enters_the_ready_queue() {
    use neon_rtos2::prelude::{Scheduler, TaskState};
    kernel_init();
    let before = Scheduler::ready_task_stats().total;
    let job = TypedTask::<Created>::new("deferred", || {}).unwrap();
    let stale = *job.inner();
    assert_eq!(stale.get_state(), TaskState::Created);
    assert_eq!(Scheduler::ready_task_stats().total, before);
    drop(job);
    assert_eq!(stale.get_state(), TaskState::Finished);
    let next = TypedTask::<Created>::new("next", || {}).unwrap().start().unwrap();
    assert_eq!(next.id(), stale.get_taskid());
    assert_eq!(Scheduler::ready_task_stats().total, before + 1);
}
