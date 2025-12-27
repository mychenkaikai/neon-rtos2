//! # Future 辅助类型
//!
//! 提供常用的 Future 实现，包括异步信号量、异步定时器等。

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use alloc::collections::VecDeque;
use spin::Mutex;
use crate::kernel::time::systick::Systick;

// ============================================================================
// 异步信号量
// ============================================================================

/// 异步信号量
///
/// 支持 async/await 的信号量实现，允许任务异步等待信号。
///
/// # 示例
///
/// ```rust,no_run
/// # use neon_rtos2::runtime::AsyncSignal;
/// # use neon_rtos2::kernel::time::timer::Timer;
/// static SIGNAL: AsyncSignal = AsyncSignal::new();
///
/// // 生产者
/// async fn producer() {
///     loop {
///         // 生产数据...
///         SIGNAL.signal();
///         Timer::sleep(100).await;
///     }
/// }
///
/// // 消费者
/// async fn consumer() {
///     loop {
///         SIGNAL.wait().await;
///         // 消费数据...
///     }
/// }
/// ```
pub struct AsyncSignal {
    /// 等待队列
    waiters: Mutex<VecDeque<Waker>>,
    /// 信号计数
    count: Mutex<usize>,
}

impl AsyncSignal {
    /// 创建新的异步信号量
    pub const fn new() -> Self {
        Self {
            waiters: Mutex::new(VecDeque::new()),
            count: Mutex::new(0),
        }
    }

    /// 发送信号
    ///
    /// 增加信号计数，并唤醒一个等待的任务。
    pub fn signal(&self) {
        let mut count = self.count.lock();
        *count += 1;
        drop(count);
        
        // 唤醒一个等待者
        let mut waiters = self.waiters.lock();
        if let Some(waker) = waiters.pop_front() {
            waker.wake();
        }
    }

    /// 发送多个信号
    ///
    /// # 参数
    /// - `n`: 信号数量
    pub fn signal_n(&self, n: usize) {
        let mut count = self.count.lock();
        *count += n;
        drop(count);
        
        // 唤醒所有等待者
        let mut waiters = self.waiters.lock();
        for _ in 0..n {
            if let Some(waker) = waiters.pop_front() {
                waker.wake();
            } else {
                break;
            }
        }
    }

    /// 异步等待信号
    ///
    /// 返回一个 Future，在信号到达时完成。
    pub fn wait(&self) -> SignalFuture<'_> {
        SignalFuture { signal: self }
    }

    /// 尝试获取信号（非阻塞）
    ///
    /// # 返回值
    /// - `true`: 成功获取信号
    /// - `false`: 没有可用信号
    pub fn try_wait(&self) -> bool {
        let mut count = self.count.lock();
        if *count > 0 {
            *count -= 1;
            true
        } else {
            false
        }
    }

    /// 获取当前信号计数
    pub fn count(&self) -> usize {
        *self.count.lock()
    }
}

impl Default for AsyncSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// 信号 Future
///
/// 等待异步信号的 Future 实现
pub struct SignalFuture<'a> {
    signal: &'a AsyncSignal,
}

impl<'a> Future for SignalFuture<'a> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 尝试获取信号
        let mut count = self.signal.count.lock();
        
        if *count > 0 {
            *count -= 1;
            Poll::Ready(())
        } else {
            // 没有信号，注册 waker
            drop(count);
            let mut waiters = self.signal.waiters.lock();
            waiters.push_back(cx.waker().clone());
            Poll::Pending
        }
    }
}

// ============================================================================
// 异步定时器
// ============================================================================

/// 异步睡眠
///
/// 返回一个在指定时间后完成的 Future。
///
/// # 参数
/// - `duration_ms`: 睡眠时间（毫秒）
///
/// # 示例
///
/// ```rust,ignore
/// async fn periodic_task() {
///     loop {
///         // 执行任务...
///         sleep(100).await;
///     }
/// }
/// ```
pub fn sleep(duration_ms: usize) -> Sleep {
    Sleep::new(duration_ms)
}

/// 睡眠 Future
///
/// 在指定时间后完成的 Future
pub struct Sleep {
    /// 目标时间（tick）
    deadline: usize,
    /// 是否已注册 waker
    registered: bool,
}

impl Sleep {
    /// 创建新的睡眠 Future
    pub fn new(duration_ms: usize) -> Self {
        Self {
            deadline: Systick::get_current_time() + duration_ms,
            registered: false,
        }
    }

    /// 获取剩余时间
    pub fn remaining(&self) -> usize {
        let current = Systick::get_current_time();
        if current >= self.deadline {
            0
        } else {
            self.deadline - current
        }
    }

