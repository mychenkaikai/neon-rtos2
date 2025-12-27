use crate::sync::event::Event;
use crate::kernel::scheduler::Scheduler;
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::hal::trigger_schedule;

/// 信号量 ID 计数器
static NEXT_SIGNAL_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Signal {
    id: usize,
}

impl Signal {
    /// 用于静态初始化的 const 构造函数
    /// 
    /// 注意：使用前必须调用 `open()` 或 `ensure_init()` 进行初始化
    /// 
    /// # 示例
    /// ```rust,ignore
    /// static mut MY_SIGNAL: Signal = Signal::new_uninit();
    /// 
    /// // 使用前初始化
    /// unsafe {
    ///     MY_SIGNAL.ensure_init();
    ///     MY_SIGNAL.wait();
    /// }
    /// ```
    pub const fn new_uninit() -> Self {
        Self { id: usize::MAX }
    }
    
    /// 创建并初始化信号量（推荐使用）
    /// 
    /// 这是创建信号量的推荐方式，无需额外调用 `open()`
    /// 
    /// # 示例
    /// ```rust,ignore
    /// let signal = Signal::create();
    /// signal.wait();
    /// ```
    pub fn create() -> Self {
        let id = NEXT_SIGNAL_ID.fetch_add(1, Ordering::Relaxed);
        Self { id }
    }
    
    /// 兼容旧 API 的构造函数
    /// 
    /// 注意：使用前必须调用 `open()` 进行初始化
    /// 推荐使用 `Signal::create()` 替代
    #[deprecated(since = "0.2.0", note = "请使用 Signal::create() 替代")]
    pub const fn new() -> Self {
        Self::new_uninit()
    }
    
    /// 初始化信号量系统
    /// 
    /// 重置信号量 ID 计数器，用于测试环境
    pub fn init() {
        NEXT_SIGNAL_ID.store(0, Ordering::Relaxed);
    }
    
    /// 确保信号量已初始化
    /// 
    /// 如果信号量尚未初始化，则分配一个新的 ID。
    /// 如果已初始化，则不做任何操作。
    /// 
    /// # 示例
    /// ```rust,ignore
    /// let mut signal = Signal::new_uninit();
    /// signal.ensure_init();
    /// signal.wait();
    /// ```
    pub fn ensure_init(&mut self) {
        if self.id == usize::MAX {
            self.id = NEXT_SIGNAL_ID.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    /// 检查信号量是否已���始化
    pub fn is_initialized(&self) -> bool {
        self.id != usize::MAX
    }
    
    /// 初始化信号量（兼容旧 API）
    /// 
    /// 推荐使用 `ensure_init()` 或直接使用 `Signal::create()`
    pub fn open(&mut self) {
        self.ensure_init();
    }
    
    pub fn send(&self) {
        Event::wake_task(Event::Signal(self.id));
    }

    /// 等待一个信号，阻塞当前任务
    pub fn wait(&self) {
        Scheduler::get_current_task().block(Event::Signal(self.id));
        trigger_schedule();
    }
}

#[macro_export]
macro_rules! define_signal {
    ($name:ident) => {
        $crate::paste::paste! {
            #[allow(deprecated)]
            static mut [<__SIGNAL_ $name>]: $crate::sync::signal::Signal = $crate::sync::signal::Signal::new();
            
            #[allow(non_snake_case)]
            fn $name() -> &'static mut $crate::sync::signal::Signal {
                unsafe {
                    [<__SIGNAL_ $name>].open();
                    &mut [<__SIGNAL_ $name>]
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::utils::kernel_init;
    use serial_test::serial;

    //测试任务调度之后，一个正常执行的task被阻塞，之后被唤醒
    #[test]
    #[serial]
    fn test_signal() {
        kernel_init();
        
        // 使用新的 create() 方法创建信号量
        let signal = Signal::create();
        
        // 创建任务
        let task = Task::new("test_signal", |_| {}).unwrap();
        
        // 启动调度器
        Scheduler::start();
        
        // 此时 task 是当前运行的任务（id=0）
        // 调用 wait 会阻塞当前任务
        signal.wait();
        
        // 验证任务被阻塞
        assert_eq!(
            task.get_state(),
            TaskState::Blocked(Event::Signal(signal.id))
        );
        
        // 发送信号唤醒任务
        signal.send();
        assert_eq!(task.get_state(), TaskState::Ready);
        
        // 调度后任务应该运行
        Scheduler::task_switch();
        assert_eq!(task.get_state(), TaskState::Running);
    }
    
    #[test]
    #[serial]
    fn test_signal_multiple() {
        kernel_init();
        
        // 使用新的 create() 方法
        let signal1 = Signal::create();
        let signal2 = Signal::create();
        
        // 验证两个信号量有不同的 ID
        assert_ne!(signal1.id, signal2.id);
    }
    
    #[test]
    #[serial]
    fn test_signal_ensure_init() {
        kernel_init();
        
        // 测试 new_uninit + ensure_init 模式
        let mut signal = Signal::new_uninit();
        assert!(!signal.is_initialized());
        
        signal.ensure_init();
        assert!(signal.is_initialized());
        
        // 再次调用 ensure_init 不应改变 ID
        let id = signal.id;
        signal.ensure_init();
        assert_eq!(signal.id, id);
    }
    
    #[test]
    #[serial]
    fn test_signal_create_vs_new() {
        kernel_init();
        
        // create() 直接返回已初始化的信号量
        let signal1 = Signal::create();
        assert!(signal1.is_initialized());
        
        // new_uninit() 返回未初始化的信号量
        let signal2 = Signal::new_uninit();
        assert!(!signal2.is_initialized());
    }
}
