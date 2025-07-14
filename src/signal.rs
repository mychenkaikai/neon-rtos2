use crate::config::MAX_SIGNALS;
use crate::event::Event;
use crate::schedule::Scheduler;
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Signal {
    id: usize,
}

impl Signal {
    pub const fn new() -> Self {
        // 在实际初始化时分配ID
        Self { id: usize::MAX } // 使用特殊值表示未初始化
    }
    
    pub fn open(&mut self) {
        if self.id == usize::MAX {
            use core::sync::atomic::{AtomicUsize, Ordering};
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            self.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    pub fn send(&self) {
        Event::wake_task(Event::Signal(self.id));
    }


    //等待一个信号 阻塞当前任务
    pub fn wait(&self) {
        Scheduler::get_current_task().block(Event::Signal(self.id));
    }
}

#[macro_export]
macro_rules! define_signal {
    ($name:ident) => {
        $crate::paste::paste! {
            static mut [<__SIGNAL_ $name>]: $crate::signal::Signal = $crate::signal::Signal::new();
            
            #[allow(non_snake_case)]
            fn $name() -> &'static mut $crate::signal::Signal {
                unsafe {
                    &mut [<__SIGNAL_ $name>]
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Task;
    use crate::task::TaskState;
    use crate::utils::kernel_init;

    //测试任务调度之后，一个正常执行的task被阻塞，之后被唤醒
    #[test]
    fn test_signal() {
        kernel_init();
        let signal = Signal::new();
        let task = Task::new("test_signal", |_| {});
        Scheduler::start();
        signal.wait();
        assert_eq!(
            task.get_state(),
            TaskState::Blocked(Event::Signal(signal.id))
        );
        signal.send();
        assert_eq!(task.get_state(), TaskState::Ready);
        Scheduler::schedule();
        assert_eq!(task.get_state(), TaskState::Running);
    }
}
