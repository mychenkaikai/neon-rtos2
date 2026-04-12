//! # 任务唤醒器
//!
//! 实现 Rust 异步运行时所需的 Waker 机制。
//!
//! Waker 用于在异步任务需要被唤醒时通知执行器。

use core::task::{RawWaker, RawWakerVTable, Waker};
use crate::sync::event::Event;
use crate::compat::{Arc, VecDeque, Vec};
use critical_section::Mutex;
use core::cell::RefCell;

pub(crate) struct WokenState {
    pub queue: VecDeque<usize>,
    pub in_queue: Vec<bool>,
}

pub(crate) struct WakerData {
    pub(crate) rtos_task_id: usize,
    pub(crate) future_id: usize,
    pub(crate) woken_state: Arc<Mutex<RefCell<WokenState>>>,
}

/// 任务唤醒器
///
/// 基于任务 ID 的唤醒机制，当异步操作完成时，
/// 通过 Waker 将对应的任务标记为就绪状态。
pub struct TaskWaker;

impl TaskWaker {
    /// 创建新的 Waker
    ///
    /// # 参数
    /// - `data`: Waker 所需的数据
    ///
    /// # 返回值
    /// 标准库的 `Waker` 类型
    pub(crate) fn new(data: Arc<WakerData>) -> Waker {
        let raw = RawWaker::new(
            Arc::into_raw(data) as *const (),
            &VTABLE,
        );
        // SAFETY: 我们正确实现了 vtable 中的所有函数
        unsafe { Waker::from_raw(raw) }
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
unsafe fn clone_waker(data: *const ()) -> RawWaker {
    // 修复 unsafe fn 中的 unsafe 操作
    let arc = unsafe { Arc::from_raw(data as *const WakerData) };
    let cloned = arc.clone();
    let _ = Arc::into_raw(arc);
    RawWaker::new(Arc::into_raw(cloned) as *const (), &VTABLE)
}

/// 唤醒任务（消耗 Waker）
unsafe fn wake(data: *const ()) {
    let arc = unsafe { Arc::from_raw(data as *const WakerData) };
    critical_section::with(|cs| {
        let mut state = arc.woken_state.borrow_ref_mut(cs);
        if state.in_queue.len() <= arc.future_id {
            state.in_queue.resize(arc.future_id + 1, false);
        }
        if !state.in_queue[arc.future_id] {
            state.in_queue[arc.future_id] = true;
            state.queue.push_back(arc.future_id);
        }
    });
    Event::wake_task_by_id(arc.rtos_task_id);
    // arc 随离开作用域被 drop
}

/// 唤醒任务（不消耗 Waker）
unsafe fn wake_by_ref(data: *const ()) {
    let arc = unsafe { Arc::from_raw(data as *const WakerData) };
    critical_section::with(|cs| {
        let mut state = arc.woken_state.borrow_ref_mut(cs);
        if state.in_queue.len() <= arc.future_id {
            state.in_queue.resize(arc.future_id + 1, false);
        }
        if !state.in_queue[arc.future_id] {
            state.in_queue[arc.future_id] = true;
            state.queue.push_back(arc.future_id);
        }
    });
    Event::wake_task_by_id(arc.rtos_task_id);
    let _ = Arc::into_raw(arc);
}

/// 释放 Waker
unsafe fn drop_waker(data: *const ()) {
    let _ = unsafe { Arc::from_raw(data as *const WakerData) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;

    #[test]
    fn test_waker_wake() {
        kernel_init();
        let mut task = Task::new("wake_test", |_| {}).unwrap();
        
        // 将任务设为阻塞状态
        task.block(Event::Async(task.get_taskid()));
        assert!(matches!(task.get_state(), TaskState::Blocked(_)));
        
        let state = Arc::new(Mutex::new(RefCell::new(WokenState {
            queue: VecDeque::new(),
            in_queue: Vec::new(),
        })));
        
        let data = Arc::new(WakerData {
            rtos_task_id: task.get_taskid(),
            future_id: 2,
            woken_state: state.clone(),
        });
        
        // 使用 Waker 唤醒
        let waker = TaskWaker::new(data);
        waker.wake();
        
        // 任务应该变为就绪状态
        assert_eq!(task.get_state(), TaskState::Ready);
        critical_section::with(|cs| {
            let state = state.borrow_ref(cs);
            assert!(state.in_queue[2]);
            assert_eq!(state.queue.front(), Some(&2));
        });
    }
}
