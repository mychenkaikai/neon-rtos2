use crate::schedule::Scheduler;
use crate::utils::task_exit_error;
use core::mem::size_of;
use cortex_m::peripheral::SCB;
use cortex_m::register::psp;
use cortex_m_rt::exception;
use cortex_m_rt::ExceptionFrame;
use crate::systick::Systick;

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
    Scheduler::schedule();
    Scheduler::get_current_task().get_stack_top() as *mut u32
}

fn set_psp(psp: usize) {
    unsafe {
        psp::write(psp as u32);
    }
}

pub(crate) fn start_first_task() {
    set_psp(Scheduler::get_current_task().get_stack_top() + 8 * size_of::<usize>());
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    SCB::set_pendsv();
}



#[exception]
unsafe fn SysTick() {
    Systick::systick_inc();
    SCB::set_pendsv();
}

#[exception]
unsafe fn HardFault(_ef: &ExceptionFrame) -> ! {
    loop {}
}
#[exception]
unsafe fn DefaultHandler(_val: i16) -> ! {
    loop {}
}