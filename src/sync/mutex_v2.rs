//! # Mutex V2 - 支持闭包传递的互斥锁
//!
//! 基于 Arc 的互斥锁实现，无需全局变量，可以通过闭包传递。
//! 支持 RAII 风格的锁守卫，自动释放锁。
//!
//! ## 设计思路
//!
//! 传统 Mutex 使用全局 ID 来标识，必须用全局变量。
//! Mutex V2 使用 `Arc<MutexInner<T>>` 封装数据和锁状态，
//! 可以像 `std::sync::Mutex` 一样在局部创建并传递给任务。
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::MutexV2;
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     // 局部创建，无需 static！
//!     let counter = MutexV2::new(0u32);
//!     let counter_clone = counter.clone();
//!
//!     Task::builder("incrementer")
//!         .spawn(move |_| {
//!             loop {
//!                 let mut guard = counter.lock().unwrap();
//!                 *guard += 1;
//!                 // guard 离开作用域自动释放锁
//!             }
//!         });
//!
//!     Task::builder("reader")
//!         .spawn(move |_| {
//!             loop {
//!                 let guard = counter_clone.lock().unwrap();
//!                 println!("Counter: {}", *guard);
//!             }
//!         });
//! }
//! ```

use crate::compat::{Arc, VecDeque};
use crate::kernel::scheduler::Scheduler;
use crate::kernel::task::TaskState;
use crate::kernel::time::systick::Systick;
use crate::hal::trigger_schedule;
use crate::error::{Result, RtosError};
use crate::sync::signal_v2::WaiterList;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::task::Waker;
use spin::Mutex as SpinMutex;

/// 互斥锁内部状态
struct MutexInner<T> {
    /// 被保护的数据
    data: UnsafeCell<T>,
    /// 锁状态
    locked: AtomicBool,
    /// 当前持有者的任务 ID（usize::MAX 表示无持有者）
    owner: AtomicUsize,
    /// 持有者的原始优先级（用于优先级继承恢复）
    owner_original_priority: SpinMutex<Option<crate::kernel::task::Priority>>,
    /// 同步等待者列表
    waiters: SpinMutex<WaiterList>,
    /// 异步等待者列表
    async_waiters: SpinMutex<VecDeque<Waker>>,
    /// 是否已被毒化（持有锁的任务 panic 了）
    poisoned: AtomicBool,
    /// 是否启用优先级继承
    priority_inheritance: bool,
}

// Safety: MutexInner 通过锁机制保证线程安全
unsafe impl<T: Send> Send for MutexInner<T> {}
unsafe impl<T: Send> Sync for MutexInner<T> {}

