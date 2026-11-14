//! # Future 辅助类型
//!
//! 提供常用的 Future 实现，包括异步信号量、异步定时器等。

use crate::kernel::time::systick::Ticks;
use crate::kernel::time::systick::{Deadline, Systick};
use crate::kernel::time::timer::Timer;
use crate::sync::wait::AsyncWaiters;
use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use critical_section::Mutex;

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
/// # use neon_rtos2::runtime::{AsyncSignal, sleep};
/// static SIGNAL: AsyncSignal = AsyncSignal::new();
///
/// // 生产者
/// async fn producer() {
///     loop {
///         // 生产数据...
///         SIGNAL.send();
///         sleep(neon_rtos2::prelude::Ticks(100)).await;
///     }
/// }
///
/// // 消费者
/// async fn consumer() {
///     loop {
///         SIGNAL.wait_async().await;
///         // 消费数据...
///     }
/// }
/// ```
pub struct AsyncSignal {
    state: Mutex<RefCell<AsyncSignalState>>,
}

struct AsyncSignalState {
    count: usize,
    waiters: AsyncWaiters,
}

impl AsyncSignal {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(AsyncSignalState {
                count: 0,
                waiters: AsyncWaiters::new(),
            })),
        }
    }

    pub fn send(&self) {
        let waker = critical_section::with(|cs| {
            let mut state = self.state.borrow_ref_mut(cs);
            state.count = state.count.saturating_add(1);
            state.waiters.pop()
        });
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub fn send_many(&self, n: usize) {
        let wakers: alloc::vec::Vec<Waker> = critical_section::with(|cs| {
            let mut state = self.state.borrow_ref_mut(cs);
            state.count = state.count.saturating_add(n);
            (0..n).map_while(|_| state.waiters.pop()).collect()
        });
        for waker in wakers {
            waker.wake();
        }
    }

    pub fn wait_async(&self) -> SignalFuture<'_> {
        SignalFuture {
            signal: self,
            token: None,
        }
    }

    pub fn try_wait(&self) -> bool {
        critical_section::with(|cs| {
            let mut state = self.state.borrow_ref_mut(cs);
            if state.count == 0 {
                return false;
            }
            state.count -= 1;
            true
        })
    }

    pub fn count(&self) -> usize {
        critical_section::with(|cs| self.state.borrow_ref(cs).count)
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
    token: Option<u64>,
}

impl Future for SignalFuture<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let wait = self.get_mut();
        let mut incoming = Some(cx.waker().clone());
        let (poll, obsolete) = critical_section::with(|cs| {
            let mut state = wait.signal.state.borrow_ref_mut(cs);
            if state.count > 0 {
                state.count -= 1;
                (Poll::Ready(()), state.waiters.remove(wait.token.take()))
            } else {
                let old = state
                    .waiters
                    .register(&mut wait.token, incoming.take().unwrap());
                (Poll::Pending, old)
            }
        });
        drop(obsolete);
        poll
    }
}

