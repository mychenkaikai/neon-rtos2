//! # 类型化消息通道
//!
//! 多生产者单接收端，支持 RTOS 阻塞等待和 async/await。
//!
//! ## 特性
//!
//! - 🚀 **异步发送/接收**: 支持 async/await 语法
//! - 📦 **有界缓冲**: 可配置的通道容量
//! - 🔄 **多生产者**: 支持克隆发送端
//! - ⚡ **非阻塞尝试**: 提供 try_send/try_recv 方法
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::runtime::channel;
//!
//! async fn example() {
//!     let (tx, rx) = channel::<u32>(16);
//!
//!     // 生产者
//!     let tx1 = tx.clone();
//!     let producer = async move {
//!         for i in 0..10 {
//!             tx1.send(i).await.unwrap();
//!         }
//!     };
//!
//!     // 消费者
//!     let consumer = async move {
//!         while let Some(value) = rx.recv().await {
//!             // println!("Received: {}", value);
//!         }
//!     };
//! }
//! ```

use crate::compat::{Arc, VecDeque};
use crate::sync::wait::AsyncWaiters;
use crate::kernel::scheduler::Scheduler;
use crate::sync::Event;

// Each direction has a distinct, stable address inside the Arc allocation.
// Check the predicate and park with interrupts masked to avoid a lost wake.
#[derive(Default)]
struct TaskWait {
    pending: bool,
}

impl TaskWait {
    fn event(&self) -> Event {
        Event::Ipc(core::ptr::from_ref(self) as usize)
    }

    fn park(&mut self) {
        self.pending = true;
        Scheduler::get_current_task().block(self.event());
    }

    fn notify(&mut self) {
        if core::mem::take(&mut self.pending) {
            Event::wake_task(self.event());
        }
    }
}

use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use critical_section::Mutex;

// ============================================================================
// 错误类型
// ============================================================================

/// A blocking or async send failed because the channel closed. Contains the unsent message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendError<T>(pub T);

/// Nonblocking send failure. Both variants return the unsent message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrySendError<T> {
    /// 通道已关闭
    Closed(T),
    /// 通道已满
    Full(T),
}

/// 接收错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    /// 通道已关闭
    Closed,
    /// 通道为空
    Empty,
}

// ============================================================================
// 通道内部状态
// ============================================================================

/// 通道内部状态
struct ChannelInner<T> {
    /// 消息缓冲区
    buffer: VecDeque<T>,
    /// 通道容量
    capacity: usize,
    /// 是否已关闭
    closed: bool,
    /// 等待发送的 Waker
    send_waiters: AsyncWaiters,
    /// 等待接收的 Waker
    recv_waiters: AsyncWaiters,
    senders: usize,
    blocked_senders: TaskWait,
    blocked_receivers: TaskWait,
}

impl<T> ChannelInner<T> {
    fn new(capacity: usize, buffer: VecDeque<T>) -> Self {
        Self {
            buffer,
            capacity,
            closed: false,
            send_waiters: AsyncWaiters::new(),
            recv_waiters: AsyncWaiters::new(),
            senders: 1,
            blocked_senders: TaskWait::default(),
            blocked_receivers: TaskWait::default(),
        }
    }
    fn push(&mut self, value: T) -> (Result<(), TrySendError<T>>, Option<Waker>) {
        if self.closed {
            return (Err(TrySendError::Closed(value)), None);
        }
        if self.buffer.len() == self.capacity {
            return (Err(TrySendError::Full(value)), None);
        }
        self.buffer.push_back(value);
        self.blocked_receivers.notify();
        (Ok(()), self.recv_waiters.pop())
    }

    fn pop(&mut self) -> (Result<T, TryRecvError>, Option<Waker>) {
        match self.buffer.pop_front() {
            Some(value) => {
                self.blocked_senders.notify();
                (Ok(value), self.send_waiters.pop())
            }
            None if self.closed => (Err(TryRecvError::Closed), None),
            None => (Err(TryRecvError::Empty), None),
        }
    }

    fn close(&mut self) -> ChannelWake {
        self.closed = true;
        self.blocked_senders.notify();
        self.blocked_receivers.notify();
        ChannelWake(self.send_waiters.drain(), self.recv_waiters.drain())
    }
}

struct ChannelWake(VecDeque<(u64, Waker)>, VecDeque<(u64, Waker)>);
impl ChannelWake {
    fn wake(self) {
        for (_, waker) in self.0.into_iter().chain(self.1) {
            waker.wake();
        }
    }
}

// ============================================================================
// 发送端
// ============================================================================

/// 异步通道发送端
///
/// 可以克隆以支持多生产者模式。
pub struct Sender<T> {
    inner: Arc<Mutex<RefCell<ChannelInner<T>>>>,
}

