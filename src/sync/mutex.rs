//! # Mutex - 支持闭包传递的互斥锁
//!
//! 基于 Arc 的互斥锁实现，无需全局变量，可以通过闭包传递。
//! 支持 RAII 风格的锁守卫，自动释放锁。
//!
//! ## 设计思路
//!
//! 使用 `Arc<MutexInner<T>>` 封装数据和锁状态，
//! 可以像 `std::sync::Mutex` 一样在局部创建并传递给任务。
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::Mutex;
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     // 局部创建，无需 static！
//!     let counter = Mutex::new(0u32);
//!     let counter_clone = counter.clone();
//!
//!     Task::builder("incrementer")
//!         .spawn(move || {
//!             loop {
//!                 let mut guard = counter.lock().unwrap();
//!                 *guard += 1;
//!                 // guard 离开作用域自动释放锁
//!             }
//!         });
//!
//!     Task::builder("reader")
//!         .spawn(move || {
//!             loop {
//!                 let guard = counter_clone.lock().unwrap();
//!                 println!("Counter: {}", *guard);
//!             }
//!         });
//! }
//! ```

use crate::kernel::time::systick::Ticks;
use crate::compat::Arc;
use crate::error::{Result, RtosError};
use crate::kernel::scheduler::Scheduler;
use crate::kernel::task::Task;
use crate::kernel::time::systick::Deadline;
use crate::sync::event::Event;
use crate::sync::wait::{Registration, WaitQueue, WakeBatch};
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use critical_section::CriticalSection;

/// 互斥锁内部状态
#[repr(C)]
struct MutexInner<T> {
    owner: critical_section::Mutex<core::cell::Cell<Option<Task>>>,
    priority_inheritance: bool,
    /// 锁状态
    locked: AtomicBool,
    waiters: WaitQueue,
    /// 是否已被毒化（持有锁的任务 panic 了）
    poisoned: AtomicBool,
    /// 被保护的数据
    data: UnsafeCell<T>,
}

// Safety: MutexInner 通过锁机制保证线程安全
unsafe impl<T: Send> Send for MutexInner<T> {}
unsafe impl<T: Send> Sync for MutexInner<T> {}

impl<T> MutexInner<T> {
    fn release(&self, cs: CriticalSection<'_>) -> WakeBatch {
        self.owner.borrow(cs).set(None);
        self.locked.store(false, Ordering::Release);
        self.waiters
            .take_all(cs, Event::Mutex(self as *const Self as usize))
    }

