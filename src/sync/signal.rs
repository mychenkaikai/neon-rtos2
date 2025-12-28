//! # Signal - 支持闭包传递的信号量
//!
//! 基于 Arc 的信号量实现，无需全局变量，可以通过闭包传递。
//!
//! ## 设计思路
//!
//! 使用 `Arc<SignalInner>` 的内存地址作为唯一标识，
//! 可以像 `std::sync::mpsc::channel` 一样在局部创建并传递给任务。
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::Signal;
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     // 局部创建，无需 static！
//!     let signal = Signal::new();
//!     let signal_clone = signal.clone();
//!
//!     Task::builder("producer")
//!         .spawn(move |_| {
//!             loop {
//!                 // 产生数据...
//!                 signal.send();
//!                 // delay...
//!             }
//!         });
//!
//!     Task::builder("consumer")
//!         .spawn(move |_| {
//!             loop {
//!                 signal_clone.wait().unwrap();
//!                 // 处理数据...
//!             }
//!         });
//! }
//! ```

use crate::compat::{Arc, VecDeque};
use crate::kernel::scheduler::Scheduler;
use crate::kernel::task::{Task, TaskState};
use crate::kernel::time::systick::Systick;
use crate::hal::trigger_schedule;
use crate::error::{Result, RtosError};
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use core::task::Waker;
use spin::Mutex;

/// 信号量内部状态
struct SignalInner {
    /// 信号计数（用于计数信号量场景）
    count: AtomicUsize,
    /// 等待者列表（存储被阻塞任务的 ID）- 用于同步等待
    waiters: Mutex<WaiterList>,
    /// 异步等待者列表（存储 Waker）- 用于异步等待
    async_waiters: Mutex<VecDeque<Waker>>,
    /// 是否已关闭
    closed: AtomicBool,
    /// 超时时间点（用于超时等待）
    timeout: AtomicUsize,
}

/// 等待者列表
/// 使用固定大小数组避免动态分配
pub struct WaiterList {
    tasks: [Option<usize>; 16], // 最多 16 个等待者
    len: usize,
}

impl WaiterList {
    /// 创建新的等待者列表
    pub const fn new() -> Self {
        Self {
            tasks: [None; 16],
            len: 0,
        }
    }

    /// 添加等待者
    pub fn push(&mut self, task_id: usize) -> bool {
        if self.len < self.tasks.len() {
            self.tasks[self.len] = Some(task_id);
            self.len += 1;
            true
        } else {
            false // 等待队列已满
        }
    }

    /// 弹出一个等待者（LIFO）
    pub fn pop(&mut self) -> Option<usize> {
        if self.len > 0 {
            self.len -= 1;
            self.tasks[self.len].take()
        } else {
            None
        }
    }

    /// 弹出第一个等待者（FIFO）
    pub fn pop_front(&mut self) -> Option<usize> {
        if self.len > 0 {
            let task_id = self.tasks[0].take();
            // 移动所有元素
            for i in 0..self.len - 1 {
                self.tasks[i] = self.tasks[i + 1];
            }
            self.tasks[self.len - 1] = None;
            self.len -= 1;
            task_id
        } else {
            None
        }
    }

    /// 移除指定的等待者
    pub fn remove(&mut self, task_id: usize) -> bool {
        for i in 0..self.len {
            if self.tasks[i] == Some(task_id) {
                // 移除并压缩数组
                for j in i..self.len - 1 {
                    self.tasks[j] = self.tasks[j + 1];
                }
                self.tasks[self.len - 1] = None;
                self.len -= 1;
                return true;
            }
        }
        false
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 获取等待者数量
    pub fn len(&self) -> usize {
        self.len
    }

    /// 获取所有等待者的迭代器
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.tasks[..self.len].iter().filter_map(|&id| id)
    }

    /// 清空所有等待者，返回被清空的任务 ID 列表
    pub fn drain(&mut self) -> [Option<usize>; 16] {
        let mut result = [None; 16];
        for i in 0..self.len {
            result[i] = self.tasks[i].take();
        }
        self.len = 0;
        result
    }
}

impl Default for WaiterList {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalInner {
    fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            waiters: Mutex::new(WaiterList::new()),
            async_waiters: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
            timeout: AtomicUsize::new(0),
        }
    }

    fn new_with_count(initial_count: usize) -> Self {
        Self {
            count: AtomicUsize::new(initial_count),
            waiters: Mutex::new(WaiterList::new()),
            async_waiters: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
            timeout: AtomicUsize::new(0),
        }
    }
}

/// 可克隆、可传递的信号量
///
/// 类似于 `std::sync::mpsc::Sender`，可以 clone 后 move 到不同任务中。
#[derive(Clone)]
pub struct Signal {
    inner: Arc<SignalInner>,
}