impl<T> Sender<T> {
    /// 异步发送消息
    ///
    /// 如果通道已满，会等待直到有空间可用。
    ///
    /// # 返回值
    /// - `Ok(())`: 发送成功
    /// - `Err(SendError(value))`: 通道已关闭
    pub fn send(&self, value: T) -> SendFuture<'_, T> {
        SendFuture {
            sender: self,
            value: Some(value),
            token: None,
        }
    }

    /// 尝试发送（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(())`: 发送成功
    /// - `Err(TrySendError::Full(value))`: 通道已满
    /// - `Err(TrySendError::Closed(value))`: 通道已关闭
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        let (result, next) = critical_section::with(|cs| self.inner.borrow_ref_mut(cs).push(value));
        if let Some(waker) = next {
            waker.wake();
        }
        result
    }

    /// Send from an RTOS task, parking it while the buffer is full.
    pub fn send_blocking(&self, mut value: T) -> Result<(), SendError<T>> {
        loop {
            let (result, wake) = critical_section::with(|cs| {
                let mut state = self.inner.borrow_ref_mut(cs);
                let outcome = state.push(value);
                if matches!(outcome.0, Err(TrySendError::Full(_))) {
                    state.blocked_senders.park();
                }
                outcome
            });
            if let Some(waker) = wake { waker.wake(); }
            match result {
                Err(TrySendError::Full(returned)) => value = returned,
                Err(TrySendError::Closed(value)) => return Err(SendError(value)),
                Ok(()) => return Ok(()),
            }
            crate::hal::trigger_schedule();
        }
    }

    /// 关闭发送端
    ///
    /// 关闭后，接收端仍可接收已发送的消息，
    /// 但新的发送操作会失败。
    pub fn close(&self) {
        critical_section::with(|cs| self.inner.borrow_ref_mut(cs).close()).wake();
    }

    /// 检查通道是否已关闭
    pub fn is_closed(&self) -> bool {
        critical_section::with(|cs| self.inner.borrow_ref(cs).closed)
    }

    /// 获取当前缓冲区中的消息数量
    pub fn len(&self) -> usize {
        critical_section::with(|cs| self.inner.borrow_ref(cs).buffer.len())
    }

    /// 检查缓冲区是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 检查缓冲区是否已满
    pub fn is_full(&self) -> bool {
        critical_section::with(|cs| {
            let inner = self.inner.borrow_ref(cs);
            inner.buffer.len() == inner.capacity
        })
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let inner = self.inner.clone();
        critical_section::with(|cs| inner.borrow_ref_mut(cs).senders += 1);
        Self { inner }
    }
}

// ============================================================================
// 接收端
// ============================================================================

/// 异步通道接收端
///
/// 接收端拥有通道的接收权。
pub struct Receiver<T> {
    inner: Arc<Mutex<RefCell<ChannelInner<T>>>>,
}

impl<T> Receiver<T> {
    /// 异步接收消息
    ///
    /// 如果通道为空，会等待直到有消息可用或通道关闭。
    ///
    /// # 返回值
    /// - `Some(value)`: 成功接收消息
    /// - `None`: 通道已关闭且缓冲区为空
    pub fn recv(&self) -> RecvFuture<'_, T> {
        RecvFuture {
            receiver: self,
            token: None,
        }
    }

    /// 尝试接收（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(value)`: 成功接收消息
    /// - `Err(TryRecvError::Empty)`: 通道为空
    /// - `Err(TryRecvError::Closed)`: 通道已关闭且缓冲区为空
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let (result, next) = critical_section::with(|cs| self.inner.borrow_ref_mut(cs).pop());
        if let Some(waker) = next {
            waker.wake();
        }
        result
    }

    /// Receive from an RTOS task. A drained, closed channel returns None.
    pub fn recv_blocking(&self) -> Option<T> {
        loop {
            let (result, wake) = critical_section::with(|cs| {
                let mut state = self.inner.borrow_ref_mut(cs);
                let outcome = state.pop();
                if matches!(outcome.0, Err(TryRecvError::Empty)) {
                    state.blocked_receivers.park();
                }
                outcome
            });
            if let Some(waker) = wake { waker.wake(); }
            match result {
                Ok(value) => return Some(value),
                Err(TryRecvError::Closed) => return None,
                Err(TryRecvError::Empty) => crate::hal::trigger_schedule(),
            }
        }
    }

    /// 检查通道是否已关闭
    pub fn is_closed(&self) -> bool {
        critical_section::with(|cs| self.inner.borrow_ref(cs).closed)
    }

    /// 获取当前缓冲区中的消息数量
    pub fn len(&self) -> usize {
        critical_section::with(|cs| self.inner.borrow_ref(cs).buffer.len())
    }

    /// 检查缓冲区是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ============================================================================