impl<T> MutexInner<T> {
    fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            locked: AtomicBool::new(false),
            owner: AtomicUsize::new(usize::MAX),
            owner_original_priority: SpinMutex::new(None),
            waiters: SpinMutex::new(WaiterList::new()),
            async_waiters: SpinMutex::new(VecDeque::new()),
            poisoned: AtomicBool::new(false),
            priority_inheritance: false,
        }
    }

    fn new_with_priority_inheritance(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            locked: AtomicBool::new(false),
            owner: AtomicUsize::new(usize::MAX),
            owner_original_priority: SpinMutex::new(None),
            waiters: SpinMutex::new(WaiterList::new()),
            async_waiters: SpinMutex::new(VecDeque::new()),
            poisoned: AtomicBool::new(false),
            priority_inheritance: true,
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
/// use neon_rtos2::sync::MutexV2;
///
/// let mutex = MutexV2::new(vec![1, 2, 3]);
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
pub struct MutexV2<T> {
    inner: Arc<MutexInner<T>>,
}

impl<T> Clone for MutexV2<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> MutexV2<T> {
    /// 创建新的互斥锁
    ///
    /// # 参数
    /// - `data`: 要保护的数据
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// let mutex = MutexV2::new(42);
    /// ```
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(MutexInner::new(data)),
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
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// // 创建带优先级继承的互斥锁
    /// let mutex = MutexV2::with_priority_inheritance(42);
    /// ```
    pub fn with_priority_inheritance(data: T) -> Self {
        Self {
            inner: Arc::new(MutexInner::new_with_priority_inheritance(data)),
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
    /// - `Ok(MutexGuardV2)`: 成功获取锁
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// let mutex = MutexV2::new(0);
    /// {
    ///     let mut guard = mutex.lock().unwrap();
    ///     *guard += 1;
    /// } // 自动释放锁
    /// ```
    pub fn lock(&self) -> Result<MutexGuardV2<'_, T>> {
        // 检查是否已毒化
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        loop {
            // 尝试获取锁
            if self.inner.locked.compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            ).is_ok() {
                // 成功获取锁
                let mut current = Scheduler::get_current_task();
                let task_id = current.get_taskid();
                self.inner.owner.store(task_id, Ordering::Release);
                
                // 如果启用优先级继承，保存原始优先级
                if self.inner.priority_inheritance {
                    let mut orig_priority = self.inner.owner_original_priority.lock();
                    *orig_priority = Some(current.get_priority());
                }
                
                return Ok(MutexGuardV2 { mutex: self, _marker: PhantomData });
            }

            // 锁被占用，加入等待队列并阻塞
            let current = Scheduler::get_current_task();
            let task_id = current.get_taskid();
            let current_priority = current.get_priority();

            {
                let mut waiters = self.inner.waiters.lock();
                if !waiters.push(task_id) {
                    return Err(RtosError::WaiterQueueFull);
                }
            }

            // 优先级继承：提升持有者的优先级
            if self.inner.priority_inheritance {
                let owner_id = self.inner.owner.load(Ordering::Acquire);
                if owner_id != usize::MAX {
                    crate::kernel::task::Task::for_each(|mut task, id| {
                        if id == owner_id {
                            let owner_priority = task.get_priority();
                            // 如果等待者优先级更高，提升持有者优先级
                            if current_priority > owner_priority {
                                task.set_priority(current_priority);
                            }
                        }
                    });
                }
            }

            // 使用 Arc 的地址作为唯一标识
            let mutex_id = Arc::as_ptr(&self.inner) as usize;
            
            // 阻塞当前任务
            Scheduler::get_current_task().block(crate::sync::event::Event::Mutex(mutex_id));
            trigger_schedule();

            // 被唤醒后检查是否毒化
            if self.inner.poisoned.load(Ordering::Acquire) {
                return Err(RtosError::MutexPoisoned);
            }

            // 被唤醒后重新尝试获取锁
        }
    }

    /// 尝试获取锁（非阻塞）
    ///
    /// 如果锁可用，立即获取并返回守卫。
    /// 如果锁被占用，立即返回错误而不阻塞。
    ///
    /// # 返回值
    /// - `Ok(MutexGuardV2)`: 成功获取锁
    /// - `Err(RtosError::WouldBlock)`: 锁被占用
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// let mutex = MutexV2::new(0);
    /// match mutex.try_lock() {
    ///     Ok(guard) => println!("Got lock: {}", *guard),
    ///     Err(_) => println!("Lock is busy"),
    /// }
    /// ```
    pub fn try_lock(&self) -> Result<MutexGuardV2<'_, T>> {
        // 检查是否已毒化
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        // 尝试获取锁
        if self.inner.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed,
        ).is_ok() {
            let task_id = Scheduler::get_current_task().get_taskid();
            self.inner.owner.store(task_id, Ordering::Release);
            Ok(MutexGuardV2 { mutex: self, _marker: PhantomData })
        } else {
            Err(RtosError::WouldBlock)
        }
    }

    /// 带超时的获取锁
    ///
    /// 如果在指定时间内获取到锁，返回守卫。
    /// 如果超时，返回 `Err(RtosError::Timeout)`。
    ///
    /// # 参数
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # 返回值
    /// - `Ok(MutexGuardV2)`: 成功获取锁
    /// - `Err(RtosError::Timeout)`: 等待超时
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// let mutex = MutexV2::new(0);
    /// match mutex.lock_timeout(1000) {
    ///     Ok(guard) => println!("Got lock: {}", *guard),
    ///     Err(neon_rtos2::error::RtosError::Timeout) => println!("Timeout"),
    ///     Err(_) => println!("Other error"),
    /// }
    /// ```
    pub fn lock_timeout(&self, timeout_ms: usize) -> Result<MutexGuardV2<'_, T>> {
        // 检查是否已毒化
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        let deadline = Systick::get_current_time() + timeout_ms;

        loop {
            // 尝试获取锁
            if self.inner.locked.compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            ).is_ok() {
                // 成功获取锁
                let task_id = Scheduler::get_current_task().get_taskid();
                self.inner.owner.store(task_id, Ordering::Release);
                return Ok(MutexGuardV2 { mutex: self, _marker: PhantomData });
            }

            // 检查是否超时
            if Systick::get_current_time() >= deadline {
                return Err(RtosError::Timeout);
            }

            // 锁被占用，加入等待队列并阻塞
            let current = Scheduler::get_current_task();
            let task_id = current.get_taskid();

            {
                let mut waiters = self.inner.waiters.lock();
                if !waiters.push(task_id) {
                    return Err(RtosError::WaiterQueueFull);
                }
            }

            // 使用 Arc 的地址作为唯一标识
            let mutex_id = Arc::as_ptr(&self.inner) as usize;
            
            // 阻塞当前任务
            Scheduler::get_current_task().block(crate::sync::event::Event::Mutex(mutex_id));
            trigger_schedule();

            // 被唤醒后检查是否毒化
            if self.inner.poisoned.load(Ordering::Acquire) {
                return Err(RtosError::MutexPoisoned);
            }

            // 检查是否超时
            if Systick::get_current_time() >= deadline {
                // 从等待队列中移除自己
                let mut waiters = self.inner.waiters.lock();
                waiters.remove(task_id);
                return Err(RtosError::Timeout);
            }

            // 被唤醒后重新尝试获取锁
        }
    }

    /// 带超时的尝试获取锁（轮询模式）
    ///
    /// 在指定时间内反复尝试获取锁，不会阻塞任务。
    /// 适用于不想让任务进入阻塞状态的场景。
    ///
    /// # 参数
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # 返回值
    /// - `Ok(MutexGuardV2)`: 成功获取锁
    /// - `Err(RtosError::Timeout)`: 超时
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    pub fn try_lock_timeout(&self, timeout_ms: usize) -> Result<MutexGuardV2<'_, T>> {
        let deadline = Systick::get_current_time() + timeout_ms;
        
        loop {
            match self.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(RtosError::WouldBlock) => {
                    if Systick::get_current_time() >= deadline {
                        return Err(RtosError::Timeout);
                    }
                    // 继续轮询
                }
                Err(e) => return Err(e),
            }
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
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// let mutex = MutexV2::new(vec![1, 2, 3]);
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
        let current_id = Scheduler::get_current_task().get_taskid();
        self.inner.locked.load(Ordering::Acquire) 
            && self.inner.owner.load(Ordering::Acquire) == current_id
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
        // 优先级继承：恢复原始优先级
        if self.inner.priority_inheritance {
            let owner_id = self.inner.owner.load(Ordering::Acquire);
            if owner_id != usize::MAX {
                let original_priority = {
                    let mut orig = self.inner.owner_original_priority.lock();
                    orig.take()
                };
                
                if let Some(priority) = original_priority {
                    crate::kernel::task::Task::for_each(|mut task, id| {
                        if id == owner_id {
                            task.set_priority(priority);
                        }
                    });
                }
            }
        }

        // 清除持有者
        self.inner.owner.store(usize::MAX, Ordering::Release);
        
        // 释放锁
        self.inner.locked.store(false, Ordering::Release);

        // 首先尝试唤醒同步等待者
        let task_id = {
            let mut waiters = self.inner.waiters.lock();
            waiters.pop_front()
        };

        if let Some(task_id) = task_id {
            crate::kernel::task::Task::for_each(|task, id| {
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

    /// 内部方法：标记为毒化并释放锁
    fn poison_and_unlock(&self) {
        self.inner.poisoned.store(true, Ordering::Release);
        self.unlock();
    }
}

impl<T: Default> Default for MutexV2<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for MutexV2<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("MutexV2");
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
/// 持有 `MutexGuardV2` 期间，锁保持被持有状态。
/// 当 `MutexGuardV2` 离开作用域被 drop 时，锁会自动释放。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::MutexV2;
///
/// let mutex = MutexV2::new(42);
/// {
///     let guard = mutex.lock().unwrap();
///     println!("Value: {}", *guard);
///     // 离开作用域自动释放锁
/// }
/// ```
pub struct MutexGuardV2<'a, T> {
    pub(crate) mutex: &'a MutexV2<T>,
    // 使用 PhantomData 标记此类型不应该被发送到其他任务
    // *const () 是 !Send 的，这使得 MutexGuardV2 也是 !Send
    _marker: PhantomData<*const ()>,
}

impl<'a, T> MutexGuardV2<'a, T> {
    /// 获取关联的互斥锁引用
    ///
    /// 这个方法主要用于条件变量等需要访问底层互斥锁的场景。
    pub fn mutex(&self) -> &'a MutexV2<T> {
        self.mutex
    }

    /// 手动解锁并返回互斥锁引用
    ///
    /// 这个方法会释放锁并返回互斥锁的引用，
    /// 允许在不等待 guard 离开作用域的情况下释放锁。
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// let mutex = MutexV2::new(42);
    /// let guard = mutex.lock().unwrap();
    /// let mutex_ref = MutexGuardV2::unlock(guard);
    /// // 锁已释放，可以再次获取
    /// let guard2 = mutex_ref.lock().unwrap();
    /// ```
    pub fn unlock(guard: Self) -> &'a MutexV2<T> {
        let mutex = guard.mutex;
        // 手动释放锁
        mutex.unlock();
        // 阻止 Drop 再次释放
        core::mem::forget(guard);
        mutex
    }
}

