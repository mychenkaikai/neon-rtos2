#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtosError {
    // 任务相关
    TaskNotFound,
    TaskSlotsFull,
    InvalidTaskState,
    
    // 同步相关 - Mutex
    MutexNotOwned,
    MutexSlotsFull,
    MutexPoisoned,
    MutexLockFailed,
    
    // 同步相关 - Signal
    SignalSlotsFull,
    SignalClosed,
    WaiterQueueFull,
    
    // 同步相关 - Semaphore
    SemaphoreOverflow,
    SemaphoreClosed,
    
    // 同步相关 - CondVar
    CondVarClosed,
    CondVarTimeout,
    
    // IPC 相关
    QueueFull,
    QueueEmpty,
    InvalidHandle,
    TypeMismatch,
    ChannelClosed,
    ChannelDisconnected,
    
    // 定时器相关
    TimerSlotsFull,
    TimerNotFound,
    Timeout,
    
    // 内存相关
    OutOfMemory,
    
    // 通用错误
    WouldBlock,
    InvalidArgument,
}

impl core::fmt::Display for RtosError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            // 任务相关
            RtosError::TaskNotFound => write!(f, "Task not found"),
            RtosError::TaskSlotsFull => write!(f, "Task slots full"),
            RtosError::InvalidTaskState => write!(f, "Invalid task state"),
            
            // Mutex
            RtosError::MutexNotOwned => write!(f, "Mutex not owned by current task"),
            RtosError::MutexSlotsFull => write!(f, "Mutex slots full"),
            RtosError::MutexPoisoned => write!(f, "Mutex poisoned"),
            RtosError::MutexLockFailed => write!(f, "Failed to acquire mutex lock"),
            
            // Signal
            RtosError::SignalSlotsFull => write!(f, "Signal slots full"),
            RtosError::SignalClosed => write!(f, "Signal closed"),
            RtosError::WaiterQueueFull => write!(f, "Waiter queue full"),
            
            // Semaphore
            RtosError::SemaphoreOverflow => write!(f, "Semaphore overflow"),
            RtosError::SemaphoreClosed => write!(f, "Semaphore closed"),
            
            // CondVar
            RtosError::CondVarClosed => write!(f, "Condition variable closed"),
            RtosError::CondVarTimeout => write!(f, "Condition variable wait timeout"),
            
            // IPC
            RtosError::QueueFull => write!(f, "Queue full"),
            RtosError::QueueEmpty => write!(f, "Queue empty"),
            RtosError::InvalidHandle => write!(f, "Invalid handle"),
            RtosError::TypeMismatch => write!(f, "Type mismatch"),
            RtosError::ChannelClosed => write!(f, "Channel closed"),
            RtosError::ChannelDisconnected => write!(f, "Channel disconnected"),
            
            // Timer
            RtosError::TimerSlotsFull => write!(f, "Timer slots full"),
            RtosError::TimerNotFound => write!(f, "Timer not found"),
            RtosError::Timeout => write!(f, "Operation timed out"),
            
            // Memory
            RtosError::OutOfMemory => write!(f, "Out of memory"),
            
            // Generic
            RtosError::WouldBlock => write!(f, "Operation would block"),
            RtosError::InvalidArgument => write!(f, "Invalid argument"),
        }
    }
}

pub type Result<T> = core::result::Result<T, RtosError>;
