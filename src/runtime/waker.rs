//! # 任务唤醒器
//!
//! 实现 Rust 异步运行时所需的 Waker 机制。
//!
//! Waker 用于在异步任务需要被唤醒时通知执行器。

use crate::compat::{Arc, VecDeque};
use crate::kernel::task::Task;
use crate::sync::event::Event;
use alloc::sync::Weak;
use alloc::task::Wake;
use core::cell::{Cell, RefCell};
use critical_section::{CriticalSection, Mutex};

pub(crate) struct WokenState {
    pub queue: VecDeque<usize>,
    pub parked: Option<Task>,
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Queued,
    Waiting,
    Complete,
}

// Each spawn gets its own registration even if it reuses a slot. Old external
// wakers cannot enqueue the replacement or keep a dropped executor alive.
pub(crate) struct WakerData {
    slot: usize,
    state: Mutex<Cell<State>>,
    cancelled: Mutex<Cell<bool>>,
    executor: Weak<Mutex<RefCell<WokenState>>>,
}

impl WakerData {
    pub fn queued(slot: usize, executor: &Arc<Mutex<RefCell<WokenState>>>) -> Self {
        Self {
            slot,
            state: Mutex::new(Cell::new(State::Queued)),
            cancelled: Mutex::new(Cell::new(false)),
            executor: Arc::downgrade(executor),
        }
    }

    pub fn begin_poll(&self, cs: CriticalSection<'_>) {
        self.state.borrow(cs).set(State::Waiting);
    }

    pub fn complete(&self, cs: CriticalSection<'_>, shared: &mut WokenState) {
        if self.state.borrow(cs).replace(State::Complete) == State::Queued {
            shared.queue.retain(|id| *id != self.slot);
        }
    }

    pub fn cancel(&self) {
        critical_section::with(|cs| self.cancelled.borrow(cs).set(true));
        self.notify();
    }

    pub fn cancelled(&self) -> bool {
        critical_section::with(|cs| self.cancelled.borrow(cs).get())
    }

    fn notify(&self) {
        let Some(executor) = self.executor.upgrade() else {
            return;
        };
        critical_section::with(|cs| {
            if self.state.borrow(cs).get() != State::Waiting {
                return;
            }
            self.state.borrow(cs).set(State::Queued);
            let mut shared = executor.borrow_ref_mut(cs);
            shared.queue.push_back(self.slot);
            if let Some(task) = shared.parked.take() {
                task.wake_if(Event::Async(Arc::as_ptr(&executor) as usize));
            }
        });
    }
}
impl Wake for WakerData {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}