// Future 实现
// ============================================================================

/// 发送 Future
///
/// 等待通道有空间可用时完成发送。
pub struct SendFuture<'a, T> {
    sender: &'a Sender<T>,
    value: Option<T>,
    token: Option<u64>,
}

// SendFuture 是 Unpin 的，因为它不包含自引用
impl<'a, T> Unpin for SendFuture<'a, T> {}

impl<'a, T> Future for SendFuture<'a, T> {
    type Output = Result<(), SendError<T>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let value = this
            .value
            .take()
            .expect("SendFuture polled after completion");
        let mut incoming = Some(cx.waker().clone());
        let (result, old, next) = critical_section::with(|cs| {
            let mut state = this.sender.inner.borrow_ref_mut(cs);
            let (result, next) = state.push(value);
            let completed = match result {
                Err(TrySendError::Full(value)) => {
                    this.value = Some(value);
                    let old = state.send_waiters.register(&mut this.token, incoming.take().unwrap());
                    return (Poll::Pending, old, None);
                }
                Err(TrySendError::Closed(value)) => Err(SendError(value)),
                Ok(()) => Ok(()),
            };
            (Poll::Ready(completed), state.send_waiters.remove(this.token.take()), next)
        });
        drop(old);
        if let Some(waker) = next {
            waker.wake();
        }
        result
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let wake = critical_section::with(|cs| {
            let mut state = self.inner.borrow_ref_mut(cs);
            state.senders -= 1;
            (state.senders == 0).then(|| state.close())
        });
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let (wake, buffer) = critical_section::with(|cs| {
            let mut state = self.inner.borrow_ref_mut(cs);
            (state.close(), core::mem::take(&mut state.buffer))
        });
        wake.wake();
        drop(buffer);
    }
}

/// 接收 Future
///
/// 等待通道有消息可用时完成接收。
pub struct RecvFuture<'a, T> {
    receiver: &'a Receiver<T>,
    token: Option<u64>,
}

impl<'a, T> Future for RecvFuture<'a, T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut incoming = Some(cx.waker().clone());
        let (result, old, next) = critical_section::with(|cs| {
            let mut state = this.receiver.inner.borrow_ref_mut(cs);
            let (result, next) = state.pop();
            match result {
                Err(TryRecvError::Empty) => {
                    let old = state
                        .recv_waiters
                        .register(&mut this.token, incoming.take().unwrap());
                    (Poll::Pending, old, None)
                }
                ready => (
                    Poll::Ready(ready.ok()),
                    state.recv_waiters.remove(this.token.take()),
                    next,
                ),
            }
        });
        drop(old);
        if let Some(waker) = next {
            waker.wake();
        }
        result
    }
}

// ============================================================================
impl<T> Drop for RecvFuture<'_, T> {
    fn drop(&mut self) {
        let Some(id) = self.token.take() else {
            return;
        };
        let (old, next) = critical_section::with(|cs| {
            let mut state = self.receiver.inner.borrow_ref_mut(cs);
            let old = state.recv_waiters.remove(Some(id));
            let next = if old.is_none() && !state.buffer.is_empty() {
                state.recv_waiters.pop()
            } else {
                None
            };
            (old, next)
        });
        drop(old);
        if let Some(waker) = next {
            waker.wake();
        }
    }
}

impl<T> Drop for SendFuture<'_, T> {
    fn drop(&mut self) {
        let Some(id) = self.token.take() else {
            return;
        };
        let (old, next) = critical_section::with(|cs| {
            let mut state = self.sender.inner.borrow_ref_mut(cs);
            let old = state.send_waiters.remove(Some(id));
            let next = if old.is_none() && !state.closed && state.buffer.len() < state.capacity {
                state.send_waiters.pop()
            } else {
                None
            };
            (old, next)
        });
        drop(old);
        if let Some(waker) = next {
            waker.wake();
        }
    }
}

// 通道创建函数
// ============================================================================

/// 创建异步通道
///
/// # 参数
/// - `capacity`: 通道容量（缓冲区大小）
///
/// # 返回值
/// - `(Sender<T>, Receiver<T>)`: 发送端和接收端
///
/// # 示例
/// ```rust,no_run
/// use neon_rtos2::runtime::channel;
///
/// async fn example() {
///     let (tx, rx) = channel::<u32>(16);
///
///     // 生产者
///     let tx1 = tx.clone();
///     let producer = async move {
///         for i in 0..10 {
///             tx1.send(i).await.unwrap();
///         }
///     };
///
///     // 消费者
///     let consumer = async move {
///         while let Some(value) = rx.recv().await {
///             // println!("Received: {}", value);
///         }
///     };
///
///     // 注意：实际使用中通常会将 producer 和 consumer spawn 到执行器中
///     // executor.spawn(producer);
///     // executor.spawn(consumer);
/// }
/// ```
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    assert!(capacity > 0, "channel capacity must be positive");
    let buffer = if capacity == usize::MAX { VecDeque::new() } else { VecDeque::with_capacity(capacity) };
    endpoints(capacity, buffer)
}

