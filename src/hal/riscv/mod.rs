//! # RISC-V 架构支持
//!
//! 提供 RISC-V 架构的硬件抽象层实现。
//!
//! ## 支持的 RISC-V 变体
//!
//! - RV32I: 32位基础整数指令集
//! - RV32IM: 带乘法扩展
//! - RV32IMC: 带压缩指令扩展
//!
//! ## 特性
//!
//! - 上下文切换
//! - 中断处理
//! - 定时器支持（CLINT）
//! - 机器模式（M-mode）运行
//!
//! ## 寄存器约定
//!
//! RISC-V 调用约定（RV32I）：
//!
//! | 寄存器 | ABI 名称 | 说明 | 保存者 |
//! |--------|----------|------|--------|
//! | x0 | zero | 硬连线零 | - |
//! | x1 | ra | 返回地址 | 调用者 |
//! | x2 | sp | 栈指针 | 被调用者 |
//! | x3 | gp | 全局指针 | - |
//! | x4 | tp | 线程指针 | - |
//! | x5-x7 | t0-t2 | 临时寄存器 | 调用者 |
//! | x8 | s0/fp | 保存寄存器/帧指针 | 被调用者 |
//! | x9 | s1 | 保存寄存器 | 被调用者 |
//! | x10-x11 | a0-a1 | 参数/返回值 | 调用者 |
//! | x12-x17 | a2-a7 | 参数 | 调用者 |
//! | x18-x27 | s2-s11 | 保存寄存器 | 被调用者 |
//! | x28-x31 | t3-t6 | 临时寄存器 | 调用者 |
//!
//! ## 栈帧布局
//!
//! ```text
//! 高地址
//! ┌──────────────┐
//! │     mepc     │  +124  (返回地址)
//! ├──────────────┤
//! │   mstatus    │  +120  (机器状态)
//! ├──────────────┤
//! │     x31      │  +116  (t6)
//! │     ...      │
//! │     x1       │  +4    (ra)
//! ├──────────────┤
//! │   (保留)     │  +0
//! └──────────────┘
//! 低地址 (SP)
//! ```

use crate::kernel::scheduler::Scheduler;
mod trap;
use core::arch::asm;
use core::arch::global_asm;

// 引入汇编上下文切换代码
global_asm!(include_str!("asm/context.s"));

// ============================================================================
// 常量定义
// ============================================================================

/// 保存寄存器和 CSR 的栈帧，含 16 字节 ABI 对齐填充。
pub const CONTEXT_FRAME_SIZE: usize = 144; // 16-byte aligned at Rust call boundaries

/// CLINT 基地址（平台相关，这里使用 QEMU virt 平台的地址）
pub const CLINT_BASE: usize = 0x0200_0000;

/// CLINT mtime 寄存器偏移
pub const CLINT_MTIME: usize = 0xBFF8;

/// CLINT mtimecmp 寄存器偏移
pub const CLINT_MTIMECMP: usize = 0x4000;

/// 机器模式软件中断挂起寄存器偏移
pub const CLINT_MSIP: usize = 0x0000;

// ============================================================================
// CSR 寄存器操作
// ============================================================================

/// 读取 mstatus 寄存器
#[inline]
pub fn read_mstatus() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, mstatus", out(reg) value);
    }
    value
}



/// 读取 mepc 寄存器
#[inline]
pub fn read_mepc() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, mepc", out(reg) value);
    }
    value
}



/// 读取 mcause 寄存器
#[inline]
pub fn read_mcause() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, mcause", out(reg) value);
    }
    value
}

/// 读取 mie 寄存器（机器模式中断使能）
#[inline]
pub fn read_mie() -> usize {
    let value: usize;
    unsafe {
        asm!("csrr {}, mie", out(reg) value);
    }
    value
}



/// 设置 mie 寄存器的位
#[inline]
fn set_mie(bits: usize) {
    unsafe {
        asm!("csrs mie, {}", in(reg) bits);
    }
}



// ============================================================================
// 中断控制
// ============================================================================

/// MIE 位在 mstatus 中的位置
pub const MSTATUS_MIE: usize = 1 << 3;

/// MPIE 位在 mstatus 中的位置
pub const MSTATUS_MPIE: usize = 1 << 7;

/// 机器模式软件中断使能���
pub const MIE_MSIE: usize = 1 << 3;

/// 机器模式定时器中断使能位
pub const MIE_MTIE: usize = 1 << 7;

/// 机器模式外部中断使能位
pub const MIE_MEIE: usize = 1 << 11;

/// 全局中断使能
#[inline]
fn enable_interrupts() {
    unsafe {
        asm!("csrs mstatus, {}", in(reg) MSTATUS_MIE);
    }
}

/// 全局中断禁用
#[inline]
fn disable_interrupts() {
    unsafe {
        asm!("csrc mstatus, {}", in(reg) MSTATUS_MIE);
    }
}


