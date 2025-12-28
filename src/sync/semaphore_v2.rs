//! # Semaphore V2 - 支持闭包传递的计数信号量
//!
//! 基于 Arc 的计数信号量实现，无需全局变量，可以通过闭包传递。
//! 与 SignalV2 不同，SemaphoreV2 支持设置最大计数限制。
//!
//! ## 设计思路
//!
//! 计数信号量用于控制对有限资源的并发访问。
//! 例如：限制同时访问某个资源的任务数量。
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::SemaphoreV2;
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     // 创建一个最多允许 3 个任务同时访问的信号量
//!     let sem = SemaphoreV2::new(3);
//!     
//!     for i in 0..5 {
//!         let sem_clone = sem.clone();
//!         Task::builder(&format!("worker_{}", i))
//!             .spawn(move |_| {
//!                 // 获取许可
//!                 sem_clone.acquire().unwrap();
//!                 // 访问受限资源...
//!                 // 释放许可
//!                 sem_clone.release();
//!             });
//!     }
//! }
//! ```

use crate::compat::{Arc, VecDeque};
use crate::kernel::scheduler::Scheduler;
use crate::kernel::task::{Task, TaskState};
use crate::kernel::time::systick::Systick;
use crate::hal::trigger_schedule;
use crate::error::{Result, RtosError};
use crate::sync::signal_v2::WaiterList;
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use core::task::Waker;
use spin::Mutex;

/// 信号量内部状态
struct SemaphoreInner {
    /// 当前可用许可数
    permits: AtomicUsize,
    /// 最大许可数（0 表示无限制）
    max_permits: usize,
    /// 同步等待者列表
    waiters: Mutex<WaiterList>,
    /// 异步等待者列表
    async_waiters: Mutex<VecDeque<Waker>>,
    /// 是否已关闭
    closed: AtomicBool,
}

impl SemaphoreInner {
    fn new(initial_permits: usize, max_permits: usize) -> Self {
        Self {
            permits: AtomicUsize::new(initial_permits),
            max_permits,
            waiters: Mutex::new(WaiterList::new()),
            async_waiters: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
        }
    }
}

/// 可克隆、可传递的计数信号量
///
/// 用于控制对有限资源的并发访问。
/// 类似于 `std::sync::Semaphore`（虽然标准库没有），但可以 clone 后 move 到不同任务中。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::SemaphoreV2;
///
/// // 创建一个有 3 个许可的信号量
/// let sem = SemaphoreV2::new(3);
///
/// // 获取许可
/// sem.acquire().unwrap();
///
/// // 释放许可
/// sem.release();
/// ```
#[derive(Clone)]
pub struct SemaphoreV2 {
    inner: Arc<SemaphoreInner>,
}

impl SemaphoreV2 {
    /// 创建新的计数信号量
    ///
    /// # 参数
    /// - `initial_permits`: 初始许可数量
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::SemaphoreV2;
    ///
    /// let sem = SemaphoreV2::new(5); // 5 个初始许可
    /// ```
    pub fn new(initial_permits: usize) -> Self {
        Self {
            inner: Arc::new(SemaphoreInner::new(initial_permits, 0)),
        }
    }

    /// 创建带最大限制的计数信号量
    ///
    /// # 参数
    /// - `initial_permits`: 初始许可数量
    /// - `max_permits`: 最大许可数量
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::SemaphoreV2;
    ///
    /// // 初始 3 个许可，最多 5 个
    /// let sem = SemaphoreV2::with_max(3, 5);
    /// ```
    pub fn with_max(initial_permits: usize, max_permits: usize) -> Self {
        let permits = initial_permits.min(max_permits);
        Self {
            inner: Arc::new(SemaphoreInner::new(permits, max_permits)),
        }
    }

    /// 获取一个许可
    ///
    /// 如果没有可用许可，当前任务会被阻塞直到有许可可用。
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取许可
    /// - `Err(RtosError::SemaphoreClosed)`: 信号量已关闭
    ///
    /// # 示例
    /// ```rust,no_run
    /// # use neon_rtos2::sync::SemaphoreV2;
    /// let sem = SemaphoreV2::new(1);
    /// sem.acquire().unwrap();
    /// // 使用资源...
    /// sem.release();
    /// ```
    pub fn acquire(&self) -> Result<()> {
        self.acquire_many(1)
    }