    fn new(data: T, priority_inheritance: bool) -> Self {
        Self {
            owner: critical_section::Mutex::new(core::cell::Cell::new(None)),
            priority_inheritance,
            locked: AtomicBool::new(false),
            waiters: WaitQueue::new(),
            poisoned: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }
}

/// 可克隆、可传递的互斥锁
///
/// 类似于 `std::sync::Mutex`，但可以 clone 后 move 到不同任务中。
/// 内部使用 `Arc` 共享状态。
///
/// # 类型参数
/// - `T`: 被保护的数据类型，必须实现 `Send`
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::Mutex;
///
/// let mutex = Mutex::new(vec![1, 2, 3]);
/// let mutex_clone = mutex.clone();
///
/// // 在一个任务中修改
/// {
///     let mut guard = mutex.lock().unwrap();
///     guard.push(4);
/// }
///
/// // 在另一个任务中读取
/// {
///     let guard = mutex_clone.lock().unwrap();
///     assert_eq!(guard.len(), 4);
/// }
/// ```
pub struct Mutex<T> {
    inner: Arc<MutexInner<T>>,
}

impl<T> Clone for Mutex<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Mutex<T> {
    /// 创建新的互斥锁
    ///
    /// # 参数
    /// - `data`: 要保护的数据
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Mutex;
    ///
    /// let mutex = Mutex::new(42);
    /// ```
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(MutexInner::new(data, false)),
        }
    }

    /// 创建带优先级继承的互斥锁
    ///
    /// 优先级继承可以防止优先级反转问题：当高优先级任务等待低优先级任务
    /// 持有的锁时，低优先级任务会临时提升到高优先级任务的优先级。
    ///
    /// # 参数
    /// - `data`: 要保护的数据
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Mutex;
    ///
    /// // 创建带优先级继承的互斥锁
    /// let mutex = Mutex::with_priority_inheritance(42);
    /// ```
    pub fn with_priority_inheritance(data: T) -> Self {
        Self {
            inner: Arc::new(MutexInner::new(data, true)),
        }
    }

    /// 检查是否启用了优先级继承
    pub fn has_priority_inheritance(&self) -> bool {
        self.inner.priority_inheritance
    }

    /// 获取锁
    ///
    /// 如果锁已被其他任务持有，当前任务会被阻塞直到锁可用。
    /// 返回一个 RAII 守卫，离开作用域时自动释放锁。
    ///
    /// 如果启用了优先级继承，当高优先级任务等待时，持有锁的低优先级任务
    /// 会临时提升到等待任务的优先级，以防止优先级反转。
    ///
    /// # 返回值
    /// - `Ok(MutexGuard)`: 成功获取锁
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Mutex;
    ///
    /// let mutex = Mutex::new(0);
    /// {
    ///     let mut guard = mutex.lock().unwrap();
    ///     *guard += 1;
    /// } // 自动释放锁
    /// ```
    pub fn lock(&self) -> Result<MutexGuard<'_, T>> {
        self.lock_until(None)
    }

    /// 尝试获取锁（非阻塞）
    ///
    /// 如果锁可用，立即获取并返回守卫。
    /// 如果锁被占用，立即返回错误而不阻塞。
    ///
    /// # 返回值
    /// - `Ok(MutexGuard)`: 成功获取锁
    /// - `Err(RtosError::WouldBlock)`: 锁被占用
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Mutex;
    ///
    /// let mutex = Mutex::new(0);
    /// match mutex.try_lock() {
    ///     Ok(guard) => println!("Got lock: {}", *guard),
    ///     Err(_) => println!("Lock is busy"),
    /// }
    /// ```
    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>> {
        critical_section::with(|cs| {
            if self.is_poisoned() {
                return Err(RtosError::MutexPoisoned);
            }
            if self.inner.locked.swap(true, Ordering::Acquire) {
                return Err(RtosError::WouldBlock);
            }
            let task = Scheduler::get_current_task();
            self.inner.owner.borrow(cs).set(Some(task));
            Ok(MutexGuard {
                mutex: self,
                _marker: PhantomData,
            })
        })
    }

    /// 带超时的获取锁
    ///
    /// 如果在指定时间内获取到锁，返回守卫。
    /// 如果超时，返回 `Err(RtosError::Timeout)`。
    ///
    /// # 参数
    /// - `timeout`: 超时时间（Duration 或 Ticks）
    ///
    /// # 返回值
    /// - `Ok(MutexGuard)`: 成功获取锁
    /// - `Err(RtosError::Timeout)`: 等待超时
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Mutex;
    ///
    /// let mutex = Mutex::new(0);
    /// match mutex.lock_timeout(neon_rtos2::prelude::Ticks(1000)) {
    ///     Ok(guard) => println!("Got lock: {}", *guard),
    ///     Err(neon_rtos2::error::RtosError::Timeout) => println!("Timeout"),
    ///     Err(_) => println!("Other error"),
    /// }
    /// ```
    pub fn lock_timeout(&self, timeout: impl Into<Ticks>) -> Result<MutexGuard<'_, T>> {
        self.lock_until(Some(Deadline::after(timeout.into().0)))
    }

    fn lock_until(&self, deadline: Option<Deadline>) -> Result<MutexGuard<'_, T>> {
        self.inner.waiters.wait_until(
            Event::Mutex(self.id()),
            deadline,
            || self.try_lock(),
            || self.donate_to_owner(),
        )
    }

    fn donate_to_owner(&self) {
        if self.has_priority_inheritance() {
            critical_section::with(|cs| {
                if let Some(owner) = self.inner.owner.borrow(cs).get() {
                    Scheduler::donate_to_owner(Scheduler::get_current_task(), owner);
                }
            });
        }
    }

    /// 闭包风格 API
    ///
    /// 在持有锁期间执行闭包，闭包执行完毕后自动释放锁。
    ///
    /// # 参数
    /// - `f`: 在持有锁期间执行的闭包，接收数据的可变引用
    ///
    /// # 返回值
    /// - `Ok(R)`: 闭包的返回值
    /// - `Err(RtosError)`: 获取锁失败
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Mutex;
    ///
    /// let mutex = Mutex::new(vec![1, 2, 3]);
    /// let sum = mutex.with_lock(|data| {
    ///     data.iter().sum::<i32>()
    /// }).unwrap();
    /// assert_eq!(sum, 6);
    /// ```
    pub fn with_lock<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut guard = self.lock()?;
        Ok(f(&mut *guard))
    }

    /// 检查锁是否被当前任务持有
    pub fn is_locked_by_current(&self) -> bool {
        critical_section::with(|cs| self.inner.owner.borrow(cs).get() == Some(Scheduler::get_current_task()))
    }

    /// 检查锁是否被占用
    pub fn is_locked(&self) -> bool {
        self.inner.locked.load(Ordering::Acquire)
    }

    /// 检查锁是否已被毒化
    pub fn is_poisoned(&self) -> bool {
        self.inner.poisoned.load(Ordering::Acquire)
    }

    /// 获取互斥锁的唯一标识（用于调试）
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// 内部方法：释放锁
    fn unlock(&self) {
        critical_section::with(|cs| self.inner.release(cs)).wake();
    }

    /// 内部方法：标记为毒化并释放锁
    fn poison_and_unlock(&self) {
        self.inner.poisoned.store(true, Ordering::Release);
        self.unlock();
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("Mutex");
        d.field("id", &self.id());
        d.field("locked", &self.is_locked());
        d.field("poisoned", &self.is_poisoned());
        d.field("waiters", &self.waiter_count());

        // 尝试显示数据（如果能获取锁）
        match self.try_lock() {
            Ok(guard) => {
                d.field("data", &*guard);
            }
            Err(_) => {
                d.field("data", &"<locked>");
            }
        }

        d.finish()
    }
}

