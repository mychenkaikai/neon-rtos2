use neon_rtos2::prelude::{RtosError, kernel_init};
use neon_rtos2::runtime::{TryRecvError, SendError, channel, unbounded};
use neon_rtos2::sync::{CondVar, Mutex, RwLock, Semaphore};
use serial_test::serial;
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};

#[derive(Default)]
struct Counter(AtomicUsize);
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
impl Counter {
    fn wakes(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}
fn poll<F: Future + Unpin>(future: &mut F, wake: &Arc<Counter>) -> Poll<F::Output> {
    Pin::new(future).poll(&mut Context::from_waker(&Waker::from(wake.clone())))
}

#[test]
#[serial]
fn mutex_wait_refreshes_and_registers_again_after_losing_the_lock() {
    kernel_init();
    let m = Mutex::new(1);
    let owned = m.lock_owned().unwrap();
    let mut wait = m.lock_async();
    let old = Arc::new(Counter::default());
    let new = Arc::new(Counter::default());
    assert!(poll(&mut wait, &old).is_pending());
    assert!(poll(&mut wait, &new).is_pending());
    let mut cancelled = m.lock_async();
    assert!(poll(&mut cancelled, &old).is_pending());
    drop(cancelled);
    assert_eq!(m.async_waiter_count(), 1);
    drop(owned);
    let other = m.try_lock().unwrap();
    assert_eq!((old.wakes(), new.wakes()), (0, 1));
    assert!(poll(&mut wait, &new).is_pending());
    drop(other);
    assert_eq!(new.wakes(), 2);
    assert!(matches!(poll(&mut wait, &new), Poll::Ready(Ok(_))));
}

#[test]
#[serial]
fn writer_that_loses_to_two_readers_wakes_when_the_last_reader_leaves() {
    kernel_init();
    let m = RwLock::new(0);
    let first = m.write().unwrap();
    let wake = Arc::new(Counter::default());
    let mut writer = m.write_async();
    let mut reader = m.read_async();
    assert!(poll(&mut writer, &wake).is_pending());
    assert!(poll(&mut reader, &wake).is_pending());
    drop(first);
    let Poll::Ready(Ok(reader)) = poll(&mut reader, &wake) else {
        panic!()
    };
    let another = m.try_read().unwrap();
    assert!(poll(&mut writer, &wake).is_pending());
    drop(another);
    assert_eq!(wake.wakes(), 2);
    drop(reader);
    assert_eq!(wake.wakes(), 3);
    assert!(matches!(poll(&mut writer, &wake), Poll::Ready(Ok(_))));
}

#[test]
#[serial]
fn returning_owned_permit_wakes_small_request_behind_large_one() {
    kernel_init();
    let sem = Semaphore::with_max(1, 2);
    let held = sem.acquire_owned().unwrap();
    let mut large = sem.acquire_many_async(2);
    let mut small = sem.acquire_async();
    let wake = Arc::new(Counter::default());
    assert!(poll(&mut large, &wake).is_pending());
    assert!(poll(&mut small, &wake).is_pending());
    drop(held);
    assert_eq!(wake.wakes(), 2);
    assert!(poll(&mut large, &wake).is_pending());
    assert_eq!(poll(&mut small, &wake), Poll::Ready(Ok(())));
    drop(large);
    assert_eq!(sem.waiter_count(), 0);
    sem.release_many(2).unwrap();
    assert_eq!(
        sem.release_many(usize::MAX),
        Err(RtosError::SemaphoreOverflow)
    );
    assert_eq!(sem.available_permits(), 2);
    let held = sem.acquire_owned().unwrap();
    sem.close();
    drop(held);
    assert_eq!(sem.available_permits(), 1);
}

#[test]
#[serial]
fn condvar_requires_a_notification_and_forwards_a_cancelled_notify_one() {
    kernel_init();
    neon_rtos2::prelude::Scheduler::start();
    let m = Mutex::new(0);
    let cv = CondVar::new();
    let old = Arc::new(Counter::default());
    let wake = Arc::new(Counter::default());
    let mut a = Box::pin(cv.wait_async(m.try_lock().unwrap()));
    assert!(poll(&mut a, &old).is_pending());
    assert!(poll(&mut a, &wake).is_pending());
    let mut b = Box::pin(cv.wait_async(m.try_lock().unwrap()));
    assert!(poll(&mut b, &wake).is_pending());
    let guard = m.try_lock().unwrap();
    cv.notify_one();
    assert_eq!((old.wakes(), wake.wakes()), (0, 1));
    assert!(poll(&mut a, &wake).is_pending()); // Notified, waiting for the lock.
    drop(a);
    assert_eq!(wake.wakes(), 2);
    drop(guard);
    let Poll::Ready(Ok(mut guard)) = poll(&mut b, &wake) else { panic!("lost notification") };
    *guard = 42;
    let mut late = Box::pin(cv.wait_while_async(guard, |value| *value == 42));
    cv.notify_all();
    assert!(poll(&mut late, &wake).is_pending());
    cv.close();
    assert!(matches!(poll(&mut late, &wake), Poll::Ready(Err(RtosError::CondVarClosed))));
    assert_eq!(*m.try_lock().unwrap(), 42);
}

#[test]
fn channel_forwards_space_and_messages_on_cancellation() {
    let (tx, rx) = channel(1);
    let old = Arc::new(Counter::default());
    let wake = Arc::new(Counter::default());
    tx.try_send(1).unwrap();
    let mut a = tx.send(2);
    let mut b = tx.send(3);
    assert!(poll(&mut a, &old).is_pending());
    assert!(poll(&mut a, &wake).is_pending());
    assert!(poll(&mut b, &wake).is_pending());
    assert_eq!(rx.try_recv(), Ok(1));
    drop(a);
    assert_eq!((old.wakes(), wake.wakes()), (0, 2));
    assert_eq!(poll(&mut b, &wake), Poll::Ready(Ok(())));
    assert_eq!(rx.try_recv(), Ok(3));
    let mut r1 = rx.recv();
    let mut r2 = rx.recv();
    assert!(poll(&mut r1, &wake).is_pending());
    assert!(poll(&mut r2, &wake).is_pending());
    tx.try_send(4).unwrap();
    drop(r1);
    assert_eq!(wake.wakes(), 4);
    assert_eq!(poll(&mut r2, &wake), Poll::Ready(Some(4)));
}

#[test]
fn channel_disconnect_drains_messages_and_returns_unsent_values() {
    let (tx, rx) = unbounded::<u32>();
    let clone = tx.clone();
    drop(tx);
    clone.try_send(42).unwrap();
    drop(clone);
    assert_eq!(rx.try_recv(), Ok(42));
    assert_eq!(rx.try_recv(), Err(TryRecvError::Closed));
    let (tx, rx) = channel(1);
    tx.try_send(1).unwrap();
    let mut pending = tx.send(2);
    let wake = Arc::new(Counter::default());
    assert!(poll(&mut pending, &wake).is_pending());
    drop(rx);
    assert_eq!(wake.wakes(), 1);
    assert_eq!(
        poll(&mut pending, &wake),
        Poll::Ready(Err(SendError(2)))
    );
}

#[test]
fn channel_waker_can_receive_from_the_same_channel() {
    use neon_rtos2::runtime::Receiver;
    struct Consume(Arc<Receiver<u32>>);
    impl Wake for Consume {
        fn wake(self: Arc<Self>) {
            assert_eq!(self.0.try_recv(), Ok(7));
        }
    }

    let (tx, rx) = channel(1);
    let rx = Arc::new(rx);
    let mut pending = rx.recv();
    let waker = Waker::from(Arc::new(Consume(rx.clone())));
    let mut cx = Context::from_waker(&waker);
    assert!(Pin::new(&mut pending).poll(&mut cx).is_pending());
    tx.try_send(7).unwrap();
    assert!(Pin::new(&mut pending).poll(&mut cx).is_pending());
    drop(pending);
    tx.try_send(8).unwrap();
    assert_eq!(rx.try_recv(), Ok(8));
}

#[test]
#[serial]
fn condvar_delivers_mutex_wakers_before_parking_and_remembers_their_notify() {
    use neon_rtos2::prelude::{Scheduler, TaskState};
    struct Notifier(CondVar, Mutex<bool>);
    impl Wake for Notifier {
        fn wake(self: Arc<Self>) {
            assert!(!matches!(
                Scheduler::get_current_task().get_state(),
                TaskState::Blocked(_)
            ));
            *self.1.try_lock().unwrap() = true;
            self.0.notify_one();
        }
    }
    kernel_init();
    let cv = CondVar::new();
    let mutex = Mutex::new(false);
    let guard = mutex.lock().unwrap();
    let waker = Waker::from(Arc::new(Notifier(cv.clone(), mutex.clone())));
    let mut notifier = mutex.lock_async();
    assert!(
        Pin::new(&mut notifier)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    let (guard, timed_out) = cv.wait_timeout(guard, neon_rtos2::prelude::Ticks(100)).unwrap();
    assert!(!timed_out && *guard);
    assert_eq!(cv.waiter_count(), 0);
}