// 显式实现 !Send - MutexGuardV2 不能被发送到其他任务
// 这是故意的，因为锁必须在获取它的任务中释放
// 注意：由于 PhantomData<*const ()>，MutexGuardV2 已经是 !Send 的
// 但我们仍然可以显式实现 Sync（如果 T: Sync）
unsafe impl<T: Sync> Sync for MutexGuardV2<'_, T> {}

impl<T> Deref for MutexGuardV2<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: 我们持有锁，所以可以安全访问数据
        unsafe { &*self.mutex.inner.data.get() }
    }
}

impl<T> DerefMut for MutexGuardV2<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: 我们持有锁，所以可以安全访问数据
        unsafe { &mut *self.mutex.inner.data.get() }
    }
}

impl<T> Drop for MutexGuardV2<'_, T> {
    fn drop(&mut self) {
        // 检查是否在 panic 中（简化实现，实际需要更复杂的检测）
        // 这里我们假设正常释放
        self.mutex.unlock();
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for MutexGuardV2<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MutexGuardV2")
            .field("data", &**self)
            .finish()
    }
}

impl<T: core::fmt::Display> core::fmt::Display for MutexGuardV2<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

// ============================================================================
// OwnedMutexGuard - 拥有所有权的锁守卫
// ============================================================================

