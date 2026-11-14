use crate::compat::Vec;
use crate::kernel::time::systick::Ticks;
use crate::config::{MAX_TASKS, MAX_TIMERS};
use crate::error::{Result, RtosError};
use crate::hal::trigger_schedule;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::task::Task;
use crate::kernel::time::systick::{Deadline, Systick};
use crate::sync::event::Event;
use core::cell::RefCell;
use core::task::Waker;
use critical_section::Mutex;

#[derive(Debug, PartialEq, Clone, Copy)]
struct TimerInner {
    generation: u64,
    duration: usize,
    deadline: Option<Deadline>, // None once the tick handler has delivered expiry.
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerSnapshot {
    pub id: u64,
    pub remaining: Ticks,
}

struct AsyncSleepEntry {
    id: u64,
    deadline: Deadline,
    waker: Option<Waker>,
}

struct TaskTimeoutEntry {
    id: u64,
    deadline: Deadline,
    task: Task,
    event: Event,
}

pub(crate) struct TaskTimeout {
    slot: usize,
    id: u64,
}

impl Drop for TaskTimeout {
    fn drop(&mut self) {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            let slot = &mut manager.task_timeouts[self.slot];
            if slot.as_ref().is_some_and(|entry| entry.id == self.id) {
                *slot = None;
            }
        });
    }
}

struct TimerManager {
    timers: [Option<TimerInner>; MAX_TIMERS],
    async_sleeps: Vec<Option<AsyncSleepEntry>>,
    next_sleep_id: u64,
    next_timer_id: u64,
    task_timeouts: [Option<TaskTimeoutEntry>; MAX_TASKS],
    next_task_timeout_id: u64,
}

impl TimerManager {
    const fn new() -> Self {
        Self {
            timers: [None; MAX_TIMERS],
            async_sleeps: Vec::new(),
            next_sleep_id: 0,
            next_timer_id: 0,
            task_timeouts: [const { None }; MAX_TASKS],
            next_task_timeout_id: 0,
        }
    }
}

static TIMER_MANAGER: Mutex<RefCell<TimerManager>> = Mutex::new(RefCell::new(TimerManager::new()));

#[must_use = "dropping the timer cancels it"]
pub struct Timer {
    slot: usize,
    generation: u64,
}