/// Create a typed channel with room for N messages, shared by tasks and futures.
/// The buffer is reserved on the heap at creation. N must be positive.
///
/// ```
/// use neon_rtos2::prelude::*;
/// let (tx, rx) = bounded::<String, 1>().unwrap();
/// tx.try_send(String::from("first")).unwrap();
/// let Err(TrySendError::Full(second)) = tx.try_send(String::from("second")) else {
///     panic!("expected a full channel");
/// };
/// assert_eq!(rx.try_recv().unwrap(), "first");
/// tx.try_send(second).unwrap();
/// drop(tx);
/// assert_eq!(rx.try_recv().unwrap(), "second");
/// assert_eq!(rx.try_recv(), Err(TryRecvError::Closed));
/// ```
pub fn bounded<T, const N: usize>() -> crate::error::Result<(Sender<T>, Receiver<T>)> {
    if N == 0 { return Err(crate::error::RtosError::InvalidArgument); }
    let mut buffer = VecDeque::new();
    buffer.try_reserve_exact(N).map_err(|_| crate::error::RtosError::OutOfMemory)?;
    Ok(endpoints(N, buffer))
}

fn endpoints<T>(capacity: usize, buffer: VecDeque<T>) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(RefCell::new(ChannelInner::new(capacity, buffer))));

    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}

/// 创建无界通道
///
/// 实际上是一个容量为 usize::MAX 的有界通道。
/// 注意：在嵌入式环境中应谨慎使用，可能导致内存耗尽。
pub fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
    channel(usize::MAX)
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_basic() {
        let (tx, rx) = channel::<u32>(10);

        assert!(tx.try_send(1).is_ok());
        assert!(tx.try_send(2).is_ok());

        assert_eq!(rx.try_recv().unwrap(), 1);
        assert_eq!(rx.try_recv().unwrap(), 2);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_channel_full() {
        let (tx, rx) = channel::<u32>(2);

        assert!(tx.try_send(1).is_ok());
        assert!(tx.try_send(2).is_ok());

        // 通道已满
        match tx.try_send(3) {
            Err(TrySendError::Full(3)) => {}
            _ => panic!("Expected Full error"),
        }

        // 接收一个后可以再发送
        assert_eq!(rx.try_recv().unwrap(), 1);
        assert!(tx.try_send(3).is_ok());
    }

    #[test]
    fn test_channel_closed() {
        let (tx, rx) = channel::<u32>(10);

        tx.try_send(1).unwrap();
        tx.close();

        // 可以接收已发送的消息
        assert_eq!(rx.try_recv().unwrap(), 1);

        // 通道已关闭
        assert_eq!(rx.try_recv(), Err(TryRecvError::Closed));

        // 发送失败
        match tx.try_send(2) {
            Err(TrySendError::Closed(2)) => {}
            _ => panic!("Expected Closed error"),
        }
    }

    #[test]
    fn test_sender_clone() {
        let (tx1, rx) = channel::<u32>(10);
        let tx2 = tx1.clone();

        tx1.try_send(1).unwrap();
        tx2.try_send(2).unwrap();

        assert_eq!(rx.try_recv().unwrap(), 1);
        assert_eq!(rx.try_recv().unwrap(), 2);
    }

    #[test]
    fn test_channel_len() {
        let (tx, rx) = channel::<u32>(10);

        assert_eq!(tx.len(), 0);
        assert!(tx.is_empty());

        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();

        assert_eq!(tx.len(), 2);
        assert_eq!(rx.len(), 2);
        assert!(!tx.is_empty());

        rx.try_recv().unwrap();
        assert_eq!(tx.len(), 1);
    }

    #[test]
    fn test_channel_is_full() {
        let (tx, _rx) = channel::<u32>(2);

        assert!(!tx.is_full());
        tx.try_send(1).unwrap();
        assert!(!tx.is_full());
        tx.try_send(2).unwrap();
        assert!(tx.is_full());
    }

    #[test]
    fn test_channel_is_closed() {
        let (tx, rx) = channel::<u32>(10);

        assert!(!tx.is_closed());
        assert!(!rx.is_closed());

        tx.close();

        assert!(tx.is_closed());
        assert!(rx.is_closed());
    }
}
