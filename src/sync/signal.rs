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
//!         .spawn(move || {
//!             loop {
//!                 // 产生数据...
//!                 signal.send();
//!                 // delay...
//!             }
//!         });
//!
//!     Task::builder("consumer")
//!         .spawn(move || {
//!             loop {
//!                 signal_clone.wait().unwrap();
//!                 // 处理数据...
//!             }
//!         });
//! }
//! ```

use crate::kernel::time::systick::Ticks;
use super::wait::AsyncWaiters;
use crate::compat::Arc;
use crate::error::{Result, RtosError};
use crate::hal::trigger_schedule;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::systick::Deadline;
#[cfg(test)]
use crate::kernel::time::systick::Systick;
use crate::kernel::time::timer::{TaskTimeout, Timer};
use crate::sync::event::Event;
use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use critical_section::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SignalStatus {
    Open,
    Disconnected,
    Closed,
}

struct SignalState {
    count: usize,
    status: SignalStatus,
    close_epoch: usize,
    waiters: WaiterList,
    async_waiters: AsyncWaiters,
}

impl SignalState {
    fn try_take(&mut self) -> Result<bool> {
        if self.status == SignalStatus::Closed {
            return Err(RtosError::SignalClosed);
        }
        if self.count > 0 {
            self.count -= 1;
            return Ok(true);
        }
        if self.status == SignalStatus::Disconnected {
            return Err(RtosError::SignalClosed);
        }
        Ok(false)
    }
}

struct SignalInner {
    state: Mutex<RefCell<SignalState>>,
}

/// 等待者列表
/// 使用位图 + 数组实现高效的等待者管理
///
/// ## 优化说明
/// - 使用 `u16` 位图标记哪些槽位被使用
/// - `push` 操作：O(1) - 使用 `trailing_zeros` 找到第一个空闲位
/// - `remove` 操作：O(n) 查找 + O(1) 移除 - 直接清除对应位
/// - `pop_front` 操作：O(1) - 使用 `trailing_zeros` 找到第一个使用位
/// - 内存布局更紧凑，缓存友好
pub struct WaiterList {
    /// 存储任务 ID 的数组
    tasks: [usize; 16],
    /// 位图：第 i 位为 1 表示 tasks[i] 有效
    bitmap: u16,
    /// FIFO 顺序追踪：记录插入顺序
    /// 使用环形缓冲区索引
    order: [u8; 16],
    /// 下一个插入位置（用于 FIFO 顺序）
    order_head: u8,
    /// 下一个弹出位置（用于 FIFO 顺序）
    order_tail: u8,
    /// 有效元素数量
    order_len: u8,
}

impl WaiterList {
    /// 创建新的等待者列表
    pub const fn new() -> Self {
        Self {
            tasks: [0; 16],
            bitmap: 0,
            order: [0; 16],
            order_head: 0,
            order_tail: 0,
            order_len: 0,
        }
    }

    /// 添加等待者
    ///
    /// 时间复杂度：O(1)
    ///
    /// # 返回值
    /// - `true`: 成功添加
    /// - `false`: 队列已满
    pub fn push(&mut self, task_id: usize) -> bool {
        // 使用位图找到第一个空闲槽位
        let free_bitmap = !self.bitmap;
        if free_bitmap == 0 {
            return false; // 所有槽位都被使用
        }

        let slot = free_bitmap.trailing_zeros() as usize;
        if slot >= 16 {
            return false;
        }

        // 存储任务 ID
        self.tasks[slot] = task_id;
        // 标记槽位为已使用
        self.bitmap |= 1 << slot;

        // 记录插入顺序（用于 FIFO）
        self.order[self.order_head as usize] = slot as u8;
        self.order_head = (self.order_head + 1) % 16;
        self.order_len += 1;

        true
    }

