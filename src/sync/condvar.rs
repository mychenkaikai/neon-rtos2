//! # CondVar - 支持闭包传递的条件变量
//!
//! 基于 Arc 的条件变量实现，无需全局变量，可以通过闭包传递。
//! 条件变量用于在满足特定条件时唤醒等待的任务。
//!
//! ## 设计思路
//!
//! 条件变量通常与互斥锁配合使用，用于实现复杂的同步模式。
//! 任务可以在持有锁的情况下等待条件变量，等待时会自动释放锁，
//! 被唤醒后会自动重新获取锁。
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::{Mutex, CondVar};
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     let mutex = Mutex::new(false); // 条件：数据是否准备好
//!     let condvar = CondVar::new();
//!     
//!     let mutex_clone = mutex.clone();
//!     let condvar_clone = condvar.clone();
//!     
//!     // 生产者
//!     Task::builder("producer")
//!         .spawn(move || {
//!             // 准备数据...
//!             {
//!                 let mut guard = mutex.lock().unwrap();
//!                 *guard = true; // 数据准备好了
//!             }
//!             condvar.notify_one(); // 通知消费者
//!         });
//!     
//!     // 消费者
//!     Task::builder("consumer")
//!         .spawn(move || {
//!             let mut guard = mutex_clone.lock().unwrap();
//!             while !*guard {
//!                 guard = condvar_clone.wait(guard).unwrap();
//!             }
//!             // 数据已准备好，处理数据...
//!         });
//! }
//! ```

use crate::kernel::time::systick::Ticks;
use crate::compat::Arc;
use crate::error::{Result, RtosError};
use crate::hal::trigger_schedule;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::{systick::{Systick, Deadline}, timer::Timer};
use crate::sync::{
    event::Event,
    mutex::{Mutex, MutexGuard},
    signal::WaiterList,
    wait::{AsyncWaiters},
};
use core::cell::RefCell;
use core::task::{Poll, Waker};