/// 拥有所有权的互斥锁守卫
///
/// 与 `MutexGuardV2` 不同，`OwnedMutexGuard` 持有 `Arc<MutexInner<T>>` 的所有权，
/// 因此可以被 move 到其他任务中（虽然通常不建议这样做）。
///
/// 主要用于需要将锁守卫存储在结构体中或跨 await 点持有的场景。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::MutexV2;
///
/// let mutex = MutexV2::new(42);
/// let guard = mutex.lock_owned().unwrap();
/// 
/// // guard 可以被存储或移动
/// let value = *guard;
/// drop(guard); // 显式释放
/// ```
pub struct OwnedMutexGuard<T> {
    mutex: Arc<MutexInner<T>>,
}

// OwnedMutexGuard 可以 Send（如果 T: Send）
// 这是与 MutexGuardV2 的主要区别
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
        // 优先级继承：恢复原始优先级
        if self.mutex.priority_inheritance {
            let owner_id = self.mutex.owner.load(Ordering::Acquire);
            if owner_id != usize::MAX {
                let original_priority = {
                    let mut orig = self.mutex.owner_original_priority.lock();
                    orig.take()
                };
                
                if let Some(priority) = original_priority {
                    crate::kernel::task::Task::for_each(|mut task, id| {
                        if id == owner_id {
                            task.set_priority(priority);
                        }
                    });
                }
            }
        }

        // 清除持有者
        self.mutex.owner.store(usize::MAX, Ordering::Release);
        
        // 释放锁
        self.mutex.locked.store(false, Ordering::Release);

        // 唤醒等待者
        let task_id = {
            let mut waiters = self.mutex.waiters.lock();
            waiters.pop_front()
        };

        if let Some(task_id) = task_id {
            crate::kernel::task::Task::for_each(|task, id| {
                if id == task_id {
                    if let TaskState::Blocked(_) = task.get_state() {
                        task.ready();
                    }
                }
            });
            return;
        }

        let waker = {
            let mut async_waiters = self.mutex.async_waiters.lock();
            async_waiters.pop_front()
        };

        if let Some(waker) = waker {
            waker.wake();
        }
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

impl<T> MutexV2<T> {
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
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// let mutex = MutexV2::new(42);
    /// let guard = mutex.lock_owned_guard().unwrap();
    /// // guard 可以被存储在结构体中
    /// ```

