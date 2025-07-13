use crate::config::MAX_SIGNALS;
use crate::event::Event;
use crate::schedule::Scheduler;

static mut SIGNAL_LIST: [Signal; MAX_SIGNALS] = [Signal { used: false, id: -1 }; MAX_SIGNALS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Signal {
    used: bool,
    id: isize,
}

impl Signal {
    //初始化信号列表
    pub fn init() {
        unsafe {
            for i in 0..MAX_SIGNALS {
                SIGNAL_LIST[i] = Signal { used: false, id: -1 };
            }
        }
    }
    //创建一个信号
    pub const fn new() -> Self {
        Self { used: false, id: 0 }
    }
    pub fn open(&self) {
        unsafe {
            // 检查该信号是否已经被注册
            if self.id != -1 {
                return;
            }
            
            // 未注册，找一个空位
            for i in 0..MAX_SIGNALS {
                if !SIGNAL_LIST[i].used {
                    SIGNAL_LIST[i].used = true;
                    SIGNAL_LIST[i].id = i as isize;
                    return;
                }
            }
            
            panic!("Signal list is full");
        }
    }

    //调用event的wake_task函数
    pub fn send(&self) {
        Event::wake_task(Event::Signal(self.id as usize));
    }

    //等待一个信号 阻塞当前任务
    pub fn wait(&self) {
        Scheduler::get_current_task().block(Event::Signal(self.id as usize));
    }
}

#[macro_export]
macro_rules! define_signal {
    ($name:ident) => {
        $crate::paste::paste! {
            static [<__SIGNAL_ $name>]: $crate::signal::Signal = $crate::signal::Signal::new();
            
            #[allow(non_snake_case)]
            fn $name() -> &'static $crate::signal::Signal {
                unsafe {
                    [<__SIGNAL_ $name>].open();
                }
                &[<__SIGNAL_ $name>]
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
