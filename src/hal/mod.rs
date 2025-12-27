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
pub mod test;

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