    /// 从 MutexV2 获取拥有所有权的锁守卫
    pub fn lock_owned_guard(&self) -> Result<OwnedMutexGuard<T>> {
        // 检查是否已毒化
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        loop {
            // 尝试获取锁
            if self.inner.locked.compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            ).is_ok() {
                // 成功获取锁
                let current = Scheduler::get_current_task();
                let task_id = current.get_taskid();
                self.inner.owner.store(task_id, Ordering::Release);
                
                // 如果启用优先级继承，保存原始优先级
                if self.inner.priority_inheritance {
                    let mut orig_priority = self.inner.owner_original_priority.lock();
                    *orig_priority = Some(current.get_priority());
                }
                
                return Ok(OwnedMutexGuard { 
                    mutex: Arc::clone(&self.inner),
                });
            }

            // 锁被占用，加入等待队列并阻塞
            let current = Scheduler::get_current_task();
            let task_id = current.get_taskid();
            let current_priority = current.get_priority();

            {
                let mut waiters = self.inner.waiters.lock();
                if !waiters.push(task_id) {
                    return Err(RtosError::WaiterQueueFull);
                }
            }

            // 优先级继承
            if self.inner.priority_inheritance {
                let owner_id = self.inner.owner.load(Ordering::Acquire);
                if owner_id != usize::MAX {
                    crate::kernel::task::Task::for_each(|mut task, id| {
                        if id == owner_id {
                            let owner_priority = task.get_priority();
                            if current_priority > owner_priority {
                                task.set_priority(current_priority);
                            }
                        }
                    });
                }
            }

            let mutex_id = Arc::as_ptr(&self.inner) as usize;
            Scheduler::get_current_task().block(crate::sync::event::Event::Mutex(mutex_id));
            trigger_schedule();

            if self.inner.poisoned.load(Ordering::Acquire) {
                return Err(RtosError::MutexPoisoned);
            }
        }
    }

    /// 尝试获取拥有所有权的锁守卫���非阻塞）
    pub fn try_lock_owned(&self) -> Result<OwnedMutexGuard<T>> {
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        if self.inner.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed,
        ).is_ok() {
            let task_id = Scheduler::get_current_task().get_taskid();
            self.inner.owner.store(task_id, Ordering::Release);
            Ok(OwnedMutexGuard { 
                mutex: Arc::clone(&self.inner),
            })
        } else {
            Err(RtosError::WouldBlock)
        }
    }
}

// ============================================================================
// MappedMutexGuard - 映射锁守卫
// ============================================================================

/// 映射的互斥锁守卫
///
/// 允许将 `MutexGuardV2<T>` 映射到 `MappedMutexGuard<U>`，
/// 其中 `U` 是 `T` 的一部分。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::MutexV2;
///
/// struct Data {
///     field1: i32,
///     field2: String,
/// }
///
/// let mutex = MutexV2::new(Data { field1: 42, field2: "hello".into() });
/// let guard = mutex.lock().unwrap();
/// 
/// // 映射到 field1
/// let mapped = MutexGuardV2::map(guard, |data| &mut data.field1);
/// *mapped = 100;
/// ```
pub struct MappedMutexGuard<'a, T, U> {
    data: *mut U,
    mutex: &'a MutexV2<T>,
    _marker: PhantomData<&'a mut U>,
}

impl<'a, T, U> MappedMutexGuard<'a, T, U> {
    /// 创建映射守卫（内部使用）
    fn new(data: *mut U, mutex: &'a MutexV2<T>) -> Self {
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

impl<'a, T> MutexGuardV2<'a, T> {
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
    pub fn try_map<U, F>(guard: Self, f: F) -> core::result::Result<MappedMutexGuard<'a, T, U>, Self>
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
// MutexV2 异步支持
// ============================================================================

/// MutexV2 的异步锁获取 Future
///
/// 实现 `Future` trait，允许在 async/await 上下文中获取锁。
pub struct MutexLockFuture<'a, T> {
    mutex: &'a MutexV2<T>,
    registered: bool,
}

impl<'a, T> MutexLockFuture<'a, T> {
    fn new(mutex: &'a MutexV2<T>) -> Self {
        Self {
            mutex,
            registered: false,
        }
    }
}

impl<'a, T> core::future::Future for MutexLockFuture<'a, T> {
    type Output = Result<MutexGuardV2<'a, T>>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        // 检查是否已毒化
        if self.mutex.inner.poisoned.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::MutexPoisoned));
        }

        // 尝试获取锁
        if self.mutex.inner.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed,
        ).is_ok() {
            // 成功获取锁
            let task_id = Scheduler::get_current_task().get_taskid();
            self.mutex.inner.owner.store(task_id, Ordering::Release);
            return core::task::Poll::Ready(Ok(MutexGuardV2 { 
                mutex: self.mutex, 
                _marker: PhantomData 
            }));
        }

        // 锁被占用，注册 waker
        if !self.registered {
            let mut async_waiters = self.mutex.inner.async_waiters.lock();
            async_waiters.push_back(cx.waker().clone());
            self.registered = true;
        }

        core::task::Poll::Pending
    }
}