impl Signal {
    /// 创建新的信号量
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Signal;
    ///
    /// let signal = Signal::new();
    /// let signal_clone = signal.clone(); // 可以克隆
    /// ```
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SignalInner::new()),
        }
    }

    /// 创建带初始计数的信号量
    ///
    /// # 参数
    /// - `initial_count`: 初始信号计数
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Signal;
    ///
    /// // 创建一个已有 3 个信号的信号量
    /// let signal = Signal::with_count(3);
    /// assert!(signal.try_wait()); // 立即成功
    /// ```
    pub fn with_count(initial_count: usize) -> Self {
        Self {
            inner: Arc::new(SignalInner::new_with_count(initial_count)),
        }
    }

    /// 发送信号
    ///
    /// 唤醒一个等待中的任务（FIFO 顺序）。如果没有等待者，信号会被"存储"，
    /// 下一个调用 `wait()` 的任务会立即返回。
    ///
    /// # 示例
    /// ```rust,no_run
    /// # use neon_rtos2::sync::Signal;
    /// let signal = Signal::new();
    /// signal.send(); // 发送信号
    /// ```
    pub fn send(&self) {
        if self.inner.closed.load(Ordering::Acquire) {
            return; // 信号量已关闭，忽略发送
        }

        // 首先尝试唤醒同步等待者
        let mut waiters = self.inner.waiters.lock();
        
        if let Some(task_id) = waiters.pop_front() {
            // 有同步等待者，直接唤醒（FIFO 顺序）
            drop(waiters);
            Self::wake_task_by_id(task_id);
            return;
        }
        drop(waiters);

        // 然后尝试唤醒异步等待者
        let mut async_waiters = self.inner.async_waiters.lock();
        if let Some(waker) = async_waiters.pop_front() {
            drop(async_waiters);
            waker.wake();
            return;
        }
        drop(async_waiters);

        // 没有任何等待者，增加计数
        self.inner.count.fetch_add(1, Ordering::Release);
    }

    /// 发送信号并触发调度
    ///
    /// 与 `send()` 相同，但会立即触发任务调度。
    pub fn send_and_schedule(&self) {
        self.send();
        trigger_schedule();
    }

    /// 广播信号 - 唤醒所有等待者
    ///
    /// 与 `send()` 不同，`broadcast()` 会唤醒所有等待中的任务，
    /// 而不仅仅是一个。
    ///
    /// # 示例
    /// ```rust,no_run
    /// # use neon_rtos2::sync::Signal;
    /// let signal = Signal::new();
    /// // 假设有多个任务在等待
    /// signal.broadcast(); // 唤醒所有等待者
    /// ```
    pub fn broadcast(&self) {
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
            Self::wake_task_by_id(task_id);
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

    /// 广播信号并触发调度
    pub fn broadcast_and_schedule(&self) {
        self.broadcast();
        trigger_schedule();
    }

    /// 等待信号
    ///
    /// 如果有待处理的信号（count > 0），立即返回 Ok。
    /// 否则阻塞当前任务，直到收到信号。
    /// 如果信号量已关闭，返回 Err。
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取信号
    /// - `Err(RtosError::SignalClosed)`: 信号量已关闭
    ///
    /// # 示例
    /// ```rust,no_run
    /// # use neon_rtos2::sync::Signal;
    /// let signal = Signal::new();
    /// if signal.wait().is_ok() {
    ///     // 获取到信号
    /// }
    /// ```
    pub fn wait(&self) -> Result<()> {
        // 检查是否已关闭
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SignalClosed);
        }

        // 先检查是否有待处理的信号
        loop {
            let count = self.inner.count.load(Ordering::Acquire);
            if count > 0 {
                // 尝试消费一个信号
                if self.inner.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(()); // 成功获取信号，立即返回
                }
                // CAS 失败，重试
                continue;
            }
            break;
        }

        // 没有待处理的信号，需要阻塞
        let current = Scheduler::get_current_task();
        let task_id = current.get_taskid();

        // 将当前任务加入等待队列
        {
            let mut waiters = self.inner.waiters.lock();
            
            // 再次检查是否已关闭（双重检查）
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(RtosError::SignalClosed);
            }
            
            if !waiters.push(task_id) {
                return Err(RtosError::WaiterQueueFull);
            }
        }

        // 使用 Arc 的地址作为唯一标识
        let signal_id = Arc::as_ptr(&self.inner) as usize;
        
        // 阻塞当前任务
        Scheduler::get_current_task().block(crate::sync::event::Event::Signal(signal_id));
        trigger_schedule();

        // 被唤醒后检查是否因为关闭而唤醒
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SignalClosed);
        }

        Ok(())
    }

    /// 尝试等待信号（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(true)`: 成功获取信号
    /// - `Ok(false)`: 没有可用信号
    /// - `Err(RtosError::SignalClosed)`: 信号量已关闭
    ///
    /// # 示例
    /// ```rust,no_run
    /// # use neon_rtos2::sync::Signal;
    /// let signal = Signal::new();
    /// match signal.try_wait() {
    ///     Ok(true) => { /* 获取到信号 */ }
    ///     Ok(false) => { /* 没有信号 */ }
    ///     Err(_) => { /* 信号量已关闭 */ }
    /// }
    /// ```
    pub fn try_wait(&self) -> Result<bool> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SignalClosed);
        }

        loop {
            let count = self.inner.count.load(Ordering::Acquire);
            if count > 0 {
                if self.inner.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(true);
                }
                // CAS 失败，重试
                continue;
            }
            return Ok(false);
        }
    }

    /// 带超时的等待信号
    ///
    /// 如果在指定时间内收到信号，返回 `Ok(())`。
    /// 如果超时，返回 `Err(RtosError::Timeout)`。
    ///
    /// # 参数
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取信号
    /// - `Err(RtosError::Timeout)`: 等待超时
    /// - `Err(RtosError::SignalClosed)`: 信号量已关闭
    ///
    /// # 示例
    /// ```rust,no_run
    /// # use neon_rtos2::sync::Signal;
    /// let signal = Signal::new();
    /// match signal.wait_timeout(1000) {
    ///     Ok(()) => { /* 获取到信号 */ }
    ///     Err(neon_rtos2::error::RtosError::Timeout) => { /* 超时 */ }
    ///     Err(_) => { /* 其他错误 */ }
    /// }
    /// ```
    pub fn wait_timeout(&self, timeout_ms: usize) -> Result<()> {
        // 检查是否已关闭
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SignalClosed);
        }

        // 先检查是否有待处理的信号
        loop {
            let count = self.inner.count.load(Ordering::Acquire);
            if count > 0 {
                if self.inner.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return Ok(()); // 成功获取信号，立即返回
                }
                continue;
            }
            break;
        }

        // 计算超时时间点
        let deadline = Systick::get_current_time() + timeout_ms;

        // 没有待处理的信号，需要阻塞
        let current = Scheduler::get_current_task();
        let task_id = current.get_taskid();

        // 将当前任务加入等待队列
        {
            let mut waiters = self.inner.waiters.lock();
            
            // 再次检查是否已关闭
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(RtosError::SignalClosed);
            }
            
            if !waiters.push(task_id) {
                return Err(RtosError::WaiterQueueFull);
            }
        }

        // 设置超时时间
        self.inner.timeout.store(deadline, Ordering::Release);

        // 使用 Arc 的地址作为唯一标识
        let signal_id = Arc::as_ptr(&self.inner) as usize;
        
        // 阻塞当前任务
        Scheduler::get_current_task().block(crate::sync::event::Event::Signal(signal_id));
        trigger_schedule();

        // 被唤醒后检查原因
        // 1. 检查是否因为关闭而唤醒
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(RtosError::SignalClosed);
        }

        // 2. 检查是否超时
        if Systick::get_current_time() >= deadline {
            // 从等待队列中移除自己（可能已经被移除了）
            let mut waiters = self.inner.waiters.lock();
            waiters.remove(task_id);
            return Err(RtosError::Timeout);
        }

        Ok(())
    }

    /// 带超时的尝试等待（轮询模式）
    ///
    /// 在指定时间内反复尝试获取信号，不会阻塞任务。
    /// 适用于不想让任务进入阻塞状态的场景。
    ///
    /// # 参数
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # 返回值
    /// - `Ok(true)`: 成功获取信号
    /// - `Ok(false)`: 超时，未获取到信号
    /// - `Err(RtosError::SignalClosed)`: 信号量已关闭
    pub fn try_wait_timeout(&self, timeout_ms: usize) -> Result<bool> {
        let deadline = Systick::get_current_time() + timeout_ms;
        
        loop {
            match self.try_wait() {
                Ok(true) => return Ok(true),
                Ok(false) => {
                    if Systick::get_current_time() >= deadline {
                        return Ok(false);
                    }
                    // 继续轮询
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 关闭信号量
    ///
    /// 关闭后，所有等待的任务会被唤醒并收到错误，
    /// 后续的 `wait()` 调用会立即返回错误。
    ///
    /// # 示例
    /// ```rust,no_run
    /// # use neon_rtos2::sync::Signal;
    /// let signal = Signal::new();
    /// signal.close();
    /// assert!(signal.wait().is_err());
    /// ```
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        // 唤醒所有等待者
        self.broadcast();
    }

    /// 检查信号量是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// 重置信号量
    ///
    /// 清除所有待处理的信号，重新打开信号量。
    /// 注意：不会影响正在等待的任务。
    pub fn reset(&self) {
        self.inner.count.store(0, Ordering::Release);
        self.inner.closed.store(false, Ordering::Release);
    }

    /// 获取当前信号计数
    pub fn count(&self) -> usize {
        self.inner.count.load(Ordering::Relaxed)
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

    /// 获取信号的唯一标识（用于调试）
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    /// 内部辅助函数：根据任务 ID 唤醒任务
    fn wake_task_by_id(task_id: usize) {
        Task::for_each(|task, id| {
            if id == task_id {
                if let TaskState::Blocked(_) = task.get_state() {
                    task.ready();
                }
            }
        });
    }
}

impl Default for Signal {
    fn default() -> Self {
        Self::new()
    }
}

// 实现 Debug trait 方便调试
impl core::fmt::Debug for Signal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Signal")
            .field("id", &self.id())
            .field("count", &self.count())
            .field("waiters", &self.waiter_count())
            .field("closed", &self.is_closed())
            .finish()
    }
}
///
/// 这提供了更明确的 API，发送端只能发送，接收端只能接收。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::signal::signal_pair;
///
/// let (sender, receiver) = signal_pair();
///
/// // sender 只能 send
/// sender.send();
///
/// // receiver 只能 wait
/// receiver.wait();
/// ```
pub fn signal_pair() -> (SignalSender, SignalReceiver) {
    let signal = Signal::new();
    (
        SignalSender { inner: signal.clone() },
        SignalReceiver { inner: signal },
    )
}

/// 信号发送端
///
/// 只能发送信号，不能等待。
#[derive(Clone)]
pub struct SignalSender {
    inner: Signal,
}

impl SignalSender {
    /// 发送信号
    pub fn send(&self) {
        self.inner.send();
    }

    /// 发送信号并触发调度
    pub fn send_and_schedule(&self) {
        self.inner.send_and_schedule();
    }

    /// ��播信号
    pub fn broadcast(&self) {
        self.inner.broadcast();
    }

    /// 广播信号并触发调度
    pub fn broadcast_and_schedule(&self) {
        self.inner.broadcast_and_schedule();
    }

    /// 关闭信号量
    pub fn close(&self) {
        self.inner.close();
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

impl core::fmt::Debug for SignalSender {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SignalSender")
            .field("id", &self.inner.id())
            .field("closed", &self.inner.is_closed())
            .finish()
    }
}

/// 信号接收端
///
/// 只能等待信号，不能发送。
pub struct SignalReceiver {
    inner: Signal,
}

impl SignalReceiver {
    /// 等待信号
    pub fn wait(&self) -> Result<()> {
        self.inner.wait()
    }

    /// 尝试等待信号（非阻塞）
    pub fn try_wait(&self) -> Result<bool> {
        self.inner.try_wait()
    }

    /// 带超时的等待信号
    pub fn wait_timeout(&self, timeout_ms: usize) -> Result<()> {
        self.inner.wait_timeout(timeout_ms)
    }

    /// 带超时的尝试等待（轮询模式）
    pub fn try_wait_timeout(&self, timeout_ms: usize) -> Result<bool> {
        self.inner.try_wait_timeout(timeout_ms)
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// 获取当前信号计数
    pub fn count(&self) -> usize {
        self.inner.count()
    }
}

impl core::fmt::Debug for SignalReceiver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SignalReceiver")
            .field("id", &self.inner.id())
            .field("count", &self.inner.count())
            .field("closed", &self.inner.is_closed())
            .finish()
    }
}

// ============================================================================
// 异步支持
// ============================================================================

/// Signal 的异步等待 Future
///
/// 实现 `Future` trait，允许在 async/await 上下文中等待信号。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::Signal;
///
/// async fn wait_for_signal(signal: &Signal) {
///     signal.wait_async().await.unwrap();
///     // 收到信号后继续执行
/// }
/// ```
pub struct SignalFuture<'a> {
    signal: &'a Signal,
    registered: bool,
}