    /// 获取多个许可
    ///
    /// # 参数
    /// - `n`: 要获取的许可数量
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取许可
    /// - `Err(RtosError::SemaphoreClosed)`: 信号量已关闭
    pub fn acquire_many(&self, n: usize) -> Result<()> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SemaphoreClosed);
        }

        loop {
            let current = self.inner.permits.load(Ordering::Acquire);
            if current >= n {
                if self.inner.permits.compare_exchange(
                    current,
                    current - n,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(());
                }
                continue;
            }

            // 许可不足，需要阻塞
            let task_id = Scheduler::get_current_task().get_taskid();

            {
                let mut waiters = self.inner.waiters.lock();
                
                if self.inner.closed.load(Ordering::Acquire) {
                    return Err(RtosError::SemaphoreClosed);
                }
                
                if !waiters.push(task_id) {
                    return Err(RtosError::WaiterQueueFull);
                }
            }

            let sem_id = Arc::as_ptr(&self.inner) as usize;
            Scheduler::get_current_task().block(crate::sync::event::Event::Signal(sem_id));
            trigger_schedule();

            if self.inner.closed.load(Ordering::Acquire) {
                return Err(RtosError::SemaphoreClosed);
            }
        }
    }

    /// 尝试获取一个许可（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(true)`: 成功获取许可
    /// - `Ok(false)`: 没有可用许可
    /// - `Err(RtosError::SemaphoreClosed)`: 信号量已关闭
    pub fn try_acquire(&self) -> Result<bool> {
        self.try_acquire_many(1)
    }

    /// 尝试获取多个许可（非阻塞）
    pub fn try_acquire_many(&self, n: usize) -> Result<bool> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SemaphoreClosed);
        }

        loop {
            let current = self.inner.permits.load(Ordering::Acquire);
            if current >= n {
                if self.inner.permits.compare_exchange(
                    current,
                    current - n,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(true);
                }
                continue;
            }
            return Ok(false);
        }
    }

    /// 带超时的获取许可
    ///
    /// # 参数
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取许可
    /// - `Err(RtosError::Timeout)`: 等待超时
    /// - `Err(RtosError::SemaphoreClosed)`: 信号量已关闭
    pub fn acquire_timeout(&self, timeout_ms: usize) -> Result<()> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SemaphoreClosed);
        }

        let deadline = Systick::get_current_time() + timeout_ms;

        loop {
            let current = self.inner.permits.load(Ordering::Acquire);
            if current > 0 {
                if self.inner.permits.compare_exchange(
                    current,
                    current - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(());
                }
                continue;
            }

            if Systick::get_current_time() >= deadline {
                return Err(RtosError::Timeout);
            }

            let task_id = Scheduler::get_current_task().get_taskid();

            {
                let mut waiters = self.inner.waiters.lock();
                
                if self.inner.closed.load(Ordering::Acquire) {
                    return Err(RtosError::SemaphoreClosed);
                }
                
                if !waiters.push(task_id) {
                    return Err(RtosError::WaiterQueueFull);
                }
            }

            let sem_id = Arc::as_ptr(&self.inner) as usize;
            Scheduler::get_current_task().block(crate::sync::event::Event::Signal(sem_id));
            trigger_schedule();

            if self.inner.closed.load(Ordering::Acquire) {
                return Err(RtosError::SemaphoreClosed);
            }

            if Systick::get_current_time() >= deadline {
                let mut waiters = self.inner.waiters.lock();
                waiters.remove(task_id);
                return Err(RtosError::Timeout);
            }
        }
    }

    /// 释放一个许可
    ///
    /// 唤醒一个等待中的任务（如果有）。
    ///
    /// # 返回值
    /// - `Ok(())`: 成功释放
    /// - `Err(RtosError::SemaphoreOverflow)`: 超过最大许可数
    pub fn release(&self) -> Result<()> {
        self.release_many(1)
    }

    /// 释放多个许可
    pub fn release_many(&self, n: usize) -> Result<()> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Ok(()); // 已关闭，忽略释放
        }

        // 检查是否会超过最大限制
        if self.inner.max_permits > 0 {
            let current = self.inner.permits.load(Ordering::Acquire);
            if current + n > self.inner.max_permits {
                return Err(RtosError::SemaphoreOverflow);
            }
        }

        // 增加许可计数
        self.inner.permits.fetch_add(n, Ordering::Release);

        // 唤醒等待者
        for _ in 0..n {
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
                continue;
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

        Ok(())
    }

    /// ���闭信号量
    ///
    /// 关闭后，所有等待的任务会被唤醒并收到错误。
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        
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

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// 获取当前可用许可数
    pub fn available_permits(&self) -> usize {
        self.inner.permits.load(Ordering::Relaxed)
    }

    /// 获取最大许可数（0 表示无限制）
    pub fn max_permits(&self) -> usize {
        self.inner.max_permits
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

impl Default for SemaphoreV2 {
    fn default() -> Self {
        Self::new(1)
    }
}

impl core::fmt::Debug for SemaphoreV2 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SemaphoreV2")
            .field("id", &self.id())
            .field("permits", &self.available_permits())
            .field("max_permits", &self.max_permits())
            .field("waiters", &self.waiter_count())
            .field("closed", &self.is_closed())
            .finish()
    }
}

