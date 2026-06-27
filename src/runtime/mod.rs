//! # 异步运行时模块
//!
//! 提供当前已落地的异步运行时支持，包括执行器、Waker、异步休眠和常用 Future
//! 辅助类型。
//!
//! ## 当前特性
//!
//! - 轻量级执行器：只轮询被唤醒的 Future，适合嵌入式环境
//! - 基于任务 ID 的 Waker：唤醒 Future 的同时唤醒承载执行器的 RTOS 任务
//! - 异步原语：异步信号量、异步休眠、异步通道和 `yield_now()`
//! - `select!` 宏：同时等待多个异步操作
//!
//! ## 当前范围
//!
//! - `Executor::run()` 与 `Executor::poll_once()` 共享同一条核心轮询路径
//! - `sleep(duration_ms)` 在首次 `poll` 时注册一次异步休眠槽位，截止时间到达后由
//!   定时器路径唤醒，下一次 `poll` 返回 `Ready`
//! - `run()` 在没有新唤醒任务但仍有未完成任务时，会阻塞当前 RTOS 任务，保持现有
//!   空闲阻塞语义
//! - 本模块当前不承诺 `Duration` 风格接口、多核执行或公平性策略
//!
//! ## 使用示例
//!
//! ### 基本用法
//!
//! ```rust,no_run
//! use neon_rtos2::runtime::{Executor, sleep};
//!
//! async fn worker() {
//!     sleep(10).await;
//! }
//!
//! fn main() {
//!     let mut executor = Executor::new();
//!     executor.spawn(worker());
//!     executor.run();
//! }
//! ```
//!
//! ### 使用 Select
//!
//! ```rust,no_run
//! # use neon_rtos2::runtime::sleep;
//! # use neon_rtos2::select;
//! # struct Rx;
//! # impl Rx { async fn recv(&self) -> i32 { 0 } }
//! # let rx = Rx;
//! async fn handle_events() {
//!     select! {
//!         msg = rx.recv() => println!("Received: {:?}", msg),
//!         _ = sleep(1000) => println!("Timeout!"),
//!     }
//! }
//! ```

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