impl<'a> SignalFuture<'a> {
    fn new(signal: &'a Signal) -> Self {
        Self {
            signal,
            registered: false,
        }
    }
}

impl<'a> core::future::Future for SignalFuture<'a> {
    type Output = Result<()>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        // 检查是否已关闭
        if self.signal.inner.closed.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::SignalClosed));
        }

        // 尝试获取信号
        loop {
            let count = self.signal.inner.count.load(Ordering::Acquire);
            if count > 0 {
                if self.signal.inner.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return core::task::Poll::Ready(Ok(()));
                }
                // CAS 失败，重试
                continue;
            }
            break;
        }

        // 没有信号，注册 waker
        if !self.registered {
            let mut async_waiters = self.signal.inner.async_waiters.lock();
            async_waiters.push_back(cx.waker().clone());
            self.registered = true;
        }

        core::task::Poll::Pending
    }
}

/// 带超时的异步等待 Future
pub struct SignalTimeoutFuture<'a> {
    signal: &'a Signal,
    deadline: usize,
    registered: bool,
}

impl<'a> SignalTimeoutFuture<'a> {
    fn new(signal: &'a Signal, timeout_ms: usize) -> Self {
        Self {
            signal,
            deadline: Systick::get_current_time() + timeout_ms,
            registered: false,
        }
    }
}

