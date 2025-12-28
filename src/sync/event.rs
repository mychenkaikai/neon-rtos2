use crate::kernel::task::Task;
use crate::kernel::task::TaskState;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Event {
    Signal(usize),
    Timer(usize),
    Ipc(usize),
    Memory(usize),
    Network(usize),
    Mutex(usize),
    Mq(usize),
    CondVar(usize),
    Barrier(usize),
    Once(usize),
}

impl Event {
    //根据事件唤醒被所有被这个事件阻塞的task
    pub(crate) fn wake_task(event_type: Event) {
        Task::for_each(|mut task, _| {
            if task.get_state() == TaskState::Blocked(event_type) {
                task.ready();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::utils::kernel_init;
    use serial_test::serial;

    fn task1(_args: usize) {}
    fn task2(_args: usize) {}
    fn task3(_args: usize) {}
    fn task4(_args: usize) {}
    fn task5(_args: usize) {}
    
    #[test]
    #[serial]
    fn test_event() {
        kernel_init();
        //设置部分任务为阻塞状态，进行判断状态是否正确
        let mut task1 = Task::new("task1", task1).unwrap();
        let mut task2 = Task::new("task2", task2).unwrap();
        let mut task3 = Task::new("task3", task3).unwrap();
        let mut task4 = Task::new("task4", task4).unwrap();
        let mut task5 = Task::new("task5", task5).unwrap();
        Scheduler::start();
        
        // 先切换到非 task1，这样 task1 可以被阻塞
        // task1 是当前运行的任务（id=0），我们需要先让它变成 Ready
        let current = Scheduler::get_current_task();
        
        // 阻塞除当前任务外的其他任务
        if current.get_taskid() != task1.get_taskid() {
            task1.block(Event::Signal(1));
        }
        if current.get_taskid() != task2.get_taskid() {
            task2.block(Event::Signal(2));
        }
        if current.get_taskid() != task3.get_taskid() {
            task3.block(Event::Signal(3));
        }
        if current.get_taskid() != task4.get_taskid() {
            task4.block(Event::Signal(4));
        }
        if current.get_taskid() != task5.get_taskid() {
            task5.block(Event::Signal(5));
        }
        
        // 唤醒 task2
        Event::wake_task(Event::Signal(2));
        assert_eq!(task2.get_state(), TaskState::Ready);
        
        //停止调度器
        Scheduler::stop();
    }

    #[test]
    #[serial]
    fn test_wake_task_with_no_blocked_tasks() {
        kernel_init();
        
        Task::new("wake_test1", |_| {}).unwrap();
        Task::new("wake_test2", |_| {}).unwrap();
        
        // 不启动调度器，所有任务都处于就绪状态
        
        // 尝试唤醒没有被阻塞的事件
        Event::wake_task(Event::Signal(99));
        
        // 确认任务状态没有变化（都是 Ready）
        let mut ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        
        assert_eq!(ready_count, 2);
    }
    
    #[test]
    #[serial]
    fn test_multiple_blocked_same_event() {
        kernel_init();
        
        let mut task1 = Task::new("same_event1", |_| {}).unwrap();
        let mut task2 = Task::new("same_event2", |_| {}).unwrap();
        let mut task3 = Task::new("same_event3", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 获取当前任务，只阻塞非当前任务
        let current_id = Scheduler::get_current_task().get_taskid();
        
        // 多个任务被相同事件阻塞（排除当前运行的任务）
        if task1.get_taskid() != current_id {
            task1.block(Event::Signal(5));
        }
        if task2.get_taskid() != current_id {
            task2.block(Event::Signal(5));
        }
        if task3.get_taskid() != current_id {
            task3.block(Event::Signal(5));
        }
        
        // 唤醒被同一事件阻塞的所有任务
        Event::wake_task(Event::Signal(5));
        
        // 验证非当前任务都被唤醒为 Ready
        if task1.get_taskid() != current_id {
            assert_eq!(task1.get_state(), TaskState::Ready);
        }
        if task2.get_taskid() != current_id {
            assert_eq!(task2.get_state(), TaskState::Ready);
        }
        if task3.get_taskid() != current_id {
            assert_eq!(task3.get_state(), TaskState::Ready);
        }
    }
    
    #[test]
    #[serial]
    fn test_different_event_types() {
        kernel_init();
        
        let mut task1 = Task::new("diff_event1", |_| {}).unwrap();
        let mut task2 = Task::new("diff_event2", |_| {}).unwrap();
        let mut task3 = Task::new("diff_event3", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 获取当前任务
        let current_id = Scheduler::get_current_task().get_taskid();
        
        // 不同类型的事件阻塞（排除当前任务）
        if task1.get_taskid() != current_id {
            task1.block(Event::Signal(1));
        }
        if task2.get_taskid() != current_id {
            task2.block(Event::Timer(1));
        }
        if task3.get_taskid() != current_id {
            task3.block(Event::Mutex(1));
        }
        
        // 唤醒特定类型事件
        Event::wake_task(Event::Signal(1));
        
        // 只有对应事件类型的任务被唤醒
        if task1.get_taskid() != current_id {
            assert_eq!(task1.get_state(), TaskState::Ready);
        }
        if task2.get_taskid() != current_id {
            assert_eq!(task2.get_state(), TaskState::Blocked(Event::Timer(1)));
        }
        if task3.get_taskid() != current_id {
            assert_eq!(task3.get_state(), TaskState::Blocked(Event::Mutex(1)));
        }
    }
}