/// 恢复中断状态
#[inline]
fn restore_interrupts(prev: usize) {
    if prev & MSTATUS_MIE != 0 {
        enable_interrupts();
    }
}

/// 等待中断（WFI 指令）
#[inline]
pub fn wait_for_interrupt() {
    unsafe {
        asm!("wfi");
    }
}

// ============================================================================
// 定时器操作（CLINT）
// ============================================================================

/// 读取 mtime 计数器
#[inline]
pub fn read_mtime() -> u64 {
    read_clint64(CLINT_MTIME)
}

fn read_clint64(offset: usize) -> u64 {
    let low = (CLINT_BASE + offset) as *const u32;
    unsafe {
        loop {
            let high = low.add(1).read_volatile();
            let value = low.read_volatile();
            if high == low.add(1).read_volatile() {
                return (u64::from(high) << 32) | u64::from(value);
            }
        }
    }
}

/// RV32 updates the two halves without temporarily setting an early compare.
fn write_mtimecmp(value: u64) {
    let low = (CLINT_BASE + CLINT_MTIMECMP) as *mut u32;
    unsafe {
        low.write_volatile(u32::MAX);
        low.add(1).write_volatile((value >> 32) as u32);
        low.write_volatile(value as u32);
    }
}

/// 触发软件中断
#[inline]
fn trigger_software_interrupt() {
    let ptr = (CLINT_BASE + CLINT_MSIP) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, 1) }
}

/// 清除软件中断
#[inline]
fn clear_software_interrupt() {
    let ptr = (CLINT_BASE + CLINT_MSIP) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, 0) }
}

// ============================================================================
// HAL Trait 实现
// ============================================================================

/// 初始化任务栈
///
/// 设置初始栈帧，使任务可以通过上下文切换启动。
///
/// # 参数
///
/// - `stack_top`: 栈顶指针（会被修改为新的栈顶）
/// - `entry`: 任务入口函数
/// - `arg`: 传递给任务的参数
///
/// # 栈帧布局
///
/// 初始化后的栈帧包含：
/// - 所有通用寄存器（x1-x31）初始化为 0
/// - mepc 设置为任务入口地址
/// - mstatus 设置为启用中断
/// - a0 (x10) 设置为任务参数
pub(crate) fn init_task_stack(stack_top: &mut usize, entry: fn(usize), arg: usize) {
    // 栈向下生长，预留上下文帧空间
    let sp = (*stack_top - CONTEXT_FRAME_SIZE) & !0xf; // RISC-V ABI stack alignment
    
    let frame = sp as *mut usize;
    
    // 栈帧布局 (与 context.s ���持一致):
    //   偏移 0:   保留
    //   偏移 4:   x1 (ra)
    //   偏移 8:   x3 (gp)
    //   ...
    //   偏移 36:  x10 (a0) - 参数
    //   ...
    //   偏移 120: x31 (t6)
    //   偏移 124: mepc - 任务入口
    //   偏移 128: mstatus
    //   偏移 132: 保留/对齐
    
    unsafe {
        // 清零寄存器和 ABI 对齐填充
        for i in 0..CONTEXT_FRAME_SIZE / 4 {
            *frame.add(i) = 0;
        }
        
        // 设置返回地址 (ra/x1) - 偏移 4，即 frame[1]
        *frame.add(1) = crate::utils::task_exit_error as *const () as usize;
        // All tasks share the hart's global pointer and TLS configuration.
        let (gp, tp): (usize, usize);
        asm!("mv {0}, gp", "mv {1}, tp", out(reg) gp, out(reg) tp);
        *frame.add(2) = gp;
        *frame.add(3) = tp;
        
        // 设置参数 (a0/x10) - 偏移 36，即 frame[9]
        // 注意：x10 是第 10 个寄存器，但 x0 不保存，x2(sp) 不保存
        // 实际布局：frame[1]=x1, frame[2]=x3, ..., frame[9]=x10
        *frame.add(9) = arg;
        
        // 设置 mepc（任务入口）- 偏移 124，即 frame[31]
        *frame.add(31) = entry as usize;
        
        // 设置 mstatus（启用中断：MPIE=1, MPP=11 机器模式）- 偏移 128，即 frame[32]
        *frame.add(32) = MSTATUS_MPIE | (3 << 11); // MPP = Machine mode
    }
    
    *stack_top = sp;
}

/// 触发调度（通过软件中断）
///
/// 触发一个软件中断来执行上下文切换
pub(crate) fn trigger_schedule() {
    if PORT_READY.load(core::sync::atomic::Ordering::Acquire) { trigger_software_interrupt(); }
}