impl<'a> core::future::Future for SignalTimeoutFuture<'a> {
    type Output = Result<()>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        // 检查是否已关闭
        if self.signal.inner.closed.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::SignalClosed));
        }

        // 检查是否超时
        if Systick::get_current_time() >= self.deadline {
            return core::task::Poll::Ready(Err(RtosError::Timeout));
        }

        // 尝试获取信号
        loop {
            let count = self.signal.inner.count.load(Ordering::Acquire);
            if count > 0 {
                if self.signal.inner.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return core::task::Poll::Ready(Ok(()));
                }
                continue;
            }
            break;
        }

        // 没有信号，注册 waker
        if !self.registered {
            let mut async_waiters = self.signal.inner.async_waiters.lock();
            async_waiters.push_back(cx.waker().clone());
            self.registered = true;
        }

        core::task::Poll::Pending
    }
}

/// 拥有所有权的异步等待 Future
///
/// 与 `SignalFuture` 不同，`OwnedSignalFuture` 持有 `Arc<SignalInner>` 的所有权，
/// 因此可以被存储在结构体中或跨 await 点持有。
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::signal::OwnedSignal;
///
/// async fn example() {
///     let signal = OwnedSignal::new();
///     signal.wait_async().await.unwrap();
/// }
/// ```
pub struct OwnedSignalFuture {
    inner: Arc<SignalInner>,
    registered: bool,
}