struct CondVarInner {
    state: critical_section::Mutex<RefCell<CondVarState>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskWait {
    ReleasingMutex,
    Parked,
    Notified,
}

struct CondVarState {
    tasks: WaiterList,
    phase: [TaskWait; crate::config::MAX_TASKS],
    futures: AsyncWaiters,
    broadcast_epoch: u64,
    closed: bool,
}

impl CondVarState {
    fn notify_task(&mut self, id: usize, event: Event) -> bool {
        // A task delivering mutex wakeups is still runnable. Remember the notification
        // so it skips parking. Once parked, only its matching event may be woken.
        if self.phase[id] == TaskWait::ReleasingMutex || Event::wake_task_by_id_if(id, event) {
            self.phase[id] = TaskWait::Notified;
            true
        } else {
            false
        }
    }
}

impl CondVarInner {
    fn new() -> Self {
        Self {
            state: critical_section::Mutex::new(RefCell::new(CondVarState {
                tasks: WaiterList::new(),
                phase: [TaskWait::Parked; crate::config::MAX_TASKS],
                futures: AsyncWaiters::new(),
                broadcast_epoch: 0,
                closed: false,
            })),
        }
    }
}

/// 可克隆、可传递的条件变量
///
/// 条件变量用于在满足特定条件时唤醒等待的任务。
/// 通常与 `Mutex` 配合使用。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::{Mutex, CondVar};
///
/// let mutex = Mutex::new(0);
/// let condvar = CondVar::new();
///
/// // 等待条件
/// let mut guard = mutex.lock().unwrap();
/// while *guard < 10 {
///     guard = condvar.wait(guard).unwrap();
/// }
/// ```
#[derive(Clone)]
pub struct CondVar {
    inner: Arc<CondVarInner>,
}

impl CondVar {
    /// 创建新的条件变量
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::CondVar;
    ///
    /// let condvar = CondVar::new();
    /// ```
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CondVarInner::new()),
        }
    }

    /// 等待条件变量
    ///
    /// 当前任务会被阻塞，直到被 `notify_one()` 或 `notify_all()` 唤醒。
    /// 等待期间会自动释放互斥锁，被唤醒后会自动重新获取锁。
    ///
    /// # 参数
    /// - `guard`: 互斥锁守卫
    ///
    /// # 返回值
    /// - `Ok(MutexGuard)`: 成功等待并重新获取锁
    /// - `Err(RtosError::CondVarClosed)`: 条件变量已关闭
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::{Mutex, CondVar};
    ///
    /// let mutex = Mutex::new(false);
    /// let condvar = CondVar::new();
    ///
    /// let mut guard = mutex.lock().unwrap();
    /// while !*guard {
    ///     guard = condvar.wait(guard).unwrap();
    /// }
    /// ```
    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> Result<MutexGuard<'a, T>> {
        self.wait_until(guard, None).map(|(guard, _)| guard)
    }

    /// 带超时的等待条件变量
    ///
    /// # 参数
    /// - `guard`: 互斥锁守卫
    /// - `timeout`: 超时时间（Duration 或 Ticks）
    ///
    /// # 返回值
    /// - `Ok((MutexGuard, false))`: 成功等待并重新获取锁
    /// - `Ok((MutexGuard, true))`: 超时，但仍重新获取了锁
    /// - `Err(RtosError)`: 错误
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::{Mutex, CondVar};
    ///
    /// let mutex = Mutex::new(false);
    /// let condvar = CondVar::new();
    ///
    /// let mut guard = mutex.lock().unwrap();
    /// let (new_guard, timed_out) = condvar.wait_timeout(guard, neon_rtos2::prelude::Ticks(1000)).unwrap();
    /// if timed_out {
    ///     // 超时处理
    /// }
    /// guard = new_guard;
    /// ```
    pub fn wait_timeout<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout: impl Into<Ticks>,
    ) -> Result<(MutexGuard<'a, T>, bool)> {
        self.wait_until(guard, Some(Deadline::after(timeout.into().0)))
    }

    fn wait_until<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        deadline: Option<Deadline>,
    ) -> Result<(MutexGuard<'a, T>, bool)> {
        let mutex = guard.mutex();
        let mut guard = Some(guard);
        let mut task = Scheduler::get_current_task();
        let event = Event::CondVar(self.id());
        let mut timer = None;
        let wake = critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            if state.closed {
                return Err(RtosError::CondVarClosed);
            }
            if let Some(deadline) = deadline {
                if deadline.elapsed() {
                    return Ok(None);
                }
                timer = Some(Timer::register_task_timeout(deadline, task, event)?);
            }
            if !state.tasks.push(task.0) {
                return Err(RtosError::WaiterQueueFull);
            }
            state.phase[task.0] = TaskWait::ReleasingMutex;
            Ok(Some(guard.take().unwrap().unlock_deferred(cs)))
        })?;
        let Some(wakers) = wake else {
            return Ok((guard.unwrap(), true));
        };
        // A notifier may itself be an async waiter on this mutex. Deliver its waker
        // while this task is runnable, then recheck the notification before parking.
        wakers.wake();
        let parked = critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            if state.closed
                || state.phase[task.0] == TaskWait::Notified
                || deadline.is_some_and(|end| end.elapsed())
            {
                return false;
            }
            state.phase[task.0] = TaskWait::Parked;
            task.block(event);
            true
        });
        if parked {
            trigger_schedule();
        }

        let timed_out = critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            state.tasks.remove(task.0);
            if state.closed {
                return Err(RtosError::CondVarClosed);
            }
            Ok(state.phase[task.0] != TaskWait::Notified
                && deadline.is_some_and(|end| end.elapsed()))
        })?;
        drop(timer);
        Ok((mutex.lock()?, timed_out))
    }

    /// 带条件的等待
    ///
    /// 等待直到条件满足。这是一个便捷方法，自动处理虚假唤醒。
    ///
    /// # 参数
    /// - `guard`: 互斥锁守卫
    /// - `condition`: 条件检查函数
    ///
    /// # 返回值
    /// - `Ok(MutexGuard)`: 条件满足，返回锁守卫
    /// - `Err(RtosError)`: 错误
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::{Mutex, CondVar};
    ///
    /// let mutex = Mutex::new(0i32);
    /// let condvar = CondVar::new();
    ///
    /// let guard = mutex.lock().unwrap();
    /// let guard = condvar.wait_while(guard, |value| *value < 10).unwrap();
    /// // 现在 *guard >= 10
    /// ```
    pub fn wait_while<'a, T, F>(
        &self,
        mut guard: MutexGuard<'a, T>,
        mut condition: F,
    ) -> Result<MutexGuard<'a, T>>
    where
        F: FnMut(&T) -> bool,
    {
        while condition(&*guard) {
            guard = self.wait(guard)?;
        }
        Ok(guard)
    }

    /// 带条件和超时的等待
    ///
    /// # 参数
    /// - `guard`: 互斥锁守卫
    /// - `timeout`: 超时时间（Duration 或 Ticks）
    /// - `condition`: 条件检查函数
    ///
    /// # 返回值
    /// - `Ok((MutexGuard, false))`: 条件满足
    /// - `Ok((MutexGuard, true))`: 超时
    /// - `Err(RtosError)`: 错误
    pub fn wait_while_timeout<'a, T, F>(
        &self,
        mut guard: MutexGuard<'a, T>,
        timeout: impl Into<Ticks>,
        mut condition: F,
    ) -> Result<(MutexGuard<'a, T>, bool)>
    where
        F: FnMut(&T) -> bool,
    {
        let deadline = Deadline::after(timeout.into().0);

        while condition(&*guard) {
            let remaining = deadline.remaining_at(Systick::now());
            if remaining == 0 {
                return Ok((guard, true));
            }

            let (new_guard, timed_out) = self.wait_until(guard, Some(deadline))?;
            guard = new_guard;

            if timed_out {
                return Ok((guard, true));
            }
        }

        Ok((guard, false))
    }

    /// 唤醒一个等待的任务
    ///
    /// 如果有任务在等待，唤醒其中一个（FIFO 顺序）。
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::CondVar;
    ///
    /// let condvar = CondVar::new();
    /// condvar.notify_one();
    /// ```
    pub fn notify_one(&self) {
        let waker = critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            if state.closed {
                return None;
            }
            self.take_one(&mut state)
        });
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn take_one(&self, state: &mut CondVarState) -> Option<Waker> {
        while let Some(id) = state.tasks.pop_front() {
            if state.notify_task(id, Event::CondVar(self.id())) {
                return None;
            }
        }
        state.futures.pop()
    }

    /// 唤醒所有等待的任务
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::CondVar;
    ///
    /// let condvar = CondVar::new();
    /// condvar.notify_all();
    /// ```
    pub fn notify_all(&self) {
        self.broadcast(false);
    }

    /// 关闭条件变量
    ///
    /// 关闭后，所有等待的任务会被唤醒并收到错误。
    pub fn close(&self) {
        self.broadcast(true);
    }

    fn broadcast(&self, close: bool) {
        let wakers = critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            state.closed |= close;
            state.broadcast_epoch = state.broadcast_epoch.wrapping_add(1);
            for id in state.tasks.drain().into_iter().flatten() {
                state.notify_task(id, Event::CondVar(self.id()));
            }
            state.futures.drain()
        });
        for (_, waker) in wakers {
            waker.wake();
        }
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        critical_section::with(|cs| self.inner.state.borrow_ref(cs).closed)
    }

    /// 获取等待者数量
    pub fn waiter_count(&self) -> usize {
        critical_section::with(|cs| {
            let state = self.inner.state.borrow_ref(cs);
            state.tasks.len() + state.futures.len()
        })
    }

    /// 获取唯一标识（用于调试）
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }
}

