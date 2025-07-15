use core::panic::PanicInfo;
use crate::task::Task;
use crate::signal::Signal;
use crate::schedule::Scheduler;
use crate::timer::Timer;
use crate::systick::Systick;
use crate::mutex::Mutex;

pub fn kernel_init() {
    Task::init();
    Scheduler::init();
    Timer::init();
    Systick::init();
    Mutex::init();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

pub(crate) fn task_exit_error() -> ! {
    loop {}
}