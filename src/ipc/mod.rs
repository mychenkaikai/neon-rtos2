pub mod channel;
pub mod queue;
pub mod typed;

// 重新导出常用类型
pub use channel::{Ipc, IpcHandle, IpcError};
pub use queue::Mq;

/// Typed message channels for RTOS tasks and async futures.
pub use typed::{bounded, unbounded, Sender, Receiver, SendError, TrySendError, TryRecvError};