impl<T> MutexV2<T> {
    /// 异步获取锁
    ///
    /// 返回一个 Future，在获取到锁时完成。
    /// 可以在 async/await 上下文中使用。
    ///
    /// # 返回值
    /// - `Ok(MutexGuardV2)`: 成功获取锁
    /// - `Err(RtosError::MutexPoisoned)`: 锁已被毒化
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::MutexV2;
    ///
    /// async fn example(mutex: MutexV2<i32>) {
    ///     let guard = mutex.lock_async().await.unwrap();
    ///     // 使用数据
    /// }
    /// ```
    pub fn lock_async(&self) -> MutexLockFuture<'_, T> {
        MutexLockFuture::new(self)
    }

    /// 获取等待者数量（包括同步和异步等待者）
    pub fn waiter_count(&self) -> usize {
        self.inner.waiters.lock().len() + self.inner.async_waiters.lock().len()
    }

    /// 获取同步等待者数量
    pub fn sync_waiter_count(&self) -> usize {
        self.inner.waiters.lock().len()
    }

    /// 获取异步等待者数量
    pub fn async_waiter_count(&self) -> usize {
        self.inner.async_waiters.lock().len()
    }
}

// ============================================================================
// RwLockV2 - 读写锁
// ============================================================================

/// 读写锁内部状态
struct RwLockInner<T> {
    /// 被保护的数据
    data: UnsafeCell<T>,
    /// 读者计数（正数表示读者数量，-1 表示有写者）
    /// 0 = 无锁，>0 = 读锁数量，-1 = 写锁
    state: AtomicIsize,
    /// 写者等待队列（同步）
    write_waiters: SpinMutex<WaiterList>,
    /// 读者等待队列（同步）
    read_waiters: SpinMutex<WaiterList>,
    /// 异步写者等待队列
    async_write_waiters: SpinMutex<VecDeque<Waker>>,
    /// 异步读者等待队列
    async_read_waiters: SpinMutex<VecDeque<Waker>>,
    /// 是否已被毒化
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
            write_waiters: SpinMutex::new(WaiterList::new()),
            read_waiters: SpinMutex::new(WaiterList::new()),
            async_write_waiters: SpinMutex::new(VecDeque::new()),
            async_read_waiters: SpinMutex::new(VecDeque::new()),
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
/// use neon_rtos2::sync::RwLockV2;
///
/// let lock = RwLockV2::new(vec![1, 2, 3]);
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
pub struct RwLockV2<T> {
    inner: Arc<RwLockInner<T>>,
}

impl<T> Clone for RwLockV2<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> RwLockV2<T> {
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
        // 检查是否已毒化
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        loop {
            let state = self.inner.state.load(Ordering::Acquire);
            
            // 如果没有写者（state >= 0），尝试增加读者计数
            if state >= 0 {
                if self.inner.state.compare_exchange(
                    state,
                    state + 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(RwLockReadGuard { lock: self, _marker: PhantomData });
                }
                // CAS 失败，重试
                continue;
            }

            // 有写者，需要阻塞
            let current = Scheduler::get_current_task();
            let task_id = current.get_taskid();

            {
                let mut read_waiters = self.inner.read_waiters.lock();
                if !read_waiters.push(task_id) {
                    return Err(RtosError::WaiterQueueFull);
                }
            }

            let lock_id = Arc::as_ptr(&self.inner) as usize;
            Scheduler::get_current_task().block(crate::sync::event::Event::Mutex(lock_id));
            trigger_schedule();

            // 被唤醒后检查是否毒化
            if self.inner.poisoned.load(Ordering::Acquire) {
                return Err(RtosError::MutexPoisoned);
            }
        }
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
                if self.inner.state.compare_exchange(
                    state,
                    state + 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(RwLockReadGuard { lock: self, _marker: PhantomData });
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
        if self.inner.poisoned.load(Ordering::Acquire) {
            return Err(RtosError::MutexPoisoned);
        }

        loop {
            // 尝试从 0 变为 -1（获取写锁）
            if self.inner.state.compare_exchange(
                0,
                -1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ).is_ok() {
                return Ok(RwLockWriteGuard { lock: self, _marker: PhantomData });
            }

            // 锁被占用，需要阻塞
            let current = Scheduler::get_current_task();
            let task_id = current.get_taskid();

            {
                let mut write_waiters = self.inner.write_waiters.lock();
                if !write_waiters.push(task_id) {
                    return Err(RtosError::WaiterQueueFull);
                }
            }

            let lock_id = Arc::as_ptr(&self.inner) as usize;
            Scheduler::get_current_task().block(crate::sync::event::Event::Mutex(lock_id));
            trigger_schedule();

            if self.inner.poisoned.load(Ordering::Acquire) {
                return Err(RtosError::MutexPoisoned);
            }
        }
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

        if self.inner.state.compare_exchange(
            0,
            -1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ).is_ok() {
            Ok(RwLockWriteGuard { lock: self, _marker: PhantomData })
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
        let prev = self.inner.state.fetch_sub(1, Ordering::Release);
        
        // 如果这是最后一个读者，尝试唤醒写者
        if prev == 1 {
            // 首先尝试唤醒同步写者
            let task_id = {
                let mut write_waiters = self.inner.write_waiters.lock();
                write_waiters.pop_front()
            };

            if let Some(task_id) = task_id {
                crate::kernel::task::Task::for_each(|task, id| {
                    if id == task_id {
                        if let TaskState::Blocked(_) = task.get_state() {
                            task.ready();
                        }
                    }
                });
                return;
            }

            // 然后尝试唤醒异步写者
            let waker = {
                let mut async_write_waiters = self.inner.async_write_waiters.lock();
                async_write_waiters.pop_front()
            };

            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    /// 内部方法：释放写锁
    fn unlock_write(&self) {
        self.inner.state.store(0, Ordering::Release);

        // 优先唤醒所有等待的读者
        let read_task_ids: [Option<usize>; 16];
        {
            let mut read_waiters = self.inner.read_waiters.lock();
            read_task_ids = read_waiters.drain();
        }

        let mut woke_readers = false;
        for task_id in read_task_ids.iter().filter_map(|&id| id) {
            woke_readers = true;
            crate::kernel::task::Task::for_each(|task, id| {
                if id == task_id {
                    if let TaskState::Blocked(_) = task.get_state() {
                        task.ready();
                    }
                }
            });
        }

        // 唤醒异步读者
        let async_read_wakers: VecDeque<Waker>;
        {
            let mut async_read_waiters = self.inner.async_read_waiters.lock();
            async_read_wakers = core::mem::take(&mut *async_read_waiters);
        }

        for waker in async_read_wakers {
            woke_readers = true;
            waker.wake();
        }

        // 如果没有读者等待，唤醒一个写者
        if !woke_readers {
            let task_id = {
                let mut write_waiters = self.inner.write_waiters.lock();
                write_waiters.pop_front()
            };

            if let Some(task_id) = task_id {
                crate::kernel::task::Task::for_each(|task, id| {
                    if id == task_id {
                        if let TaskState::Blocked(_) = task.get_state() {
                            task.ready();
                        }
                    }
                });
                return;
            }

            let waker = {
                let mut async_write_waiters = self.inner.async_write_waiters.lock();
                async_write_waiters.pop_front()
            };

            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}

impl<T: Default> Default for RwLockV2<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for RwLockV2<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("RwLockV2");
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
pub struct RwLockReadGuard<'a, T> {
    lock: &'a RwLockV2<T>,
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
pub struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLockV2<T>,
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
    lock: &'a RwLockV2<T>,
    registered: bool,
}

impl<'a, T> RwLockReadFuture<'a, T> {
    fn new(lock: &'a RwLockV2<T>) -> Self {
        Self { lock, registered: false }
    }
}

impl<'a, T> core::future::Future for RwLockReadFuture<'a, T> {
    type Output = Result<RwLockReadGuard<'a, T>>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        if self.lock.inner.poisoned.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::MutexPoisoned));
        }

        loop {
            let state = self.lock.inner.state.load(Ordering::Acquire);
            
            if state >= 0 {
                if self.lock.inner.state.compare_exchange(
                    state,
                    state + 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return core::task::Poll::Ready(Ok(RwLockReadGuard { 
                        lock: self.lock, 
                        _marker: PhantomData 
                    }));
                }
                continue;
            }

            // 有写者，注册 waker
            if !self.registered {
                let mut async_read_waiters = self.lock.inner.async_read_waiters.lock();
                async_read_waiters.push_back(cx.waker().clone());
                self.registered = true;
            }

            return core::task::Poll::Pending;
        }
    }
}

/// 异步写锁 Future
pub struct RwLockWriteFuture<'a, T> {
    lock: &'a RwLockV2<T>,
    registered: bool,
}

impl<'a, T> RwLockWriteFuture<'a, T> {
    fn new(lock: &'a RwLockV2<T>) -> Self {
        Self { lock, registered: false }
    }
}

impl<'a, T> core::future::Future for RwLockWriteFuture<'a, T> {
    type Output = Result<RwLockWriteGuard<'a, T>>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        if self.lock.inner.poisoned.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::MutexPoisoned));
        }

