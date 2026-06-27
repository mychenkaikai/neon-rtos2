//! # 异步执行器
//!
//! 提供简单的异步任务执行器，适合嵌入式环境使用。
//!
//! ## 设计原则
//!
//! - **避免轮询**: 与 RTOS 调度器集成，在没有任务就绪时挂起，而不是死循环
//! - **轻量**: 最小化内存占用
//! - **可预测**: 确定性的执行顺序

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use crate::compat::{Box, Vec, Arc, VecDeque};
use super::waker::{TaskWaker, WakerData, WokenState};
use critical_section::Mutex;
use core::cell::RefCell;

/// 异步任务包装器
///
/// 将 Future 包装为可执行的任务
pub struct AsyncTask {
    /// 被包装的 Future
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    /// 任务 ID，用于创建 Waker
    task_id: usize,
}

impl AsyncTask {
    /// 创建新的异步任务
    pub fn new<F>(future: F, task_id: usize) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            future: Box::pin(future),
            task_id,
        }
    }
}

/// 异步执行器
///
/// 管理和执行异步任务的简单执行器。
pub struct Executor {
    /// 所有的异步任务
    tasks: Vec<Option<AsyncTask>>,
    /// 被唤醒的任务队列
    woken_state: Arc<Mutex<RefCell<WokenState>>>,
    /// 下一个任务 ID
    next_task_id: usize,
    /// 活跃任务数
    task_count: usize,
}