impl Timer {
    /// 创建并启动一次性定时器
    ///
    /// 从固定表中取得槽位。创建时立即开始计时。
    pub fn after(timeout: impl Into<Ticks>) -> Result<Timer> {
        let timeout = timeout.into().0;
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            for i in 0..MAX_TIMERS {
                if manager.timers[i].is_none() {
                    let generation = manager.next_timer_id;
                    manager.next_timer_id = generation.checked_add(1).expect("timer IDs exhausted");
                    manager.timers[i] = Some(TimerInner {
                        generation,
                        duration: timeout,
                        deadline: Some(Deadline::after(timeout)),
                    });
                    return Ok(Timer {
                        slot: i,
                        generation,
                    });
                }
            }
            Err(RtosError::TimerSlotsFull)
        })
    }

    pub(crate) fn init() {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            for i in 0..MAX_TIMERS {
                manager.timers[i] = None;
            }
            manager.task_timeouts.fill_with(|| None);
            core::mem::take(&mut manager.async_sleeps)
        });
    }

    /// Cancel the deadline and release its slot immediately.
    pub fn cancel(self) {}

    /// Arm the same timer again for its original duration, measured from now.
    pub fn restart(&mut self) -> Result<()> {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            let timer = manager.timers[self.slot]
                .as_mut()
                .filter(|timer| timer.generation == self.generation)
                .ok_or(RtosError::TimerNotFound)?;
            timer.deadline = Some(Deadline::after(timer.duration));
            Ok(())
        })
    }

    pub fn is_elapsed(&self) -> bool {
        critical_section::with(|cs| {
            TIMER_MANAGER.borrow_ref(cs).timers[self.slot]
                .filter(|timer| timer.generation == self.generation)
                .and_then(|timer| timer.deadline)
                .is_none_or(Deadline::elapsed)
        })
    }

    /// Block the calling task until expiry. Already elapsed timers return immediately.
    pub fn wait(&self) -> Result<()> {
        loop {
            let elapsed = critical_section::with(|_| {
                if self.is_elapsed() {
                    return Ok(true);
                }
                let mut task = Scheduler::get_current_task();
                if !Scheduler::is_running() || task.get_taskid() == 0 {
                    return Err(RtosError::InvalidTaskState);
                }
                task.block(Event::Timer(self.generation));
                Ok(false)
            })?;
            if elapsed {
                return Ok(());
            }
            trigger_schedule();
        }
    }

    /// 遍历检查是否有满足条件的定时器，如果有的话就发送信号
    /// 普通定时器直接扫描固定槽位，启停不会积累待清理的堆节点。
    pub fn timer_check_and_send_event() {
        let current_time = Systick::now();
        // Scan empty slots together, but release the interrupt mask after each
        // expiry. Check and deliver the event atomically with respect to reuse.
        let mut timer_id = 0;
        while timer_id < MAX_TIMERS {
            critical_section::with(|cs| {
                let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
                while timer_id < MAX_TIMERS {
                    let id = timer_id;
                    timer_id += 1;
                    let Some(timer) = &mut manager.timers[id] else { continue };
                    if timer.deadline.is_some_and(|end| end.elapsed_at(current_time)) {
                        timer.deadline = None;
                        Event::wake_task(Event::Timer(timer.generation));
                        break;
                    }
                }
            });
        }
        let mut index = 0;
        while index < MAX_TASKS {
            critical_section::with(|cs| {
                let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
                while index < MAX_TASKS {
                    let slot = &mut manager.task_timeouts[index];
                    index += 1;
                    if slot.as_ref().is_some_and(|entry| entry.deadline.elapsed_at(current_time)) {
                        let entry = slot.take().unwrap();
                        entry.task.wake_if(entry.event);
                        break;
                    }
                }
            });
        }
        let sleep_slots = critical_section::with(|cs| TIMER_MANAGER.borrow_ref(cs).async_sleeps.len());

        // Fixed scratch space. Interrupt dispatch itself never allocates a Vec
        // for due sleepers, and arbitrary wake callbacks run outside the borrow.
        const BATCH: usize = 8;
        for start in (0..sleep_slots).step_by(BATCH) {
            let due: [Option<Waker>; BATCH] = critical_section::with(|cs| {
                let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
                core::array::from_fn(|i| {
                    let slot = manager.async_sleeps.get_mut(start + i)?;
                    if slot.as_ref()?.deadline.elapsed_at(current_time) {
                        slot.take()?.waker
                    } else {
                        None
                    }
                })
            });
            for waker in due.into_iter().flatten() {
                waker.wake();
            }
        }
    }

    pub(crate) fn register_task_timeout(
        deadline: Deadline,
        task: Task,
        event: Event,
    ) -> Result<TaskTimeout> {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            let slot = manager
                .task_timeouts
                .iter()
                .position(Option::is_none)
                .ok_or(RtosError::TimerSlotsFull)?;
            let id = manager.next_task_timeout_id;
            manager.next_task_timeout_id = id.checked_add(1).ok_or(RtosError::OutOfMemory)?;
            manager.task_timeouts[slot] = Some(TaskTimeoutEntry {
                id,
                deadline,
                task,
                event,
            });
            Ok(TaskTimeout { slot, id })
        })
    }

    pub(crate) fn register_async_sleep(deadline: Deadline, waker: Waker) -> u64 {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            let id = manager.next_sleep_id;
            manager.next_sleep_id = id.checked_add(1).expect("sleep registration IDs exhausted");
            let entry = AsyncSleepEntry {
                id,
                deadline,
                waker: Some(waker),
            };

            for slot in manager.async_sleeps.iter_mut() {
                if slot.is_none() {
                    *slot = Some(entry);
                    return id;
                }
            }

            manager.async_sleeps.push(Some(entry));
            id
        })
    }

    pub(crate) fn unregister_async_sleep(id: u64) {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            manager
                .async_sleeps
                .iter_mut()
                .find(|slot| slot.as_ref().is_some_and(|entry| entry.id == id))
                .and_then(Option::take)
        });
    }

    pub(crate) fn update_async_sleep(id: u64, waker: Waker) {
        let mut incoming = Some(waker);
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            let entry = manager
                .async_sleeps
                .iter_mut()
                .flatten()
                .find(|entry| entry.id == id)?;
            core::mem::replace(&mut entry.waker, incoming.take())
        });
    }

    #[cfg(test)]
    pub(crate) fn async_sleep_count() -> usize {
        critical_section::with(|cs| {
            let manager = TIMER_MANAGER.borrow_ref(cs);
            manager
                .async_sleeps
                .iter()
                .filter(|slot| slot.is_some())
                .count()
        })
    }

    /// 获取距离下一个定时器超时的剩余时间（tick）
    pub fn get_next_timeout() -> Option<Ticks> {
        let current_time = Systick::now();
        critical_section::with(|cs| {
            let manager = TIMER_MANAGER.borrow_ref(cs);
            manager
                .timers
                .iter()
                .flatten()
                .filter_map(|t| t.deadline)
                .chain(manager.async_sleeps.iter().flatten().map(|s| s.deadline))
                .chain(manager.task_timeouts.iter().flatten().map(|t| t.deadline))
                .min()
                .map(|end| Ticks(end.remaining_at(current_time)))
        })
    }

    /// Inspect occupied slots at one instant without borrowing live timers.
    pub fn snapshots() -> impl Iterator<Item = TimerSnapshot> {
        let snapshots: [_; MAX_TIMERS] = critical_section::with(|cs| {
            let now = Systick::now();
            TIMER_MANAGER.borrow_ref(cs).timers.map(|entry| entry.map(|timer| TimerSnapshot {
                id: timer.generation,
                remaining: Ticks(timer.deadline.map_or(0, |end| end.remaining_at(now))),
            }))
        });
        snapshots.into_iter().flatten()
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            let slot = &mut manager.timers[self.slot];
            if slot.is_some_and(|timer| timer.generation == self.generation) {
                *slot = None;
                Event::wake_task(Event::Timer(self.generation));
            }
        });
    }
}

