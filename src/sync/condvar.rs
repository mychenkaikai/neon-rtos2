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
//!         .spawn(move |_| {
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
//!         .spawn(move |_| {
//!             let mut guard = mutex_clone.lock().unwrap();
//!             while !*guard {
//!                 guard = condvar_clone.wait(guard).unwrap();
//!             }
//!             // 数据已准备好，处理数据...
//!         });
//! }
//! ```

use crate::compat::{Arc, VecDeque};
use crate::kernel::scheduler::Scheduler;
use crate::kernel::task::{Task, TaskState};
use crate::kernel::time::systick::Systick;
use crate::hal::trigger_schedule;
use crate::error::{Result, RtosError};
use crate::sync::signal::WaiterList;
use crate::sync::mutex::{Mutex, MutexGuard};
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Waker;
use spin::Mutex as SpinMutex;

/// 条件变量内部状态
struct CondVarInner {
    /// 同步���待者列表
    waiters: SpinMutex<WaiterList>,
    /// 异步等待者列表
    async_waiters: SpinMutex<VecDeque<Waker>>,
    /// 是否已关闭
    closed: AtomicBool,
}

impl CondVarInner {
    fn new() -> Self {
        Self {
            waiters: SpinMutex::new(WaiterList::new()),
            async_waiters: SpinMutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
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
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::CondVarClosed);
        }

        // 获取互斥锁的引用（在释放 guard 之前）
        let mutex = guard.mutex();
        
        // 获取当前任务 ID
        let task_id = Scheduler::get_current_task().get_taskid();

        // 将当前任务加入等待队列
        {
            let mut waiters = self.inner.waiters.lock();
            if !waiters.push(task_id) {
                return Err(RtosError::WaiterQueueFull);
            }
        }

        // 释放互斥锁（通过 unlock 方法，避免 drop 后无法访问 mutex）
        let mutex = MutexGuard::unlock(guard);

        // 阻塞当前任务
        let condvar_id = Arc::as_ptr(&self.inner) as usize;
        Scheduler::get_current_task().block(crate::sync::event::Event::CondVar(condvar_id));
        trigger_schedule();

        // 被唤醒后检查是否因为关闭而唤醒
        if self.inner.closed.load(Ordering::Acquire) {
            // 尝试重新获取锁后返回错误
            let _ = mutex.lock();
            return Err(RtosError::CondVarClosed);
        }