    /// 弹出一个等待者（LIFO - 最后插入的）
    ///
    /// 时间复杂度：O(1)
    pub fn pop(&mut self) -> Option<usize> {
        if self.bitmap == 0 {
            return None;
        }

        // LIFO: 弹出最后插入的
        if self.order_len == 0 {
            return None;
        }

        // 回退 head 指针
        self.order_head = if self.order_head == 0 {
            15
        } else {
            self.order_head - 1
        };
        let slot = self.order[self.order_head as usize] as usize;
        self.order_len -= 1;

        // 清除位图
        self.bitmap &= !(1 << slot);

        Some(self.tasks[slot])
    }

    /// 弹出第一个等待者（FIFO - 最先插入的）
    ///
    /// 时间复杂度：O(1)
    pub fn pop_front(&mut self) -> Option<usize> {
        if self.bitmap == 0 || self.order_len == 0 {
            return None;
        }

        // FIFO: 弹出最先插入的
        let slot = self.order[self.order_tail as usize] as usize;
        self.order_tail = (self.order_tail + 1) % 16;
        self.order_len -= 1;

        // 清除位图
        self.bitmap &= !(1 << slot);

        Some(self.tasks[slot])
    }

    /// 移除指定的��待者
    ///
    /// 时间复杂度：O(n) 查找 + O(1) 移除
    ///
    /// # 返回值
    /// - `true`: 成功移除
    /// - `false`: 未找到指定任务
    pub fn remove(&mut self, task_id: usize) -> bool {
        // 遍历所有有效槽位
        let mut bitmap = self.bitmap;
        while bitmap != 0 {
            let slot = bitmap.trailing_zeros() as usize;
            if self.tasks[slot] == task_id {
                // 找到了，清除位图
                self.bitmap &= !(1 << slot);

                // 从顺序数组中移除（需要遍历找到并标记）
                // 这里简化处理：不从 order 数组中移除，
                // 而是在 pop_front 时跳过已清除的槽位
                self.compact_order();

                return true;
            }
            bitmap &= !(1 << slot);
        }
        false
    }

    /// 压缩顺序数组，移除已删除的槽位
    fn compact_order(&mut self) {
        if self.order_len == 0 {
            return;
        }

        let mut new_order = [0u8; 16];
        let mut new_len = 0u8;

        let mut i = self.order_tail;
        for _ in 0..self.order_len {
            let slot = self.order[i as usize];
            // 检查该槽位是否仍然有效
            if (self.bitmap & (1 << slot)) != 0 {
                new_order[new_len as usize] = slot;
                new_len += 1;
            }
            i = (i + 1) % 16;
        }

        self.order = new_order;
        self.order_tail = 0;
        self.order_head = new_len;
        self.order_len = new_len;
    }

    /// 检查是否为空
    ///
    /// 时间复杂度：O(1)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bitmap == 0
    }

    /// 获取等待者数量
    ///
    /// 时间复杂度：O(1) - 使用 popcount 指令
    #[inline]
    pub fn len(&self) -> usize {
        self.bitmap.count_ones() as usize
    }

    /// 获取所有等待者的迭代器
    ///
    /// 按 FIFO 顺序返回
    pub fn iter(&self) -> WaiterListIter<'_> {
        WaiterListIter {
            list: self,
            current: self.order_tail,
            remaining: self.order_len,
        }
    }

    /// 清空所有等待者，返回被清空的任务 ID 列表
    ///
    /// 时间复杂度：O(n)，n 为等待者数量
    pub fn drain(&mut self) -> [Option<usize>; 16] {
        let mut result = [None; 16];
        let mut idx = 0;

        // 按 FIFO 顺序返回
        let mut i = self.order_tail;
        for _ in 0..self.order_len {
            let slot = self.order[i as usize] as usize;
            if (self.bitmap & (1 << slot)) != 0 {
                result[idx] = Some(self.tasks[slot]);
                idx += 1;
            }
            i = (i + 1) % 16;
        }

        self.clear();
        result
    }

    pub(crate) fn wake_all(&mut self, cs: critical_section::CriticalSection<'_>, event: Event) -> usize {
        if self.is_empty() { return 0; }
        let count = Scheduler::wake_tasks_if(cs, self.iter(), event);
        self.clear();
        count
    }

    fn clear(&mut self) {
        self.bitmap = 0;
        self.order_head = 0;
        self.order_tail = 0;
        self.order_len = 0;
    }

    /// 检查是否包含指定任务
    ///
    /// 时间复杂度：O(n)
    pub fn contains(&self, task_id: usize) -> bool {
        let mut bitmap = self.bitmap;
        while bitmap != 0 {
            let slot = bitmap.trailing_zeros() as usize;
            if self.tasks[slot] == task_id {
                return true;
            }
            bitmap &= !(1 << slot);
        }
        false
    }

    /// 获取容量
    #[inline]
    pub const fn capacity(&self) -> usize {
        16
    }
}