/// RAII 互斥锁守卫
///
/// 持有 `MutexGuard` 期间，锁保持被持有状态。
/// 当 `MutexGuard` 离开作用域被 drop 时，锁会自动释放。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::Mutex;
///
/// let mutex = Mutex::new(42);
/// {
///     let guard = mutex.lock().unwrap();
///     println!("Value: {}", *guard);
///     // 离开作用域自动释放锁
/// }
/// ```
#[must_use = "keep the guard alive while accessing the protected data"]
pub struct MutexGuard<'a, T> {
    pub(crate) mutex: &'a Mutex<T>,
    // 使用 PhantomData 标记此类型不应该被发送到其他任务
    // *const () 是 !Send 的，这使得 MutexGuard 也是 !Send
    _marker: PhantomData<*const ()>,
}

impl<'a, T> MutexGuard<'a, T> {
    /// 获取关联的互斥锁引用
    ///
    /// 这个方法主要用于条件变量等需要访问底层互斥锁的场景。
    pub fn mutex(&self) -> &'a Mutex<T> {
        self.mutex
    }

    /// 手动解锁并返回互斥锁引用
    ///
    /// 这个方法会释放锁并返回互斥锁的引用，
    /// 允许在不等待 guard 离开作用域的情况下释放锁。
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::{Mutex, MutexGuard};
    ///
    /// let mutex = Mutex::new(42);
    /// let guard = mutex.lock().unwrap();
    /// let mutex_ref = MutexGuard::unlock(guard);
    /// // 锁已释放，可以再次获取
    /// let guard2 = mutex_ref.lock().unwrap();
    /// ```
    pub fn unlock(guard: Self) -> &'a Mutex<T> {
        let mutex = guard.mutex;
        // 手动释放锁
        mutex.unlock();
        // 阻止 Drop 再次释放
        core::mem::forget(guard);
        mutex
    }
    pub(crate) fn unlock_deferred(self, cs: CriticalSection<'_>) -> WakeBatch {
        let mutex = self.mutex;
        core::mem::forget(self);
        mutex.inner.release(cs)
    }
}

// 显式实现 !Send - MutexGuard 不能被发送到其他任务
// 这是故意的，因为锁必须在获取它的任务中释放
// 注意：由于 PhantomData<*const ()>，MutexGuard 已经是 !Send 的
// 但我们仍然可以显式实现 Sync（如果 T: Sync）
unsafe impl<T: Send + Sync> Sync for MutexGuard<'_, T> {}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: 我们持有锁，所以可以安全访问数据
        unsafe { &*self.mutex.inner.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: 我们持有锁，所以可以安全访问数据
        unsafe { &mut *self.mutex.inner.data.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        // 检查是否在 panic 中（简化实现，实际需要更复杂的检测）
        // 这里我们假设正常释放
        self.mutex.unlock();
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MutexGuard").field("data", &**self).finish()
    }
}

impl<T: core::fmt::Display> core::fmt::Display for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

// ============================================================================
// OwnedMutexGuard - 拥有所有权的锁守卫
// ============================================================================

/// 拥有所有权的互斥锁守卫
///
/// 与 `MutexGuard` 不同，`OwnedMutexGuard` 持有 `Arc<MutexInner<T>>` 的所有权，
/// 因此可以被 move 到其他任务中（虽然通常不建议这样做）。
///
/// 主要用于需要将锁守卫存储在结构体中或跨 await 点持有的场景。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::Mutex;
///
/// let mutex = Mutex::new(42);
/// let guard = mutex.lock_owned().unwrap();
///
/// // guard 可以被存储或移动
/// let value = *guard;
/// drop(guard); // 显式释放
/// ```
#[must_use = "keep the guard alive while accessing the protected data"]
pub struct OwnedMutexGuard<T> {
    mutex: Arc<MutexInner<T>>,
}

// OwnedMutexGuard 可以 Send（如果 T: Send）
// 这是与 MutexGuard 的主要区别
unsafe impl<T: Send> Send for OwnedMutexGuard<T> {}
unsafe impl<T: Send + Sync> Sync for OwnedMutexGuard<T> {}