/// 提供延时功能
pub struct Delay;

impl Delay {
    /// 阻塞当前任务指定的时间（Duration 或 Ticks）
    ///
    /// 等待登记随调用作用域自动释放。
    pub fn delay(timeout: impl Into<Ticks>) -> Result<()> {
        let timeout = timeout.into().0;
        if timeout == 0 {
            trigger_schedule();
            return Ok(());
        }
        let deadline = Deadline::after(timeout);
        let mut task = Scheduler::get_current_task();
        let mut alarm = None;
        loop {
            let elapsed = critical_section::with(|_| -> Result<bool> {
                if deadline.elapsed() {
                    return Ok(true);
                }
                if alarm.is_none() {
                    alarm = Some(Timer::register_task_timeout(deadline, task, Event::Delay)?);
                }
                task.block(Event::Delay);
                Ok(false)
            })?;
            if elapsed {
                return Ok(());
            }
            trigger_schedule();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::kernel::time::systick::Systick;
    use crate::sync::event::Event;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn deadlines_cross_the_32_bit_boundary_and_restart_from_now() {
        use core::task::{Context, Waker};
        use core::{future::Future, pin::Pin};
        kernel_init();
        Systick::add_current_time(u32::MAX as u64 - 2);
        let mut timer = Timer::after(Ticks(5)).unwrap();
        let mut sleep = crate::runtime::sleep(Ticks(4));
        let mut cx = Context::from_waker(Waker::noop());
        timer.restart().unwrap();
        assert!(Pin::new(&mut sleep).poll(&mut cx).is_pending());
        Systick::add_current_time(3);
        assert_eq!(Systick::now(), 1u64 << 32);
        assert!(!timer.is_elapsed());
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(1)));
        timer.restart().unwrap();
        Systick::add_current_time(1);
        Timer::timer_check_and_send_event();
        assert!(Pin::new(&mut sleep).poll(&mut cx).is_ready());
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(4)));
        timer.cancel();
        assert_eq!(Timer::get_next_timeout(), None);
        let before = Systick::now();
        crate::kernel::power::enter_idle(Some(1000));
        assert_eq!(Systick::now(), before);
    }

    #[test]
    #[serial]
    fn task_timeout_only_wakes_its_event_and_survives_slot_reuse() {
        kernel_init();
        let mut task = Task::new("timed waiter", || {}).unwrap();
        let event = Event::Signal(7);
        let other = Event::Signal(9);
        task.block(other);
        let expired = Timer::register_task_timeout(Deadline::after(1), task, event).unwrap();
        Systick::add_current_time(1);
        Timer::timer_check_and_send_event();
        assert_eq!(task.get_state(), TaskState::Blocked(other));
        let next = Timer::register_task_timeout(Deadline::after(2), task, other).unwrap();
        drop(expired);
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(2)));
        Systick::add_current_time(2);
        Timer::timer_check_and_send_event();
        assert_eq!(task.get_state(), TaskState::Ready);
        drop(next);
        let cancelled = Timer::register_task_timeout(Deadline::after(2), task, other).unwrap();
        drop(cancelled);
        assert_eq!(Timer::get_next_timeout(), None);
    }

    #[test]
    #[serial]
    fn test_timer_for_each() {
        kernel_init();
        let first = Timer::after(Ticks(1000)).unwrap();
        let second = Timer::after(Ticks(2000)).unwrap();
        let snapshot = Timer::snapshots();
        first.cancel();
        Systick::add_current_time(1000);
        let remaining: Vec<_> = snapshot.map(|timer| timer.remaining).collect();
        assert_eq!(remaining, [Ticks(1000), Ticks(2000)]);
        assert!(!second.is_elapsed());
    }

    #[test]
    #[serial]
    fn test_timer_slots_full() {
        kernel_init();
        // 创建最大数量的定时器
        let mut timers = Vec::new();
        for i in 0..MAX_TIMERS {
            let timer = Timer::after(Ticks(1000 * i));
            assert!(timer.is_ok(), "Timer {} should be created successfully", i);
            timers.push(timer.unwrap());
        }

        // 再创建一个应该失败
        let result = Timer::after(Ticks(1000));
        assert_eq!(result.err(), Some(RtosError::TimerSlotsFull));
    }

    #[test]
    #[serial]
    fn test_timer_start_stop() {
        kernel_init();
        let timer = Timer::after(Ticks(1000)).unwrap();
        assert!(!timer.is_elapsed());
        timer.cancel();
        assert_eq!(Timer::snapshots().count(), 0);
    }

    #[test]
    #[serial]
    fn test_timer_timeout() {
        kernel_init();
        let mut timer = Timer::after(Ticks(500)).unwrap();
        timer.restart().unwrap();

        assert_eq!(timer.is_elapsed(), false);

        Systick::add_current_time(500);
        assert_eq!(timer.is_elapsed(), true);
    }

    #[test]
    #[serial]
    fn test_timer_deadline_ordering() {
        kernel_init();

        let _last = Timer::after(Ticks(300)).unwrap();
        let _other = Timer::after(Ticks(100)).unwrap();
        let _middle = Timer::after(Ticks(200)).unwrap();

        // 期望下一个超时时间是 100
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(100)));

        Systick::add_current_time(100);
        Timer::timer_check_and_send_event(); // 处理 t2 超时

        // 期望下一个超时时间是 100 (t3 剩余)
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(100)));

        Systick::add_current_time(100);
        Timer::timer_check_and_send_event(); // 处理 t3 超时

        // 期望下一个超时时间是 100 (t1 剩余)
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(100)));
    }

    #[test]
    #[serial]
    fn test_stopped_timer_does_not_set_next_timeout() {
        kernel_init();

        let t1 = Timer::after(Ticks(100)).unwrap();
        let _other = Timer::after(Ticks(200)).unwrap();

        assert_eq!(Timer::get_next_timeout(), Some(Ticks(100)));

        // 停止最早到期的定时器
        t1.cancel();

        // 下一个应该是 t2 (200)
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(200)));
    }

    #[test]
    #[serial]
    fn test_timer_event_wakeup() {
        kernel_init();

        let mut task = Task::new("timer_waiter", || {}).unwrap();
        let mut timer = Timer::after(Ticks(100)).unwrap();
        timer.restart().unwrap();

        Scheduler::start();

        let current_id = Scheduler::get_current_task().get_taskid();
        if task.get_taskid() != current_id {
            task.block(Event::Timer(timer.generation));
            Systick::add_current_time(100);
            Timer::timer_check_and_send_event();
            assert_eq!(task.get_state(), TaskState::Ready);
        }
    }

    #[test]
    #[serial]
    fn deleted_timer_cannot_delete_or_fire_reused_slot() {
        kernel_init();
        let old = Timer::after(Ticks(1)).unwrap();
        let stale_event = Event::Timer(old.generation);
        let slot = old.slot;
        let mut task = Task::new("waiter", || {}).unwrap();
        task.block(stale_event);
        old.cancel();
        assert_eq!(task.get_state(), TaskState::Ready);
        let new = Timer::after(Ticks(10)).unwrap();
        assert_eq!(slot, new.slot);
        task.block(stale_event);
        Systick::add_current_time(1);
        Timer::timer_check_and_send_event();
        assert!(!new.is_elapsed());
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(9)));
        Systick::add_current_time(9);
        Timer::timer_check_and_send_event();
        assert_eq!(task.get_state(), TaskState::Blocked(stale_event));
    }

    #[test]
    #[serial]
    fn timer_iteration_does_not_destroy_owned_timers() {
        kernel_init();
        let mut timer = Timer::after(Ticks(100)).unwrap();
        timer.restart().unwrap();
        assert_eq!(Timer::snapshots().count(), 1);
        assert!(!timer.is_elapsed());
        assert_eq!(Timer::get_next_timeout(), Some(Ticks(100)));
    }
}