/// WaiterList 的迭代器
#[derive(Clone)]
pub struct WaiterListIter<'a> {
    list: &'a WaiterList,
    current: u8,
    remaining: u8,
}

impl<'a> Iterator for WaiterListIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.remaining > 0 {
            let slot = self.list.order[self.current as usize] as usize;
            self.current = (self.current + 1) % 16;
            self.remaining -= 1;

            // 检查槽位是否有效
            if (self.list.bitmap & (1 << slot)) != 0 {
                return Some(self.list.tasks[slot]);
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.remaining as usize;
        (len, Some(len))
    }
}

impl Default for WaiterList {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalInner {
    fn new() -> Self {
        Self::new_with_count(0)
    }

    fn new_with_count(count: usize) -> Self {
        Self {
            state: Mutex::new(RefCell::new(SignalState {
                count,
                status: SignalStatus::Open,
                close_epoch: 0,
                waiters: WaiterList::new(),
                async_waiters: AsyncWaiters::new(),
            })),
        }
    }

    fn event(&self) -> Event {
        Event::Signal(self as *const Self as usize)
    }

    // Called while state is borrowed in a critical section; task wakeups have no user callbacks.
    fn notify_one(&self, state: &mut SignalState) -> Option<Waker> {
        while let Some(id) = state.waiters.pop_front() {
            if Event::wake_task_by_id_if(id, self.event()) {
                return None;
            }
        }
        state.async_waiters.pop()
    }

    fn poll_wait(
        &self,
        token: &mut Option<u64>,
        epoch: &mut Option<usize>,
        cx: &Context<'_>,
    ) -> Poll<Result<()>> {
        let mut incoming = Some(cx.waker().clone());
        let (result, obsolete) = critical_section::with(|cs| {
            let mut state = self.state.borrow_ref_mut(cs);
            let outcome = if *epoch.get_or_insert(state.close_epoch) != state.close_epoch {
                Err(RtosError::SignalClosed)
            } else {
                state.try_take()
            };
            if outcome != Ok(false) {
                return (Poll::Ready(outcome.map(|_| ())), state.async_waiters.remove(token.take()));
            }
            if !state
                .async_waiters
                .has_room(*token, crate::config::MAX_ASYNC_WAITERS)
            {
                return (Poll::Ready(Err(RtosError::WaiterQueueFull)), None);
            }
            let old = state
                .async_waiters
                .register(token, incoming.take().unwrap());
            (Poll::Pending, old)
        });
        drop(obsolete);
        result
    }

    fn cancel(&self, token: Option<u64>) {
        if token.is_none() {
            return;
        }
        let (removed, next) = critical_section::with(|cs| {
            let mut state = self.state.borrow_ref_mut(cs);
            let removed = state.async_waiters.remove(token);
            let next = if state.status == SignalStatus::Open && state.count > 0 {
                self.notify_one(&mut state)
            } else {
                None
            };
            (removed, next)
        });
        drop(removed);
        if let Some(waker) = next {
            waker.wake();
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
    /// assert!(signal.try_wait().unwrap()); // 立即成功
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
        let waker = critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            if state.status != SignalStatus::Open {
                return None;
            }
            state.count = state.count.saturating_add(1);
            self.inner.notify_one(&mut state)
        });
        if let Some(waker) = waker {
            waker.wake();
        }
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
        self.notify_all(SignalStatus::Open);
    }