impl<T> Deref for OwnedMutexGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: 我们持有锁，所以可以安全访问数据
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for OwnedMutexGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: 我们持有锁，所以可以安全访问数据
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for OwnedMutexGuard<T> {
    fn drop(&mut self) {
        critical_section::with(|cs| self.mutex.release(cs)).wake();
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for OwnedMutexGuard<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OwnedMutexGuard")
            .field("data", &**self)
            .finish()
    }
}

impl<T: core::fmt::Display> core::fmt::Display for OwnedMutexGuard<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T> Mutex<T> {
    /// 获取拥有所有权的锁守卫
    ///
    /// 与 `lock()` 类似，但返回的守卫持有 `Arc` 的所有权，
    /// 可以被 move 到其他地方。
    ///
    /// # 返回值
    /// - `Ok(OwnedMutexGuard)`: 成功获取锁
    /// - `Err(RtosError)`: 获取失败
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Mutex;
    ///
    /// let mutex = Mutex::new(42);
    /// let guard = mutex.lock_owned().unwrap();
    /// // guard 可以被存储在结构体中
    /// ```

    /// 从 Mutex 获取拥有所有权的锁守卫
    pub fn lock_owned(&self) -> Result<OwnedMutexGuard<T>> {
        let guard = self.lock()?;
        let owned = OwnedMutexGuard {
            mutex: self.inner.clone(),
        };
        core::mem::forget(guard);
        Ok(owned)
    }

    /// 尝试获取拥有所有权的锁守卫���非阻塞）
    pub fn try_lock_owned(&self) -> Result<OwnedMutexGuard<T>> {
        let guard = self.try_lock()?;
        let owned = OwnedMutexGuard {
            mutex: self.inner.clone(),
        };
        core::mem::forget(guard);
        Ok(owned)
    }
}

// ============================================================================
// MappedMutexGuard - 映射锁守卫
// ============================================================================

/// 映射的互斥锁守卫
///
/// 允许将 `MutexGuard<T>` 映射到 `MappedMutexGuard<U>`，
/// 其中 `U` 是 `T` 的一部分。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::{Mutex, MutexGuard};
///
/// struct Data {
///     field1: i32,
///     field2: String,
/// }
///
/// let mutex = Mutex::new(Data { field1: 42, field2: "hello".into() });
/// let guard = mutex.lock().unwrap();
///
/// // 映射到 field1
/// let mut mapped = MutexGuard::map(guard, |data| &mut data.field1);
/// *mapped = 100;
/// ```
#[must_use = "keep the guard alive while accessing the protected data"]
pub struct MappedMutexGuard<'a, T, U> {
    data: *mut U,
    mutex: &'a Mutex<T>,
    _marker: PhantomData<&'a mut U>,
}

impl<'a, T, U> MappedMutexGuard<'a, T, U> {
    /// 创建映射守卫（内部使用）
    fn new(data: *mut U, mutex: &'a Mutex<T>) -> Self {
        Self {
            data,
            mutex,
            _marker: PhantomData,
        }
    }
}

impl<T, U> Deref for MappedMutexGuard<'_, T, U> {
    type Target = U;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data }
    }
}

impl<T, U> DerefMut for MappedMutexGuard<'_, T, U> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.data }
    }
}

impl<T, U> Drop for MappedMutexGuard<'_, T, U> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

impl<'a, T> MutexGuard<'a, T> {
    /// 将锁守卫映射到数据的一部分
    ///
    /// # 参数
    /// - `guard`: 原始锁守卫
    /// - `f`: 映射函数，将 `&mut T` 映射到 `&mut U`
    ///
    /// # 返回值
    /// 映射后的锁守卫
    pub fn map<U, F>(guard: Self, f: F) -> MappedMutexGuard<'a, T, U>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        let mutex = guard.mutex;
        let data = unsafe { &mut *mutex.inner.data.get() };
        let mapped_data = f(data) as *mut U;

        // 阻止原始 guard 的 drop
        core::mem::forget(guard);

        MappedMutexGuard::new(mapped_data, mutex)
    }

    /// 尝试将锁守卫映射到数据的一部分
    ///
    /// 如果映射函数返回 `None`，则返回原始守卫。
    pub fn try_map<U, F>(
        guard: Self,
        f: F,
    ) -> core::result::Result<MappedMutexGuard<'a, T, U>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
    {
        let mutex = guard.mutex;
        let data = unsafe { &mut *mutex.inner.data.get() };

        match f(data) {
            Some(mapped_data) => {
                let mapped_ptr = mapped_data as *mut U;
                core::mem::forget(guard);
                Ok(MappedMutexGuard::new(mapped_ptr, mutex))
            }
            None => Err(guard),
        }
    }
}

// ============================================================================
// Mutex 异步支持
// ============================================================================

