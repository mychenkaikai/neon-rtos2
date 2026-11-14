use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::timer::Timer;
use crate::kernel::time::systick::Systick;
use super::{clear_software_interrupt, elapsed_timer_ticks};

#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(mcause: usize, mepc: usize, sp: usize) -> usize {
    let is_interrupt = (mcause as isize) < 0;
    let code = mcause & !(1 << 31);

    if is_interrupt {
        let mut interrupted = Scheduler::get_current_task();
        interrupted.check_frame(sp);
        interrupted.set_stack_top(sp);

        match code {
            // Machine Software Interrupt
            3 => {
                clear_software_interrupt();
                Scheduler::select_after_save();
            }
            // Machine Timer Interrupt
            7 => {
                Systick::add_current_time(elapsed_timer_ticks());
                Timer::timer_check_and_send_event();
                Scheduler::select_after_save();
            }
            _ => {}
        }
        
        let current_task = Scheduler::get_current_task();
        let new_sp = current_task.get_stack_top();
        
        current_task.check_frame(new_sp);
        return new_sp;
    } else {
        // 异常处理
        panic!("Exception: mcause={}, mepc={:x}, sp={:x}", code, mepc, sp);
    }
}
