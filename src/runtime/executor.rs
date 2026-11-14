//! # 异步执行器
//!
//! 提供简单的异步任务执行器，适合嵌入式环境使用。
//!
//! ## 设计原则
//!
//! - **避免轮询**: 与 RTOS 调度器集成，在没有任务就绪时挂起，而不是死循环
//! - **轻量**: 最小化内存占用
//! - **可预测**: 确定性的执行顺序

use super::waker::{WakerData, WokenState};
use crate::compat::{Arc, Box, Vec, VecDeque};
use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Waker};
use crate::ipc::Receiver;
#[cfg(test)]
use core::task::Poll;
use critical_section::Mutex;

/// 异步任务包装器
///
/// 将 Future 包装为可执行的任务
struct AsyncTask<F: ?Sized> {
    future: Pin<Box<F>>,
    registration: Arc<WakerData>,
}

/// 异步执行器
///
/// 管理和执行异步任务的简单执行器。
pub struct TaskExecutor<F: Future<Output = ()> + ?Sized> {
    /// 所有的异步任务
    tasks: Vec<Option<AsyncTask<F>>>,
    /// 被唤醒的任务队列
    woken_state: Arc<Mutex<RefCell<WokenState>>>,
    /// 活跃任务数
    task_count: usize,
}

/// Executor that can be moved between tasks.
pub type Executor = TaskExecutor<dyn Future<Output = ()> + Send>;

/// Executor for futures that stay on the creating task, including borrowed
/// mutex guards. Its wakers can still notify it from another task or interrupt.
pub type LocalExecutor = TaskExecutor<dyn Future<Output = ()>>;

/// Result of a single executor step. Waiting tasks need an external wake.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RunState { Runnable, Waiting, Finished }

/// Dropping the handle schedules cancellation. The executor releases the future
/// on its next poll. Dropping the executor releases all of its futures directly.
#[must_use = "join the task or keep the handle for the desired task lifetime"]
pub struct JoinHandle<T> {
    result: Receiver<T>,
    registration: Arc<WakerData>,
}

impl<T> JoinHandle<T> {
    /// None means the executor was dropped before producing a result.
    /// Dropping the wait leaves the task and its result with the handle.
    /// A produced result can be taken once.
    pub async fn join(&mut self) -> Option<T> { self.result.recv().await }

    /// Request cancellation; the executor drops the future on its next poll.
    pub fn cancel(self) {}
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) { self.registration.cancel(); }
}

impl Executor {
    pub fn spawn(&mut self, future: impl Future<Output = ()> + Send + 'static) {
        self.spawn_boxed(Box::pin(future));
    }
    pub fn spawn_joinable<T: Send + 'static>(
        &mut self, future: impl Future<Output = T> + Send + 'static,
    ) -> JoinHandle<T> {
        let (tx, result) = crate::runtime::channel(1);
        let registration = self.spawn_boxed(Box::pin(async move {
            let _ = tx.try_send(future.await);
        }));
        JoinHandle { result, registration }
    }
}

impl LocalExecutor {
    pub fn spawn(&mut self, future: impl Future<Output = ()> + 'static) {
        self.spawn_boxed(Box::pin(future));
    }
    pub fn spawn_joinable<T: 'static>(
        &mut self, future: impl Future<Output = T> + 'static,
    ) -> JoinHandle<T> {
        let (tx, result) = crate::runtime::channel(1);
        let registration = self.spawn_boxed(Box::pin(async move {
            let _ = tx.try_send(future.await);
        }));
        JoinHandle { result, registration }
    }
}

impl<F: Future<Output = ()> + ?Sized> TaskExecutor<F> {
    /// 创建新的执行器
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            woken_state: Arc::new(Mutex::new(RefCell::new(WokenState {
                queue: VecDeque::new(),
                parked: None,
            }))),
            task_count: 0,
        }
    }

    fn spawn_boxed(&mut self, future: Pin<Box<F>>) -> Arc<WakerData> {
        let slot = self
            .tasks
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.tasks.len());
        let registration = Arc::new(WakerData::queued(slot, &self.woken_state));
        let task = Some(AsyncTask {
            future,
            registration: registration.clone(),
        });
        if slot == self.tasks.len() {
            self.tasks.push(task);
        } else {
            self.tasks[slot] = task;
        }
        self.task_count += 1;
        critical_section::with(|cs| self.woken_state.borrow_ref_mut(cs).queue.push_back(slot));
        registration
    }

    /// 运行执行器
    pub fn run(&mut self) {
        use crate::hal::trigger_schedule;

        loop {
            if !self.poll_next_woken_task() {
                if self.is_empty() {
                    break;
                }

                // 没有新唤醒的任务，阻塞当前 RTOS 任务
                if self.park_if_idle() {
                    trigger_schedule();
                }
            }
        }
    }

    // 检查唤醒队列和标记阻塞必须是一个事务，否则中断唤醒会被后续 block 覆盖。
    fn park_if_idle(&self) -> bool {
        critical_section::with(|cs| {
            let mut shared = self.woken_state.borrow_ref_mut(cs);
            if !shared.queue.is_empty() {
                return false;
            }
            let mut current = crate::kernel::scheduler::Scheduler::get_current_task();
            shared.parked = Some(current);
            current.block(crate::sync::event::Event::Async(
                Arc::as_ptr(&self.woken_state) as usize,
            ));
            true
        })
    }

    /// 执行一轮调度
    pub fn poll_once(&mut self) -> RunState {
        self.poll_next_woken_task();
        if self.is_empty() { return RunState::Finished; }
        critical_section::with(|cs| {
            if self.woken_state.borrow_ref(cs).queue.is_empty() {
                RunState::Waiting
            } else {
                RunState::Runnable
            }
        })
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
            let slot = state.queue.pop_front()?;
            self.tasks[slot]
                .as_ref()
                .unwrap()
                .registration
                .begin_poll(cs);
            state.parked = None;
            Some(slot)
        })
    }

    fn poll_task(&mut self, slot: usize) {
        let task = self.tasks[slot].as_mut().unwrap();
        let waker = Waker::from(task.registration.clone());
        if task.registration.cancelled() || task
            .future
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_ready()
        {
            critical_section::with(|cs| {
                task.registration
                    .complete(cs, &mut self.woken_state.borrow_ref_mut(cs))
            });
            self.tasks[slot] = None;
            self.task_count -= 1;
        }
    }
}