// ============================================================================
// RAII 许可守卫
// ============================================================================

/// RAII 风格的许可守卫
///
/// 持有 `SemaphorePermit` 期间，许可被占用。
/// 当 `SemaphorePermit` 离开作用域被 drop 时，许可会自动释放。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::SemaphoreV2;
///
/// let sem = SemaphoreV2::new(1);
/// {
///     let _permit = sem.acquire_permit().unwrap();
///     // 使用资源...
/// } // 自动释放许可
/// ```
pub struct SemaphorePermit<'a> {
    semaphore: &'a SemaphoreV2,
    permits: usize,
}

impl<'a> SemaphorePermit<'a> {
    /// 忘记这个许可，不释放
    ///
    /// 调用此方法后，许可不会在 drop 时释放。
    pub fn forget(self) {
        core::mem::forget(self);
    }
}

impl Drop for SemaphorePermit<'_> {
    fn drop(&mut self) {
        let _ = self.semaphore.release_many(self.permits);
    }
}

impl core::fmt::Debug for SemaphorePermit<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SemaphorePermit")
            .field("permits", &self.permits)
            .finish()
    }
}

// ============================================================================
// OwnedSemaphorePermit - 拥有所有权的许可守卫
// ============================================================================

/// 拥有所有权的信号量许可
///
/// 与 `SemaphorePermit` 不同，`OwnedSemaphorePermit` 持有 `Arc` 的所有权，
/// 因此可以被 move 到其他任务中或存储在结构体中。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::SemaphoreV2;
///
/// let sem = SemaphoreV2::new(3);
/// let permit = sem.acquire_owned().unwrap();
/// 
/// // permit 可以被存储或移动
/// drop(permit); // 显式释放
/// ```
pub struct OwnedSemaphorePermit {
    semaphore: Arc<SemaphoreInner>,
    permits: usize,
}

// OwnedSemaphorePermit 可以 Send
unsafe impl Send for OwnedSemaphorePermit {}
unsafe impl Sync for OwnedSemaphorePermit {}

impl OwnedSemaphorePermit {
    /// 忘记这个许可，不释放
    pub fn forget(self) {
        core::mem::forget(self);
    }

    /// 获取持有的许可数量
    pub fn permits(&self) -> usize {
        self.permits
    }

    /// 合并另一个许可到当前许可
    ///
    /// 两个许可必须来自同一个信号量。
    pub fn merge(&mut self, other: OwnedSemaphorePermit) {
        assert_eq!(
            Arc::as_ptr(&self.semaphore),
            Arc::as_ptr(&other.semaphore),
            "Cannot merge permits from different semaphores"
        );
        self.permits += other.permits;
        core::mem::forget(other);
    }

    /// 分割出指定数量的许可
    ///
    /// # 参数
    /// - `n`: 要分割出的许可数量
    ///
    /// # 返回值
    /// - `Some(OwnedSemaphorePermit)`: 成功分割
    /// - `None`: 许可数量不足
    pub fn split(&mut self, n: usize) -> Option<OwnedSemaphorePermit> {
        if n > self.permits {
            return None;
        }
        self.permits -= n;
        Some(OwnedSemaphorePermit {
            semaphore: Arc::clone(&self.semaphore),
            permits: n,
        })
    }
}

