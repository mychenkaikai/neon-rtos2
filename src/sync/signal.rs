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
    pub const fn new() -> Self {
        // 在实际初始化时分配ID
        Self { id: usize::MAX } // 使用特殊值表示未初始化
    }
    
    /// 初始化信号量系统
    /// 
    /// 重置信号量 ID 计数器，用于测试环境
    pub fn init() {
        NEXT_SIGNAL_ID.store(0, Ordering::Relaxed);
    }
    
    pub fn open(&mut self) {
        if self.id == usize::MAX {
            self.id = NEXT_SIGNAL_ID.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    pub fn send(&self) {
        Event::wake_task(Event::Signal(self.id));
    }


    //等待一个信号 阻塞当前任务
    pub fn wait(&self) {
        Scheduler::get_current_task().block(Event::Signal(self.id));
        trigger_schedule();
    }
}

#[macro_export]
macro_rules! define_signal {
    ($name:ident) => {
        $crate::paste::paste! {
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
        
        // 创建一个新的信号量并初始化
        let mut signal = Signal::new();
        signal.open();
        
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
        
        let mut signal1 = Signal::new();
        let mut signal2 = Signal::new();
        signal1.open();
        signal2.open();
        
        // 验证两个信号量有不同的 ID
        assert_ne!(signal1.id, signal2.id);
    }
}
