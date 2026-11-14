#![no_std]
#![no_main]

//! Measure a software-pended SysTick across a real priority-inheriting unlock.
//! The periodic tick is disabled. The timeout workload pends one real tick per round.
extern crate alloc;

use alloc::{sync::Arc, task::Wake, vec::Vec};
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    task::{Context, Waker},
};
use cortex_m::peripheral::{SCB, SYST, syst::SystClkSource};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use neon_rtos2::{
    config::{MAX_ASYNC_WAITERS, MAX_TASKS},
    prelude::{Delay, Event, Priority, Scheduler, Task, TaskState, kernel_init},
    sync::{Mutex, Semaphore},
};

const WORKERS: usize = MAX_TASKS - 2; // idle and owner occupy the two other slots
const ROUNDS: usize = 16;
const RELOAD: u32 = 0x00ff_ffff;
static ARM: AtomicBool = AtomicBool::new(false);
static RUN_TICK: AtomicBool = AtomicBool::new(false);
// SysTick is the only writer while profiling. A nested CPSID keeps the
// outer timestamp; the critical-section backend issues CPSIE on outer release.
static PROFILE_MASK: AtomicBool = AtomicBool::new(false);
static MASK_START: AtomicU32 = AtomicU32::new(u32::MAX);
static MASK_MAX: AtomicU32 = AtomicU32::new(0);
static TICK_COST: AtomicU32 = AtomicU32::new(0);
static COMPLETIONS: AtomicUsize = AtomicUsize::new(0);
static START: AtomicU32 = AtomicU32::new(0);
static ELAPSED: AtomicU32 = AtomicU32::new(u32::MAX);

unsafe extern "C" {
    fn __real___cpsid();
    fn __real_SysTick();
    fn __real___cpsie();
}

// Linker wraps the cortex-m assembly function only in this executable. After the
// normal CPSID, inject exactly one IRQ in the first critical section of drop(guard).
#[unsafe(no_mangle)]
extern "C" fn __wrap___cpsid() {
    unsafe { __real___cpsid() };
    if PROFILE_MASK.load(Ordering::Relaxed) && MASK_START.load(Ordering::Relaxed) == u32::MAX {
        MASK_START.store(SYST::get_current(), Ordering::Relaxed);
    }
    if ARM.load(Ordering::Relaxed) {
        ARM.store(false, Ordering::Relaxed);
        START.store(SYST::get_current(), Ordering::Relaxed);
        SCB::set_pendst();
    }
}

#[unsafe(no_mangle)]
extern "C" fn __wrap___cpsie() {
    if PROFILE_MASK.load(Ordering::Relaxed) {
        let end = SYST::get_current();
        let start = MASK_START.swap(u32::MAX, Ordering::Relaxed);
        assert_ne!(start, u32::MAX);
        let span = start.wrapping_sub(end) & RELOAD;
        MASK_MAX.store(span.max(MASK_MAX.load(Ordering::Relaxed)), Ordering::Relaxed);
    }
    unsafe { __real___cpsie() };
}

#[unsafe(no_mangle)]
extern "C" fn __wrap_SysTick() {
    let end = SYST::get_current();
    if RUN_TICK.swap(false, Ordering::Relaxed) {
        PROFILE_MASK.store(true, Ordering::Relaxed);
        unsafe { __real_SysTick() };
        PROFILE_MASK.store(false, Ordering::Relaxed);
        TICK_COST.store(end.wrapping_sub(SYST::get_current()) & RELOAD, Ordering::Release);
        return;
    }
    let start = START.load(Ordering::Relaxed);
    ELAPSED.store(start.wrapping_sub(end) & RELOAD, Ordering::Release);
}

fn sample(f: impl FnOnce()) -> u32 {
    assert!(cortex_m::register::primask::read().is_active());
    ELAPSED.store(u32::MAX, Ordering::Relaxed);
    ARM.store(true, Ordering::Release);
    f();
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    let ticks = ELAPSED.load(Ordering::Acquire);
    assert!(
        !ARM.load(Ordering::Relaxed) && ticks != u32::MAX,
        "probe IRQ was not delivered"
    );
    ticks
}

struct CountWake(AtomicUsize);
impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        assert!(cortex_m::register::primask::read().is_active());
        assert_ne!(ELAPSED.load(Ordering::Acquire), u32::MAX);
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

// Each worker rearms Delay(1) before the lower-priority owner can run again.
fn timeout_burst() -> u32 {
    let mut maximum = 0;
    for round in 1..=ROUNDS {
        assert_eq!(Task::blocked_tasks().filter(|t| t.get_state() == TaskState::Blocked(Event::Delay)).count(), WORKERS);
        RUN_TICK.store(true, Ordering::Release);
        SCB::set_pendst();
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        assert_eq!(COMPLETIONS.load(Ordering::Acquire), round * WORKERS);
        let cost = TICK_COST.load(Ordering::Acquire);
        assert!(cost > 0);
        maximum = maximum.max(cost);
    }
    maximum
}