    fn notify_all(&self, status: SignalStatus) {
        let wakers = critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            if state.status != SignalStatus::Open {
                return crate::compat::VecDeque::new();
            }
            state.status = status;
            if status == SignalStatus::Closed {
                state.close_epoch = state.close_epoch.wrapping_add(1);
            }
            let task_count = state.waiters.wake_all(cs, self.inner.event());
            let async_count = state.async_waiters.len();
            if status == SignalStatus::Open {
                state.count = state.count.saturating_add(task_count + async_count);
            }
            state.async_waiters.drain()
        });
        for (_, waker) in wakers {
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
        self.wait_internal(None)
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
        critical_section::with(|cs| self.inner.state.borrow_ref_mut(cs).try_take())
    }

    /// 带超时的等待信号
    ///
    /// 如果在指定时间内收到信号，返回 `Ok(())`。
    /// 如果超时，返回 `Err(RtosError::Timeout)`。
    ///
    /// # 参数
    /// - `timeout`: 超时时间（Duration 或 Ticks）
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
    /// match signal.wait_timeout(neon_rtos2::prelude::Ticks(1000)) {
    ///     Ok(()) => { /* 获取到信号 */ }
    ///     Err(neon_rtos2::error::RtosError::Timeout) => { /* 超时 */ }
    ///     Err(_) => { /* 其他错误 */ }
    /// }
    /// ```
    pub fn wait_timeout(&self, timeout: impl Into<Ticks>) -> Result<()> {
        self.wait_internal(Some(timeout.into().0))
    }

    fn wait_internal(&self, timeout: Option<usize>) -> Result<()> {
        let deadline = timeout.map(Deadline::after);
        let task = Scheduler::get_current_task();
        let mut timer: Option<TaskTimeout> = None;
        let mut epoch = None;
        loop {
            let ready = critical_section::with(|cs| -> Result<bool> {
                let mut state = self.inner.state.borrow_ref_mut(cs);
                // Remove any old wake registration before rechecking the condition.
                state.waiters.remove(task.0);
                if *epoch.get_or_insert(state.close_epoch) != state.close_epoch {
                    return Err(RtosError::SignalClosed);
                }
                if state.try_take()? {
                    return Ok(true);
                }
                if let Some(deadline) = deadline {
                    if deadline.elapsed() {
                        return Err(RtosError::Timeout);
                    }
                    if timer.is_none() {
                        timer = Some(Timer::register_task_timeout(
                            deadline,
                            task,
                            self.inner.event(),
                        )?);
                    }
                }
                if !state.waiters.push(task.0) {
                    return Err(RtosError::WaiterQueueFull);
                }
                let mut task = task;
                task.block(self.inner.event());
                Ok(false)
            })?;
            if ready {
                return Ok(());
            }
            trigger_schedule();
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
        self.notify_all(SignalStatus::Closed);
    }

    /// 检查信号量是否已关闭
    pub fn is_closed(&self) -> bool {
        critical_section::with(|cs| self.inner.state.borrow_ref(cs).status != SignalStatus::Open)
    }

    /// 重置信号量
    ///
    /// 清除所有待处理的信号，重新打开信号量。
    /// 注意：不会影响正在等待的任务。
    pub fn reset(&self) {
        critical_section::with(|cs| {
            let mut state = self.inner.state.borrow_ref_mut(cs);
            state.count = 0;
            state.status = SignalStatus::Open;
        });
    }

    /// 获取当前信号计数
    pub fn count(&self) -> usize {
        critical_section::with(|cs| self.inner.state.borrow_ref(cs).count)
    }

    /// 获取等待者数量（包括同步和异步等待者）
    pub fn waiter_count(&self) -> usize {
        critical_section::with(|cs| {
            let state = self.inner.state.borrow_ref(cs);
            state.waiters.len() + state.async_waiters.len()
        })
    }

    /// 获取同步等待者数量
    pub fn sync_waiter_count(&self) -> usize {
        critical_section::with(|cs| self.inner.state.borrow_ref(cs).waiters.len())
    }

    /// 获取异步等待者数量
    pub fn async_waiter_count(&self) -> usize {
        critical_section::with(|cs| self.inner.state.borrow_ref(cs).async_waiters.len())
    }

    /// 获取信号的唯一标识（用于调试）
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
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
        SignalSender {
            inner: Arc::new(SignalEndpoint(signal.clone())),
        },
        SignalReceiver { inner: Arc::new(SignalEndpoint(signal)) },
    )
}

