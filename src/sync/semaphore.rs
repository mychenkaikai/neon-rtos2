//! # Semaphore - 支持闭包传递的计数信号量
//!
//! 基于 Arc 的计数信号量实现，无需全局变量，可以通过闭包传递。
//! 与 Signal 不同，Semaphore 支持设置最大计数限制。
//!
//! ## 设计思路
//!
//! 计数信号量用于控制对有限资源的并发访问。
//! 例如：限制同时访问某个资源的任务数量。
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::Semaphore;
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     // 创建一个最多允许 3 个任务同时访问的信号量
//!     let sem = Semaphore::new(3);
//!     
//!     for i in 0..5 {
//!         let sem_clone = sem.clone();
//!         Task::builder("worker")
//!             .spawn(move || {
//!                 // 获取许可
//!                 sem_clone.acquire().unwrap();
//!                 // 访问受限资源...
//!                 // 释放许可
//!                 sem_clone.release();
//!             });
//!     }
//! }
//! ```

use crate::kernel::time::systick::Ticks;
use crate::kernel::time::systick::Deadline;
use crate::compat::Arc;
use crate::error::{Result, RtosError};
use crate::sync::event::Event;
use crate::sync::wait::{Registration, WaitQueue, WakeBatch};
use core::cell::Cell;
use critical_section::Mutex;

struct SemaphoreInner {
    permits: Mutex<Cell<usize>>,
    closed: Mutex<Cell<bool>>,
    max_permits: usize,
    waiters: WaitQueue,
}

impl SemaphoreInner {
    fn new(initial_permits: usize, max_permits: usize) -> Self {
        Self {
            permits: Mutex::new(Cell::new(initial_permits)),
            max_permits,
            closed: Mutex::new(Cell::new(false)),
            waiters: WaitQueue::new(),
        }
    }

    fn release(&self, n: usize) -> Result<()> {
        let wakers = critical_section::with(|cs| {
            if self.closed.borrow(cs).get() || n == 0 {
                return Ok(WakeBatch::default());
            }
            let permits = self.permits.borrow(cs);
            let total = permits
                .get()
                .checked_add(n)
                .ok_or(RtosError::SemaphoreOverflow)?;
            if self.max_permits != 0 && total > self.max_permits {
                return Err(RtosError::SemaphoreOverflow);
            }
            permits.set(total);
            Ok(self
                .waiters
                .take_all(cs, Event::Signal(self as *const Self as usize)))
        })?;
        wakers.wake();
        Ok(())
    }
}

/// 可克隆、可传递的计数信号量
///
/// 用于控制对有限资源的并发访问。
/// 类似于 `std::sync::Semaphore`（虽然标准库没有），但可以 clone 后 move 到不同任务中。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::Semaphore;
///
/// // 创建一个有 3 个许可的信号量
/// let sem = Semaphore::new(3);
///
/// // 获取许可
/// sem.acquire().unwrap();
///
/// // 释放许可
/// sem.release();
/// ```
#[derive(Clone)]
pub struct Semaphore {
    inner: Arc<SemaphoreInner>,
}

