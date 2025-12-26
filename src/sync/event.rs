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

    fn task1(_args: usize) {}
    fn task2(_args: usize) {}
    fn task3(_args: usize) {}
    fn task4(_args: usize) {}
    fn task5(_args: usize) {}
    #[test]
    fn test_event() {
        Task::init();
        //设置部分任务为阻塞状态，进行判断状态是否正确
        let mut task1 = Task::new("task1", task1).unwrap();
        let mut task2 = Task::new("task2", task2).unwrap();
        let mut task3 = Task::new("task3", task3).unwrap();
        let mut task4 = Task::new("task4", task4).unwrap();
        let mut task5 = Task::new("task5", task5).unwrap();
        Scheduler::start();
        task1.block(Event::Signal(1));
        task2.block(Event::Signal(2));
        task3.block(Event::Signal(3));
        task4.block(Event::Signal(4));
        task5.block(Event::Signal(5));
        assert_eq!(task1.get_state(), TaskState::Blocked(Event::Signal(1)));
        assert_eq!(task2.get_state(), TaskState::Blocked(Event::Signal(2)));
        assert_eq!(task3.get_state(), TaskState::Blocked(Event::Signal(3)));
        assert_eq!(task4.get_state(), TaskState::Blocked(Event::Signal(4)));
        assert_eq!(task5.get_state(), TaskState::Blocked(Event::Signal(5)));
        Scheduler::task_switch();
        //调度之后任务依然为阻塞状态
        assert_eq!(task1.get_state(), TaskState::Blocked(Event::Signal(1)));
        assert_eq!(task2.get_state(), TaskState::Blocked(Event::Signal(2)));
        assert_eq!(task3.get_state(), TaskState::Blocked(Event::Signal(3)));
        assert_eq!(task4.get_state(), TaskState::Blocked(Event::Signal(4)));
        assert_eq!(task5.get_state(), TaskState::Blocked(Event::Signal(5)));
        //唤醒任务
        Event::wake_task(Event::Signal(1));
        assert_eq!(task1.get_state(), TaskState::Ready);
        assert_eq!(task2.get_state(), TaskState::Blocked(Event::Signal(2)));
        assert_eq!(task3.get_state(), TaskState::Blocked(Event::Signal(3)));
        assert_eq!(task4.get_state(), TaskState::Blocked(Event::Signal(4)));
        assert_eq!(task5.get_state(), TaskState::Blocked(Event::Signal(5)));
        Scheduler::task_switch();
        //调度之后当前任务为运行状态，其他任务为阻塞状态
        //打印所有任务状态
        Task::for_each(|task, _| {
            println!("task{}: {:?}", task.get_taskid(), task.get_state());
        });
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        //遍历所有任务，通过cnt判断只有一个任务是运行状态，其他是就绪状态
        let mut cnt = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Running {
                cnt += 1;
            } else {
                //通过模式匹配只是判断是不是Signal事件不关注Signal的id
                //首先肯定是阻塞的Signal事件，之后通过通配符捕获id，如果之前的步骤不匹配，则assert失败
                match task.get_state() {
                    TaskState::Blocked(Event::Signal(id)) => {
                        assert_eq!(id, task.get_taskid() + 1)
                    }
                    _ => assert!(false),
                }
            }
        });
        assert_eq!(cnt, 1); //只有一个任务是运行状态
        //停止调度器
        Scheduler::stop();
    }

    #[test]
    fn test_wake_task_with_no_blocked_tasks() {
        kernel_init();
        
        Task::new("wake_test1", |_| {}).unwrap();
        Task::new("wake_test2", |_| {}).unwrap();
        
        // 所有任务都处于就绪状态
        
        // 尝试唤醒没有被阻塞的事件
        Event::wake_task(Event::Signal(99));
        
        // 确认任务状态没有变化
        let mut ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        
        assert_eq!(ready_count, 2);
    }
    
    #[test]
    fn test_multiple_blocked_same_event() {
        kernel_init();
        
        let mut task1 = Task::new("same_event1", |_| {}).unwrap();
        let mut task2 = Task::new("same_event2", |_| {}).unwrap();
        let mut task3 = Task::new("same_event3", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 多个任务被相同事件阻塞
        task1.block(Event::Signal(5));
        task2.block(Event::Signal(5));
        task3.block(Event::Signal(5));
        
        assert_eq!(task1.get_state(), TaskState::Blocked(Event::Signal(5)));
        assert_eq!(task2.get_state(), TaskState::Blocked(Event::Signal(5)));
        assert_eq!(task3.get_state(), TaskState::Blocked(Event::Signal(5)));
        
        // 唤醒被同一事件阻塞的所有任务
        Event::wake_task(Event::Signal(5));
        
        assert_eq!(task1.get_state(), TaskState::Ready);
        assert_eq!(task2.get_state(), TaskState::Ready);
        assert_eq!(task3.get_state(), TaskState::Ready);
    }
    
    #[test]
    fn test_different_event_types() {
        kernel_init();
        
        let mut task1 = Task::new("diff_event1", |_| {}).unwrap();
        let mut task2 = Task::new("diff_event2", |_| {}).unwrap();
        let mut task3 = Task::new("diff_event3", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 不同类型的事件阻塞
        task1.block(Event::Signal(1));
        task2.block(Event::Timer(1));
        task3.block(Event::Mutex(1));
        
        // 唤醒特定类型事件
        Event::wake_task(Event::Signal(1));
        
        // 只有对应事件类型的任务被唤醒
        assert_eq!(task1.get_state(), TaskState::Ready);
        assert_eq!(task2.get_state(), TaskState::Blocked(Event::Timer(1)));
        assert_eq!(task3.get_state(), TaskState::Blocked(Event::Mutex(1)));
    }
}