    /// 检查是否已超时
    pub fn is_elapsed(&self) -> bool {
        Systick::get_current_time() >= self.deadline
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if Systick::get_current_time() >= self.deadline {
            Poll::Ready(())
        } else {
            // 注册 waker（在实际实现中，需要将 waker 注册到定时器系统）
            if !self.registered {
                // TODO: 将 waker 注册到定时器中断处理程序
                // 这里简化处理，实际需要与 SysTick 中断集成
                let _ = cx.waker().clone();
                self.registered = true;
            }
            Poll::Pending
        }
    }
}

// ============================================================================
// Yield Future
// ============================================================================

/// 让出执行权
///
/// 返回一个立即让出执行权的 Future，允许其他任务运行。
///
/// # 示例
///
/// ```rust,ignore
/// async fn cooperative_task() {
///     loop {
///         // 执行一些工作...
///         yield_now().await; // 让其他任务有机会运行
///     }
/// }
/// ```
pub fn yield_now() -> Yield {
    Yield { yielded: false }
}

/// Yield Future
///
/// 让出一次执行权的 Future
pub struct Yield {
    yielded: bool,
}

impl Future for Yield {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            // 重新调度自己
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

// ============================================================================
// Ready Future
// ============================================================================

/// 立即就绪的 Future
///
/// 返回一个立即完成的 Future，携带指定的值。
///
/// # 示例
///
/// ```rust,ignore
/// let value = ready(42).await;
/// assert_eq!(value, 42);
/// ```
pub fn ready<T>(value: T) -> Ready<T> {
    Ready { value: Some(value) }
}

/// Ready Future
pub struct Ready<T> {
    value: Option<T>,
}

impl<T> Future for Ready<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: 我们不会移动 self，只是修改内部的 Option
        let this = unsafe { self.get_unchecked_mut() };
        match this.value.take() {
            Some(v) => Poll::Ready(v),
            None => panic!("Ready polled after completion"),
        }
    }
}

// ============================================================================
// Pending Future
// ============================================================================

/// 永不完成的 Future
///
/// 返回一个永远处于 Pending 状态的 Future。
/// 主要用于测试和特殊场景。
pub fn pending<T>() -> Pending<T> {
    Pending { _marker: core::marker::PhantomData }
}

/// Pending Future
pub struct Pending<T> {
    _marker: core::marker::PhantomData<T>,
}

impl<T> Future for Pending<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;

    #[test]
    fn test_async_signal_basic() {
        let signal = AsyncSignal::new();
        
        assert_eq!(signal.count(), 0);
        assert!(!signal.try_wait());
        
        signal.signal();
        assert_eq!(signal.count(), 1);
        assert!(signal.try_wait());
        assert_eq!(signal.count(), 0);
    }

    #[test]
    fn test_async_signal_multiple() {
        let signal = AsyncSignal::new();
        
        signal.signal_n(5);
        assert_eq!(signal.count(), 5);
        
        for _ in 0..5 {
            assert!(signal.try_wait());
        }
        assert!(!signal.try_wait());
    }

    #[test]
    fn test_sleep_creation() {
        kernel_init();
        
        let sleep = Sleep::new(100);
        assert!(!sleep.is_elapsed());
        assert!(sleep.remaining() <= 100);
    }

    #[test]
    fn test_sleep_elapsed() {
        kernel_init();
        
        let sleep = Sleep::new(50);
        Systick::add_current_time(100);
        
        assert!(sleep.is_elapsed());
        assert_eq!(sleep.remaining(), 0);
    }

    #[test]
    fn test_ready_future() {
        use core::task::{RawWaker, RawWakerVTable, Waker};
        
        // 创建一个简单的 waker
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(core::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        let raw_waker = RawWaker::new(core::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);
        
        let mut future = ready(42);
        let pinned = Pin::new(&mut future);
        
        match pinned.poll(&mut cx) {
            Poll::Ready(v) => assert_eq!(v, 42),
            Poll::Pending => panic!("Ready should be immediately ready"),
        }
    }

    #[test]
    fn test_yield_future() {
        use core::task::{RawWaker, RawWakerVTable, Waker};
        
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(core::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        let raw_waker = RawWaker::new(core::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);
        
        let mut future = yield_now();
        let mut pinned = Pin::new(&mut future);
        
        // 第一次 poll 应该返回 Pending
        match pinned.as_mut().poll(&mut cx) {
            Poll::Pending => {},
            Poll::Ready(_) => panic!("First poll should be Pending"),
        }
        
        // 第二次 poll 应该返回 Ready
        match pinned.poll(&mut cx) {
            Poll::Ready(_) => {},
            Poll::Pending => panic!("Second poll should be Ready"),
        }
    }
}

