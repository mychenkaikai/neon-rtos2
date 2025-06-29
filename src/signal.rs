use crate::config::MAX_SIGNALS;
use crate::event::EventType;
use crate::schedule::Scheduler;


static mut SIGNAL_LIST: [Signal; MAX_SIGNALS] = [Signal { used: false, id: 0 }; MAX_SIGNALS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Signal {
    used: bool,
    id: usize,
}

impl Signal {
    //初始化信号列表
    pub fn init() {
        unsafe {
            for i in 0..MAX_SIGNALS {
                SIGNAL_LIST[i] = Signal { used: false, id: i };
            }
        }
    }
    //创建一个信号
    pub fn new() -> Self {
        unsafe {
            for i in 0..MAX_SIGNALS {
                if !SIGNAL_LIST[i].used {
                    SIGNAL_LIST[i].used = true;
                    return Self { used: true, id: i };
                }
            }
        }
        panic!("Signal list is full");
    }
    //调用event的wake_task函数
    pub fn send(&self) {
        EventType::wake_task(EventType::Signal(self.id));
    }

    //等待一个信号 阻塞当前任务
    pub fn wait(&self) {
        Scheduler::get_current_task().block(EventType::Signal(self.id));
    }
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
        assert_eq!(task.get_state(), TaskState::Blocked(EventType::Signal(signal.id)));
        signal.send();
        assert_eq!(task.get_state(), TaskState::Ready);
        Scheduler::schedule();
        assert_eq!(task.get_state(), TaskState::Running);
    }
}