// Each direction shares one owner. Its destructor runs exactly once even
// when endpoint clones are dropped concurrently.
struct SignalEndpoint(Signal);

impl Drop for SignalEndpoint {
    fn drop(&mut self) {
        self.0.notify_all(SignalStatus::Disconnected);
    }
}

/// 信号发送端
///
/// 只能发送信号，不能等待。
#[derive(Clone)]
pub struct SignalSender {
    inner: Arc<SignalEndpoint>,
}

impl SignalSender {
    /// 发送信号
    pub fn send(&self) {
        self.inner.0.send();
    }

    /// 发送信号并触发调度
    pub fn send_and_schedule(&self) {
        self.inner.0.send_and_schedule();
    }

    /// ��播信号
    pub fn broadcast(&self) {
        self.inner.0.broadcast();
    }

    /// 广播信号并触发调度
    pub fn broadcast_and_schedule(&self) {
        self.inner.0.broadcast_and_schedule();
    }

    /// 关闭信号量
    pub fn close(&self) {
        self.inner.0.close();
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.0.is_closed()
    }
}

impl core::fmt::Debug for SignalSender {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SignalSender")
            .field("id", &self.inner.0.id())
            .field("closed", &self.inner.0.is_closed())
            .finish()
    }
}

/// 信号接收端
///
/// 只能等待信号，不能发送。
#[derive(Clone)]
pub struct SignalReceiver {
    inner: Arc<SignalEndpoint>,
}

impl SignalReceiver {
    /// 等待信号
    pub fn wait(&self) -> Result<()> {
        self.inner.0.wait()
    }

    /// 尝试等待信号（非阻塞）
    pub fn try_wait(&self) -> Result<bool> {
        self.inner.0.try_wait()
    }

    /// 带超时的等待信号
    pub fn wait_timeout(&self, timeout: impl Into<Ticks>) -> Result<()> {
        self.inner.0.wait_timeout(timeout)
    }



    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.0.is_closed()
    }

    /// 获取当前信号计数
    pub fn count(&self) -> usize {
        self.inner.0.count()
    }
}

impl core::fmt::Debug for SignalReceiver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SignalReceiver")
            .field("id", &self.inner.0.id())
            .field("count", &self.inner.0.count())
            .field("closed", &self.inner.0.is_closed())
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
struct SignalWait {
    inner: Arc<SignalInner>,
    token: Option<u64>,
    epoch: Option<usize>,
    sleep: Option<crate::runtime::Sleep>,
    finished: bool,
}

impl SignalWait {
    fn new(inner: Arc<SignalInner>, timeout: Option<usize>) -> Self {
        Self {
            inner,
            token: None,
            epoch: None,
            sleep: timeout.map(|ticks| crate::runtime::Sleep::new(Ticks(ticks))),
            finished: false,
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<()>> {
        assert!(!self.finished, "Signal future polled after completion");
        let result = self.inner.poll_wait(&mut self.token, &mut self.epoch, cx);
        if result.is_ready() {
            self.finished = true;
            self.sleep = None;
            return result;
        }
        if let Some(sleep) = &mut self.sleep {
            if Pin::new(sleep).poll(cx).is_ready() {
                self.inner.cancel(self.token.take());
                self.sleep = None;
                self.finished = true;
                return Poll::Ready(Err(RtosError::Timeout));
            }
        }
        Poll::Pending
    }
}

impl Drop for SignalWait {
    fn drop(&mut self) {
        self.inner.cancel(self.token.take());
    }
}

pub struct SignalFuture<'a> {
    wait: SignalWait,
    lifetime: core::marker::PhantomData<&'a Signal>,
}
impl<'a> SignalFuture<'a> {
    fn new(signal: &'a Signal) -> Self {
        Self {
            wait: SignalWait::new(signal.inner.clone(), None),
            lifetime: core::marker::PhantomData,
        }
    }
}
impl Future for SignalFuture<'_> {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.wait.poll(cx)
    }
}

