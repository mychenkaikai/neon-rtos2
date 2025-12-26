use core::panic::PanicInfo;
use crate::kernel::task::Task;
use crate::sync::signal::Signal;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::timer::Timer;
use crate::kernel::time::systick::Systick;
use crate::sync::mutex::Mutex;

pub fn kernel_init() {
    crate::mem::allocator::init_heap();
    Task::init();
    Scheduler::init();
    Timer::init();
    Systick::init();
    Mutex::init();
}


pub(crate) fn task_exit_error() -> ! {
    loop {}
}