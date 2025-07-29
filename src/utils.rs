use core::panic::PanicInfo;
use crate::task::Task;
use crate::signal::Signal;
use crate::schedule::Scheduler;
use crate::timer::Timer;
use crate::systick::Systick;
use crate::mutex::Mutex;

pub fn kernel_init() {
    #[cfg(not(test))]
    crate::allocator::init_heap();
    Task::init();
    Scheduler::init();
    Timer::init();
    Systick::init();
    Mutex::init();
}


pub(crate) fn task_exit_error() -> ! {
    loop {}
}