impl Drop for OwnedSemaphorePermit {
    fn drop(&mut self) {
        if self.permits > 0 {
            // 增加许可计数
            self.semaphore.permits.fetch_add(self.permits, Ordering::Release);

            // 唤醒等待者
            for _ in 0..self.permits {
                let task_id = {
                    let mut waiters = self.semaphore.waiters.lock();
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
                    continue;
                }

                let waker = {
                    let mut async_waiters = self.semaphore.async_waiters.lock();
                    async_waiters.pop_front()
                };

                if let Some(waker) = waker {
                    waker.wake();
                }
            }
        }
    }
}

impl core::fmt::Debug for OwnedSemaphorePermit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OwnedSemaphorePermit")
            .field("permits", &self.permits)
            .finish()
    }
}

impl SemaphoreV2 {
    /// 获取一个拥有所有权的许可
    ///
    /// 返回的 `OwnedSemaphorePermit` 可以被 move 到其他地方。
    ///
    /// # 返回值
    /// - `Ok(OwnedSemaphorePermit)`: 成功获取许可
    /// - `Err(RtosError)`: 获取失败
    pub fn acquire_owned(&self) -> Result<OwnedSemaphorePermit> {
        self.acquire()?;
        Ok(OwnedSemaphorePermit {
            semaphore: Arc::clone(&self.inner),
            permits: 1,
        })
    }

    /// 获取多个拥有所有权的许可
    pub fn acquire_many_owned(&self, n: usize) -> Result<OwnedSemaphorePermit> {
        self.acquire_many(n)?;
        Ok(OwnedSemaphorePermit {
            semaphore: Arc::clone(&self.inner),
            permits: n,
        })
    }

    /// 尝试获取一个拥有所有权的许可（非阻塞）
    pub fn try_acquire_owned(&self) -> Result<Option<OwnedSemaphorePermit>> {
        if self.try_acquire()? {
            Ok(Some(OwnedSemaphorePermit {
                semaphore: Arc::clone(&self.inner),
                permits: 1,
            }))
        } else {
            Ok(None)
        }
    }

    /// 尝试获取多个拥有所有权的许可（非阻塞）
    pub fn try_acquire_many_owned(&self, n: usize) -> Result<Option<OwnedSemaphorePermit>> {
        if self.try_acquire_many(n)? {
            Ok(Some(OwnedSemaphorePermit {
                semaphore: Arc::clone(&self.inner),
                permits: n,
            }))
        } else {
            Ok(None)
        }
    }

    /// 带超时的获取拥有所有权的许可
    pub fn acquire_owned_timeout(&self, timeout_ms: usize) -> Result<OwnedSemaphorePermit> {
        self.acquire_timeout(timeout_ms)?;
        Ok(OwnedSemaphorePermit {
            semaphore: Arc::clone(&self.inner),
            permits: 1,
        })
    }
}

// ============================================================================
// 便捷函数
// ============================================================================

/// 创建一个新的计数信号量
///
/// # 参数
/// - `permits`: 初始许可数量
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::semaphore_v2::semaphore;
///
/// let sem = semaphore(5);
/// ```
pub fn semaphore(permits: usize) -> SemaphoreV2 {
    SemaphoreV2::new(permits)
}

/// 创建一个带最大限制的计数信号量
///
/// # 参数
/// - `initial`: 初始许可数量
/// - `max`: 最大许可数量
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::semaphore_v2::semaphore_with_max;
///
/// let sem = semaphore_with_max(3, 5);
/// ```
pub fn semaphore_with_max(initial: usize, max: usize) -> SemaphoreV2 {
    SemaphoreV2::with_max(initial, max)
}

/// 创建一个二值信号量（互斥信号量）
///
/// 二值信号量只有 0 和 1 两个状态，类似于互斥锁。
///
/// # 参数
/// - `available`: 初始是否可用
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::semaphore_v2::binary_semaphore;
///
/// let sem = binary_semaphore(true); // 初始可用
/// sem.acquire().unwrap();
/// // 现在不可用
/// sem.release().unwrap();
/// ```
pub fn binary_semaphore(available: bool) -> SemaphoreV2 {
    SemaphoreV2::with_max(if available { 1 } else { 0 }, 1)
}

