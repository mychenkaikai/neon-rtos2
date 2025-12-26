use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::systick::Systick;
use crate::kernel::task::Task;
use crate::kernel::time::timer::Timer;
use crate::utils::task_exit_error;
use core::mem::size_of;
use cortex_m::peripheral::SCB;
use cortex_m::register::psp;
use cortex_m_rt::ExceptionFrame;
use cortex_m_rt::exception;
use crate::{info, error, warn, debug, trace};

pub(crate) fn init_task_stack(top_of_stack: &mut usize, func: fn(usize), p_args: usize) {
    unsafe {
        *top_of_stack &= !7;
        *top_of_stack -= 1 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = 0x0100_0000;
        *top_of_stack -= 1 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = 0xffff_fffe & (func as usize);
        *top_of_stack -= 1 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = task_exit_error as usize;
        *top_of_stack -= 5 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = p_args;
        *top_of_stack -= 8 * size_of::<usize>();
    }
}

#[unsafe(no_mangle)]
fn task_switch_context(psp: *mut u32) -> *mut u32 {
    Scheduler::get_current_task().set_stack_top(psp as usize);
    Scheduler::task_switch();
    Scheduler::get_current_task().get_stack_top() as *mut u32
}

fn set_psp(psp: usize) {
    unsafe {
        psp::write(psp as u32);
    }
}

pub(crate) fn trigger_schedule() {
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    SCB::set_pendsv();
}

pub(crate) fn start_first_task() {
    set_psp(Scheduler::get_current_task().get_stack_top() + 8 * size_of::<usize>());
    systick_init();
    trigger_schedule();
}

#[exception]
unsafe fn SysTick() {
    Systick::systick_inc();
    Timer::timer_check_and_send_event();
    trigger_schedule();
}

#[exception]
unsafe fn HardFault(_ef: &ExceptionFrame) -> ! {
    loop {}
}
#[exception]
unsafe fn DefaultHandler(_val: i16) -> ! {
    loop {}
}

pub(crate) fn init_idle_task() {
    fn idle_task(_arg: usize) {
        loop {
            cortex_m::asm::wfi();
        }
    }
    let task = Task::new("idle", idle_task).unwrap();
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
use critical_section::RawRestoreState;
struct CriticalSection;
critical_section::set_impl!(CriticalSection);

unsafe impl critical_section::Impl for CriticalSection {
    unsafe fn acquire() -> RawRestoreState {
        cortex_m::interrupt::disable();
    }

    unsafe fn release(_: RawRestoreState) {
        unsafe {
            cortex_m::interrupt::enable();
        }
    }
}
use core::panic::PanicInfo;
use cortex_m::Peripherals;
use cortex_m::peripheral::syst::SystClkSource;

const SYST_FREQ: u32 = 1000;
const SYS_CLOCK: u32 = 12_000_000;
const SYST_RELOAD: u32 = SYS_CLOCK / SYST_FREQ;
fn systick_init() {
    let p = Peripherals::take().unwrap();
    let mut syst = p.SYST;

    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(SYST_RELOAD); 
    syst.enable_counter();
    syst.enable_interrupt();
    //info!("SysTick初始化完成");
}
