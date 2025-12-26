//! # 异步运行时模块
//!
//! 提供基础的异步运行时支持，包括执行器、Waker 和 Future 辅助类型。
//!
//! ## 特性
//!
//! - 🚀 **轻量级执行器**: 适合嵌入式环境的简单执行器
//! - ⚡ **零成本 Waker**: 基于任务 ID 的唤醒机制
//! - 🔄 **异步原语**: 异步信号量、定时器、通道
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! use neon_rtos2::runtime::{Executor, spawn, channel};
//!
//! // 创建执行器
//! let mut executor = Executor::new();
//!
//! // 创建通道
//! let (tx, rx) = channel::<u32>(16);
//!
//! // 添加异步任务
//! executor.spawn(async {
//!     loop {
//!         signal.wait().await;
//!         // 处理信号
//!     }
//! });
//!
//! // 运行执行器
//! executor.run();
//! ```

mod waker;
mod executor;
mod future;
mod channel;

pub use waker::TaskWaker;
pub use executor::Executor;
pub use future::*;
pub use channel::{channel, unbounded, Sender, Receiver, SendError, RecvError};