impl Default for CondVar {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for CondVar {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CondVar")
            .field("id", &self.id())
            .field("waiters", &self.waiter_count())
            .field("closed", &self.is_closed())
            .finish()
    }
}

// ============================================================================
// 异步支持
// ============================================================================

// Keep a selected notify_one transferable until the waiter has reacquired
// the mutex. Dropping the future at either await point runs this registration.
struct Notification<'a> {
    condvar: &'a CondVar,
    token: Option<u64>,
    epoch: u64,
}

impl Notification<'_> {
    fn poll(&mut self, cx: &mut core::task::Context<'_>) -> Poll<Result<()>> {
        let mut incoming = Some(cx.waker().clone());
        let (poll, old) = critical_section::with(|cs| {
            let mut state = self.condvar.inner.state.borrow_ref_mut(cs);
            if state.closed {
                return (
                    Poll::Ready(Err(RtosError::CondVarClosed)),
                    state.futures.remove(self.token.take()),
                );
            }
            if self.token.is_some_and(|id| !state.futures.contains(id)) {
                return (Poll::Ready(Ok(())), None);
            }
            if !state
                .futures
                .has_room(self.token, crate::config::MAX_ASYNC_WAITERS)
            {
                return (Poll::Ready(Err(RtosError::WaiterQueueFull)), None);
            }
            self.epoch = state.broadcast_epoch;
            (
                Poll::Pending,
                state
                    .futures
                    .register(&mut self.token, incoming.take().unwrap()),
            )
        });
        drop(old);
        poll
    }
}