impl Semaphore {
    /// 创建新的计数信号量
    ///
    /// # 参数
    /// - `initial_permits`: 初始许可数量
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Semaphore;
    ///
    /// let sem = Semaphore::new(5); // 5 个初始许可
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
    /// use neon_rtos2::sync::Semaphore;
    ///
    /// // 初始 3 个许可，最多 5 个
    /// let sem = Semaphore::with_max(3, 5);
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
    /// # use neon_rtos2::sync::Semaphore;
    /// let sem = Semaphore::new(1);
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
        self.acquire_until(n, None)
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
        critical_section::with(|cs| {
            if self.inner.closed.borrow(cs).get() {
                return Err(RtosError::SemaphoreClosed);
            }
            let permits = self.inner.permits.borrow(cs);
            if permits.get() < n {
                return Ok(false);
            }
            permits.set(permits.get() - n);
            Ok(true)
        })
    }

    /// 带超时的获取许可
    ///
    /// # 参数
    /// - `timeout`: 超时时间（Duration 或 Ticks）
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取许可
    /// - `Err(RtosError::Timeout)`: 等待超时
    /// - `Err(RtosError::SemaphoreClosed)`: 信号量已关闭
    pub fn acquire_timeout(&self, timeout: impl Into<Ticks>) -> Result<()> {
        self.acquire_until(1, Some(Deadline::after(timeout.into().0)))
    }

    fn attempt(&self, n: usize) -> Result<()> {
        if self.try_acquire_many(n)? {
            Ok(())
        } else {
            Err(RtosError::WouldBlock)
        }
    }

    fn acquire_until(&self, n: usize, deadline: Option<Deadline>) -> Result<()> {
        self.inner.waiters.wait_until(
            Event::Signal(self.id()),
            deadline,
            || self.attempt(n),
            || {},
        )
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
        self.inner.release(n)
    }

    /// ���闭信号量
    ///
    /// 关闭后，所有等待的任务会被唤醒并收到错误。
    pub fn close(&self) {
        critical_section::with(|cs| {
            self.inner.closed.borrow(cs).set(true);
            self.inner.waiters.take_all(cs, Event::Signal(self.id()))
        })
        .wake();
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        critical_section::with(|cs| self.inner.closed.borrow(cs).get())
    }

    /// 获取当前可用许可数
    pub fn available_permits(&self) -> usize {
        critical_section::with(|cs| self.inner.permits.borrow(cs).get())
    }

    /// 获取最大许可数（0 表示无限制）
    pub fn max_permits(&self) -> usize {
        self.inner.max_permits
    }

    /// 获取等待者数量
    pub fn waiter_count(&self) -> usize {
        let (tasks, futures) = self.inner.waiters.counts();
        tasks + futures
    }

    /// 获取唯一标识（用于调试）
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }
}

impl Default for Semaphore {
    fn default() -> Self {
        Self::new(1)
    }
}

impl core::fmt::Debug for Semaphore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Semaphore")
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
/// use neon_rtos2::sync::Semaphore;
///
/// let sem = Semaphore::new(1);
/// {
///     let _permit = sem.acquire_permit().unwrap();
///     // 使用资源...
/// } // 自动释放许可
/// ```
#[must_use = "keep the permit alive while using the resource"]
pub struct SemaphorePermit<'a> {
    semaphore: &'a Semaphore,
    permits: usize,
}

impl<'a> SemaphorePermit<'a> {
    /// 忘记这个许可，不释放
    ///
    /// 调用此方法后，许可不会在 drop 时释放。
    pub fn forget(mut self) {
        self.permits = 0;
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
/// use neon_rtos2::sync::Semaphore;
///
/// let sem = Semaphore::new(3);
/// let permit = sem.acquire_owned().unwrap();
///
/// // permit 可以被存储或移动
/// drop(permit); // 显式释放
/// ```
#[must_use = "keep the permit alive while using the resource"]
pub struct OwnedSemaphorePermit {
    semaphore: Arc<SemaphoreInner>,
    permits: usize,
}

// OwnedSemaphorePermit 可以 Send
unsafe impl Send for OwnedSemaphorePermit {}
unsafe impl Sync for OwnedSemaphorePermit {}

impl OwnedSemaphorePermit {
    /// 忘记这个许可，不释放
    pub fn forget(mut self) {
        self.permits = 0;
    }

    /// 获取持有的许可数量
    pub fn permits(&self) -> usize {
        self.permits
    }

    /// 合并另一个许可到当前许可
    ///
    /// 两个许可必须来自同一个信号量。
    /// On failure the argument retains all of its permits.
    pub fn try_merge(&mut self, mut other: Self) -> core::result::Result<(), Self> {
        if !Arc::ptr_eq(&self.semaphore, &other.semaphore) {
            return Err(other);
        }
        let Some(total) = self.permits.checked_add(other.permits) else { return Err(other) };
        self.permits = total;
        other.permits = 0;
        Ok(())
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
        let _ = self.semaphore.release(self.permits);
    }
}

impl core::fmt::Debug for OwnedSemaphorePermit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OwnedSemaphorePermit")
            .field("permits", &self.permits)
            .finish()
    }
}

