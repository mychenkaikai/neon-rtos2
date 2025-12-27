//! # 异步运行时模块
//!
//! 提供基础的异步运行时支持，包括执行器、Waker 和 Future 辅助类型。
//!
//! ## 特性
//!
//! - 🚀 **轻量级执行器**: 适合嵌入式环境的简单执行器
//! - ⚡ **零成本 Waker**: 基于任务 ID 的唤醒机制
//! - 🔄 **异步原语**: 异步信号量、定时器、通道
//! - 🎯 **Select 宏**: 同时等待多个异步操作
//!
//! ## 使用示例
//!
//! ### 基本用法
//!
//! ```rust,no_run
//! use neon_rtos2::runtime::{Executor, channel::channel};
//!
//! fn main() {
//!     // 创建执行器
//!     let mut executor = Executor::new();
//!
//!     // 创建通道
//!     let (tx, rx) = channel::<u32>(16);
//!
//!     // 添加异步任务
//!     executor.spawn(async move {
//!         loop {
//!             // 模拟等待信号
//!             // signal.wait().await;
//!             // 处理信号
//!             break; // 避免无限循环导致测试卡死
//!         }
//!     });
//!
//!     // 运行执行器
//!     executor.run();
//! }
//! ```
//!
//! ### 使用 Select
//!
/// ```rust,no_run
/// # use neon_rtos2::select;
/// # use neon_rtos2::kernel::time::timer::Timer;
/// # struct Rx;
/// # impl Rx { async fn recv(&self) -> i32 { 0 } }
/// # let rx = Rx;
/// # let timer = Timer;
/// async fn handle_events() {
///     select! {
///         msg = rx.recv() => println!("Received: {:?}", msg),
///         _ = Timer::sleep(1000) => println!("Timeout!"),
///     }
/// }
/// ```

mod waker;
mod executor;
mod future;
mod channel;
pub mod select;

pub use waker::TaskWaker;
pub use executor::Executor;
pub use future::*;
pub use channel::{channel, unbounded, Sender, Receiver, SendError, RecvError};

// 重新导出 select 模块的类型
pub use select::{
    Select2, Select3, Select4,
    Either, Either3, Either4,
    select2, select3, select4,
    Race, race2, race3,
};