        if self.lock.inner.state.compare_exchange(
            0,
            -1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ).is_ok() {
            return core::task::Poll::Ready(Ok(RwLockWriteGuard { 
                lock: self.lock, 
                _marker: PhantomData 
            }));
        }

        // 锁被占用，注册 waker
        if !self.registered {
            let mut async_write_waiters = self.lock.inner.async_write_waiters.lock();
            async_write_waiters.push_back(cx.waker().clone());
            self.registered = true;
        }

        core::task::Poll::Pending
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
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(42);
        
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
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(0);
        
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
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex1 = MutexV2::new(0);
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
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(vec![1, 2, 3]);
        
        // 使用 with_lock 闭包风格
        let sum = mutex.with_lock(|data| {
            data.iter().sum::<i32>()
        }).unwrap();
        
        assert_eq!(sum, 6);
        
        // 修改数据
        mutex.with_lock(|data| {
            data.push(4);
        }).unwrap();
        
        // 验证修改
        let len = mutex.with_lock(|data| data.len()).unwrap();
        assert_eq!(len, 4);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_is_locked() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(0);
        
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
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex: MutexV2<i32> = MutexV2::default();
        
        let guard = mutex.lock().unwrap();
        assert_eq!(*guard, 0); // i32 的默认值是 0
    }