/// 启动第一个任务
///
/// 从调度器获取第一个任务并开始执行
///
/// # Safety
///
/// 此函数不会返回，它会直接跳转到第一个任务
pub(crate) fn start_first_task() {
    disable_interrupts();
    // 配置 mtvec，将异常和中断入口指向 trap_entry
    unsafe {
        unsafe extern "C" {
            fn trap_entry();
        }
        // Direct mode
        asm!("csrw mtvec, {}", in(reg) trap_entry as usize);
    }

    // 获取第一个任务的栈指针
    let first_task = Scheduler::get_current_task();
    let stack_ptr = first_task.get_stack_top();
    
    first_task.check_frame(stack_ptr);
    // mret enables MIE only after SP and the complete first frame are restored.
    clear_software_interrupt();
    arm_tick();
    set_mie(MIE_MTIE | MIE_MSIE);
    PORT_READY.store(true, core::sync::atomic::Ordering::Release);
    unsafe extern "C" { fn start_first_task_asm(sp: usize) -> !; }
    unsafe { start_first_task_asm(stack_ptr) }
}

/// 初始化空闲任务
///
/// 创建一个低优先级的空闲任务，在没有其他任务运行时执行
pub(crate) fn init_idle_task() {
    // 空闲任务在 RISC-V 上使用 WFI 指令等待中断
    fn idle_task() {
        loop {
            crate::kernel::power::enter_idle(None);
        }
    }
    let _task = crate::kernel::task::Task::builder("idle").priority(crate::kernel::task::Priority::Idle).spawn(idle_task).unwrap();
}

/// 初始化系统定时器
///
/// Configure the kernel tick on QEMU virt (10 MHz CLINT timebase).
pub fn configure_systick(period: core::time::Duration) -> crate::error::Result<()> {
    let (cycles, actual_period) = super::tick_reload(10_000_000, period, u32::MAX)?;
    super::with_interrupts_masked(|_| {
        crate::kernel::time::systick::Systick::set_tick_period(actual_period);
        TICK_PERIOD.store(cycles, core::sync::atomic::Ordering::Relaxed);
        if PORT_READY.load(core::sync::atomic::Ordering::Acquire) { arm_tick(); }
    });
    Ok(())
}

// The virt machine exposes a 10 MHz timebase. One kernel tick defaults to 1 ms.
static PORT_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static TICK_PERIOD: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(10_000);

fn arm_tick() {
    let period = TICK_PERIOD.load(core::sync::atomic::Ordering::Relaxed);
    write_mtimecmp(read_mtime().wrapping_add(u64::from(period)));
}

// Count missed periods against the previous compare. Re-arming from "now"
// would permanently lose time whenever interrupts are masked for a long period.
pub(super) fn elapsed_timer_ticks() -> u64 {
    let due = read_clint64(CLINT_MTIMECMP);
    let now = read_mtime();
    if now < due { return 0; }
    let period = u64::from(TICK_PERIOD.load(core::sync::atomic::Ordering::Relaxed));
    let elapsed = (now - due) / period + 1;
    write_mtimecmp(due.wrapping_add(elapsed.wrapping_mul(period)));
    elapsed
}

// ============================================================================
// 上下文切换（汇编实现）
// ============================================================================

// 上下文切换的汇编代码在 asm/context.s 中实现

// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_frame_size() {
        // 34 个 32 位寄存器 = 136 字节
        assert_eq!(CONTEXT_FRAME_SIZE % 16, 0);
    }

    #[test]
    fn test_mstatus_bits() {
        assert_eq!(MSTATUS_MIE, 0x08);
        assert_eq!(MSTATUS_MPIE, 0x80);
    }

    #[test]
    fn test_mie_bits() {
        assert_eq!(MIE_MSIE, 0x08);
        assert_eq!(MIE_MTIE, 0x80);
        assert_eq!(MIE_MEIE, 0x800);
    }

    #[test]
    fn test_stack_alignment() {
        let mut stack_top: usize = 0x2000_1000;
        let original = stack_top;
        
        // 模拟栈初始化（不实际调用，因为需要真实硬件）
        let aligned = (original - CONTEXT_FRAME_SIZE) & !0xf;
        
        // 检查对齐
        assert_eq!(aligned & 0xf, 0);
    }
}

// ============================================================================
// Critical Section 实现
// ============================================================================

use critical_section::RawRestoreState;

struct RiscvCriticalSectionImpl;
critical_section::set_impl!(RiscvCriticalSectionImpl);

unsafe impl critical_section::Impl for RiscvCriticalSectionImpl {
    unsafe fn acquire() -> RawRestoreState {
        let previous: usize;
        // Atomically snapshot MIE and mask interrupts.
        unsafe { asm!("csrrci {}, mstatus, 8", out(reg) previous) };
        previous as RawRestoreState
    }

    unsafe fn release(prev: RawRestoreState) {
        restore_interrupts(prev as usize);
    }
}