/// Mutex 的异步锁获取 Future
///
/// 实现 `Future` trait，允许在 async/await 上下文中获取锁。
pub struct MutexLockFuture<'a, T> {
    mutex: &'a Mutex<T>,
    registration: Registration<'a>,
}

impl<'a, T> MutexLockFuture<'a, T> {
    fn new(mutex: &'a Mutex<T>) -> Self {
        Self {
            mutex,
            registration: mutex.inner.waiters.registration(),
        }
    }
}

impl<'a, T> core::future::Future for MutexLockFuture<'a, T> {
    type Output = Result<MutexGuard<'a, T>>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let mutex = self.mutex;
        self.registration.poll_until(cx, || mutex.try_lock())
    }
}

impl<T> Mutex<T> {
    /// 异步获取锁
    ///
    /// 返回一个 Future，在获取到锁时完成。
    /// 可以在 async/await 上下文中使用。
    ///
    /// # 返回值
    /// - `Ok(MutexGuard)`: 成功获取锁
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::{Mutex, MutexGuard};
    ///
    /// async fn example(mutex: Mutex<i32>) {
    ///     let guard = mutex.lock_async().await.unwrap();
    ///     // 使用数据
    /// }
    /// ```
    pub fn lock_async(&self) -> MutexLockFuture<'_, T> {
        MutexLockFuture::new(self)
    }

    /// 获取等待者数量（包括同步和异步等待者）
    pub fn waiter_count(&self) -> usize {
        {
            let (tasks, futures) = self.inner.waiters.counts();
            tasks + futures
        }
    }

    /// 获取同步等待者数量
    pub fn sync_waiter_count(&self) -> usize {
        self.inner.waiters.counts().0
    }

    /// 获取异步等待者数量
    pub fn async_waiter_count(&self) -> usize {
        self.inner.waiters.counts().1
    }
}

// ============================================================================
// RwLock - 读写锁
// ============================================================================

/// 读写锁内部状态
struct RwLockInner<T> {
    /// 被保护的数据
    data: UnsafeCell<T>,
    /// 读者计数（正数表示读者数量，-1 表示有写者）
    /// 0 = 无锁，>0 = 读锁数量，-1 = 写锁
    state: AtomicIsize,
    /// 写者等待队列（同步）
    waiters: WaitQueue,
    poisoned: AtomicBool,
}

use core::sync::atomic::AtomicIsize;

// Safety: RwLockInner 通过锁机制保证线程安全
unsafe impl<T: Send> Send for RwLockInner<T> {}
unsafe impl<T: Send + Sync> Sync for RwLockInner<T> {}

impl<T> RwLockInner<T> {
    fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            state: AtomicIsize::new(0),
            waiters: WaitQueue::new(),
            poisoned: AtomicBool::new(false),
        }
    }
}

/// 可克隆、可传递的读写锁
///
/// 允许多个读者同时访问，或单个写者独占访问。
/// 类似于 `std::sync::RwLock`，但可以 clone 后 move 到不同任务中。
///
/// # 类型参数
/// - `T`: 被保护的数据类型，必须实现 `Send + Sync`
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::RwLock;
///
/// let lock = RwLock::new(vec![1, 2, 3]);
/// let lock_clone = lock.clone();
///
/// // 多个读者可以同时访问
/// {
///     let guard1 = lock.read().unwrap();
///     let guard2 = lock_clone.read().unwrap();
///     assert_eq!(guard1.len(), guard2.len());
/// }
///
/// // 写者独占访问
/// {
///     let mut guard = lock.write().unwrap();
///     guard.push(4);
/// }
/// ```
pub struct RwLock<T> {
    inner: Arc<RwLockInner<T>>,
}