// Low owns A, normal owns B and waits for A, high waits for B.
fn chain_unlock() -> u32 {
    let outer = Mutex::with_priority_inheritance(());
    let inner = Mutex::with_priority_inheritance(());
    let owner = Scheduler::get_current_task();
    let mut maximum = 0;
    for _ in 0..ROUNDS {
        let guard = outer.lock().unwrap();
        let (a, b) = (outer.clone(), inner.clone());
        let middle = Task::builder("middle").priority(Priority::Normal).spawn(move || {
            let _hold = b.lock().unwrap();
            drop(a.lock().unwrap());
        }).unwrap();
        Scheduler::task_switch();
        assert_eq!(owner.get_priority(), Priority::Normal);
        let b = inner.clone();
        let high = Task::builder("high").priority(Priority::High).spawn(move || {
            drop(b.lock().unwrap());
        }).unwrap();
        Scheduler::task_switch();
        assert_eq!(middle.get_priority(), Priority::High);
        assert_eq!(owner.get_priority(), Priority::High);
        maximum = maximum.max(sample(|| drop(guard)));
        Scheduler::task_switch();
        assert_eq!(owner.get_priority(), Priority::Low);
        assert_eq!(high.get_state(), TaskState::Finished);
        assert_eq!(middle.get_state(), TaskState::Finished);
    }
    maximum
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    hprintln!("FAIL: {}", info);
    debug::exit(debug::EXIT_FAILURE);
    loop {
        cortex_m::asm::bkpt();
    }
}

#[entry]
fn main() -> ! {
    kernel_init();
    let mut syst = cortex_m::Peripherals::take().unwrap().SYST;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(RELOAD);
    syst.clear_current();
    syst.disable_interrupt();
    syst.enable_counter();

    let lock = Mutex::with_priority_inheritance(());
    let go = Semaphore::new(0);
    let arrived = Semaphore::new(0);
    let done = Semaphore::new(0);
    for _ in 0..WORKERS {
        let (lock, go, arrived, done) = (lock.clone(), go.clone(), arrived.clone(), done.clone());
        Task::builder("waiter")
            .priority(Priority::High)
            .spawn(move || {
                for _ in 0..ROUNDS {
                    go.acquire().unwrap();
                    arrived.release().unwrap();
                    drop(lock.lock().unwrap());
                    done.release().unwrap();
                }
                for _ in 0..ROUNDS {
                    Delay::delay(neon_rtos2::prelude::Ticks(1)).unwrap();
                    COMPLETIONS.fetch_add(1, Ordering::Release);
                }
            })
            .unwrap();
    }
    Task::builder("owner").priority(Priority::Critical).spawn(move || {
        // Owner runs first, then blocks on arrivals even if its priority is boosted.
        Scheduler::get_current_task().set_priority(Priority::Low);
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut cx = Context::from_waker(&waker);
        let mut pending = Vec::with_capacity(MAX_ASYNC_WAITERS);
        let mut empty_max = 0;
        let mut loaded_max = 0;
        for round in 0..ROUNDS {
            let guard = lock.lock().unwrap();
            empty_max = empty_max.max(sample(|| drop(guard)));

            let guard = lock.lock().unwrap();
            for _ in 0..MAX_ASYNC_WAITERS {
                let mut future = lock.lock_async();
                assert!(Pin::new(&mut future).poll(&mut cx).is_pending());
                pending.push(future);
            }
            go.release_many(WORKERS).unwrap();
            arrived.acquire_many(WORKERS).unwrap();
            assert_eq!(lock.sync_waiter_count(), WORKERS);
            assert_eq!(lock.async_waiter_count(), MAX_ASYNC_WAITERS);
            assert_eq!(Scheduler::get_current_task().get_priority(), Priority::High);
            loaded_max = loaded_max.max(sample(|| drop(guard)));
            assert_eq!(Scheduler::get_current_task().get_priority(), Priority::Low);
            assert_eq!(counter.0.load(Ordering::Relaxed), (round + 1) * MAX_ASYNC_WAITERS);
            for mut future in pending.drain(..) {
                assert!(matches!(Pin::new(&mut future).poll(&mut cx), core::task::Poll::Ready(Ok(_))));
            }
            done.acquire_many(WORKERS).unwrap();
        }
        assert!(loaded_max > empty_max && empty_max > 0);
        let burst = timeout_burst();
        let masked = MASK_MAX.load(Ordering::Acquire);
        // Catch a regression to holding the mask across the entire timeout batch.
        assert!(masked > 0 && masked < burst / 2);
        let chain = chain_unlock();
        hprintln!("PASS wait_latency rounds={} tasks={} futures={} empty_max_ticks={} loaded_max_ticks={} burst_handler_max_ticks={} burst_masked_max_ticks={} chain_unlock_max_ticks={}",
            ROUNDS, WORKERS, MAX_ASYNC_WAITERS, empty_max, loaded_max, burst, masked, chain);
        debug::exit(debug::EXIT_SUCCESS);
        loop { cortex_m::asm::bkpt(); }
    }).unwrap();
    Scheduler::enable_priority_scheduling();
    Scheduler::start();
    loop {
        cortex_m::asm::wfi();
    }
}