    #[test]
    #[serial]
    fn test_mutex_v2_debug() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(42);
        
        let debug_str = format!("{:?}", mutex);
        assert!(debug_str.contains("MutexV2"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    #[serial]
    fn test_mutex_v2_try_lock_timeout() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(42);
        
        // 第一次获取应该成功
        let guard = mutex.try_lock_timeout(0).unwrap();
        assert_eq!(*guard, 42);
        drop(guard);
        
        // 释放后再次获取应该成功
        let guard2 = mutex.try_lock_timeout(100).unwrap();
        assert_eq!(*guard2, 42);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_lock_timeout() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(0);
        
        // 获取锁
        let guard = mutex.lock_timeout(1000).unwrap();
        assert_eq!(*guard, 0);
        drop(guard);
        
        // 再次获取应该成功
        let guard2 = mutex.lock_timeout(1000).unwrap();
        assert_eq!(*guard2, 0);
    }

    #[test]
    #[serial]
    fn test_mutex_v2_owned_guard() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(42);
        
        // 获取 owned guard
        let guard = mutex.lock_owned_guard().unwrap();
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
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        struct Data {
            field1: i32,
            field2: i32,
        }
        
        let mutex = MutexV2::new(Data { field1: 10, field2: 20 });
        
        // 映射到 field1
        {
            let guard = mutex.lock().unwrap();
            let mut mapped = MutexGuardV2::map(guard, |data| &mut data.field1);
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
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        let mutex = MutexV2::new(Some(42i32));
        
        // 成功映射
        {
            let guard = mutex.lock().unwrap();
            let mapped = MutexGuardV2::try_map(guard, |opt| opt.as_mut());
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
            let result = MutexGuardV2::try_map(guard, |opt| opt.as_mut());
            assert!(result.is_err());
        }
    }

    #[test]
    #[serial]
    fn test_mutex_v2_priority_inheritance() {
        kernel_init();
        Task::new("test", |_| {}).unwrap();
        crate::kernel::scheduler::Scheduler::start();
        
        // 创建带优先级继承的互斥锁
        let mutex = MutexV2::with_priority_inheritance(42);
        assert!(mutex.has_priority_inheritance());
        
        // 基本功能测试
        {
            let guard = mutex.lock().unwrap();
            assert_eq!(*guard, 42);
        }
    }
}

