use crate::task::{Task, TaskState};

static mut SCHEDULER: Scheduler = Scheduler {
    current_task: None,
    is_running: false,
};

struct Scheduler {
    current_task: Option<Task>,
    is_running: bool,
}

impl Scheduler {
    //使用task::for_each_from遍历所有任务,找到当前任务之后的下一个非阻塞任务,如果当前任务是最后一个任务,则找到第一个任务
    //但也要考虑其他任务找不到准备状态，此时currenttask还是原任务
    pub fn schedule() {
        if !unsafe { SCHEDULER.is_running } {
            return;
        }
        let mut next_task: Task = unsafe { SCHEDULER.current_task.unwrap() };
        let current_task = unsafe { SCHEDULER.current_task.unwrap() };
        Task::for_each_from(current_task.get_taskid() + 1, |task, _| {
            if task.get_state() == TaskState::Ready {
                next_task = task.clone();
            }
        });
        if next_task == current_task {
            //如果next_task还是原任务，则说明没有找到下一个非阻塞任务，此时currenttask还是原任务
            return;
        }
        if current_task.get_state() == TaskState::Running {
            Task::operate(current_task.get_taskid(), |task| {
                task.ready();
            });
        }
        Task::operate(next_task.get_taskid(), |task| {
            task.run();
        });
        unsafe { SCHEDULER.current_task = Some(next_task) };
    }

    pub fn start() {
        //此时当前任务还未设置,所以需要设置为第一个任务
        unsafe {
            SCHEDULER.current_task = Some(Task(0));
            //设置当前任务为运行状态
            Task::operate(0, |task| {
                task.run();
            });
        }
        unsafe { SCHEDULER.is_running = true };
    }

    //关闭调度器
    pub fn stop() {
        unsafe { SCHEDULER.is_running = false };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::BlockReason;
    use crate::task::Task;
    use crate::task::TaskState;

    fn task1(_args: usize) {
        // 简化的任务函数
    }

    fn task2(_args: usize) {
        // 简化的任务函数
    }

    fn task3(_args: usize) {
        // 简化的任务函数
    }

    fn task4(_args: usize) {
        // 简化的任务函数
    }

    fn task5(_args: usize) {
        // 简化的任务函数
    }

    #[test]
    fn test_schedule() {
        Task::reset_tasks();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Task::new("task3", task3);
        Task::new("task4", task4);
        Task::new("task5", task5);

        Scheduler::start();
        //统计任务状态为Running的次数,只能有一个任务处于Running状态
        let mut running_count = 0;
        //统计所有的ready任务
        let mut ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Running {
                running_count += 1;
            }
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        assert_eq!(running_count, 1);
        assert_eq!(ready_count, 4);
        Scheduler::schedule();
        running_count = 0;
        ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Running {
                running_count += 1;
            }
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        assert_eq!(running_count, 1);
        assert_eq!(ready_count, 4);
        Scheduler::schedule();
        running_count = 0;
        ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Running {
                running_count += 1;
            }
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        assert_eq!(running_count, 1);
        assert_eq!(ready_count, 4);
    }

    #[test]
    fn test_schedule_block() {
        Task::reset_tasks();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Task::new("task3", task3);
        Task::new("task4", task4);
        Task::new("task5", task5);
        Scheduler::start();
        unsafe { SCHEDULER.current_task.unwrap() }.block(BlockReason::Signal);
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Blocked(BlockReason::Signal)
        );
        Scheduler::schedule();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
        Scheduler::schedule();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
    }

    #[test]
    fn test_schedule_block_and_schedule() {
        Task::reset_tasks();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Task::new("task3", task3);
        Task::new("task4", task4);
        Task::new("task5", task5);
        Scheduler::start();
        unsafe { SCHEDULER.current_task.unwrap() }.block(BlockReason::Signal);
        //保存此时的current_task为block_task
        let block_task = unsafe { SCHEDULER.current_task.unwrap() };
        Scheduler::schedule();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
        //测试block_task是否还是原任务
        assert_eq!(
            block_task.get_state(),
            TaskState::Blocked(BlockReason::Signal)
        );
        Scheduler::schedule();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
        //测试block_task是否还是原任务
        assert_eq!(
            block_task.get_state(),
            TaskState::Blocked(BlockReason::Signal)
        );
    }

    //测试调度器关闭后，是否还能调度
    #[test]
    fn test_schedule_stop() {
        Task::reset_tasks();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Scheduler::start();
        let current_task = unsafe { SCHEDULER.current_task.unwrap() };
        Scheduler::stop();
        Scheduler::schedule();
        assert_eq!(current_task.get_state(), TaskState::Running);
    }
}