impl SemaphoreV2 {
    /// 获取一个 RAII 风格的许可
    ///
    /// 返回的 `SemaphorePermit` 在 drop 时会自动释放许可。
    ///
    /// # 返回值
    /// - `Ok(SemaphorePermit)`: 成功获取许可
    /// - `Err(RtosError)`: 获取失败
    pub fn acquire_permit(&self) -> Result<SemaphorePermit<'_>> {
        self.acquire()?;
        Ok(SemaphorePermit {
            semaphore: self,
            permits: 1,
        })
    }

    /// 获取多个 RAII 风格的许可
    pub fn acquire_permits(&self, n: usize) -> Result<SemaphorePermit<'_>> {
        self.acquire_many(n)?;
        Ok(SemaphorePermit {
            semaphore: self,
            permits: n,
        })
    }

    /// 尝试获取一个 RAII 风格的许可（非阻塞）
    pub fn try_acquire_permit(&self) -> Result<Option<SemaphorePermit<'_>>> {
        if self.try_acquire()? {
            Ok(Some(SemaphorePermit {
                semaphore: self,
                permits: 1,
            }))
        } else {
            Ok(None)
        }
    }

    /// 带超时的获取 RAII 风格的许可
    pub fn acquire_permit_timeout(&self, timeout_ms: usize) -> Result<SemaphorePermit<'_>> {
        self.acquire_timeout(timeout_ms)?;
        Ok(SemaphorePermit {
            semaphore: self,
            permits: 1,
        })
    }
}

// ============================================================================
// 异步支持
// ============================================================================

/// 异步获取许可的 Future
pub struct SemaphoreAcquireFuture<'a> {
    semaphore: &'a SemaphoreV2,
    permits: usize,
    registered: bool,
}

impl<'a> SemaphoreAcquireFuture<'a> {
    fn new(semaphore: &'a SemaphoreV2, permits: usize) -> Self {
        Self {
            semaphore,
            permits,
            registered: false,
        }
    }
}

impl<'a> core::future::Future for SemaphoreAcquireFuture<'a> {
    type Output = Result<()>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        if self.semaphore.inner.closed.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::SemaphoreClosed));
        }

        loop {
            let current = self.semaphore.inner.permits.load(Ordering::Acquire);
            if current >= self.permits {
                if self.semaphore.inner.permits.compare_exchange(
                    current,
                    current - self.permits,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return core::task::Poll::Ready(Ok(()));
                }
                continue;
            }
            break;
        }

        if !self.registered {
            let mut async_waiters = self.semaphore.inner.async_waiters.lock();
            async_waiters.push_back(cx.waker().clone());
            self.registered = true;
        }

        core::task::Poll::Pending
    }
}