impl<T> Clone for RwLock<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> RwLock<T> {
    /// 创建新的读写锁
    ///
    /// # 参数
    /// - `data`: 要保护的数据
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(RwLockInner::new(data)),
        }
    }

    /// 获取读锁
    ///
    /// 如果有写者持有锁，当前任务会被阻塞直到写者释放锁。
    /// 多个读者可以同时持有读锁。
    ///
    /// # 返回值
    /// - `Ok(RwLockReadGuard)`: 成功获取读锁
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    pub fn read(&self) -> Result<RwLockReadGuard<'_, T>> {
        self.inner
            .waiters
            .wait_until(Event::Mutex(self.id()), None, || self.try_read(), || {})
    }

    /// 尝试获取读锁（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(RwLockReadGuard)`: 成功获取读锁
    /// - `Err(RtosError::WouldBlock)`: 有写者持有锁
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    pub fn try_read(&self) -> Result<RwLockReadGuard<'_, T>> {
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        loop {
            let state = self.inner.state.load(Ordering::Acquire);

            if state >= 0 {
                if self
                    .inner
                    .state
                    .compare_exchange(state, state + 1, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(RwLockReadGuard {
                        lock: self,
                        _marker: PhantomData,
                    });
                }
                continue;
            }

            return Err(RtosError::WouldBlock);
        }
    }

    /// 获取写锁
    ///
    /// 如果有任何读者或写者持有锁，当前任务会被阻塞。
    ///
    /// # 返回值
    /// - `Ok(RwLockWriteGuard)`: 成功获取写锁
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    pub fn write(&self) -> Result<RwLockWriteGuard<'_, T>> {
        self.inner
            .waiters
            .wait_until(Event::Mutex(self.id()), None, || self.try_write(), || {})
    }

    /// 尝试获取写锁（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(RwLockWriteGuard)`: 成功获取写锁
    /// - `Err(RtosError::WouldBlock)`: 锁被占用
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    pub fn try_write(&self) -> Result<RwLockWriteGuard<'_, T>> {
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        if self
            .inner
            .state
            .compare_exchange(0, -1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            Ok(RwLockWriteGuard {
                lock: self,
                _marker: PhantomData,
            })
        } else {
            Err(RtosError::WouldBlock)
        }
    }

    /// 异步获取读锁
    pub fn read_async(&self) -> RwLockReadFuture<'_, T> {
        RwLockReadFuture::new(self)
    }

    /// 异步获取写锁
    pub fn write_async(&self) -> RwLockWriteFuture<'_, T> {
        RwLockWriteFuture::new(self)
    }

    /// 检查是否已被毒化
    pub fn is_poisoned(&self) -> bool {
        self.inner.poisoned.load(Ordering::Acquire)
    }

    /// 获取读者数量（如果有写者返回 0）
    pub fn reader_count(&self) -> usize {
        let state = self.inner.state.load(Ordering::Relaxed);
        if state > 0 { state as usize } else { 0 }
    }

    /// 检查是否有写者
    pub fn is_write_locked(&self) -> bool {
        self.inner.state.load(Ordering::Relaxed) == -1
    }

    /// 获取唯一标识（用于调试）
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// 内部方法：释放读锁
    fn unlock_read(&self) {
        critical_section::with(|cs| {
            if self.inner.state.fetch_sub(1, Ordering::Release) == 1 {
                self.inner.waiters.take_all(cs, Event::Mutex(self.id()))
            } else {
                WakeBatch::default()
            }
        })
        .wake();
    }

    /// 内部方法：释放写锁
    fn unlock_write(&self) {
        critical_section::with(|cs| {
            self.inner.state.store(0, Ordering::Release);
            self.inner.waiters.take_all(cs, Event::Mutex(self.id()))
        })
        .wake();
    }
}

impl<T: Default> Default for RwLock<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for RwLock<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("RwLock");
        d.field("id", &self.id());
        d.field("poisoned", &self.is_poisoned());

        let state = self.inner.state.load(Ordering::Relaxed);
        if state == -1 {
            d.field("state", &"write_locked");
        } else if state > 0 {
            d.field("state", &format_args!("read_locked({})", state));
        } else {
            d.field("state", &"unlocked");
        }

        d.finish()
    }
}

/// 读锁守卫
#[must_use = "keep the guard alive while accessing the protected data"]
pub struct RwLockReadGuard<'a, T> {
    lock: &'a RwLock<T>,
    _marker: PhantomData<*const ()>,
}

unsafe impl<T: Sync> Sync for RwLockReadGuard<'_, T> {}

impl<T> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.inner.data.get() }
    }
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.unlock_read();
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for RwLockReadGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RwLockReadGuard")
            .field("data", &**self)
            .finish()
    }
}

/// 写锁守卫
#[must_use = "keep the guard alive while accessing the protected data"]
pub struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLock<T>,
    _marker: PhantomData<*const ()>,
}

unsafe impl<T: Sync> Sync for RwLockWriteGuard<'_, T> {}

impl<T> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.inner.data.get() }
    }
}

impl<T> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.inner.data.get() }
    }
}

impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.unlock_write();
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for RwLockWriteGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RwLockWriteGuard")
            .field("data", &**self)
            .finish()
    }
}

/// 异步读锁 Future
pub struct RwLockReadFuture<'a, T> {
    lock: &'a RwLock<T>,
    registration: Registration<'a>,
}

impl<'a, T> RwLockReadFuture<'a, T> {
    fn new(lock: &'a RwLock<T>) -> Self {
        Self {
            lock,
            registration: lock.inner.waiters.registration(),
        }
    }
}

impl<'a, T> core::future::Future for RwLockReadFuture<'a, T> {
    type Output = Result<RwLockReadGuard<'a, T>>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let lock = self.lock;
        self.registration.poll_until(cx, || lock.try_read())
    }
}