pub struct SignalTimeoutFuture<'a> {
    wait: SignalWait,
    lifetime: core::marker::PhantomData<&'a Signal>,
}
impl<'a> SignalTimeoutFuture<'a> {
    fn new(signal: &'a Signal, timeout: usize) -> Self {
        Self {
            wait: SignalWait::new(signal.inner.clone(), Some(timeout)),
            lifetime: core::marker::PhantomData,
        }
    }
}
impl Future for SignalTimeoutFuture<'_> {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.wait.poll(cx)
    }
}

pub struct OwnedSignalFuture {
    wait: SignalWait,
}
impl OwnedSignalFuture {
    fn new(inner: Arc<SignalInner>) -> Self {
        Self {
            wait: SignalWait::new(inner, None),
        }
    }
}
impl Future for OwnedSignalFuture {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.wait.poll(cx)
    }
}

pub struct OwnedSignalTimeoutFuture {
    wait: SignalWait,
}
impl OwnedSignalTimeoutFuture {
    fn new(inner: Arc<SignalInner>, timeout: usize) -> Self {
        Self {
            wait: SignalWait::new(inner, Some(timeout)),
        }
    }
}
impl Future for OwnedSignalTimeoutFuture {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.wait.poll(cx)
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
    /// - `timeout`: 超时时间（Duration 或 Ticks）
    ///
    /// # 返回值
    /// - `Ok(())`: 成功获取信号
    /// - `Err(RtosError::Timeout)`: 等待超时
    /// - `Err(RtosError::SignalClosed)`: 信号量已关闭
    pub fn wait_async_timeout(&self, timeout: impl Into<Ticks>) -> SignalTimeoutFuture<'_> {
        SignalTimeoutFuture::new(self, timeout.into().0)
    }
}

