use crate::task::Task;
use crate::task::TaskState;

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum Event {
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
    use crate::schedule::Scheduler;
    use crate::task::Task;
    use crate::task::TaskState;

    fn task1(_args: usize) {}
    fn task2(_args: usize) {}
    fn task3(_args: usize) {}
    fn task4(_args: usize) {}
    fn task5(_args: usize) {}
    #[test]
    fn test_event() {
        Task::init();
        //设置部分任务为阻塞状态，进行判断状态是否正确
        let mut task1 = Task::new("task1", task1);
        let mut task2 = Task::new("task2", task2);
        let mut task3 = Task::new("task3", task3);
        let mut task4 = Task::new("task4", task4);
        let mut task5 = Task::new("task5", task5);
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
}