/// 异步写锁 Future
pub struct RwLockWriteFuture<'a, T> {
    lock: &'a RwLock<T>,
    registration: Registration<'a>,
}

impl<'a, T> RwLockWriteFuture<'a, T> {
    fn new(lock: &'a RwLock<T>) -> Self {
        Self {
            lock,
            registration: lock.inner.waiters.registration(),
        }
    }
}

impl<'a, T> core::future::Future for RwLockWriteFuture<'a, T> {
    type Output = Result<RwLockWriteGuard<'a, T>>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let lock = self.lock;
        self.registration.poll_until(cx, || lock.try_write())
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
    fn test_mutex_v2_basic() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(42);

        // 获取锁并读取
        {
            let guard = mutex.lock().unwrap();
            assert_eq!(*guard, 42);
        }

        // 获取锁并修改
        {
            let mut guard = mutex.lock().unwrap();
            *guard = 100;
        }

        // 验证修改
        {
            let guard = mutex.lock().unwrap();
            assert_eq!(*guard, 100);
        }
    }

    #[test]
    #[serial]
    fn test_mutex_v2_try_lock() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(0);

        // 第一次 try_lock 应该成功
        let guard = mutex.try_lock().unwrap();
        assert_eq!(*guard, 0);

        // 锁被持有时，try_lock 应该失败
        // 注意：在单任务测试中，我们持有 guard，所以无法再次获取
        // 这里我们只测试基本功能
        drop(guard);

        // 释放后应该能再次获取
        let guard2 = mutex.try_lock().unwrap();
        assert_eq!(*guard2, 0);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_clone() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex1 = Mutex::new(0);
        let mutex2 = mutex1.clone();

        // 两个句柄指向同一个互斥锁
        assert_eq!(mutex1.id(), mutex2.id());

        // 通过一个修改，另一个可以看到
        {
            let mut guard = mutex1.lock().unwrap();
            *guard = 42;
        }

        {
            let guard = mutex2.lock().unwrap();
            assert_eq!(*guard, 42);
        }
    }

    #[test]
    #[serial]
    fn test_mutex_v2_with_lock() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(vec![1, 2, 3]);

        // 使用 with_lock 闭包风格
        let sum = mutex.with_lock(|data| data.iter().sum::<i32>()).unwrap();

        assert_eq!(sum, 6);

        // 修改数据
        mutex
            .with_lock(|data| {
                data.push(4);
            })
            .unwrap();

        // 验证修改
        let len = mutex.with_lock(|data| data.len()).unwrap();
        assert_eq!(len, 4);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_is_locked() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(0);

        assert!(!mutex.is_locked());

        {
            let _guard = mutex.lock().unwrap();
            assert!(mutex.is_locked());
            assert!(mutex.is_locked_by_current());
        }

        assert!(!mutex.is_locked());
    }

    #[test]
    #[serial]
    fn test_mutex_v2_default() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex: Mutex<i32> = Mutex::default();

        let guard = mutex.lock().unwrap();
        assert_eq!(*guard, 0); // i32 的默认值是 0
    }

    #[test]
    #[serial]
    fn test_mutex_v2_debug() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(42);

        let debug_str = format!("{:?}", mutex);
        assert!(debug_str.contains("Mutex"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    #[serial]
    fn test_mutex_v2_lock_timeout_immediate() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(42);

        // 第一次获取应该成功
        let guard = mutex.lock_timeout(Ticks(0)).unwrap();
        assert_eq!(*guard, 42);
        drop(guard);

        // 释放后再次获取应该成功
        let guard2 = mutex.lock_timeout(Ticks(100)).unwrap();
        assert_eq!(*guard2, 42);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_lock_timeout() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(0);

        // 获取锁
        let guard = mutex.lock_timeout(Ticks(1000)).unwrap();
        assert_eq!(*guard, 0);
        drop(guard);

        // 再次获取应该成功
        let guard2 = mutex.lock_timeout(Ticks(1000)).unwrap();
        assert_eq!(*guard2, 0);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_owned_guard() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(42);

        // 获取 owned guard
        let guard = mutex.lock_owned().unwrap();
        assert_eq!(*guard, 42);
        drop(guard);

        // 再次获取应该成功
        let guard2 = mutex.try_lock_owned().unwrap();
        assert_eq!(*guard2, 42);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_mapped_guard() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        struct Data {
            field1: i32,
            field2: i32,
        }

        let mutex = Mutex::new(Data {
            field1: 10,
            field2: 20,
        });

        // 映射到 field1
        {
            let guard = mutex.lock().unwrap();
            let mut mapped = MutexGuard::map(guard, |data| &mut data.field1);
            *mapped = 100;
        }

        // 验证修改
        {
            let guard = mutex.lock().unwrap();
            assert_eq!(guard.field1, 100);
            assert_eq!(guard.field2, 20);
        }
    }

    #[test]
    #[serial]
    fn test_mutex_v2_try_map() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        let mutex = Mutex::new(Some(42i32));

        // 成功映射
        {
            let guard = mutex.lock().unwrap();
            let mapped = MutexGuard::try_map(guard, |opt| opt.as_mut());
            assert!(mapped.is_ok());
            let mut mapped = mapped.unwrap();
            *mapped = 100;
        }

        // 验证修改
        {
            let guard = mutex.lock().unwrap();
            assert_eq!(*guard, Some(100));
        }

        // 设置为 None
        {
            let mut guard = mutex.lock().unwrap();
            *guard = None;
        }

        // 映射失败
        {
            let guard = mutex.lock().unwrap();
            let result = MutexGuard::try_map(guard, |opt| opt.as_mut());
            assert!(result.is_err());
        }
    }

    #[test]
    #[serial]
    fn test_mutex_v2_priority_inheritance() {
        kernel_init();
        Task::new("test", || {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();

        // 创建带优先级继承的互斥锁
        let mutex = Mutex::with_priority_inheritance(42);
        assert!(mutex.has_priority_inheritance());

        // 基本功能测试
        {
            let guard = mutex.lock().unwrap();
            assert_eq!(*guard, 42);
        }
    }

    #[test]
    #[serial]
    fn test_nested_priority_inheritance() {
        use crate::compat::Arc;
        use crate::kernel::scheduler::Scheduler;
        use crate::kernel::task::{Priority, Task};

        kernel_init();
        Scheduler::enable_priority_scheduling();

        let mut task_l = Task::builder("L")
            .priority(Priority::Low)
            .spawn(|| {})
            .unwrap();
        let mut task_m = Task::builder("M")
            .priority(Priority::Normal)
            .spawn(|| {})
            .unwrap();
        let mut task_h = Task::builder("H")
            .priority(Priority::High)
            .spawn(|| {})
            .unwrap();

        let m1 = Arc::new(Mutex::with_priority_inheritance(1));
        let m2 = Arc::new(Mutex::with_priority_inheritance(2));

        // 模拟任务 L 获取 M1
        crate::kernel::scheduler::set_current_task_id_for_test(task_l.get_taskid());
        let _guard1 = m1.try_lock().unwrap();

        // 验证 L 的优先级是 Low
        assert_eq!(task_l.get_priority(), Priority::Low);

        // 模拟任务 M 获取 M2
        crate::kernel::scheduler::set_current_task_id_for_test(task_m.get_taskid());
        let _guard2 = m2.try_lock().unwrap();

        // 模拟任务 M 尝试获取 M1，会失败并阻塞，触发优先级继承
        task_m.block(crate::sync::event::Event::Mutex(
            Arc::as_ptr(&m1.inner) as usize
        ));
        m1.donate_to_owner();

        // M 被阻塞在 M1 上，L 继承了 Normal 优先级
        assert_eq!(task_l.get_priority(), Priority::Normal);

        // 模拟任务 H 尝试获取 M2
        crate::kernel::scheduler::set_current_task_id_for_test(task_h.get_taskid());
        task_h.block(crate::sync::event::Event::Mutex(
            Arc::as_ptr(&m2.inner) as usize
        ));
        m2.donate_to_owner();

        // H 被阻塞在 M2 上，M 继承了 High，同时 L 递归继承了 High！
        assert_eq!(task_m.get_priority(), Priority::High);
        assert_eq!(task_l.get_priority(), Priority::High);
        task_l.set_priority(Priority::Idle);
        assert_eq!(task_l.get_priority(), Priority::High);
        task_h.ready(); // A timeout must revoke both levels of inheritance.
        assert_eq!(task_m.get_priority(), Priority::Normal);
        assert_eq!(task_l.get_priority(), Priority::Normal);
        task_m.ready();
        assert_eq!(task_l.get_priority(), Priority::Idle);
    }

    #[test]
    #[serial]
    fn priority_inheritance_ignores_unknown_event_ids() {
        kernel_init();
        use crate::kernel::task::Priority;
        let mut owner = Task::builder("owner")
            .priority(Priority::Low)
            .spawn(|| {})
            .unwrap();
        crate::kernel::scheduler::set_current_task_id_for_test(owner.0);
        let mutex = Mutex::with_priority_inheritance(0);
        let _guard = mutex.try_lock().unwrap();
        owner.block(crate::sync::event::Event::Mutex(1));
        let mut waiter = Task::builder("waiter")
            .priority(Priority::High)
            .spawn(|| {})
            .unwrap();
        crate::kernel::scheduler::set_current_task_id_for_test(waiter.0);
        waiter.block(Event::Mutex(mutex.id()));
        mutex.donate_to_owner();
        assert_eq!(owner.get_priority(), Priority::High);
    }
}