impl SignalReceiver {
    /// 异步等待信号
    pub fn wait_async(&self) -> SignalFuture<'_> {
        self.inner.0.wait_async()
    }

    /// 带超时的异步等待信号
    pub fn wait_async_timeout(&self, timeout: impl Into<Ticks>) -> SignalTimeoutFuture<'_> {
        self.inner.0.wait_async_timeout(timeout)
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
///     .spawn(move || {
///         signal.send();
///     });
///
/// Task::builder("task2")
///     .spawn(move || {
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
        Self {
            inner: signal.inner,
        }
    }

    /// 转换为 Signal
    pub fn into_signal(self) -> Signal {
        Signal { inner: self.inner }
    }

    /// 获取 Signal 引用
    pub fn as_signal(&self) -> Signal {
        Signal {
            inner: Arc::clone(&self.inner),
        }
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
    pub fn wait_timeout(&self, timeout: impl Into<Ticks>) -> Result<()> {
        self.as_signal().wait_timeout(timeout)
    }

    /// 异步等待信号
    ///
    /// 注意：由于生命周期限制，OwnedSignal 的异步等待需要使用 OwnedSignalFuture
    pub fn wait_async(&self) -> OwnedSignalFuture {
        OwnedSignalFuture::new(Arc::clone(&self.inner))
    }

    /// 带超时的异步等待信号
    pub fn wait_async_timeout(&self, timeout: impl Into<Ticks>) -> OwnedSignalTimeoutFuture {
        OwnedSignalTimeoutFuture::new(Arc::clone(&self.inner), timeout.into().0)
    }

    /// 关闭信号量
    pub fn close(&self) {
        self.as_signal().close();
    }

    /// 检查是否已关闭
    pub fn is_closed(&self) -> bool {
        critical_section::with(|cs| self.inner.state.borrow(cs).borrow().status != SignalStatus::Open)
    }

    /// 获取当前信号计数
    pub fn count(&self) -> usize {
        critical_section::with(|cs| self.inner.state.borrow(cs).borrow().count)
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
/// use neon_rtos2::sync::signal::signal;
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
/// use neon_rtos2::sync::signal::signal_with_count;
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
/// use neon_rtos2::sync::signal::mpsc_signal;
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
    let (sender, receiver) = signal_pair();
    ((0..sender_count).map(|_| sender.clone()).collect(), receiver)
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
/// use neon_rtos2::sync::signal::broadcast_signal;
///
/// let (sender, receivers) = broadcast_signal(3);
///
/// // 广播唤醒所有接收者
/// sender.broadcast();
/// ```
pub fn broadcast_signal(
    receiver_count: usize,
) -> (SignalSender, crate::compat::Vec<SignalReceiver>) {
    let (sender, receiver) = signal_pair();
    (sender, (0..receiver_count).map(|_| receiver.clone()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn last_endpoint_drains_signals_and_wakes_all_receivers() {
        kernel_init();
        let (tx, rx) = signal_pair();
        let other = tx.clone();
        drop(tx);
        assert!(!rx.is_closed());
        let mut first = core::pin::pin!(rx.wait_async());
        let mut second = core::pin::pin!(rx.wait_async());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        other.send();
        drop(other);
        assert!(rx.is_closed());
        assert_eq!(first.as_mut().poll(&mut cx), Poll::Ready(Ok(())));
        assert_eq!(second.as_mut().poll(&mut cx), Poll::Ready(Err(RtosError::SignalClosed)));
        assert_eq!(rx.try_wait(), Err(RtosError::SignalClosed));
        assert!(mpsc_signal(0).1.is_closed());
        assert!(broadcast_signal(0).0.is_closed());
    }

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
    fn test_signal_immediate_timeout_and_available_count() {
        kernel_init();

        let signal = Signal::new();

        // 注意：在测试环境中时间不会自动流逝，所以这里会立即返回
        let result = signal.wait_timeout(Ticks(0));
        assert_eq!(result, Err(RtosError::Timeout));

        // 发送信号后应该能立即获取
        signal.send();
        signal.wait_timeout(Ticks(1000)).unwrap();
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

        assert_eq!(receiver.wait_timeout(Ticks(0)), Err(RtosError::Timeout));
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

#[cfg(test)]
mod wait_regressions {
    use super::*;
    use serial_test::serial;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::Wake;

    #[derive(Default)]
    struct Counter(AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn poll<F: Future + Unpin>(f: &mut F, counter: &Arc<Counter>) -> Poll<F::Output> {
        let waker = Waker::from(counter.clone());
        Pin::new(f).poll(&mut Context::from_waker(&waker))
    }

    #[test]
    #[serial]
    fn send_survives_repoll_and_refreshes_waker() {
        crate::utils::kernel_init();
        let signal = Signal::new();
        let old = Arc::new(Counter::default());
        let new = Arc::new(Counter::default());
        let mut future = signal.wait_async();
        assert!(poll(&mut future, &old).is_pending());
        assert!(poll(&mut future, &new).is_pending());
        assert_eq!(signal.async_waiter_count(), 1);
        signal.send();
        assert_eq!(old.0.load(Ordering::SeqCst), 0);
        assert_eq!(new.0.load(Ordering::SeqCst), 1);
        assert_eq!(poll(&mut future, &new), Poll::Ready(Ok(())));
        assert_eq!(signal.count(), 0);
    }

    #[test]
    #[serial]
    fn cancelling_notified_waiter_forwards_unconsumed_signal() {
        crate::utils::kernel_init();
        let signal = Signal::new();
        let first = Arc::new(Counter::default());
        let second = Arc::new(Counter::default());
        let mut a = signal.wait_async();
        let mut b = signal.wait_async();
        assert!(poll(&mut a, &first).is_pending());
        assert!(poll(&mut b, &second).is_pending());
        signal.send();
        drop(a);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
        assert_eq!(poll(&mut b, &second), Poll::Ready(Ok(())));
        assert_eq!(signal.waiter_count(), 0);
    }

    #[test]
    #[serial]
    fn close_then_reopen_preserves_the_close_for_registered_waiters() {
        crate::utils::kernel_init();
        let signal = Signal::new();
        let owned = OwnedSignal::from_signal(signal.clone());
        let counter = Arc::new(Counter::default());
        let mut a = signal.wait_async();
        let mut b = owned.wait_async_timeout(Ticks(100));
        assert!(poll(&mut a, &counter).is_pending());
        assert!(poll(&mut b, &counter).is_pending());
        signal.close();
        signal.reset();
        signal.send();
        assert_eq!(counter.0.load(Ordering::SeqCst), 2);
        assert_eq!(
            poll(&mut a, &counter),
            Poll::Ready(Err(RtosError::SignalClosed))
        );
        assert_eq!(
            poll(&mut b, &counter),
            Poll::Ready(Err(RtosError::SignalClosed))
        );
        assert_eq!(Timer::get_next_timeout(), None);
    }

    #[test]
    #[serial]
    fn broadcast_grants_each_registered_waiter_one_signal() {
        crate::utils::kernel_init();
        let signal = Signal::new();
        let counter = Arc::new(Counter::default());
        let mut a = signal.wait_async();
        let mut b = signal.wait_async();
        assert!(poll(&mut a, &counter).is_pending());
        assert!(poll(&mut b, &counter).is_pending());
        signal.broadcast();
        assert_eq!(counter.0.load(Ordering::SeqCst), 2);
        assert_eq!(poll(&mut a, &counter), Poll::Ready(Ok(())));
        assert_eq!(poll(&mut b, &counter), Poll::Ready(Ok(())));
        signal.broadcast();
        assert_eq!(signal.count(), 0);
    }

    #[test]
    #[serial]
    fn timeout_wakes_and_removes_both_registrations() {
        crate::utils::kernel_init();
        let signal = Signal::new();
        let counter = Arc::new(Counter::default());
        let mut future = signal.wait_async_timeout(Ticks(5));
        assert!(poll(&mut future, &counter).is_pending());
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(5)));
        Systick::add_current_time(5);
        Timer::timer_check_and_send_event();
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            poll(&mut future, &counter),
            Poll::Ready(Err(RtosError::Timeout))
        );
        assert_eq!(signal.waiter_count(), 0);
        assert_eq!(Timer::get_next_timeout(), None);
    }

    #[test]
    #[serial]
    fn success_and_drop_release_timeout_registrations() {
        crate::utils::kernel_init();
        let signal = Signal::new();
        let counter = Arc::new(Counter::default());
        let mut future = signal.wait_async_timeout(Ticks(5));
        assert!(poll(&mut future, &counter).is_pending());
        signal.send();
        assert_eq!(poll(&mut future, &counter), Poll::Ready(Ok(())));
        assert_eq!(Timer::get_next_timeout(), None);
        let mut pending = signal.wait_async_timeout(Ticks(10));
        assert!(poll(&mut pending, &counter).is_pending());
        drop(pending);
        assert_eq!(signal.waiter_count(), 0);
        assert_eq!(Timer::get_next_timeout(), None);
    }

    #[test]
    #[serial]
    fn async_wait_capacity_is_explicit_and_reusable_after_cancel() {
        crate::utils::kernel_init();
        let signal = Signal::new();
        let counter = Arc::new(Counter::default());
        let mut waiters: std::vec::Vec<_> = (0..crate::config::MAX_ASYNC_WAITERS)
            .map(|_| signal.wait_async())
            .collect();
        for waiter in &mut waiters {
            assert!(poll(waiter, &counter).is_pending());
        }
        let mut overflow = signal.wait_async();
        assert_eq!(
            poll(&mut overflow, &counter),
            Poll::Ready(Err(RtosError::WaiterQueueFull))
        );
        waiters.pop();
        let mut replacement = signal.wait_async();
        assert!(poll(&mut replacement, &counter).is_pending());
        drop(waiters);
        drop(replacement);
        assert_eq!(signal.waiter_count(), 0);
    }
}
