pub mod mpu;

use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::systick::Systick;
use crate::kernel::task::Task;
use crate::kernel::time::timer::Timer;
use crate::utils::task_exit_error;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, Ordering};
use cortex_m::peripheral::SCB;
use cortex_m::register::psp;
use cortex_m_rt::ExceptionFrame;
use cortex_m_rt::exception;
use crate::{info, error, warn, debug, trace};

/// 标记是否是第一次任务切换
/// 第一次切换时不需要保存当前上下文，因为没有"当前任务"
static FIRST_SWITCH: AtomicBool = AtomicBool::new(true);

/// 初始化任务栈
/// 
/// Cortex-M 异常栈帧布局（从高地址到低地址）：
/// - xPSR (程序状态寄存器)
/// - PC (返回地址/任务入口)
/// - LR (链接寄存器)
/// - R12
/// - R3
/// - R2
/// - R1
/// - R0 (参数)
/// - R11 (手动保存)
/// - R10
/// - R9
/// - R8
/// - R7
/// - R6
/// - R5
/// - R4
pub(crate) fn init_task_stack(top_of_stack: &mut usize, func: fn(usize), p_args: usize) {
    unsafe {
        // 8 字节对齐
        *top_of_stack &= !7;
        
        // 硬件自动保存的寄存器（异常返回时自动恢复）
        *top_of_stack -= size_of::<usize>();
        *(*top_of_stack as *mut usize) = 0x0100_0000;  // xPSR: Thumb 模式
        
        *top_of_stack -= size_of::<usize>();
        *(*top_of_stack as *mut usize) = 0xffff_fffe & (func as usize);  // PC: 任务入口
        
        *top_of_stack -= size_of::<usize>();
        *(*top_of_stack as *mut usize) = task_exit_error as usize;  // LR: 任务退出处理
        
        *top_of_stack -= size_of::<usize>();  // R12
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R3
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R2
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R1
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R0: 参数
        *(*top_of_stack as *mut usize) = p_args;
        
        // 手动保存的寄存器（PendSV 中保存/恢复）
        *top_of_stack -= size_of::<usize>();  // R11
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R10
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R9
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R8
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R7
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R6
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R5
        *(*top_of_stack as *mut usize) = 0;
        
        *top_of_stack -= size_of::<usize>();  // R4
        *(*top_of_stack as *mut usize) = 0;
    }
}

/// 任务上下文切换
/// 
/// 由 PendSV 中断处理程序调用
/// 
/// # 参数
/// - `psp`: 当前任务的栈指针（已保存 r4-r11 后的位置）
/// 
/// # 返回
/// - 下一个任务的栈指针
#[unsafe(no_mangle)]
fn task_switch_context(psp: *mut u32) -> *mut u32 {
    // 第一次切换时不保存当前上下文
    if !FIRST_SWITCH.swap(false, Ordering::SeqCst) {
        // 保存当前任务的栈指针
        Scheduler::get_current_task().set_stack_top(psp as usize);
    }
    
    // 执行任务切换
    Scheduler::task_switch();
    
    let current_task = Scheduler::get_current_task();
    
    // 动态配置 MPU，保护当前任务的栈底，防止栈溢出
    mpu::MpuConfig::configure_task_guard(current_task.get_stack_bottom() as u32);
    
    // 返回下一个任务的栈指针
    current_task.get_stack_top() as *mut u32
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
    // 重置第一次切换标志
    FIRST_SWITCH.store(true, Ordering::SeqCst);
    
    // 设置 PSP 为第一个任务的栈指针
    // 注意：栈已经包含了 r4-r11 的空间，PendSV 会从这里恢复
    set_psp(Scheduler::get_current_task().get_stack_top());
    
    systick_init();
    
    // 触发 PendSV 开始第一个任务
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
            // 获取距离下一个定时器超时的预计时间
            let next_timeout = crate::kernel::time::timer::Timer::get_next_timeout();
            
            // 进入空闲模式（支持 Tickless）
            crate::kernel::power::enter_idle(next_timeout);
        }
    }
    let task = crate::kernel::task::Task::new("idle", idle_task).unwrap();
}

// 注意：panic_handler 已移至用户代码或使用 default_panic_handler! 宏
// 这样用户可以自定义 panic 行为

use critical_section::RawRestoreState;

struct CriticalSection;
critical_section::set_impl!(CriticalSection);

unsafe impl critical_section::Impl for CriticalSection {
    unsafe fn acquire() -> RawRestoreState {
        // 保存当前中断状态（PRIMASK 寄存器）
        let was_active = cortex_m::register::primask::read().is_active();
        // 禁用中断
        cortex_m::interrupt::disable();
        // 返回之前的状态
        was_active
    }

    unsafe fn release(was_active: RawRestoreState) {
        // 只有当之前中断是启用的，才重新启用中断
        if was_active {
            unsafe {
                cortex_m::interrupt::enable();
            }
        }
    }
}

/// SysTick 初始化
/// 
/// 注意：此函数不再初始化 SysTick，因为用户代码已经在 main.rs 中初始化了。
/// 如果用户没有初始化 SysTick，调度器将无法正常工作。
fn systick_init() {
    // SysTick 应该由用户在 main.rs 中初始化
    // 这里不再重复初始化，避免 Peripherals::take() 返回 None
    // 
    // 用户需要在 main.rs 中添加类似以下代码：
    // ```
    // let p = Peripherals::take().unwrap();
    // let mut syst = p.SYST;
    // syst.set_clock_source(SystClkSource::Core);
    // syst.set_reload(SYS_CLOCK / 1000);  // 1ms tick
    // syst.enable_counter();
    // syst.enable_interrupt();
    // ```
}
