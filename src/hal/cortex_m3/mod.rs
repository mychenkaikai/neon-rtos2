pub mod mpu;

// 与 RISC-V 一样由 Rust 工具链组装，避免依赖额外的交叉 C 编译器。
core::arch::global_asm!(include_str!("asm/context.s"), options(raw));

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
static PORT_READY: AtomicBool = AtomicBool::new(false);
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
        let mut current = Scheduler::get_current_task();
        current.check_frame(psp as usize);
        current.set_stack_top(psp as usize);
    }
    
    // 执行任务切换
    Scheduler::select_after_save();
    
    let current_task = Scheduler::get_current_task();
    
    // 动态配置 MPU，保护当前任务的栈底，防止栈溢出
    mpu::MpuConfig::configure_task_guard(current_task.get_stack_bottom() as u32);
    
    // 返回下一个任务的栈指针
    let next = current_task.get_stack_top();
    current_task.check_frame(next);
    next as *mut u32
}

fn set_psp(psp: usize) {
    unsafe {
        psp::write(psp as u32);
    }
}

pub(crate) fn trigger_schedule() {
    if !PORT_READY.load(Ordering::Acquire) { return; }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    SCB::set_pendsv();
}

pub(crate) fn start_first_task() {
    // PendSV 必须低于所有普通中断，确保上下文保存和恢复不会嵌套。
    unsafe {
        cortex_m::Peripherals::steal().SCB.set_priority(
            cortex_m::peripheral::scb::SystemHandler::PendSV, 0xff,
        );
    }
    // 重置第一次切换标志
    FIRST_SWITCH.store(true, Ordering::SeqCst);
    
    // 设置 PSP 为第一个任务的栈指针
    // 注意：栈已经包含了 r4-r11 的空间，PendSV 会从这里恢复
    set_psp(Scheduler::get_current_task().get_stack_top());
    
    PORT_READY.store(true, Ordering::Release);
    
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
    fn idle_task() {
        loop {
            crate::kernel::power::enter_idle(None);
        }
    }
    let task = crate::kernel::task::Task::builder("idle").priority(crate::kernel::task::Priority::Idle).spawn(idle_task).unwrap();
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

/// An invalid clock/period pair returns the untouched SysTick peripheral.
pub struct InvalidTickConfig(pub cortex_m::peripheral::SYST);

impl core::fmt::Debug for InvalidTickConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SysTick period outside the supported reload range")
    }
}

/// Consume the SysTick peripheral for kernel timing. The core clock frequency
/// and representable period are validated before changing any register.
pub fn configure_systick(mut syst: cortex_m::peripheral::SYST, clock_hz: u32, period: core::time::Duration)
    -> Result<(), InvalidTickConfig>
{
    let Ok((cycles, actual_period)) = super::tick_reload(clock_hz, period, 1 << 24) else {
        return Err(InvalidTickConfig(syst));
    };
    super::with_interrupts_masked(|_| {
        syst.disable_counter();
        syst.set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
        syst.set_reload(cycles - 1);
        syst.clear_current();
        crate::kernel::time::systick::Systick::set_tick_period(actual_period);
        syst.enable_interrupt();
        syst.enable_counter();
    });
    Ok(())
}
