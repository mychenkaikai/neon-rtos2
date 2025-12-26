//! # 任务唤醒器
//!
//! 实现 Rust 异步运行时所需的 Waker 机制。
//!
//! Waker 用于在异步任务需要被唤醒时通知执行器。

use core::task::{RawWaker, RawWakerVTable, Waker};
use crate::kernel::task::Task;

/// 任务唤醒器
///
/// 基于任务 ID 的唤醒机制，当异步操作完成时，
/// 通过 Waker 将对应的任务标记为就绪状态。
pub struct TaskWaker {
    task_id: usize,
}

impl TaskWaker {
    /// 创建新的 Waker
    ///
    /// # 参数
    /// - `task_id`: 关联的任务 ID
    ///
    /// # 返回值
    /// 标准库的 `Waker` 类型
    pub fn new(task_id: usize) -> Waker {
        let raw = RawWaker::new(
            task_id as *const (),
            &VTABLE,
        );
        // SAFETY: 我们正确实现了 vtable 中的所有函数
        unsafe { Waker::from_raw(raw) }
    }

    /// 唤醒任务
    ///
    /// 将任务状态设置为就绪
    fn wake(task_id: usize) {
        let mut task = Task(task_id);
        task.ready();
    }
}

/// Waker 虚函数表
///
/// 定义了 Waker 的克隆、唤醒和释放行为
const VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_waker,
    wake,
    wake_by_ref,
    drop_waker,
);

/// 克隆 Waker
///
/// 由于我们只存储任务 ID（一个 usize），克隆是简单的复制
unsafe fn clone_waker(data: *const ()) -> RawWaker {
    RawWaker::new(data, &VTABLE)
}

/// 唤醒任务（消耗 Waker）
unsafe fn wake(data: *const ()) {
    let task_id = data as usize;
    TaskWaker::wake(task_id);
}

/// 唤醒任务（不消耗 Waker）
unsafe fn wake_by_ref(data: *const ()) {
    let task_id = data as usize;
    TaskWaker::wake(task_id);
}

/// 释放 Waker
///
/// 由于我们只存储任务 ID，不需要释放任何资源
unsafe fn drop_waker(_data: *const ()) {
    // 无需操作，task_id 是简单的 usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use crate::kernel::task::TaskState;

    #[test]
    fn test_waker_creation() {
        kernel_init();
        Task::new("waker_test", |_| {}).unwrap();
        
        let waker = TaskWaker::new(0);
        // Waker 应该能正常创建
        assert!(!waker.will_wake(&TaskWaker::new(1)));
        assert!(waker.will_wake(&TaskWaker::new(0)));
    }

    #[test]
    fn test_waker_wake() {
        kernel_init();
        let mut task = Task::new("wake_test", |_| {}).unwrap();
        
        // 将任务设为阻塞状态
        task.block(crate::sync::event::Event::Signal(0));
        assert!(matches!(task.get_state(), TaskState::Blocked(_)));
        
        // 使用 Waker 唤醒
        let waker = TaskWaker::new(task.get_taskid());
        waker.wake();
        
        // 任务应该变为就绪状态
        assert_eq!(task.get_state(), TaskState::Ready);
    }

    #[test]
    fn test_waker_clone() {
        kernel_init();
        Task::new("clone_test", |_| {}).unwrap();
        
        let waker1 = TaskWaker::new(0);
        let waker2 = waker1.clone();
        
        // 两个 Waker 应该唤醒同一个任务
        assert!(waker1.will_wake(&waker2));
    }
}

