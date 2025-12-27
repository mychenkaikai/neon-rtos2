//! # 异步通道
//!
//! 提供支持 async/await 的 MPSC（多生产者单消费者）通道。
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
//! ```rust,ignore
//! use neon_rtos2::runtime::channel;
//!
//! let (tx, rx) = channel::<u32>(16);
//!
//! // 生产者
//! executor.spawn(async move {
//!     for i in 0..10 {
//!         tx.send(i).await.unwrap();
//!     }
//! });
//!
//! // 消费者
//! executor.spawn(async move {
//!     while let Some(value) = rx.recv().await {
//!         println!("Received: {}", value);
//!     }
//! });
//! ```

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use crate::compat::{Arc, VecDeque};
use spin::Mutex;

// ============================================================================
// 错误类型
// ============================================================================

/// 发送错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError<T> {
    /// 通道已关闭
    Closed(T),
    /// 通道已满
    Full(T),
}

/// 接收错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvError {
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
    send_waiters: VecDeque<Waker>,
    /// 等待接收的 Waker
    recv_waiters: VecDeque<Waker>,
}

impl<T> ChannelInner<T> {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
            closed: false,
            send_waiters: VecDeque::new(),
            recv_waiters: VecDeque::new(),
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
    inner: Arc<Mutex<ChannelInner<T>>>,
}

impl<T> Sender<T> {
    /// 异步发送消息
    ///
    /// 如果通道已满，会等待直到有空间可用。
    ///
    /// # 返回值
    /// - `Ok(())`: 发送成功
    /// - `Err(SendError::Closed(value))`: 通道已关闭
    pub fn send(&self, value: T) -> SendFuture<'_, T> {
        SendFuture {
            sender: self,
            value: Some(value),
        }
    }

    /// 尝试发送（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(())`: 发送成功
    /// - `Err(SendError::Full(value))`: 通道已满
    /// - `Err(SendError::Closed(value))`: 通道已关闭
    pub fn try_send(&self, value: T) -> Result<(), SendError<T>> {
        let mut inner = self.inner.lock();
        
        if inner.closed {
            return Err(SendError::Closed(value));
        }
        
        if inner.buffer.len() >= inner.capacity {
            return Err(SendError::Full(value));
        }
        
        inner.buffer.push_back(value);
        
        // 唤醒一个等待接收的任务
        if let Some(waker) = inner.recv_waiters.pop_front() {
            waker.wake();
        }
        
        Ok(())
    }

    /// 关闭发送端
    ///
    /// 关闭后，接收端仍可接收已发送的消息，
    /// 但新的发送操作会失败。
    pub fn close(&self) {
        let mut inner = self.inner.lock();
        inner.closed = true;
        
        // 唤醒所有等待的接收者
        while let Some(waker) = inner.recv_waiters.pop_front() {
            waker.wake();
        }
        
        // 唤醒所有等待的发送者
        while let Some(waker) = inner.send_waiters.pop_front() {
            waker.wake();
        }
    }

    /// 检查通道是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.lock().closed
    }

    /// 获取当前缓冲区中的消息数量
    pub fn len(&self) -> usize {
        self.inner.lock().buffer.len()
    }

    /// 检查缓冲区是否为空
    pub fn is_empty(&self) -> bool {
        self.inner.lock().buffer.is_empty()
    }

    /// 检查缓冲区是否已满
    pub fn is_full(&self) -> bool {
        let inner = self.inner.lock();
        inner.buffer.len() >= inner.capacity
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

// ============================================================================
// 接收端
// ============================================================================

/// 异步通道接收端
///
/// 不可克隆，确保单消费者模��。
pub struct Receiver<T> {
    inner: Arc<Mutex<ChannelInner<T>>>,
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
        RecvFuture { receiver: self }
    }

    /// 尝试接收（非阻塞）
    ///
    /// # 返回值
    /// - `Ok(value)`: 成功接收消息
    /// - `Err(RecvError::Empty)`: 通道为空
    /// - `Err(RecvError::Closed)`: 通道已关闭且缓冲区为空
    pub fn try_recv(&self) -> Result<T, RecvError> {
        let mut inner = self.inner.lock();
        
        if let Some(value) = inner.buffer.pop_front() {
            // 唤醒一个等待发送的任务
            if let Some(waker) = inner.send_waiters.pop_front() {
                waker.wake();
            }
            Ok(value)
        } else if inner.closed {
            Err(RecvError::Closed)
        } else {
            Err(RecvError::Empty)
        }
    }

    /// 检查通道是否已关闭
    pub fn is_closed(&self) -> bool {
        self.inner.lock().closed
    }

    /// 获取当前缓冲区中的消息数量
    pub fn len(&self) -> usize {
        self.inner.lock().buffer.len()
    }

    /// 检查缓冲区是否为空
    pub fn is_empty(&self) -> bool {
        self.inner.lock().buffer.is_empty()
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
}

// SendFuture 是 Unpin 的，因为它不包含自引用
impl<'a, T> Unpin for SendFuture<'a, T> {}

impl<'a, T> Future for SendFuture<'a, T> {
    type Output = Result<(), SendError<T>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 因为 SendFuture 是 Unpin 的，可以安全地获取可变引用
        let this = self.get_mut();
        
        let value = match this.value.take() {
            Some(v) => v,
            None => panic!("SendFuture polled after completion"),
        };
        
        let mut inner = this.sender.inner.lock();
        
        if inner.closed {
            return Poll::Ready(Err(SendError::Closed(value)));
        }
        
        if inner.buffer.len() < inner.capacity {
            inner.buffer.push_back(value);
            
            // 唤醒一个等待接收的任务
            if let Some(waker) = inner.recv_waiters.pop_front() {
                waker.wake();
            }
            
            Poll::Ready(Ok(()))
        } else {
            // 通道已满，注册 waker 并��新存储值
            inner.send_waiters.push_back(cx.waker().clone());
            drop(inner);
            this.value = Some(value);
            Poll::Pending
        }
    }
}

/// 接收 Future
///
/// 等待通道有消息可用时完成接收。
pub struct RecvFuture<'a, T> {
    receiver: &'a Receiver<T>,
}

impl<'a, T> Future for RecvFuture<'a, T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.receiver.inner.lock();
        
        if let Some(value) = inner.buffer.pop_front() {
            // 唤醒一个等待发送的任务
            if let Some(waker) = inner.send_waiters.pop_front() {
                waker.wake();
            }
            Poll::Ready(Some(value))
        } else if inner.closed {
            Poll::Ready(None)
        } else {
            // 通道为空，注册 waker
            inner.recv_waiters.push_back(cx.waker().clone());
            Poll::Pending
        }
    }
}

// ============================================================================
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
///
/// ```rust,ignore
/// let (tx, rx) = channel::<u32>(16);
///
/// // 生产者
/// executor.spawn(async move {
///     for i in 0..10 {
///         tx.send(i).await.unwrap();
///     }
/// });
///
/// // 消费者
/// executor.spawn(async move {
///     while let Some(value) = rx.recv().await {
///         println!("Received: {}", value);
///     }
/// });
/// ```
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Mutex::new(ChannelInner::new(capacity)));

    (
        Sender { inner: inner.clone() },
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
            Err(SendError::Full(3)) => {},
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
        assert_eq!(rx.try_recv(), Err(RecvError::Closed));
        
        // 发送失败
        match tx.try_send(2) {
            Err(SendError::Closed(2)) => {},
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