impl Semaphore {
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
    pub fn acquire_owned_timeout(&self, timeout: impl Into<Ticks>) -> Result<OwnedSemaphorePermit> {
        self.acquire_timeout(timeout)?;
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
/// use neon_rtos2::sync::semaphore::semaphore;
///
/// let sem = semaphore(5);
/// ```
pub fn semaphore(permits: usize) -> Semaphore {
    Semaphore::new(permits)
}

/// 创建一个带最大限制的计数信号量
///
/// # 参数
/// - `initial`: 初始许可数量
/// - `max`: 最大许可数量
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::semaphore::semaphore_with_max;
///
/// let sem = semaphore_with_max(3, 5);
/// ```
pub fn semaphore_with_max(initial: usize, max: usize) -> Semaphore {
    Semaphore::with_max(initial, max)
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
/// use neon_rtos2::sync::semaphore::binary_semaphore;
///
/// let sem = binary_semaphore(true); // 初始可用
/// sem.acquire().unwrap();
/// // 现在不可用
/// sem.release().unwrap();
/// ```
pub fn binary_semaphore(available: bool) -> Semaphore {
    Semaphore::with_max(if available { 1 } else { 0 }, 1)
}

impl Semaphore {
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
    pub fn acquire_permit_timeout(&self, timeout: impl Into<Ticks>) -> Result<SemaphorePermit<'_>> {
        self.acquire_timeout(timeout)?;
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
    semaphore: &'a Semaphore,
    permits: usize,
    registration: Registration<'a>,
}

impl<'a> SemaphoreAcquireFuture<'a> {
    fn new(semaphore: &'a Semaphore, permits: usize) -> Self {
        Self {
            semaphore,
            permits,
            registration: semaphore.inner.waiters.registration(),
        }
    }
}

impl<'a> core::future::Future for SemaphoreAcquireFuture<'a> {
    type Output = Result<()>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let semaphore = self.semaphore;
        let permits = self.permits;
        self.registration
            .poll_until(cx, || semaphore.attempt(permits))
    }
}

impl Semaphore {
    /// 异步获取一个许可
    pub fn acquire_async(&self) -> SemaphoreAcquireFuture<'_> {
        SemaphoreAcquireFuture::new(self, 1)
    }

    /// Await a permit that returns to the semaphore when dropped.
    pub async fn acquire_permit_async(&self) -> Result<SemaphorePermit<'_>> {
        self.acquire_permits_async(1).await
    }

    pub async fn acquire_permits_async(&self, n: usize) -> Result<SemaphorePermit<'_>> {
        self.acquire_many_async(n).await?;
        Ok(SemaphorePermit { semaphore: self, permits: n })
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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(3);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::with_max(2, 3);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem1 = Semaphore::new(2);
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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(1);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(1);

        sem.close();
        assert!(sem.is_closed());

        assert!(sem.try_acquire().is_err());
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_acquire_many() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(5);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(3);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(5);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(5);

        let mut permit1 = sem.acquire_many_owned(2).unwrap();
        let permit2 = sem.acquire_many_owned(2).unwrap();

        assert_eq!(sem.available_permits(), 1);
        assert_eq!(permit1.permits(), 2);
        assert_eq!(permit2.permits(), 2);

        // 合并许可
        assert!(permit1.try_merge(permit2).is_ok());
        let separate = Semaphore::new(1);
        let returned = permit1.try_merge(separate.acquire_owned().unwrap()).unwrap_err();
        assert_eq!(separate.available_permits(), 0);
        drop(returned);
        assert_eq!(separate.available_permits(), 1);
        assert_eq!(permit1.permits(), 4);

        drop(permit1);
        assert_eq!(sem.available_permits(), 5);
    }

    #[test]
    #[serial]
    fn test_semaphore_v2_owned_permit_split() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(5);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(3);

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
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let sem = Semaphore::new(1);

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
        Task::new("test", || {}).unwrap();
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
        Task::new("test", || {}).unwrap();
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