impl Drop for SignalFuture<'_> {
    fn drop(&mut self) {
        let Some(id) = self.token.take() else { return };
        let (removed, next) = critical_section::with(|cs| {
            let mut state = self.signal.state.borrow_ref_mut(cs);
            let removed = state.waiters.remove(Some(id));
            // A sender already dequeued this waiter. Forward its unconsumed permit.
            let next = if removed.is_none() && state.count > 0 {
                state.waiters.pop()
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

// ============================================================================
// 异步定时器
// ============================================================================

/// 异步睡眠
///
/// 返回一个在指定时间后完成的 Future。
///
/// # 参数
/// - `duration`: 睡眠时间（Duration 或 Ticks）
///
/// # 示例
///
/// ```rust,ignore
/// async fn periodic_task() {
///     loop {
///         // 执行任务...
///         sleep(neon_rtos2::prelude::Ticks(100)).await;
///     }
/// }
/// ```
pub fn sleep(duration: impl Into<Ticks>) -> Sleep {
    Sleep::new(duration)
}

/// Wait at most `duration` for an operation. The deadline starts at construction.
/// If both branches are ready in the same poll, the operation wins. Expiry
/// drops the supplied future before returning, releasing its wait registration.
pub fn timeout<F: Future>(
    duration: impl Into<Ticks>,
    operation: F,
) -> impl Future<Output = crate::error::Result<F::Output>> {
    let wait = super::select2(operation, sleep(duration));
    async move {
        match wait.await {
            super::Either::First(value) => Ok(value),
            super::Either::Second(()) => Err(crate::error::RtosError::Timeout),
        }
    }
}

/// 睡眠 Future
///
/// 在指定时间后完成的 Future
pub struct Sleep {
    /// 目标时间（tick）
    deadline: Deadline,
    /// 异步睡眠等待槽位 ID
    waiter_id: Option<u64>,
}

impl Sleep {
    /// 创建新的睡眠 Future
    pub fn new(duration: impl Into<Ticks>) -> Self {
        Self {
            deadline: Deadline::after(duration.into().0),
            waiter_id: None,
        }
    }

    /// 获取剩余时间
    pub fn remaining(&self) -> Ticks {
        Ticks(self.deadline.remaining_at(Systick::now()))
    }

    /// 检查是否已超时
    pub fn is_elapsed(&self) -> bool {
        self.deadline.elapsed()
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        critical_section::with(|_| {
            if self.deadline.elapsed() {
                if let Some(id) = self.waiter_id.take() {
                    Timer::unregister_async_sleep(id);
                }
                Poll::Ready(())
            } else {
                if let Some(id) = self.waiter_id {
                    Timer::update_async_sleep(id, cx.waker().clone());
                } else {
                    self.waiter_id = Some(Timer::register_async_sleep(
                        self.deadline,
                        cx.waker().clone(),
                    ));
                }
                Poll::Pending
            }
        })
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(waiter_id) = self.waiter_id.take() {
            Timer::unregister_async_sleep(waiter_id);
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
pub use core::future::{ready, pending, Ready, Pending};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::time::timer::Timer;
    use crate::utils::kernel_init;
    use core::task::{RawWaker, RawWakerVTable, Waker};
    use serial_test::serial;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counting_waker(counter: Arc<AtomicUsize>) -> Waker {
        unsafe fn clone(data: *const ()) -> RawWaker {
            let arc = unsafe { Arc::<AtomicUsize>::from_raw(data as *const AtomicUsize) };
            let cloned = arc.clone();
            let _ = Arc::into_raw(arc);
            RawWaker::new(Arc::into_raw(cloned) as *const (), &VTABLE)
        }

        unsafe fn wake(data: *const ()) {
            let arc = unsafe { Arc::<AtomicUsize>::from_raw(data as *const AtomicUsize) };
            arc.fetch_add(1, Ordering::SeqCst);
        }

        unsafe fn wake_by_ref(data: *const ()) {
            let arc = unsafe { Arc::<AtomicUsize>::from_raw(data as *const AtomicUsize) };
            arc.fetch_add(1, Ordering::SeqCst);
            let _ = Arc::into_raw(arc);
        }

        unsafe fn drop(data: *const ()) {
            let _ = unsafe { Arc::<AtomicUsize>::from_raw(data as *const AtomicUsize) };
        }

        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

        let raw_waker = RawWaker::new(Arc::into_raw(counter) as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }

    #[test]
    #[serial]
    fn timeout_cleans_up_both_waits_and_prefers_a_ready_operation() {
        kernel_init();
        let signal = crate::sync::Signal::new();
        let mut cx = Context::from_waker(Waker::noop());
        {
            let mut wait = core::pin::pin!(timeout(Ticks(2), signal.wait_async()));
            assert!(wait.as_mut().poll(&mut cx).is_pending());
            assert_eq!(signal.async_waiter_count(), 1);
            assert_eq!(Timer::async_sleep_count(), 1);
        }
        assert_eq!(signal.async_waiter_count(), 0);
        assert_eq!(Timer::async_sleep_count(), 0);
        let mut expired = core::pin::pin!(timeout(Ticks(0), signal.wait_async()));
        assert_eq!(expired.as_mut().poll(&mut cx), Poll::Ready(Err(crate::error::RtosError::Timeout)));
        assert_eq!(signal.async_waiter_count(), 0);
        signal.send();
        let mut ready = core::pin::pin!(timeout(Ticks(0), signal.wait_async()));
        assert_eq!(ready.as_mut().poll(&mut cx), Poll::Ready(Ok(Ok(()))));
    }

    #[test]
    fn async_signal_refreshes_waiters_and_forwards_cancelled_notifications() {
        let signal = AsyncSignal::new();
        let old_count = Arc::new(AtomicUsize::new(0));
        let new_count = Arc::new(AtomicUsize::new(0));
        let old_waker = counting_waker(old_count.clone());
        let new_waker = counting_waker(new_count.clone());
        let mut old = Context::from_waker(&old_waker);
        let mut new = Context::from_waker(&new_waker);
        let mut a = signal.wait_async();
        let mut b = signal.wait_async();
        let mut c = signal.wait_async();
        assert!(Pin::new(&mut a).poll(&mut old).is_pending());
        assert!(Pin::new(&mut a).poll(&mut new).is_pending());
        assert!(Pin::new(&mut b).poll(&mut new).is_pending());
        assert!(Pin::new(&mut c).poll(&mut new).is_pending());
        signal.send_many(2);
        assert_eq!(old_count.load(Ordering::SeqCst), 0);
        assert_eq!(new_count.load(Ordering::SeqCst), 2);
        drop(a);
        assert_eq!(new_count.load(Ordering::SeqCst), 3);
        assert!(Pin::new(&mut b).poll(&mut new).is_ready());
        assert!(Pin::new(&mut c).poll(&mut new).is_ready());
        assert_eq!(signal.count(), 0);
    }

    #[test]
    fn test_async_signal_basic() {
        let signal = AsyncSignal::new();

        assert_eq!(signal.count(), 0);
        assert!(!signal.try_wait());

        signal.send();
        assert_eq!(signal.count(), 1);
        assert!(signal.try_wait());
        assert_eq!(signal.count(), 0);
    }

    #[test]
    fn test_async_signal_multiple() {
        let signal = AsyncSignal::new();

        signal.send_many(5);
        assert_eq!(signal.count(), 5);

        for _ in 0..5 {
            assert!(signal.try_wait());
        }
        assert!(!signal.try_wait());
    }

    #[test]
    #[serial]
    fn test_sleep_creation() {
        kernel_init();

        let sleep = Sleep::new(Ticks(100));
        assert!(!sleep.is_elapsed());
        assert!(sleep.remaining() <= Ticks(100));
    }

    #[test]
    #[serial]
    fn test_sleep_elapsed() {
        kernel_init();

        let sleep = Sleep::new(Ticks(50));
        Systick::add_current_time(100);

        assert!(sleep.is_elapsed());
        assert_eq!(sleep.remaining(), Ticks(0));
    }

    #[test]
    #[serial]
    fn test_sleep_register_once_and_wake() {
        kernel_init();

        let wake_count = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wake_count.clone());
        let mut cx = Context::from_waker(&waker);

        let mut sleep = Sleep::new(Ticks(10));
        let mut pinned = Pin::new(&mut sleep);

        assert!(matches!(pinned.as_mut().poll(&mut cx), Poll::Pending));
        assert_eq!(Timer::async_sleep_count(), 1);

        // 二次 poll 不应重复注册新的等待槽位
        assert!(matches!(pinned.as_mut().poll(&mut cx), Poll::Pending));
        assert_eq!(Timer::async_sleep_count(), 1);

        Systick::add_current_time(10);
        Timer::timer_check_and_send_event();

        assert_eq!(wake_count.load(Ordering::SeqCst), 1);
        assert_eq!(Timer::async_sleep_count(), 0);
        assert!(matches!(pinned.poll(&mut cx), Poll::Ready(())));
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
            Poll::Pending => {}
            Poll::Ready(_) => panic!("First poll should be Pending"),
        }

        // 第二次 poll 应该返回 Ready
        match pinned.poll(&mut cx) {
            Poll::Ready(_) => {}
            Poll::Pending => panic!("Second poll should be Ready"),
        }
    }

    #[test]
    #[serial]
    fn expired_sleep_drop_cannot_unregister_reused_slot() {
        kernel_init();
        let count = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(count.clone());
        let mut cx = Context::from_waker(&waker);
        let mut old = Sleep::new(Ticks(1));
        assert!(Pin::new(&mut old).poll(&mut cx).is_pending());
        Systick::add_current_time(1);
        Timer::timer_check_and_send_event();
        let mut new = Sleep::new(Ticks(10));
        assert!(Pin::new(&mut new).poll(&mut cx).is_pending());
        drop(old);
        assert_eq!(Timer::async_sleep_count(), 1);
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(10)));
        Systick::add_current_time(10);
        Timer::timer_check_and_send_event();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(Pin::new(&mut new).poll(&mut cx).is_ready());
    }

    #[test]
    #[serial]
    fn sleep_refreshes_waker_and_cancels_on_drop() {
        kernel_init();
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        let first_waker = counting_waker(first.clone());
        let second_waker = counting_waker(second.clone());
        let mut sleep = Sleep::new(Ticks(5));
        assert!(
            Pin::new(&mut sleep)
                .poll(&mut Context::from_waker(&first_waker))
                .is_pending()
        );
        assert!(
            Pin::new(&mut sleep)
                .poll(&mut Context::from_waker(&second_waker))
                .is_pending()
        );
        Systick::add_current_time(5);
        Timer::timer_check_and_send_event();
        assert_eq!(first.load(Ordering::SeqCst), 0);
        assert_eq!(second.load(Ordering::SeqCst), 1);
        let mut canceled = Sleep::new(Ticks(100));
        assert!(
            Pin::new(&mut canceled)
                .poll(&mut Context::from_waker(&second_waker))
                .is_pending()
        );
        drop(canceled);
        assert_eq!(Timer::async_sleep_count(), 0);
    }
}
