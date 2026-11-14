//! # 硬件抽象层 (HAL)
//!
//! 提供与底层硬件交互的抽象接口，支持多种架构。
//!
//! ## 支持的架构
//!
//! | 架构 | Feature | 状态 |
//! |------|---------|------|
//! | Cortex-M3 | `cortex_m3` | ✅ 已实现 |
//! | RISC-V | `riscv` | ✅ 已实现 |
//! | 测试模拟 | (默认) | ✅ 已实现 |
//!
//! ## 架构选择
//!
//! 通过 Cargo feature 选择目标架构：
//!
//! ```toml
//! [dependencies]
//! neon-rtos2 = { version = "0.1", features = ["cortex_m3"] }
//! # 或
//! neon-rtos2 = { version = "0.1", features = ["riscv"] }
//! ```

pub mod traits;
pub mod dma;
pub use dma::{DmaChannel, DmaCompletion, DmaTransfer};

/// Run a bounded operation with interrupts masked, restoring the previous
/// state even for nested calls and early returns.
pub use critical_section::with as with_interrupts_masked;

#[cfg(any(test, target_arch = "arm", target_arch = "riscv32"))]
fn tick_reload(clock_hz: u32, period: core::time::Duration, maximum: u32)
    -> crate::error::Result<(u32, core::time::Duration)>
{
    use crate::error::RtosError;
    if clock_hz == 0 || period.is_zero() { return Err(RtosError::InvalidArgument); }
    let cycles = (period.as_nanos() * u128::from(clock_hz)).div_ceil(1_000_000_000);
    if cycles > u128::from(maximum) { return Err(RtosError::InvalidArgument); }
    let nanos = (cycles * 1_000_000_000).div_ceil(u128::from(clock_hz));
    Ok((cycles as u32, core::time::Duration::from_nanos(nanos as u64)))
}


// ============================================================================
// 架构模块
// ============================================================================

/// Cortex-M3 架构支持
#[cfg(all(feature = "cortex_m3", not(test), target_arch = "arm"))]
pub mod cortex_m3;

/// RISC-V 架构支持
#[cfg(all(feature = "riscv", not(test), target_arch = "riscv32"))]
pub mod riscv;

/// 测试/模拟架构
#[cfg(any(test, all(not(target_arch = "arm"), not(target_arch = "riscv32"))))]
pub(crate) mod test;

// ============================================================================
// 重新导出 traits
// ============================================================================

pub use traits::*;

// ============================================================================
// 架构特定实现导出
// ============================================================================

// Cortex-M3 实现
#[cfg(all(feature = "cortex_m3", not(test), target_arch = "arm"))]
pub(crate) use cortex_m3::{init_task_stack, start_first_task, trigger_schedule, init_idle_task};

// RISC-V 实现
#[cfg(all(feature = "riscv", not(test), target_arch = "riscv32"))]
pub(crate) use riscv::{init_task_stack, start_first_task, trigger_schedule, init_idle_task};

// 测试/模拟实现
#[cfg(any(test, all(not(target_arch = "arm"), not(target_arch = "riscv32"))))]
pub(crate) use test::{init_task_stack, start_first_task, trigger_schedule, init_idle_task};  

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[test]
    fn tick_configuration_rounds_up_and_rejects_invalid_reloads() {
        assert_eq!(tick_reload(10_000_000, Duration::from_nanos(101), 2), Ok((2, Duration::from_nanos(200))));
        for (hz, period) in [(0, Duration::from_millis(1)), (1, Duration::ZERO), (u32::MAX, Duration::MAX)] {
            assert!(tick_reload(hz, period, u32::MAX).is_err());
        }
        assert!(tick_reload(10_000_000, Duration::from_nanos(201), 2).is_err());
    }
}