impl Executor {
    /// 创建新的执行器
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            woken_state: Arc::new(Mutex::new(RefCell::new(WokenState {
                queue: VecDeque::new(),
                in_queue: Vec::new(),
            }))),
            next_task_id: 0,
            task_count: 0,
        }
    }

    /// 添加异步任务
    pub fn spawn<F>(&mut self, future: F) -> usize
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        
        let task = AsyncTask::new(future, task_id);
        
        if self.tasks.len() <= task_id {
            self.tasks.resize_with(task_id + 1, || None);
        }
        self.tasks[task_id] = Some(task);
        self.task_count += 1;
        
        // 初始时将任务加入唤醒队列，以便第一次被 poll
        critical_section::with(|cs| {
            let mut state = self.woken_state.borrow_ref_mut(cs);
            if state.in_queue.len() <= task_id {
                state.in_queue.resize(task_id + 1, false);
            }
            if !state.in_queue[task_id] {
                state.in_queue[task_id] = true;
                state.queue.push_back(task_id);
            }
        });
        
        task_id
    }

    /// 运行执行器
    pub fn run(&mut self) {
        use crate::kernel::scheduler::Scheduler;
        use crate::sync::event::Event;
        use crate::hal::trigger_schedule;

        loop {
            if !self.poll_next_woken_task() {
                if self.is_empty() {
                    break;
                }

                // 没有新唤醒的任务，阻塞当前 RTOS 任务
                let mut current_task = Scheduler::get_current_task();
                current_task.block(Event::Async(current_task.get_taskid()));
                trigger_schedule();
            }
        }
    }

    /// 执行一轮调度
    pub fn poll_once(&mut self) -> bool {
        self.poll_next_woken_task();
        !self.is_empty()
    }

    /// 获取未完成任务数量
    pub fn pending_count(&self) -> usize {
        self.task_count
    }

    /// 检查执行器是否为空
    pub fn is_empty(&self) -> bool {
        self.task_count == 0
    }

    fn poll_next_woken_task(&mut self) -> bool {
        let Some(id) = self.pop_woken_task() else {
            return false;
        };

        self.poll_task(id);
        true
    }

    fn pop_woken_task(&mut self) -> Option<usize> {
        critical_section::with(|cs| {
            let mut state = self.woken_state.borrow_ref_mut(cs);
            if let Some(id) = state.queue.pop_front() {
                state.in_queue[id] = false;
                Some(id)
            } else {
                None
            }
        })
    }

    fn poll_task(&mut self, id: usize) {
        use crate::kernel::scheduler::Scheduler;

        if id >= self.tasks.len() {
            return;
        }

        let Some(mut task) = self.tasks[id].take() else {
            return;
        };

        let waker_data = Arc::new(WakerData {
            rtos_task_id: Scheduler::get_current_task().get_taskid(),
            future_id: id,
            woken_state: self.woken_state.clone(),
        });
        let waker = TaskWaker::new(waker_data);
        let mut cx = Context::from_waker(&waker);

        match task.future.as_mut().poll(&mut cx) {
            Poll::Ready(()) => {
                self.task_count -= 1;
            }
            Poll::Pending => {
                self.tasks[id] = Some(task);
            }
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use serial_test::serial;
    use crate::utils::kernel_init;

    #[test]
    #[serial]
    fn test_executor_creation() {
        kernel_init();
        let executor = Executor::new();
        assert!(executor.is_empty());
        assert_eq!(executor.pending_count(), 0);
    }

    #[test]
    #[serial]
    fn test_executor_spawn() {
        kernel_init();
        let mut executor = Executor::new();
        
        let task_id = executor.spawn(async {});
        assert_eq!(task_id, 0);
        assert_eq!(executor.pending_count(), 1);
        
        let task_id2 = executor.spawn(async {});
        assert_eq!(task_id2, 1);
        assert_eq!(executor.pending_count(), 2);
    }

    #[test]
    #[serial]
    fn test_executor_run_simple() {
        kernel_init();
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        
        let mut executor = Executor::new();
        
        executor.spawn(async {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        });
        
        executor.spawn(async {
            COUNTER.fetch_add(10, Ordering::SeqCst);
        });
        
        // 由于测试环境中不支持阻塞RTOS任务，poll_once 会在测试中更适用
        while !executor.is_empty() {
            executor.poll_once();
        }
        
        assert_eq!(COUNTER.load(Ordering::SeqCst), 11);
        assert!(executor.is_empty());
    }

    #[test]
    #[serial]
    fn test_executor_large_capacity() {
        kernel_init();
        // 测试任务数量超过原 64 位限制
        let mut executor = Executor::new();
        let num_tasks = 100;
        
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        COUNTER.store(0, Ordering::SeqCst);
        
        for _ in 0..num_tasks {
            executor.spawn(async {
                COUNTER.fetch_add(1, Ordering::SeqCst);
            });
        }
        
        assert_eq!(executor.pending_count(), num_tasks);
        
        while !executor.is_empty() {
            executor.poll_once();
        }
        
        assert_eq!(COUNTER.load(Ordering::SeqCst), num_tasks);
        assert!(executor.is_empty());
    }

    #[test]
    #[serial]
    fn test_executor_waker_queue() {
        kernel_init();
        use core::future::Future;
        use core::pin::Pin;
        use core::task::{Context, Poll};
        
        // 模拟一个需要多次 poll 才能完成的 future
        struct StepFuture {
            steps: usize,
        }
        
        impl Future for StepFuture {
            type Output = ();
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                if self.steps == 0 {
                    Poll::Ready(())
                } else {
                    self.steps -= 1;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        }
        
        let mut executor = Executor::new();
        executor.spawn(StepFuture { steps: 3 });
        
        // 第 1 次 poll (steps 变为 2, 并且 waker enqueue)
        assert!(executor.poll_once());
        assert_eq!(executor.pending_count(), 1);
        
        // 第 2 次 poll (steps 变为 1, 并且 waker enqueue)
        assert!(executor.poll_once());
        
        // 第 3 次 poll (steps 变为 0, 并且 waker enqueue)
        assert!(executor.poll_once());
        
        // 第 4 次 poll (Ready)
        assert!(!executor.poll_once());
        assert!(executor.is_empty());
    }

    #[test]
    #[serial]
    fn test_poll_once_keeps_pending_state_without_new_wake() {
        kernel_init();
        use core::future::Future;
        use core::pin::Pin;
        use core::task::{Context, Poll};

        struct PendingFuture(bool);

        impl Future for PendingFuture {
            type Output = ();

            fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                if self.0 {
                    Poll::Pending
                } else {
                    self.0 = true;
                    Poll::Pending
                }
            }
        }

        let mut executor = Executor::new();
        executor.spawn(PendingFuture(false));

        // 首次 poll 会消费初始唤醒项，但 future 不会重新唤醒自己。
        assert!(executor.poll_once());
        assert_eq!(executor.pending_count(), 1);

        // 没有新的唤醒任务时，不应误完成任务，返回值仍表示存在未完成任务。
        assert!(executor.poll_once());
        assert_eq!(executor.pending_count(), 1);
        assert!(!executor.is_empty());
    }
}
