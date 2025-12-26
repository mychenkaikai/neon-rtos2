//! 硬件抽象层 (HAL)
//!
//! 提供与底层硬件交互的抽象接口，支持多种架构。

pub mod traits;

#[cfg(all(feature = "cortex_m3", not(test), target_arch = "arm"))]
pub mod cortex_m3;
#[cfg(any(test, not(target_arch = "arm")))]
pub mod test;

// 重新导出 traits
pub use traits::*;

// 架构特定实现导出
#[cfg(all(feature = "cortex_m3", not(test), target_arch = "arm"))]
pub(crate) use cortex_m3::{init_task_stack, start_first_task, trigger_schedule, init_idle_task};

#[cfg(any(test, not(target_arch = "arm")))]
pub(crate) use test::{init_task_stack, start_first_task, trigger_schedule, init_idle_task};  