impl Drop for Notification<'_> {
    fn drop(&mut self) {
        let Some(id) = self.token.take() else {
            return;
        };
        let (removed, next) = critical_section::with(|cs| {
            let mut state = self.condvar.inner.state.borrow_ref_mut(cs);
            let old = state.futures.remove(Some(id));
            let next = if old.is_none() && !state.closed && self.epoch == state.broadcast_epoch {
                self.condvar.take_one(&mut state)
            } else {
                None
            };
            (old, next)
        });
        drop(removed);
        if let Some(waker) = next {
            waker.wake();
        }
    }
}

impl CondVar {
    /// Register while holding the mutex, release it while waiting, and
    /// return a reacquired guard. Cancellation releases both registrations.
    pub async fn wait_async<'a, T>(&self, guard: MutexGuard<'a, T>) -> Result<MutexGuard<'a, T>> {
        let mutex = guard.mutex();
        let mut held = Some(guard);
        let mut notification = Notification { condvar: self, token: None, epoch: 0 };
        core::future::poll_fn(|cx| {
            let result = notification.poll(cx);
            drop(held.take());
            result
        }).await?;
        let guard = mutex.lock_async().await?;
        notification.token = None;
        Ok(guard)
    }

    /// Recheck the predicate with the lock held after each notification.
    pub async fn wait_while_async<'a, T>(
        &self,
        mut guard: MutexGuard<'a, T>,
        mut predicate: impl FnMut(&T) -> bool,
    ) -> Result<MutexGuard<'a, T>> {
        while predicate(&guard) {
            guard = self.wait_async(guard).await?;
        }
        Ok(guard)
    }
}

// ============================================================================
// 便捷函数
// ============================================================================

/// 创建一个新的条件变量
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::condvar::condvar;
///
/// let cv = condvar();
/// ```
pub fn condvar() -> CondVar {
    CondVar::new()
}

/// 创建配对的互斥锁和条件变量
///
/// 这是一个便捷函数，用于创建常见的互斥锁+条件变量组合。
///
/// # 参数
/// - `data`: 互斥锁保护的数据
///
/// # 返回值
/// 返回 `(Mutex<T>, CondVar)` 元组
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::condvar::mutex_condvar_pair;
///
/// let (mutex, condvar) = mutex_condvar_pair(0i32);
///
/// // 等待条件
/// let mut guard = mutex.lock().unwrap();
/// while *guard < 10 {
///     guard = condvar.wait(guard).unwrap();
/// }
/// ```
pub fn mutex_condvar_pair<T>(data: T) -> (Mutex<T>, CondVar) {
    (Mutex::new(data), CondVar::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_condvar_v2_basic() {
        kernel_init();
        crate::kernel::task::Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let condvar = CondVar::new();

        assert!(!condvar.is_closed());
        assert_eq!(condvar.waiter_count(), 0);
    }

    #[test]
    #[serial]
    fn test_condvar_v2_clone() {
        kernel_init();
        crate::kernel::task::Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let condvar1 = CondVar::new();
        let condvar2 = condvar1.clone();

        assert_eq!(condvar1.id(), condvar2.id());
    }

    #[test]
    #[serial]
    fn test_condvar_v2_notify_without_waiters() {
        kernel_init();
        crate::kernel::task::Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let condvar = CondVar::new();

        // 没有��待者时 notify 不应该出错
        condvar.notify_one();
        condvar.notify_all();
    }

    #[test]
    #[serial]
    fn test_condvar_v2_close() {
        kernel_init();
        crate::kernel::task::Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let condvar = CondVar::new();

        assert!(!condvar.is_closed());
        condvar.close();
        assert!(condvar.is_closed());
    }

    #[test]
    #[serial]
    fn test_condvar_v2_debug() {
        kernel_init();
        crate::kernel::task::Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let condvar = CondVar::new();

        let debug_str = format!("{:?}", condvar);
        assert!(debug_str.contains("CondVar"));
        assert!(debug_str.contains("waiters"));
        assert!(debug_str.contains("closed"));
    }

    #[test]
    #[serial]
    fn test_mutex_condvar_pair() {
        kernel_init();
        crate::kernel::task::Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let (mutex, condvar) = mutex_condvar_pair(42);

        // 验证互斥锁
        {
            let guard = mutex.lock().unwrap();
            assert_eq!(*guard, 42);
        }

        // 验证条件变量
        assert!(!condvar.is_closed());
    }

    #[test]
    #[serial]
    fn test_condvar_convenience_function() {
        kernel_init();
        crate::kernel::task::Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let cv = condvar();
        assert!(!cv.is_closed());
    }
}
