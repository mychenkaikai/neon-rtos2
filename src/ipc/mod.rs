pub mod channel;
pub mod queue;

// 重新导出常用类型
pub use channel::{Ipc, IpcHandle, IpcError};
pub use queue::Mq;
