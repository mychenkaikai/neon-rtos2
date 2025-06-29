use core::panic::PanicInfo;
use crate::task::Task;
use crate::signal::Signal;
use crate::schedule::Scheduler;
use crate::timer::Timer;
use crate::systick::Systick;

pub(crate) fn kernel_init() {
    Task::init();
    Signal::init();
    Scheduler::init();
    Timer::init();
    Systick::init();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

pub(crate) fn task_exit_error() -> ! {
    loop {}
}