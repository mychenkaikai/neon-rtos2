use core::{cell::Cell, time::Duration};
use critical_section::Mutex;

// Both supported CPUs are 32-bit; interrupts must never observe a torn u64.
static CURRENT_TIME: Mutex<Cell<u64>> = Mutex::new(Cell::new(0));

static TICK_PERIOD: Mutex<Cell<Duration>> = Mutex::new(Cell::new(Duration::from_millis(1)));

/// A number of scheduler ticks. Duration inputs round up to the next tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Ticks(pub usize);

impl From<Duration> for Ticks {
    fn from(duration: Duration) -> Self {
        let period = Systick::tick_period().as_nanos();
        Self(usize::try_from(duration.as_nanos().div_ceil(period)).unwrap_or(usize::MAX))
    }
}

pub struct Systick;

impl Systick {
    pub(crate) fn init() {
        critical_section::with(|cs| {
            CURRENT_TIME.borrow(cs).set(0);
            TICK_PERIOD.borrow(cs).set(Duration::from_millis(1));
        });
    }

    /// Set the period used for new Duration conversions when configuring the
    /// board timer. Existing deadlines retain their tick counts. Default: 1 ms.
    pub(crate) fn set_tick_period(period: Duration) {
        assert!(!period.is_zero());
        critical_section::with(|cs| TICK_PERIOD.borrow(cs).set(period));
    }

    pub fn tick_period() -> Duration {
        critical_section::with(|cs| TICK_PERIOD.borrow(cs).get())
    }

    pub(crate) fn systick_inc() {
        Self::add_current_time(1);
    }

    /// Monotonic ticks since kernel initialization.
    pub fn now() -> u64 {
        critical_section::with(|cs| CURRENT_TIME.borrow(cs).get())
    }

    pub(crate) fn add_current_time(ticks: u64) {
        critical_section::with(|cs| {
            let clock = CURRENT_TIME.borrow(cs);
            clock.set(clock.get().saturating_add(ticks));
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Deadline(u64);

impl Deadline {
    pub fn after(ticks: usize) -> Self {
        Self(Systick::now().saturating_add(ticks as u64))
    }

    pub fn elapsed_at(self, now: u64) -> bool {
        self.0 <= now
    }
    pub fn elapsed(self) -> bool {
        self.elapsed_at(Systick::now())
    }

    pub fn remaining_at(self, now: u64) -> usize {
        usize::try_from(self.0.saturating_sub(now)).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;

    #[test]
    #[serial_test::serial]
    fn durations_round_up_at_the_board_tick_period() {
        kernel_init();
        Systick::set_tick_period(Duration::from_millis(2));
        assert_eq!(Ticks::from(Duration::ZERO), Ticks(0));
        assert_eq!(Ticks::from(Duration::from_nanos(1)), Ticks(1));
        assert_eq!(Ticks::from(Duration::from_millis(2)), Ticks(1));
        assert_eq!(Ticks::from(Duration::from_micros(2001)), Ticks(2));
        assert_eq!(Ticks::from(Duration::MAX), Ticks(usize::MAX));
    }
}