impl OwnedSignalFuture {
    fn new(inner: Arc<SignalInner>) -> Self {
        Self {
            inner,
            registered: false,
        }
    }
}

impl core::future::Future for OwnedSignalFuture {
    type Output = Result<()>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        // 检查是否已关闭
        if self.inner.closed.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::SignalClosed));
        }

        // 尝试获取信号
        loop {
            let count = self.inner.count.load(Ordering::Acquire);
            if count > 0 {
                if self.inner.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return core::task::Poll::Ready(Ok(()));
                }
                continue;
            }
            break;
        }

        // 没有信号，注册 waker
        if !self.registered {
            {
                let mut async_waiters = self.inner.async_waiters.lock();
                async_waiters.push_back(cx.waker().clone());
            }
            self.registered = true;
        }

        core::task::Poll::Pending
    }
}

/// 带超时的拥有所有权的异步等待 Future
pub struct OwnedSignalTimeoutFuture {
    inner: Arc<SignalInner>,
    deadline: usize,
    registered: bool,
}

impl OwnedSignalTimeoutFuture {
    fn new(inner: Arc<SignalInner>, timeout_ms: usize) -> Self {
        Self {
            inner,
            deadline: Systick::get_current_time() + timeout_ms,
            registered: false,
        }
    }
}

impl core::future::Future for OwnedSignalTimeoutFuture {
    type Output = Result<()>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<Self::Output> {
        // 检查是否已关闭
        if self.inner.closed.load(Ordering::Acquire) {
            return core::task::Poll::Ready(Err(RtosError::SignalClosed));
        }

        // 检查是否超时
        if Systick::get_current_time() >= self.deadline {
            return core::task::Poll::Ready(Err(RtosError::Timeout));
        }

        // 尝试获取信号
        loop {
            let count = self.inner.count.load(Ordering::Acquire);
            if count > 0 {
                if self.inner.count.compare_exchange(
                    count,
                    count - 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return core::task::Poll::Ready(Ok(()));
                }
                continue;
            }
            break;
        }

        // 没有信号，注册 waker
        if !self.registered {
            {
                let mut async_waiters = self.inner.async_waiters.lock();
                async_waiters.push_back(cx.waker().clone());
            }
            self.registered = true;
        }

        core::task::Poll::Pending
    }
}

impl Signal {
    /// 异步等待信号
    ///
    /// 返回一个 Future，在收到信号时完成。
    /// 可以在 async/await 上下文中使用。
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取信号
    /// - `Err(RtosError::SignalClosed)`: 信号量已关闭
    ///
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::sync::Signal;
    ///
    /// async fn example(signal: Signal) {
    ///     signal.wait_async().await.unwrap();
    ///     // 收到信号
    /// }
    /// ```
    pub fn wait_async(&self) -> SignalFuture<'_> {
        SignalFuture::new(self)
    }

    /// 带超时的异步等待信号
    ///
    /// # 参数
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取信号
    /// - `Err(RtosError::Timeout)`: 等待超时
    /// - `Err(RtosError::SignalClosed)`: 信号量已关闭
    pub fn wait_async_timeout(&self, timeout_ms: usize) -> SignalTimeoutFuture<'_> {
        SignalTimeoutFuture::new(self, timeout_ms)
    }
}

