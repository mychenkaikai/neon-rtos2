//! Cancellation-safe async waiter registration shared by synchronization primitives.
use crate::compat::VecDeque;
use core::task::Waker;

pub(crate) struct AsyncWaiters {
    next_id: u64,
    queue: VecDeque<(u64, Waker)>,
}

impl AsyncWaiters {
    pub const fn new() -> Self {
        Self {
            next_id: 0,
            queue: VecDeque::new(),
        }
    }

    pub fn contains(&self, token: u64) -> bool {
        self.queue.iter().any(|(id, _)| *id == token)
    }

    pub fn has_room(&self, token: Option<u64>, limit: usize) -> bool {
        self.len() < limit || self.queue.iter().any(|(id, _)| Some(*id) == token)
    }

    // The caller clones before borrowing the state and drops the old waker afterwards.
    pub fn register(&mut self, token: &mut Option<u64>, incoming: Waker) -> Option<Waker> {
        if let Some(id) = *token {
            if let Some((_, waker)) = self.queue.iter_mut().find(|(key, _)| *key == id) {
                return Some(core::mem::replace(waker, incoming));
            }
        }
        let id = self.next_id;
        self.next_id = id.checked_add(1).expect("async waiter IDs exhausted");
        self.queue.push_back((id, incoming));
        *token = Some(id);
        None
    }

    pub fn remove(&mut self, token: Option<u64>) -> Option<Waker> {
        let index = self.queue.iter().position(|(id, _)| Some(*id) == token)?;
        self.queue.remove(index).map(|(_, waker)| waker)
    }

    pub fn pop(&mut self) -> Option<Waker> {
        self.queue.pop_front().map(|(_, waker)| waker)
    }

    pub fn drain(&mut self) -> VecDeque<(u64, Waker)> {
        core::mem::take(&mut self.queue)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

use crate::error::{Result, RtosError};
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::systick::Deadline;
use crate::kernel::time::timer::Timer;
use crate::sync::{event::Event, signal::WaiterList};
use core::cell::RefCell;
use core::task::{Context, Poll};
use critical_section::{CriticalSection, Mutex};

// Wake callbacks may run user code. Move them out of the critical section first.
#[derive(Default)]
pub(crate) struct WakeBatch(VecDeque<(u64, Waker)>);

impl WakeBatch {
    pub fn wake(self) {
        for (_, waker) in self.0 {
            waker.wake();
        }
    }
}

pub(crate) struct WaitQueue {
    tasks: Mutex<RefCell<WaiterList>>,
    futures: Mutex<RefCell<AsyncWaiters>>,
}

impl WaitQueue {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(RefCell::new(WaiterList::new())),
            futures: Mutex::new(RefCell::new(AsyncWaiters::new())),
        }
    }

    // The predicate and the transition to Blocked must be indivisible to a notifier.
    pub fn wait_until<T>(
        &self,
        event: Event,
        deadline: Option<Deadline>,
        mut attempt: impl FnMut() -> Result<T>,
        mut on_park: impl FnMut(),
    ) -> Result<T> {
        let mut task = Scheduler::get_current_task();
        let mut timer = None;
        loop {
            let result = critical_section::with(|cs| {
                self.tasks.borrow_ref_mut(cs).remove(task.0);
                match attempt() {
                    Err(RtosError::WouldBlock) => {}
                    result => return result.map(Some),
                }
                if let Some(deadline) = deadline {
                    if deadline.elapsed() {
                        return Err(RtosError::Timeout);
                    }
                    if timer.is_none() {
                        timer = Some(Timer::register_task_timeout(deadline, task, event)?);
                    }
                }
                if !self.tasks.borrow_ref_mut(cs).push(task.0) {
                    return Err(RtosError::WaiterQueueFull);
                }
                task.block(event);
                on_park();
                Ok(None)
            })?;
            if let Some(value) = result {
                return Ok(value);
            }
            crate::hal::trigger_schedule();
        }
    }

    // Several predicates can share one queue (e.g. requests for different permit counts).
    // Wake every registered waiter so a cancelled or ineligible waiter cannot consume the wake.
    pub fn take_all(&self, cs: CriticalSection<'_>, event: Event) -> WakeBatch {
        self.tasks.borrow_ref_mut(cs).wake_all(cs, event);
        WakeBatch(self.futures.borrow_ref_mut(cs).drain())
    }

    pub fn counts(&self) -> (usize, usize) {
        critical_section::with(|cs| {
            (
                self.tasks.borrow_ref(cs).len(),
                self.futures.borrow_ref(cs).len(),
            )
        })
    }

    pub fn registration(&self) -> Registration<'_> {
        Registration {
            queue: self,
            token: None,
        }
    }
}

pub(crate) struct Registration<'a> {
    queue: &'a WaitQueue,
    token: Option<u64>,
}

impl Registration<'_> {
    pub fn poll_until<T>(
        &mut self,
        cx: &Context<'_>,
        attempt: impl FnOnce() -> Result<T>,
    ) -> Poll<Result<T>> {
        let mut incoming = Some(cx.waker().clone());
        let (result, old) = critical_section::with(|cs| {
            let result = attempt();
            let mut queue = self.queue.futures.borrow_ref_mut(cs);
            match result {
                Err(RtosError::WouldBlock)
                    if queue.has_room(self.token, crate::config::MAX_ASYNC_WAITERS) =>
                {
                    (
                        Poll::Pending,
                        queue.register(&mut self.token, incoming.take().unwrap()),
                    )
                }
                Err(RtosError::WouldBlock) => (Poll::Ready(Err(RtosError::WaiterQueueFull)), None),
                ready => (Poll::Ready(ready), queue.remove(self.token.take())),
            }
        });
        drop(old);
        result
    }
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.token.take() {
            critical_section::with(|cs| self.queue.futures.borrow_ref_mut(cs).remove(Some(id)));
        }
    }
}
