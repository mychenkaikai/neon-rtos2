use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::timer::Timer;
use crate::kernel::time::systick::Systick;
use super::{clear_software_interrupt, systick_handler};
use super::pmp::PmpConfig;

use core::sync::atomic::{AtomicBool, Ordering};

static FIRST_SWITCH: AtomicBool = AtomicBool::new(true);

#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(mcause: usize, mepc: usize, sp: usize) -> usize {
    let is_interrupt = (mcause as isize) < 0;
    let code = mcause & !(1 << 31);

    if is_interrupt {
        // 保存当前栈指针到任务控制块（除了第一次进入时）
        if !FIRST_SWITCH.swap(false, Ordering::SeqCst) {
            Scheduler::get_current_task().set_stack_top(sp);
        }

        match code {
            // Machine Software Interrupt
            3 => {
                clear_software_interrupt();
                Scheduler::task_switch();
            }
            // Machine Timer Interrupt
            7 => {
                // 默认 10ms (假设 10Mhz clock -> 100_000 ticks)
                systick_handler(100_000);
                Systick::systick_inc();
                Timer::timer_check_and_send_event();
                Scheduler::task_switch();
            }
            _ => {}
        }
        
        let current_task = Scheduler::get_current_task();
        let new_sp = current_task.get_stack_top();
        
        // 第四个架构问题：完善动态 MPU/PMP 隔离
        // 在上下文切换时动态配置 PMP，保护当前任务的栈底防止溢出
        PmpConfig::configure_task_guard(current_task.get_stack_bottom());
        
        return new_sp;
    } else {
        // 异常处理
        panic!("Exception: mcause={}, mepc={:x}, sp={:x}", code, mepc, sp);
    }
}