impl SignalReceiver {
    /// 异步等待信号
    pub fn wait_async(&self) -> SignalFuture<'_> {
        self.inner.wait_async()
    }

    /// 带超时的异步等待信号
    pub fn wait_async_timeout(&self, timeout_ms: usize) -> SignalTimeoutFuture<'_> {
        self.inner.wait_async_timeout(timeout_ms)
    }
}

// ============================================================================
// Owned 版本 - 支持跨任务传递
// ============================================================================

/// 拥有所有权的信号量
/// 
/// 与 `Signal` 不同，`OwnedSignal` 可以被 move 到其他任务中，
/// 因为它持有 `Arc` 的所有权而不是引用。
/// 
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::signal::OwnedSignal;
/// use neon_rtos2::kernel::task::Task;
/// 
/// let signal = OwnedSignal::new();
/// let signal_clone = signal.clone();
/// 
/// Task::builder("task1")
///     .spawn(move |_| {
///         signal.send();
///     });
/// 
/// Task::builder("task2")
///     .spawn(move |_| {
///         signal_clone.wait().unwrap();
///     });
/// ```
#[derive(Clone)]
pub struct OwnedSignal {
    inner: Arc<SignalInner>,
}

impl OwnedSignal {
    /// 创建新的拥有所有权的信号量
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SignalInner::new()),
        }
    }

    /// 创建带初始计数的信号量
    pub fn with_count(initial_count: usize) -> Self {
        Self {
            inner: Arc::new(SignalInner::new_with_count(initial_count)),
        }
    }

    /// 从 Signal 转换
    pub fn from_signal(signal: Signal) -> Self {
        Self { inner: signal.inner }
    }

    /// 转换为 Signal
    pub fn into_signal(self) -> Signal {
        Signal { inner: self.inner }
    }

    /// 获取 Signal 引用
    pub fn as_signal(&self) -> Signal {
        Signal { inner: Arc::clone(&self.inner) }
    }

    /// 发送信号
    pub fn send(&self) {
        self.as_signal().send();
    }

    /// 发送信号并触发调度
    pub fn send_and_schedule(&self) {
        self.as_signal().send_and_schedule();
    }

    /// 广播信号
    pub fn broadcast(&self) {
        self.as_signal().broadcast();
    }

    /// 等待信号
    pub fn wait(&self) -> Result<()> {
        self.as_signal().wait()
    }

    /// 尝试等待信号（非阻塞）
    pub fn try_wait(&self) -> Result<bool> {
        self.as_signal().try_wait()
    }

    /// 带超时的等待信号
    pub fn wait_timeout(&self, timeout_ms: usize) -> Result<()> {
        self.as_signal().wait_timeout(timeout_ms)
    }

    /// 异步等待信号
    /// 
    /// 注意：由于生命周期限制，OwnedSignal 的异步等待需要使用 OwnedSignalFuture
    pub fn wait_async(&self) -> OwnedSignalFuture {
        OwnedSignalFuture::new(Arc::clone(&self.inner))
    }

    /// 带超时的异步等待信号
    pub fn wait_async_timeout(&self, timeout_ms: usize) -> OwnedSignalTimeoutFuture {
        OwnedSignalTimeoutFuture::new(Arc::clone(&self.inner), timeout_ms)
    }

    /// 关闭信号量
    pub fn close(&self) {
        self.as_signal().close();
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// 获取当前信号计数
    pub fn count(&self) -> usize {
        self.inner.count.load(Ordering::Relaxed)
    }

    /// 获取唯一标识
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }
}

impl Default for OwnedSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for OwnedSignal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OwnedSignal")
            .field("id", &self.id())
            .field("count", &self.count())
            .field("closed", &self.is_closed())
            .finish()
    }
}

// ============================================================================
// 便捷函数
// ============================================================================

/// 创建一个新的信号量
/// 
/// 这是 `Signal::new()` 的便捷函数。
/// 
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::signal_v2::signal;
/// 
/// let sig = signal();
/// sig.send();
/// ```
pub fn signal() -> Signal {
    Signal::new()
}

/// 创建带初始计数的信号量
/// 
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::signal_v2::signal_with_count;
/// 
/// let sig = signal_with_count(3);
/// assert!(sig.try_wait().unwrap()); // 立即成功
/// ```
pub fn signal_with_count(count: usize) -> Signal {
    Signal::with_count(count)
}

