#![no_std]
#![no_main]

#[path = "../../../shared/task_lifecycle.rs"]
mod task_lifecycle;

use core::sync::atomic::{AtomicBool, Ordering};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use neon_rtos2::prelude::*;
use neon_rtos2::runtime::{Executor, sleep};

static FIRST_RAN: AtomicBool = AtomicBool::new(false);
static SECOND_RAN: AtomicBool = AtomicBool::new(false);
static MUTEX_TIMED_OUT: AtomicBool = AtomicBool::new(false);
static DELAY_DONE: AtomicBool = AtomicBool::new(false);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = hprintln!("FAIL: {}", info);
    debug::exit(debug::EXIT_FAILURE);
    loop {
        core::hint::spin_loop();
    }
}

fn idle_until_test_ends() {
    loop {
        Delay::delay(Ticks(1000)).unwrap();
    }
}

#[entry]
fn main() -> ! {
    kernel_init();
    let peripherals = cortex_m::Peripherals::take().unwrap();
    neon_rtos2::hal::cortex_m3::configure_systick(peripherals.SYST, 12_000_000, Duration::from_millis(1)).unwrap();
    // SysTick may fire before startup; it must not enter PendSV with an uninitialized PSP.
    cortex_m::asm::delay(100_000);

    let queue = Mq::<u32, 1>::new().unwrap();
    let sender = queue.clone();
    let close_signal = neon_rtos2::sync::Signal::new();
    let closer = close_signal.clone();
    let contested = neon_rtos2::sync::Mutex::with_priority_inheritance(());
    let restored = neon_rtos2::sync::Signal::new();
    let ack = restored.clone();
    let owner = contested.clone();
    let cv = neon_rtos2::sync::CondVar::new();
    let notifier = cv.clone();
    let predicate = neon_rtos2::sync::Mutex::new(false);
    let producer = predicate.clone();
    Task::builder("first")
        .priority(Priority::High)
        .spawn(|| {
            FIRST_RAN.store(true, Ordering::SeqCst);
            while !SECOND_RAN.load(Ordering::SeqCst) {
                core::hint::spin_loop();
            }
            Delay::delay(Ticks(2)).unwrap();
            DELAY_DONE.store(true, Ordering::SeqCst);
            idle_until_test_ends();
        })
        .unwrap();
    Task::builder("second")
        .priority(Priority::High)
        .spawn(move || {
            SECOND_RAN.store(true, Ordering::SeqCst);
            while !FIRST_RAN.load(Ordering::SeqCst) {
                core::hint::spin_loop();
            }
            let held = owner.lock_owned().unwrap();
            sender.push_wait(42).unwrap();
            while closer.sync_waiter_count() == 0 {
                core::hint::spin_loop();
            }
            closer.close();
            closer.reset();
            while !MUTEX_TIMED_OUT.load(Ordering::Acquire) {
                core::hint::spin_loop();
            }
            assert_eq!(Scheduler::get_current_task().get_priority(), Priority::High);
            ack.send_and_schedule();
            drop(held);
            while notifier.waiter_count() == 0 {
                core::hint::spin_loop();
            }
            *producer.lock().unwrap() = true;
            notifier.notify_one();
            idle_until_test_ends();
        })
        .unwrap();
    Task::builder("check").priority(Priority::Critical).spawn(move || {
        assert_eq!(queue.pop_wait().unwrap(), 42);
        assert_eq!(close_signal.wait(), Err(neon_rtos2::error::RtosError::SignalClosed));
        let signal = neon_rtos2::sync::Signal::new();
        assert_eq!(signal.wait_timeout(Ticks(3)), Err(neon_rtos2::error::RtosError::Timeout));
        assert_eq!(contested.lock_timeout(Ticks(3)).unwrap_err(), RtosError::Timeout);
        assert_eq!(contested.waiter_count(), 0);
        MUTEX_TIMED_OUT.store(true, Ordering::Release);
        restored.wait().unwrap();
        drop(contested.lock().unwrap());
        let sem = neon_rtos2::sync::Semaphore::new(0);
        assert_eq!(sem.acquire_timeout(Ticks(3)), Err(RtosError::Timeout));
        assert_eq!(sem.waiter_count(), 0);
        let guard = predicate.lock().unwrap();
        let (guard, timed_out) = cv.wait_while_timeout(guard, Ticks(100), |ready| !ready).unwrap();
        assert!(!timed_out && *guard);
        let (guard, timed_out) = cv.wait_timeout(guard, Ticks(3)).unwrap();
        assert!(timed_out && predicate.is_locked_by_current());
        drop(guard);
        let mut executor = Executor::new();
        executor.spawn(async {
            sleep(Ticks(5)).await;
            let sem = neon_rtos2::sync::Semaphore::new(2);
            let permit = sem.acquire_permits_async(2).await.unwrap();
            assert_eq!(sem.available_permits(), 0);
            drop(permit);
            assert_eq!(sem.available_permits(), 2);
            let signal = neon_rtos2::sync::Signal::new();
            assert_eq!(signal.wait_async_timeout(Ticks(3)).await, Err(neon_rtos2::error::RtosError::Timeout));
        });
        let (tx, rx) = bounded::<u32, 1>().unwrap();
        Task::builder("blocking producer").spawn(move || {
            tx.send_blocking(1u32).unwrap();
            tx.send_blocking(2u32).unwrap();
        }).unwrap();
        let (reply, replies) = bounded::<u32, 1>().unwrap();
        executor.spawn(async move {
            assert_eq!(rx.recv().await, Some(1));
            assert_eq!(rx.recv().await, Some(2));
            assert_eq!(rx.recv().await, None);
            reply.send(3).await.unwrap();
        });
        executor.run();
        assert_eq!(replies.recv_blocking(), Some(3));
        assert_eq!(replies.recv_blocking(), None);
        task_lifecycle::exercise();
        assert!(FIRST_RAN.load(Ordering::SeqCst));
        assert!(SECOND_RAN.load(Ordering::SeqCst));
        assert!(DELAY_DONE.load(Ordering::SeqCst));
        let _ = hprintln!("PASS: preemption, FIFO, shared queue, delay, async sleep, signal close, signal timeout, priority inheritance, mutex handoff, semaphore timeout, condvar notify/timeout, channel disconnect, task exit/reuse");
        debug::exit(debug::EXIT_SUCCESS);
        loop { core::hint::spin_loop(); }
    }).unwrap();
    Scheduler::enable_priority_scheduling();
    Scheduler::start();
    loop {
        cortex_m::asm::wfi();
    }
}
