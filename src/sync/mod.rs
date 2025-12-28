//! # 同步原语模块
//!
//! 提供任务间同步的各种原语，支持闭包传递，无需全局变量。
//!
//! ## 特性
//!
//! - 基于 `Arc` 实现，可以在局部创建
//! - 支持通过闭包捕获传递给任务
//! - API 设计与 `std` 风格一致
//! - 同时支持同步和异步等待
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::sync::{Signal, Mutex, Semaphore, CondVar};
//! use neon_rtos2::kernel::task::Task;
//!
//! fn main() {
//!     // 创建同步原语
//!     let signal = Signal::new();
//!     let mutex = Mutex::new(0);
//!     let sem = Semaphore::new(3);
//!     let condvar = CondVar::new();
//!
//!     // 克隆后传递给任务
//!     let signal_clone = signal.clone();
//!     Task::builder("task1")
//!         .spawn(move |_| {
//!             signal_clone.wait().unwrap();
//!         });
//! }
//! ```

// 核心模块
pub mod signal;
pub mod mutex;
pub mod semaphore;
pub mod condvar;
pub mod event;

// ============================================================================
// 主要类型导出
// ============================================================================

// Signal
pub use signal::{
    Signal,
    SignalSender,
    SignalReceiver,
    OwnedSignal,
    SignalFuture,
    signal_pair,
};

// Mutex
pub use mutex::{
    Mutex,
    MutexGuard,
    OwnedMutexGuard,
    MappedMutexGuard,
    MutexLockFuture,
    RwLock,
    RwLockReadGuard,
    RwLockWriteGuard,
};

// Semaphore
pub use semaphore::{
    Semaphore,
    SemaphorePermit,
    OwnedSemaphorePermit,
    SemaphoreAcquireFuture,
};

// CondVar
pub use condvar::{
    CondVar,
    CondVarFuture,
};

// Event (内部使用)
pub use event::Event;

// ============================================================================
// 便捷函数导出
// ============================================================================

/// 创建信号量配对
pub use signal::signal_pair as new_signal_pair;

/// 创建互斥锁和条件变量配对
pub use condvar::mutex_condvar_pair;

/// 创建二值信号量
pub use semaphore::binary_semaphore;

// ============================================================================
// Prelude - 常用类型的便捷导入
// ============================================================================

/// 同步原语的 prelude 模块
///
/// 使用 `use neon_rtos2::sync::prelude::*;` 导入所有常用类型
pub mod prelude {
    pub use super::Signal;
    pub use super::Mutex;
    pub use super::MutexGuard;
    pub use super::Semaphore;
    pub use super::CondVar;
    pub use super::RwLock;
    
    // 便捷函数
    pub use super::signal_pair;
    pub use super::mutex_condvar_pair;
    pub use super::binary_semaphore;
}