impl<F: Future<Output = ()> + ?Sized> Default for TaskExecutor<F> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use serial_test::serial;

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

        executor.spawn(async {});
        assert_eq!(executor.pending_count(), 1);

        executor.spawn(async {});
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
        use core::task::{Context, Waker};
use crate::ipc::Receiver;
        #[cfg(test)]
        use core::task::Poll;

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
        assert_eq!(executor.poll_once(), RunState::Runnable);
        assert_eq!(executor.pending_count(), 1);

        // 第 2 次 poll (steps 变为 1, 并且 waker enqueue)
        assert_eq!(executor.poll_once(), RunState::Runnable);

        // 第 3 次 poll (steps 变为 0, 并且 waker enqueue)
        assert_eq!(executor.poll_once(), RunState::Runnable);

        // 第 4 次 poll (Ready)
        assert_eq!(executor.poll_once(), RunState::Finished);
        assert!(executor.is_empty());
    }

    #[test]
    #[serial]
    fn test_poll_once_keeps_pending_state_without_new_wake() {
        kernel_init();
        use core::future::Future;
        use core::pin::Pin;
        use core::task::{Context, Waker};
use crate::ipc::Receiver;
        #[cfg(test)]
        use core::task::Poll;

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
        assert_eq!(executor.poll_once(), RunState::Waiting);
        assert_eq!(executor.pending_count(), 1);

        // 没有新的唤醒任务时，不应误完成任务，返回值仍表示存在未完成任务。
        assert_eq!(executor.poll_once(), RunState::Waiting);
        assert_eq!(executor.pending_count(), 1);
        assert!(!executor.is_empty());
    }

    #[test]
    #[serial]
    fn wake_between_empty_poll_and_park_is_not_lost() {
        kernel_init();
        let mut executor = Executor::new();
        executor.spawn(core::future::pending());
        executor.poll_once();
        assert!(executor.pop_woken_task().is_none());
        let waker = Waker::from(executor.tasks[0].as_ref().unwrap().registration.clone());
        waker.wake_by_ref();
        assert!(!executor.park_if_idle());
        executor.poll_once();
        assert!(executor.park_if_idle());
        waker.wake_by_ref();
        assert_eq!(
            crate::kernel::task::Task::from_slot(0).get_state(),
            crate::kernel::task::TaskState::Ready
        );
    }

    #[test]
    #[serial]
    fn stale_future_waker_does_not_wake_an_unrelated_wait() {
        kernel_init();
        let mut executor = Executor::new();
        executor.spawn(core::future::pending());
        executor.poll_once();
        let waker = Waker::from(executor.tasks[0].as_ref().unwrap().registration.clone());
        assert!(executor.park_if_idle());
        let event = crate::sync::event::Event::Signal(42);
        crate::kernel::task::Task::from_slot(0).block(event);
        waker.wake_by_ref();
        assert_eq!(
            crate::kernel::task::Task::from_slot(0).get_state(),
            crate::kernel::task::TaskState::Blocked(event)
        );
    }

    #[test]
    #[serial]
    fn dropping_executor_closes_join_and_cancelling_unpolled_work_skips_it() {
        kernel_init();
        let mut executor = Executor::new();
        let aborted = executor.spawn_joinable(async { panic!("cancelled task ran"); });
        drop(aborted);
        assert_eq!(executor.poll_once(), RunState::Finished);

        let mut handle = executor.spawn_joinable(core::future::pending::<u32>());
        drop(executor);
        let mut join = core::pin::pin!(handle.join());
        assert_eq!(join.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Ready(None));
    }

    #[test]
    #[serial]
    fn reused_future_slot_ignores_a_completed_jobs_waker() {
        kernel_init();
        let mut executor = Executor::new();
        executor.spawn(core::future::poll_fn(|cx| {
            cx.waker().wake_by_ref();
            Poll::Ready(())
        }));
        let stale = Waker::from(executor.tasks[0].as_ref().unwrap().registration.clone());
        executor.run();
        executor.spawn(core::future::pending());
        executor.poll_once();
        stale.wake();
        assert!(executor.pop_woken_task().is_none());
        assert_eq!(executor.tasks.len(), 1);
    }
}
