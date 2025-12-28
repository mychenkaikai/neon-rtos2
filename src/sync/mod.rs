//! # 同步原语模块
//!
//! 提供任务间同步的各种原语，包括传统版本和支持闭包传递的 V2 版本。
//!
//! ## V2 版本特性
//!
//! V2 版本的同步原语基于 `Arc` 实现，具有以下优势：
//! - 无需全局变量，可以在局部创建
//! - 支持通过闭包捕获传递给任务
//! - API 设计与 `std` 风格一致
//! - 同时支持同步和异步等待
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::{SignalV2, MutexV2, SemaphoreV2, CondVarV2};
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     // 创建同步原语
//!     let signal = SignalV2::new();
//!     let mutex = MutexV2::new(0);
//!     let sem = SemaphoreV2::new(3);
//!     let condvar = CondVarV2::new();
//!
//!     // 克隆后传递给任务
//!     let signal_clone = signal.clone();
//!     Task::builder("task1")
//!         .spawn(move |_| {
//!             signal_clone.wait().unwrap();
//!         });
//! }
//! ```

// 传统版本模块
pub mod mutex;
pub mod signal;
pub mod event;
pub mod guard;

// V2 版本模块 - 支持闭包传递
pub mod signal_v2;
pub mod mutex_v2;
pub mod semaphore_v2;
pub mod condvar_v2;


// ============================================================================
// 传统版本导出
// ============================================================================

pub use mutex::Mutex;
pub use guard::MutexGuard;
pub use signal::Signal;
pub use event::Event;

// ============================================================================
// V2 版本导出 - 推荐使用
// ============================================================================

// Signal V2
pub use signal_v2::{
    SignalV2, 
    SignalSender, 
    SignalReceiver, 
    OwnedSignal,
    SignalFutureV2,
    signal_pair,
};

// Mutex V2
pub use mutex_v2::{
    MutexV2,
    MutexGuardV2,
    OwnedMutexGuard,
    MappedMutexGuard,
    MutexLockFuture,
    RwLockV2,
    RwLockReadGuard,
    RwLockWriteGuard,
};

// Semaphore V2
pub use semaphore_v2::{
    SemaphoreV2,
    SemaphorePermit,
    OwnedSemaphorePermit,
    SemaphoreAcquireFuture,
};

// CondVar V2
pub use condvar_v2::{
    CondVarV2,
    CondVarFuture,
};

// ============================================================================
// 便捷函数导出
// ============================================================================

/// 创建信号量配对
pub use signal_v2::signal_pair as new_signal_pair;

/// 创建互斥锁和条件变量配对
pub use condvar_v2::mutex_condvar_pair;

/// 创建二值信号量
pub use semaphore_v2::binary_semaphore;

// ============================================================================
// Prelude - 常用类型的便捷导入
// ============================================================================

/// 同步原语的 prelude 模块
///
/// 使用 `use neon_rtos2::sync::prelude::*;` 导入所有常用类型
pub mod prelude {
    // V2 版本（推荐）
    pub use super::SignalV2;
    pub use super::MutexV2;
    pub use super::MutexGuardV2;
    pub use super::SemaphoreV2;
    pub use super::CondVarV2;
    pub use super::RwLockV2;
    
    // 便捷函数
    pub use super::signal_pair;
    pub use super::mutex_condvar_pair;
    pub use super::binary_semaphore;
    
    // 传统版本（向后兼容）
    pub use super::Mutex;
    pub use super::Signal;
    pub use super::Event;
}
