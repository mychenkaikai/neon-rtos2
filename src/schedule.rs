use crate::task::{Task, TaskState};
use crate::arch::init_idle_task;

static mut SCHEDULER: Scheduler = Scheduler {
    current_task: None, 
    next_task: None,
    is_running: false,
};

pub struct Scheduler {
    current_task: Option<Task>,
    next_task: Option<Task>,
    is_running: bool,
}

impl Scheduler {
    pub fn init() {
        unsafe {
            SCHEDULER = Scheduler {
                current_task: None,
                next_task: None,
                is_running: false,
            };

            init_idle_task();

        }
    }
    //使用task::for_each_from遍历所有任务,找到当前任务之后的下一个非阻塞任务,如果当前任务是最后一个任务,则找到第一个任务
    //但也要考虑其他任务找不到准备状态，此时currenttask还是原任务
    pub fn task_switch() {
        // 如果调度器未运行，直接返回
        if !unsafe { SCHEDULER.is_running } {
            return;
        }

        let mut current_task = unsafe { SCHEDULER.current_task.unwrap() };

        // 查找下一个准备好的任务
        let mut next_task: Option<Task> = None;
        Task::for_each_from(current_task.get_taskid() + 1, |task, _| {
            if task.get_state() == TaskState::Ready
                && task.get_taskid() != current_task.get_taskid()
                && next_task.is_none()
            {
                next_task = Some(*task);
            }
        });

        match (next_task, current_task.get_state()) {
            // 找到了下一个准备好的任务
            (Some(mut next), _) => {
                // 如果当前任务正在运行，将其设为就绪状态
                if current_task.get_state() == TaskState::Running {
                    current_task.ready();
                }

                // 运行下一个任务
                next.run();
                unsafe { SCHEDULER.current_task = Some(next) };
            }

            // 没找到其他任务，但当前任务就绪
            (None, TaskState::Ready) => {
                current_task.run();
                unsafe { SCHEDULER.current_task = Some(current_task) };
            }

            // 其他情况保持不变
            _ => {}
        }
    }

    pub fn start() {
        //此时当前任务还未设置,所以需要设置为第一个任务
        unsafe {
            SCHEDULER.current_task = Some(Task(0));
            Task(0).run();
        }
        unsafe { SCHEDULER.is_running = true };
        //触发当前架构的任务切换
        crate::arch::start_first_task();
    }

    //关闭调度器
    pub fn stop() {
        unsafe { SCHEDULER.is_running = false };
    }


    pub fn get_current_task() -> Task {
        unsafe { SCHEDULER.current_task.unwrap() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::task::Task;
    use crate::task::TaskState;
    use crate::utils::kernel_init;

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
        kernel_init();
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
        Scheduler::task_switch();
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
        Scheduler::task_switch();
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
        kernel_init();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Task::new("task3", task3);
        Task::new("task4", task4);
        Task::new("task5", task5);
        Scheduler::start();
        unsafe { SCHEDULER.current_task.unwrap() }.block(Event::Signal(1));
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Blocked(Event::Signal(1))
        );
        Scheduler::task_switch();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
        Scheduler::task_switch();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
    }

    #[test]
    fn test_schedule_block_and_schedule() {
        kernel_init();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Task::new("task3", task3);
        Task::new("task4", task4);
        Task::new("task5", task5);
        Scheduler::start();
        unsafe { SCHEDULER.current_task.unwrap() }.block(Event::Signal(1));
        //保存此时的current_task为block_task
        let block_task = unsafe { SCHEDULER.current_task.unwrap() };
        Scheduler::task_switch();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
        //测试block_task是否还是原任务
        assert_eq!(
            block_task.get_state(),
            TaskState::Blocked(Event::Signal(1))
        );
        Scheduler::task_switch();
        assert_eq!(
            unsafe { SCHEDULER.current_task.unwrap() }.get_state(),
            TaskState::Running
        );
        //测试block_task是否还是原任务
        assert_eq!(
            block_task.get_state(),
            TaskState::Blocked(Event::Signal(1))
        );
    }

    //测试调度器关闭后，是否还能调度
    #[test]
    fn test_schedule_stop() {
        kernel_init();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Scheduler::start();
        let current_task = unsafe { SCHEDULER.current_task.unwrap() };
        Scheduler::stop();
        Scheduler::task_switch();
        assert_eq!(current_task.get_state(), TaskState::Running);
    }

    #[test]
    fn test_all_tasks_blocked() {
        kernel_init();
        let mut task1 = Task::new("blocked_task1", |_| {});
        let mut task2 = Task::new("blocked_task2", |_| {});
        
        Scheduler::start();
        
        // 阻塞所有任务
        task1.block(Event::Signal(1));
        task2.block(Event::Signal(2));
        
        // 保存当前任务状态
        let current_state = Scheduler::get_current_task().get_state();
        
        // 尝试调度 - 此时应该没有可调度任务
        Scheduler::task_switch();
        
        // 当前任务状态应该保持不变
        assert_eq!(Scheduler::get_current_task().get_state(), current_state);
    }
    
    #[test]
    fn test_schedule_after_unblock() {
        kernel_init();
        
        let mut task1 = Task::new("unblock_test1", |_| {});
        let mut task2 = Task::new("unblock_test2", |_| {});
        
        Scheduler::start();
        
        // 阻塞当前任务
        task1.block(Event::Signal(1));
        
        // 调度到下一个任务
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_taskid(), task2.get_taskid());
        
        // 唤醒被阻塞的任务
        task1.ready();
        
        // 再次调度
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_taskid(), task1.get_taskid());
    }
    
    #[test]
    fn test_start_stop_restart() {
        kernel_init();
        Task::new("restart_test", |_| {});
        
        // 启动调度器
        Scheduler::start();
        assert!(unsafe { SCHEDULER.is_running });
        
        // 停止调度器
        Scheduler::stop();
        assert!(!unsafe { SCHEDULER.is_running });
        
        // 重新启动调度器
        Scheduler::start();
        assert!(unsafe { SCHEDULER.is_running });
    }
}
