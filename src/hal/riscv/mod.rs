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

use crate::hal::traits::*;
use core::arch::asm;

// ============================================================================
// 常量定义
// ============================================================================

/// 上下文帧大小（32个通用寄存器 + mepc + mstatus = 34 * 4 = 136 字节）
pub const CONTEXT_FRAME_SIZE: usize = 136;

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

/// 写入 mstatus 寄存器
#[inline]
pub fn write_mstatus(value: usize) {
    unsafe {
        asm!("csrw mstatus, {}", in(reg) value);
    }
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

/// 写入 mepc 寄存器
#[inline]
pub fn write_mepc(value: usize) {
    unsafe {
        asm!("csrw mepc, {}", in(reg) value);
    }
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

/// 写入 mie 寄存器
#[inline]
pub fn write_mie(value: usize) {
    unsafe {
        asm!("csrw mie, {}", in(reg) value);
    }
}

/// 设置 mie 寄存器的位
#[inline]
pub fn set_mie(bits: usize) {
    unsafe {
        asm!("csrs mie, {}", in(reg) bits);
    }
}

/// 清除 mie 寄存器的位
#[inline]
pub fn clear_mie(bits: usize) {
    unsafe {
        asm!("csrc mie, {}", in(reg) bits);
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
pub fn enable_interrupts() {
    unsafe {
        asm!("csrs mstatus, {}", in(reg) MSTATUS_MIE);
    }
}

/// 全局中断禁用
#[inline]
pub fn disable_interrupts() {
    unsafe {
        asm!("csrc mstatus, {}", in(reg) MSTATUS_MIE);
    }
}

/// 禁用中断并返回之前的状态
#[inline]
pub fn disable_interrupts_save() -> usize {
    let prev = read_mstatus();
    disable_interrupts();
    prev
}

/// 恢复中断状态
#[inline]
pub fn restore_interrupts(prev: usize) {
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
    let ptr = (CLINT_BASE + CLINT_MTIME) as *const u64;
    unsafe { core::ptr::read_volatile(ptr) }
}

/// 设置 mtimecmp 比较值
#[inline]
pub fn write_mtimecmp(value: u64) {
    let ptr = (CLINT_BASE + CLINT_MTIMECMP) as *mut u64;
    unsafe { core::ptr::write_volatile(ptr, value) }
}

/// 触发软件中断
#[inline]
pub fn trigger_software_interrupt() {
    let ptr = (CLINT_BASE + CLINT_MSIP) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, 1) }
}

/// 清除软件中断
#[inline]
pub fn clear_software_interrupt() {
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
pub fn init_task_stack(stack_top: &mut usize, entry: fn(usize), arg: usize) {
    // 栈向下生长，预留上下文帧空间
    let sp = (*stack_top - CONTEXT_FRAME_SIZE) & !0x7; // 8字节对齐
    
    let frame = sp as *mut usize;
    
    unsafe {
        // 清零所有寄存器槽位
        for i in 0..34 {
            *frame.add(i) = 0;
        }
        
        // 设置返回地址 (ra/x1) - 任务退出时的处理
        *frame.add(1) = task_exit as usize;
        
        // 设置参数 (a0/x10)
        *frame.add(10) = arg;
        
        // 设置 mepc（任务入口）
        *frame.add(32) = entry as usize;
        
        // 设置 mstatus（启用中断：MIE=1, MPIE=1, MPP=11 机器模式）
        *frame.add(33) = MSTATUS_MPIE | (3 << 11); // MPP = Machine mode
    }
    
    *stack_top = sp;
}

/// 任务退出处理函数
///
/// 当任务函数返回时会跳转到这里
fn task_exit() {
    // 任务退出，进入空闲循环
    loop {
        wait_for_interrupt();
    }
}

/// 触发调度（通过软件中断）
///
/// 触发一个软件中断来执行上下文切换
pub fn trigger_schedule() {
    trigger_software_interrupt();
}

/// 启动第一个任务
///
/// 从调度器获取第一个任务并开始执行
///
/// # Safety
///
/// 此函数不会返回，它会直接跳转到第一个任务
pub fn start_first_task() {
    // 启用定时器中断和软件中断
    set_mie(MIE_MTIE | MIE_MSIE);
    
    // 启用全局中断
    enable_interrupts();
    
    // 触发第一次调度
    trigger_schedule();
    
    // 等待中断（不应该到达这里）
    loop {
        wait_for_interrupt();
    }
}

/// 初始化空闲任务
///
/// 创建一个低优先级的空闲任务，在没有其他任务运行时执行
pub fn init_idle_task() {
    // 空闲任务在 RISC-V 上使用 WFI 指令等待中断
    // 具体实现由调度器处理
}

/// 初始化系统定时器
///
/// 配置 CLINT 定时器产生周期性中断
///
/// # 参数
///
/// - `ticks`: 定时器周期（时钟周期数）
pub fn init_systick(ticks: u32) {
    let current = read_mtime();
    write_mtimecmp(current + ticks as u64);
    
    // 启用定时器中断
    set_mie(MIE_MTIE);
}

/// 系统定时器中断处理
///
/// 更新 mtimecmp 以产生下一次中断
///
/// # 参数
///
/// - `ticks`: 定时器周期
pub fn systick_handler(ticks: u32) {
    let current = read_mtime();
    write_mtimecmp(current + ticks as u64);
}

// ============================================================================
// 上下文切换（汇编实现）
// ============================================================================

// 上下文切换的汇编代码在 asm/context.s 中实现

// 保存当前上下文并切换到新任务
//
// # Safety
//
// 此函数直接操作栈指针，必须在正确的上下文中调用
#[cfg(target_arch = "riscv32")]
unsafe extern "C" {
    pub fn context_switch(current_sp: *mut usize, next_sp: usize);
}

/// 在非 RISC-V 平台上的占位实现
#[cfg(not(target_arch = "riscv32"))]
pub fn context_switch(_current_sp: *mut usize, _next_sp: usize) {
    // 占位实现，用于编译测试
}

// ============================================================================
// 临界区实现
// ============================================================================

/// RISC-V 临界区实现
pub struct RiscvCriticalSection {
    prev_mstatus: usize,
}

impl RiscvCriticalSection {
    /// 进入临界区
    pub fn enter() -> Self {
        let prev_mstatus = disable_interrupts_save();
        Self { prev_mstatus }
    }
}

impl Drop for RiscvCriticalSection {
    fn drop(&mut self) {
        restore_interrupts(self.prev_mstatus);
    }
}

/// 在临界区中执行闭包
pub fn critical_section<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _cs = RiscvCriticalSection::enter();
    f()
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_frame_size() {
        // 34 个 32 位寄存器 = 136 字节
        assert_eq!(CONTEXT_FRAME_SIZE, 136);
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
        let aligned = (original - CONTEXT_FRAME_SIZE) & !0x7;
        
        // 检查对齐
        assert_eq!(aligned & 0x7, 0);
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
        let prev = read_mstatus();
        disable_interrupts();
        prev as RawRestoreState
    }

    unsafe fn release(prev: RawRestoreState) {
        restore_interrupts(prev as usize);
    }
}