impl SemaphoreV2 {
    /// 异步获取一个许可
    pub fn acquire_async(&self) -> SemaphoreAcquireFuture<'_> {
        SemaphoreAcquireFuture::new(self, 1)
    }

    /// 异步获取多个许可
    pub fn acquire_many_async(&self, n: usize) -> SemaphoreAcquireFuture<'_> {
        SemaphoreAcquireFuture::new(self, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::task::Task;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_semaphore_v2_basic() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(3);
        
        assert_eq!(sem.available_permits(), 3);
        
        // 获取许可
        assert!(sem.try_acquire().unwrap());
        assert_eq!(sem.available_permits(), 2);
        
        assert!(sem.try_acquire().unwrap());
        assert_eq!(sem.available_permits(), 1);
        
        assert!(sem.try_acquire().unwrap());
        assert_eq!(sem.available_permits(), 0);
        
        // 没有许可了
        assert!(!sem.try_acquire().unwrap());
        
        // 释放许可
        sem.release().unwrap();
        assert_eq!(sem.available_permits(), 1);
        
        // 又可以获取了
        assert!(sem.try_acquire().unwrap());
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_with_max() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::with_max(2, 3);
        
        assert_eq!(sem.available_permits(), 2);
        assert_eq!(sem.max_permits(), 3);
        
        // 释放到最大
        sem.release().unwrap();
        assert_eq!(sem.available_permits(), 3);
        
        // 超过最大会失败
        assert!(sem.release().is_err());
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_clone() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem1 = SemaphoreV2::new(2);
        let sem2 = sem1.clone();
        
        assert_eq!(sem1.id(), sem2.id());
        
        sem1.try_acquire().unwrap();
        assert_eq!(sem2.available_permits(), 1);
        
        sem2.release().unwrap();
        assert_eq!(sem1.available_permits(), 2);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_permit() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(1);
        
        {
            let _permit = sem.acquire_permit().unwrap();
            assert_eq!(sem.available_permits(), 0);
        } // permit 被 drop，自动释放
        
        assert_eq!(sem.available_permits(), 1);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_close() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(1);
        
        sem.close();
        assert!(sem.is_closed());
        
        assert!(sem.try_acquire().is_err());
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_acquire_many() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(5);
        
        assert!(sem.try_acquire_many(3).unwrap());
        assert_eq!(sem.available_permits(), 2);
        
        assert!(!sem.try_acquire_many(3).unwrap()); // 不够
        
        sem.release_many(3).unwrap();
        assert_eq!(sem.available_permits(), 5);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_owned_permit() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(3);
        
        // 获取 owned permit
        let permit = sem.acquire_owned().unwrap();
        assert_eq!(sem.available_permits(), 2);
        assert_eq!(permit.permits(), 1);
        
        drop(permit);
        assert_eq!(sem.available_permits(), 3);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_owned_permit_many() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(5);
        
        let permit = sem.acquire_many_owned(3).unwrap();
        assert_eq!(sem.available_permits(), 2);
        assert_eq!(permit.permits(), 3);
        
        drop(permit);
        assert_eq!(sem.available_permits(), 5);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_owned_permit_merge() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(5);
        
        let mut permit1 = sem.acquire_many_owned(2).unwrap();
        let permit2 = sem.acquire_many_owned(2).unwrap();
        
        assert_eq!(sem.available_permits(), 1);
        assert_eq!(permit1.permits(), 2);
        assert_eq!(permit2.permits(), 2);
        
        // 合并许可
        permit1.merge(permit2);
        assert_eq!(permit1.permits(), 4);
        
        drop(permit1);
        assert_eq!(sem.available_permits(), 5);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_owned_permit_split() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(5);
        
        let mut permit = sem.acquire_many_owned(4).unwrap();
        assert_eq!(sem.available_permits(), 1);
        
        // 分割许可
        let split = permit.split(2).unwrap();
        assert_eq!(permit.permits(), 2);
        assert_eq!(split.permits(), 2);
        
        // 释放分割的许可
        drop(split);
        assert_eq!(sem.available_permits(), 3);
        
        // 释放剩余许可
        drop(permit);
        assert_eq!(sem.available_permits(), 5);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_owned_permit_forget() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(3);
        
        let permit = sem.acquire_owned().unwrap();
        assert_eq!(sem.available_permits(), 2);
        
        // 忘记许可（不释放）
        permit.forget();
        assert_eq!(sem.available_permits(), 2); // 仍然是 2
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_try_acquire_owned() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = SemaphoreV2::new(1);
        
        // 第一次应该成功
        let permit = sem.try_acquire_owned().unwrap();
        assert!(permit.is_some());
        
        // 第二次应该失败
        let permit2 = sem.try_acquire_owned().unwrap();
        assert!(permit2.is_none());
        
        // 释放后应该成功
        drop(permit);
        let permit3 = sem.try_acquire_owned().unwrap();
        assert!(permit3.is_some());
    }

    #[test]
    #[serial]
    fn test_binary_semaphore() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let sem = super::binary_semaphore(true);
        
        assert_eq!(sem.available_permits(), 1);
        assert_eq!(sem.max_permits(), 1);
        
        // 获取
        assert!(sem.try_acquire().unwrap());
        assert_eq!(sem.available_permits(), 0);
        
        // 不能再获取
        assert!(!sem.try_acquire().unwrap());
        
        // 释放
        sem.release().unwrap();
        assert_eq!(sem.available_permits(), 1);
        
        // 不能超过最大值
        assert!(sem.release().is_err());
    }

    #[test]
    #[serial]
    fn test_semaphore_convenience_functions() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        // 测试 semaphore() 函数
        let sem = super::semaphore(3);
        assert_eq!(sem.available_permits(), 3);
        
        // 测试 semaphore_with_max() 函数
        let sem2 = super::semaphore_with_max(2, 5);
        assert_eq!(sem2.available_permits(), 2);
        assert_eq!(sem2.max_permits(), 5);
    }
}