/// 创建多生产者单消费者的信号对
/// 
/// 返回多个发送端和一个接收端。
/// 
/// # 参数
/// - `sender_count`: 发送端数量
/// 
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::signal_v2::mpsc_signal;
/// 
/// let (senders, receiver) = mpsc_signal(3);
/// 
/// // 多个发送者
/// for sender in senders {
///     sender.send();
/// }
/// 
/// // 单个接收者
/// for _ in 0..3 {
///     receiver.wait().unwrap();
/// }
/// ```
pub fn mpsc_signal(sender_count: usize) -> (crate::compat::Vec<SignalSender>, SignalReceiver) {
    let signal = Signal::new();
    let senders: crate::compat::Vec<SignalSender> = (0..sender_count)
        .map(|_| SignalSender { inner: signal.clone() })
        .collect();
    let receiver = SignalReceiver { inner: signal };
    (senders, receiver)
}

/// 创建广播信号
/// 
/// 返回一个发送端和多个接收端。发送端的 `broadcast()` 会唤醒所有接收端。
/// 
/// # 参数
/// - `receiver_count`: 接收端数量
/// 
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::sync::signal_v2::broadcast_signal;
/// 
/// let (sender, receivers) = broadcast_signal(3);
/// 
/// // 广播唤醒所有接收者
/// sender.broadcast();
/// ```
pub fn broadcast_signal(receiver_count: usize) -> (SignalSender, crate::compat::Vec<SignalReceiver>) {
    let signal = Signal::new();
    let sender = SignalSender { inner: signal.clone() };
    let receivers: crate::compat::Vec<SignalReceiver> = (0..receiver_count)
        .map(|_| SignalReceiver { inner: signal.clone() })
        .collect();
    (sender, receivers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_signal_v2_basic() {
        kernel_init();
        
        let signal = Signal::new();
        
        // 初始状态
        assert_eq!(signal.count(), 0);
        assert_eq!(signal.try_wait().unwrap(), false);
        
        // 发送信号
        signal.send();
        assert_eq!(signal.count(), 1);
        
        // 消费信号
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.count(), 0);
    }

    #[test]
    #[serial]
    fn test_signal_v2_with_count() {
        kernel_init();
        
        // 创建带初始计数的信号量
        let signal = Signal::with_count(3);
        assert_eq!(signal.count(), 3);
        
        // 消费信号
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), false);
    }

    #[test]
    #[serial]
    fn test_signal_v2_clone() {
        kernel_init();
        
        let signal1 = Signal::new();
        let signal2 = signal1.clone();
        
        // 两个句柄指向同一个信号
        assert_eq!(signal1.id(), signal2.id());
        
        // 通过一个发送，另一个可以接收
        signal1.send();
        assert_eq!(signal2.try_wait().unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_signal_v2_multiple_signals() {
        kernel_init();
        
        let signal = Signal::new();
        
        // 发送多个信号
        signal.send();
        signal.send();
        signal.send();
        
        assert_eq!(signal.count(), 3);
        
        // 消费所有信号
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), false);
    }

    #[test]
    #[serial]
    fn test_signal_pair() {
        kernel_init();
        
        let (sender, receiver) = signal_pair();
        
        sender.send();
        assert_eq!(receiver.try_wait().unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_signal_v2_different_instances() {
        kernel_init();
        
        let signal1 = Signal::new();
        let signal2 = Signal::new();
        
        // 两个不同的信号有不同的 ID
        assert_ne!(signal1.id(), signal2.id());
        
        // 互不影响
        signal1.send();
        assert_eq!(signal2.try_wait().unwrap(), false);
        assert_eq!(signal1.try_wait().unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_signal_v2_close() {
        kernel_init();
        
        let signal = Signal::new();
        
        // 发送一些信号
        signal.send();
        assert!(!signal.is_closed());
        
        // 关闭信号量
        signal.close();
        assert!(signal.is_closed());
        
        // 关闭后 try_wait 返回错误
        assert!(signal.try_wait().is_err());
        
        // 关闭后 send 被忽略
        signal.send();
        
        // 重置后可以继续使用
        signal.reset();
        assert!(!signal.is_closed());
        assert_eq!(signal.count(), 0);
        
        signal.send();
        assert_eq!(signal.try_wait().unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_signal_pair_close() {
        kernel_init();
        
        let (sender, receiver) = signal_pair();
        
        sender.send();
        assert_eq!(receiver.try_wait().unwrap(), true);
        
        // 关闭
        sender.close();
        assert!(sender.is_closed());
        assert!(receiver.is_closed());
        
        // 关闭后接收返回错误
        assert!(receiver.try_wait().is_err());
    }

    #[test]
    #[serial]
    fn test_waiter_list() {
        let mut list = WaiterList::new();
        
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        
        // 添加等待者
        assert!(list.push(1));
        assert!(list.push(2));
        assert!(list.push(3));
        
        assert_eq!(list.len(), 3);
        assert!(!list.is_empty());
        
        // FIFO 弹出
        assert_eq!(list.pop_front(), Some(1));
        assert_eq!(list.pop_front(), Some(2));
        assert_eq!(list.pop_front(), Some(3));
        assert_eq!(list.pop_front(), None);
        
        // 测试移除
        list.push(10);
        list.push(20);
        list.push(30);
        
        assert!(list.remove(20));
        assert_eq!(list.len(), 2);
        assert_eq!(list.pop_front(), Some(10));
        assert_eq!(list.pop_front(), Some(30));
    }

    #[test]
    #[serial]
    fn test_waiter_list_drain() {
        let mut list = WaiterList::new();
        
        list.push(1);
        list.push(2);
        list.push(3);
        
        let drained = list.drain();
        
        assert!(list.is_empty());
        assert_eq!(drained[0], Some(1));
        assert_eq!(drained[1], Some(2));
        assert_eq!(drained[2], Some(3));
        assert_eq!(drained[3], None);
    }

    #[test]
    #[serial]
    fn test_signal_v2_debug() {
        kernel_init();
        
        let signal = Signal::new();
        signal.send();
        
        let debug_str = format!("{:?}", signal);
        assert!(debug_str.contains("Signal"));
        assert!(debug_str.contains("count"));
        assert!(debug_str.contains("closed"));
    }

    #[test]
    #[serial]
    fn test_signal_v2_try_wait_timeout() {
        kernel_init();
        
        let signal = Signal::new();
        
        // 没有信号时，try_wait_timeout 应该返��� false（超时）
        // 注意：在测试环境中时间不会自动流逝，所以这里会立即返回
        let result = signal.try_wait_timeout(0);
        assert_eq!(result.unwrap(), false);
        
        // 发送信号后应该能立即获取
        signal.send();
        let result = signal.try_wait_timeout(1000);
        assert_eq!(result.unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_signal_receiver_timeout() {
        kernel_init();
        
        let (sender, receiver) = signal_pair();
        
        // 发送信号
        sender.send();
        
        // 接收端应该能获��
        assert_eq!(receiver.try_wait().unwrap(), true);
        
        // 没有信号时 try_wait_timeout 返回 false
        let result = receiver.try_wait_timeout(0);
        assert_eq!(result.unwrap(), false);
    }

    #[test]
    #[serial]
    fn test_owned_signal() {
        kernel_init();
        
        let signal = OwnedSignal::new();
        let signal_clone = signal.clone();
        
        // 两个句柄指向同一个信号
        assert_eq!(signal.id(), signal_clone.id());
        
        // 发送和接收
        signal.send();
        assert_eq!(signal_clone.try_wait().unwrap(), true);
    }

    #[test]
    #[serial]
    fn test_owned_signal_with_count() {
        kernel_init();
        
        let signal = OwnedSignal::with_count(2);
        assert_eq!(signal.count(), 2);
        
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), true);
        assert_eq!(signal.try_wait().unwrap(), false);
    }

    #[test]
    #[serial]
    fn test_signal_convenience_functions() {
        kernel_init();
        
        // 测试 signal() 函数
        let sig = signal();
        sig.send();
        assert_eq!(sig.try_wait().unwrap(), true);
        
        // 测试 signal_with_count() 函数
        let sig2 = signal_with_count(3);
        assert_eq!(sig2.count(), 3);
    }

    #[test]
    #[serial]
    fn test_mpsc_signal() {
        kernel_init();
        
        let (senders, receiver) = mpsc_signal(3);
        
        // 每个发送者发送一个信号
        for sender in &senders {
            sender.send();
        }
        
        // 接收者应该能收到 3 个信号
        assert_eq!(receiver.count(), 3);
        assert_eq!(receiver.try_wait().unwrap(), true);
        assert_eq!(receiver.try_wait().unwrap(), true);
        assert_eq!(receiver.try_wait().unwrap(), true);
        assert_eq!(receiver.try_wait().unwrap(), false);
    }

    #[test]
    #[serial]
    fn test_broadcast_signal() {
        kernel_init();
        
        let (sender, receivers) = broadcast_signal(3);
        
        // 发送信号（没有等待者时会增加计数）
        sender.send();
        
        // 由于没有等待者，信号被存储
        // 第一个接收者可以获取
        assert_eq!(receivers[0].try_wait().unwrap(), true);
        // 其他接收者获取不到（信号已被消费）
        assert_eq!(receivers[1].try_wait().unwrap(), false);
    }

    #[test]
    #[serial]
    fn test_owned_signal_conversion() {
        kernel_init();
        
        let signal_v2 = Signal::new();
        signal_v2.send();
        
        // 转换为 OwnedSignal
        let owned = OwnedSignal::from_signal(signal_v2);
        assert_eq!(owned.try_wait().unwrap(), true);
        
        // 转换回 Signal
        owned.send();
        let signal_v2_back = owned.into_signal();
        assert_eq!(signal_v2_back.try_wait().unwrap(), true);
    }
}

