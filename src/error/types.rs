#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtosError {
    // 任务相关
    TaskNotFound,
    TaskSlotsFull,
    InvalidTaskState,
    
    // 同步相关
    MutexNotOwned,
    MutexSlotsFull,
    SignalSlotsFull,
    
    // IPC 相关
    QueueFull,
    QueueEmpty,
    InvalidHandle,
    TypeMismatch,
    
    // 定时器相关
    TimerSlotsFull,
    TimerNotFound,
    
    // 内存相关
    OutOfMemory,
}

pub type Result<T> = core::result::Result<T, RtosError>;