        // 重新获取互斥锁
        mutex.lock()
    }

    /// 带超时的等待条件变量
    ///
    /// # 参数
    /// - `guard`: 互斥锁守卫
    /// - `timeout_ms`: 超时时间（毫秒）
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
    /// let (new_guard, timed_out) = condvar.wait_timeout(guard, 1000).unwrap();
    /// if timed_out {
    ///     // 超时处理
    /// }
    /// guard = new_guard;
    /// ```
    pub fn wait_timeout<'a, T>(
        &self,
        guard: MutexGuard<'a, T>,
        timeout_ms: usize,
    ) -> Result<(MutexGuard<'a, T>, bool)> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::CondVarClosed);
        }

        let deadline = Systick::get_current_time() + timeout_ms;
        let task_id = Scheduler::get_current_task().get_taskid();

        // 将当前任务加入等待队列
        {
            let mut waiters = self.inner.waiters.lock();
            if !waiters.push(task_id) {
                return Err(RtosError::WaiterQueueFull);
            }
        }

        // 释放互斥锁
        let mutex = MutexGuard::unlock(guard);

        // 阻塞当前任务
        let condvar_id = Arc::as_ptr(&self.inner) as usize;
        Scheduler::get_current_task().block(crate::sync::event::Event::CondVar(condvar_id));
        trigger_schedule();

        // 检查是否超时
        let timed_out = Systick::get_current_time() >= deadline;

        // 如果超时，从等待队列中移除
        if timed_out {
            let mut waiters = self.inner.waiters.lock();
            waiters.remove(task_id);
        }

        // 检查是否因为关闭而唤醒
        if self.inner.closed.load(Ordering::Acquire) {
            let _ = mutex.lock();
            return Err(RtosError::CondVarClosed);
        }

        // 重新获取互斥锁
        let new_guard = mutex.lock()?;
        Ok((new_guard, timed_out))
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
    /// - `timeout_ms`: 超时时间（毫秒）
    /// - `condition`: 条件检查函数
    ///
    /// # 返回值
    /// - `Ok((MutexGuard, false))`: 条件满足
    /// - `Ok((MutexGuard, true))`: 超时
    /// - `Err(RtosError)`: 错误
    pub fn wait_while_timeout<'a, T, F>(
        &self,
        mut guard: MutexGuard<'a, T>,
        timeout_ms: usize,
        mut condition: F,
    ) -> Result<(MutexGuard<'a, T>, bool)>
    where
        F: FnMut(&T) -> bool,
    {
        let deadline = Systick::get_current_time() + timeout_ms;

        while condition(&*guard) {
            let remaining = deadline.saturating_sub(Systick::get_current_time());
            if remaining == 0 {
                return Ok((guard, true));
            }

            let (new_guard, timed_out) = self.wait_timeout(guard, remaining)?;
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
        if self.inner.closed.load(Ordering::Acquire) {
            return;
        }

        // 首先尝试唤醒同步等待者
        let task_id = {
            let mut waiters = self.inner.waiters.lock();
            waiters.pop_front()
        };

        if let Some(task_id) = task_id {
            Task::for_each(|task, id| {
                if id == task_id {
                    if let TaskState::Blocked(_) = task.get_state() {
                        task.ready();
                    }
                }
            });
            return;
        }

        // 然后尝试唤醒异步等待者
        let waker = {
            let mut async_waiters = self.inner.async_waiters.lock();
            async_waiters.pop_front()
        };

        if let Some(waker) = waker {
            waker.wake();
        }
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
        if self.inner.closed.load(Ordering::Acquire) {
            return;
        }

        // 唤醒所有同步等待者
        let task_ids: [Option<usize>; 16];
        {
            let mut waiters = self.inner.waiters.lock();
            task_ids = waiters.drain();
        }

        for task_id in task_ids.iter().filter_map(|&id| id) {
            Task::for_each(|task, id| {
                if id == task_id {
                    if let TaskState::Blocked(_) = task.get_state() {
                        task.ready();
                    }
                }
            });
        }

        // 唤醒所有异步等待者
        let async_wakers: VecDeque<Waker>;
        {
            let mut async_waiters = self.inner.async_waiters.lock();
            async_wakers = core::mem::take(&mut *async_waiters);
        }

        for waker in async_wakers {
            waker.wake();
        }
    }

    /// 关闭条件变量
    ///
    /// 关闭后，所有等待的任务会被唤醒并收到错误。
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        self.notify_all();
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// 获取等待者数量
    pub fn waiter_count(&self) -> usize {
        self.inner.waiters.lock().len() + self.inner.async_waiters.lock().len()
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

/// 条件变量的异步等待 Future
pub struct CondVarFuture<'a, T> {
    condvar: &'a CondVar,
    mutex: &'a Mutex<T>,
    registered: bool,
}

impl<'a, T> CondVarFuture<'a, T> {
    fn new(condvar: &'a CondVar, mutex: &'a Mutex<T>) -> Self {
        Self {
            condvar,
            mutex,
            registered: false,
        }
    }
}

impl<'a, T> core::future::Future for CondVarFuture<'a, T> {
    type Output = Result<()>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if self.condvar.inner.closed.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::CondVarClosed));
        }

        if !self.registered {
            let mut async_waiters = self.condvar.inner.async_waiters.lock();
            async_waiters.push_back(cx.waker().clone());
            self.registered = true;
            return core::task::Poll::Pending;
        }

        // 被唤醒，返回 Ready
        core::task::Poll::Ready(Ok(()))
    }
}

impl CondVar {
    /// 异步等待条件变量
    ///
    /// 注意：这个方法需要手动管理锁的释放和重新获取。
    /// 建议使用 `wait()` 方法进行同步等待。
    pub fn wait_async<'a, T>(&'a self, mutex: &'a Mutex<T>) -> CondVarFuture<'a, T> {
        CondVarFuture::new(self, mutex)
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
        crate::kernel::task::Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let condvar = CondVar::new();
        
        assert!(!condvar.is_closed());
        assert_eq!(condvar.waiter_count(), 0);
    }

    #[test]
    #[serial]
    fn test_condvar_v2_clone() {
        kernel_init();
        crate::kernel::task::Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let condvar1 = CondVar::new();
        let condvar2 = condvar1.clone();
        
        assert_eq!(condvar1.id(), condvar2.id());
    }

    #[test]
    #[serial]
    fn test_condvar_v2_notify_without_waiters() {
        kernel_init();
        crate::kernel::task::Task::new("test", |_| {}).unwrap();
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
        crate::kernel::task::Task::new("test", |_| {}).unwrap();
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
        crate::kernel::task::Task::new("test", |_| {}).unwrap();
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
        crate::kernel::task::Task::new("test", |_| {}).unwrap();
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
        crate::kernel::task::Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let cv = condvar();
        assert!(!cv.is_closed());
    }
}

