use crate::{
    config::MAX_TASKS,
    task::{TASK_LIST, Task, TaskState},
};

static mut SCHEDULER: Scheduler = Scheduler { current_task: None };

struct Scheduler {
    current_task: Option<Task>,
}

impl Scheduler {
    pub fn schedule() {
        //找到当前任务之后的下一个非阻塞任务设置为新的当前任务,并且设置当前任务为就绪状态
        //如果当前任务是最后一个任务,则设置为第一个任务
        let mut next_task = None;
        unsafe {
            let mut current_task = SCHEDULER.current_task.unwrap();
            for i in current_task.0..MAX_TASKS {
                if TASK_LIST[i].state == TaskState::Ready {
                    next_task = Some(Task(i));

                    break;
                }
            }
            if next_task.is_none() {
                for i in 0..current_task.0 {
                    if TASK_LIST[i].state == TaskState::Ready {
                        next_task = Some(Task(i));
                        break;
                    }
                }
            }

            if let Some(task) = next_task.as_mut() {
                current_task.ready();
                task.run();
                SCHEDULER.current_task = Some(*task);
            }
        }
    }

    pub fn start() {
        //此时当前任务还未设置,所以需要设置为第一个任务
        unsafe {
            SCHEDULER.current_task = Some(Task(0));
            //设置当前任务为运行状态
            TASK_LIST[0].state = TaskState::Running;
        }
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

    #[test]
    fn test_schedule() {
        Task::reset_tasks();
        let task1 = Task::new("task1", task1);
        let task2 = Task::new("task2", task2);

        Scheduler::start();
        //统计任务状态为Running的次数,只能有一个任务处于Running状态
        let mut running_count = 0;
        running_count += if task1.get_state() == TaskState::Running {
            1
        } else {
            0
        };
        running_count += if task2.get_state() == TaskState::Running {
            1
        } else {
            0
        };
        assert_eq!(running_count, 1);
        Scheduler::schedule();
        running_count = 0;
        running_count += if task1.get_state() == TaskState::Running {
            1
        } else {
            0
        };
        running_count += if task2.get_state() == TaskState::Running {
            1
        } else {
            0
        };
        assert_eq!(running_count, 1);
        Scheduler::schedule();
        running_count = 0;
        running_count += if task1.get_state() == TaskState::Running {
            1
        } else {
            0
        };
        running_count += if task2.get_state() == TaskState::Running {
            1
        } else {
            0
        };
        assert_eq!(running_count, 1);
    }

    #[test]
    fn test_schedule_block() {
        Task::reset_tasks();
        let mut task1 = Task::new("task1", task1);
        let task2 = Task::new("task2", task2);

        Scheduler::start();
        task1.block(BlockReason::Signal);
        Scheduler::schedule();
        assert_eq!(task1.get_state(), TaskState::Blocked(BlockReason::Signal));
        assert_eq!(task2.get_state(), TaskState::Running);
    }
}
