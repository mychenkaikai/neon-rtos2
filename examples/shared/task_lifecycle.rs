// Run on a real scheduler. More sequential jobs than physical stacks must
// complete. A retained handle from the previous job cannot affect the next one.
use neon_rtos2::prelude::*;
use neon_rtos2::kernel::task::{Created, TypedTask};
use neon_rtos2::sync::{Semaphore, Signal};

// Exercise the portable DMA lifetime protocol on both CPUs. This backend
// models a transfer in software and never accesses a physical DMA engine.
struct TestDma {
    stopped: neon_rtos2::compat::Arc<core::sync::atomic::AtomicUsize>,
    ready: bool,
}

unsafe impl neon_rtos2::hal::DmaChannel for TestDma {
    type Error = core::convert::Infallible;

    unsafe fn start(&mut self, buffer: *mut u8, len: usize) -> core::result::Result<(), Self::Error> {
        assert!(len > 0);
        unsafe { buffer.write(42) };
        Ok(())
    }
    fn poll_complete(&mut self, cx: &mut core::task::Context<'_>) -> core::task::Poll<core::result::Result<(), Self::Error>> {
        if self.ready { return core::task::Poll::Ready(Ok(())); }
        self.ready = true;
        cx.waker().wake_by_ref();
        core::task::Poll::Pending
    }
    fn stop(&mut self) {
        self.ready = false;
        self.stopped.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

fn interrupts_enabled() -> bool {
    #[cfg(target_arch = "arm")]
    { cortex_m::register::primask::read().is_active() }
    #[cfg(target_arch = "riscv32")]
    { riscv::register::mstatus::read().mie() }
}

fn exercise_hal(executor: &mut LocalExecutor) {
    use core::sync::atomic::{AtomicUsize, Ordering};
    use neon_rtos2::{compat::{Arc, Box}, hal::{DmaChannel, with_interrupts_masked}};
    assert!(interrupts_enabled());
    with_interrupts_masked(|_| {
        assert!(!interrupts_enabled());
        with_interrupts_masked(|_| assert!(!interrupts_enabled()));
        assert!(!interrupts_enabled());
    });
    assert!(interrupts_enabled());

    let stopped = Arc::new(AtomicUsize::new(0));
    let channel = TestDma { stopped: stopped.clone(), ready: false };
    let operation = executor.spawn_joinable(channel.transfer(Box::from([0])));
    assert_eq!(executor.poll_once(), neon_rtos2::runtime::RunState::Runnable);
    drop(operation);
    executor.run();
    assert_eq!(stopped.load(Ordering::SeqCst), 1);
    let channel = TestDma { stopped: stopped.clone(), ready: false };
    executor.spawn(async move {
        let done = channel.transfer(Box::from([0])).await;
        done.result.unwrap();
        assert_eq!(&*done.buffer, &[42]);
    });
    executor.run();
    assert_eq!(stopped.load(Ordering::SeqCst), 2);
}

fn check_dispatch() {
    neon_rtos2::hal::with_interrupts_masked(|_| {
        let current = Scheduler::get_current_task();
        let mut running = Task::iter().filter(|task| task.get_state() == TaskState::Running);
        assert_eq!(running.next(), Some(current));
        assert!(running.next().is_none());
    });
}

pub fn exercise() {
    let mut executor = LocalExecutor::new();
    exercise_hal(&mut executor);
    // A parked future owns a signal registration. Cancelling its handle
    // schedules its destruction even when no signal is sent.
    let signal = Signal::new();
    let listener = signal.clone();
    let handle = executor.spawn_joinable(async move { listener.wait_async().await });
    assert_eq!(executor.poll_once(), neon_rtos2::runtime::RunState::Waiting);
    assert_eq!(signal.async_waiter_count(), 1);
    handle.cancel();
    executor.run();
    assert_eq!(signal.async_waiter_count(), 0);
    let mut worker = executor.spawn_joinable(async { 42u32 });
    executor.spawn(async move { assert_eq!(worker.join().await, Some(42)); });
    // Losing a select cancels only the wait. Join the same child again.
    let (send, recv) = neon_rtos2::sync::signal::signal_pair();
    let mut child = executor.spawn_joinable(async move {
        recv.wait_async().await.unwrap();
        assert_eq!(recv.wait_async().await, Err(RtosError::SignalClosed));
        7
    });
    executor.spawn(async move {
        use neon_rtos2::runtime::timeout;
        assert_eq!(timeout(Ticks(2), child.join()).await, Err(RtosError::Timeout));
        send.send();
        drop(send);
        assert_eq!(child.join().await, Some(7));
        assert_eq!(child.join().await, None);
    });
    let flag = Mutex::new(false);
    let changed = neon_rtos2::sync::CondVar::new();
    let writer = flag.clone();
    let notifier = changed.clone();
    executor.spawn(async move {
        let guard = flag.lock_async().await.unwrap();
        let guard = changed.wait_while_async(guard, |ready| !ready).await.unwrap();
        assert!(*guard);
    });
    executor.spawn(async move {
        *writer.lock_async().await.unwrap() = true;
        notifier.notify_one();
    });
    executor.run();
    drop(executor);

    // Cell is movable but not Sync. Ownership passes first into a task, then
    // through a full channel. Its value must survive rejection and retry.
    let (tx, rx) = bounded::<core::cell::Cell<u32>, 1>().unwrap();
    tx.try_send(core::cell::Cell::new(7)).unwrap();
    let frame = core::cell::Cell::new(11);
    let child = Task::builder("frame producer").priority(Priority::High).spawn(move || {
        let Err(TrySendError::Full(frame)) = tx.try_send(frame) else { panic!("expected full queue") };
        tx.send_blocking(frame).unwrap();
    }).unwrap();
    // Let the producer park before freeing the occupied slot.
    while !matches!(child.get_state(), TaskState::Blocked(_)) { Delay::delay(Ticks(1)).unwrap(); }
    assert_eq!(rx.recv_blocking().unwrap().get(), 7);
    assert_eq!(rx.recv_blocking().unwrap().get(), 11);
    assert!(rx.recv_blocking().is_none());
    drop(rx);

    // Disconnect a blocked producer while it still owns an unsent frame.
    let (tx, rx) = bounded::<u32, 1>().unwrap();
    tx.try_send(1).unwrap();
    let child = Task::builder("disconnect").priority(Priority::High).spawn(move || {
        assert_eq!(tx.send_blocking(2), Err(SendError(2)));
    }).unwrap();
    while !matches!(child.get_state(), TaskState::Blocked(_)) { Delay::delay(Ticks(1)).unwrap(); }
    drop(rx);
    child.join().unwrap();

    let (send, recv) = neon_rtos2::sync::signal::signal_pair();
    let waiter = Task::builder("endpoint disconnect").priority(Priority::High).spawn(move || {
        assert_eq!(recv.wait_timeout(Ticks(1000)), Err(RtosError::SignalClosed));
    }).unwrap();
    while !matches!(waiter.get_state(), TaskState::Blocked(_)) { Delay::delay(Ticks(1)).unwrap(); }
    drop(send);
    waiter.join().unwrap();

    // Two observers wait on the same creation. Cancelling the reservation wakes both.
    let reserved = TypedTask::<Created>::new("reserved", || panic!("cancelled entry ran")).unwrap();
    let target = *reserved.inner();
    let observers: [Task; 2] = core::array::from_fn(|_| {
        Task::builder("join observer").priority(Priority::High)
            .spawn(move || target.join().unwrap()).unwrap()
    });
    for observer in observers {
        while observer.get_state() != TaskState::Blocked(Event::TaskExit(target)) {
            Delay::delay(Ticks(1)).unwrap();
        }
    }
    drop(reserved);
    for observer in observers {
        observer.join().unwrap();
    }
    assert_eq!(target.get_state(), TaskState::Finished);
    assert_eq!(Scheduler::get_current_task().join(), Err(RtosError::InvalidTaskState));

    // Filling the user timer table must not prevent a kernel delay.
    let timers: [Timer; neon_rtos2::config::MAX_TIMERS] =
        core::array::from_fn(|_| Timer::after(Ticks(1000)).unwrap());
    assert!(matches!(Timer::after(Ticks(1)), Err(RtosError::TimerSlotsFull)));
    Delay::delay(Ticks(1)).unwrap();
    drop(timers);

    let gate = Semaphore::new(0);
    let timer = Timer::after(Ticks(2)).unwrap();
    timer.wait().unwrap();
    assert!(timer.is_elapsed());
    timer.cancel();
    let cancelled = Timer::after(Ticks(1000)).unwrap();
    cancelled.cancel();
    assert_eq!(Timer::snapshots().count(), 0);
    let mut previous: Option<Task> = None;
    for _ in 0..3 * neon_rtos2::config::MAX_TASKS {
        let permission = gate.clone();
        let child = critical_section::with(|_| {
            let child = Task::builder("short job")
                .priority(Priority::High)
                .spawn(move || {
                    check_dispatch();
                    permission.acquire().unwrap();
                })
                .unwrap();
            assert!(child.block_if_ready(Event::None));
            assert!(!child.wake_if(Event::Delay));
            assert!(child.wake_if(Event::None));
            child
        });
        if let Some(mut old) = previous {
            assert_eq!(old.get_taskid(), child.get_taskid());
            assert_ne!(old, child);
            old.join().unwrap();
            old.set_priority(Priority::Idle);
            assert!(!old.block_if_ready(Event::None));
            assert!(!old.wake_if(Event::None));
            assert_eq!(child.get_priority(), Priority::High);
            assert_eq!(old.get_state(), TaskState::Finished);
        }
        check_dispatch();
        gate.release().unwrap();
        child.join().unwrap();
        assert_eq!(child.get_state(), TaskState::Finished);
        previous = Some(child);
    }
}
