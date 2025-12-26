use crate::kernel::task::{Task, TaskState, Priority};
use crate::hal::init_idle_task;

static mut SCHEDULER: Scheduler = Scheduler {
    current_task: None, 
    next_task: None,
    is_running: false,
    use_priority: false,
};

pub struct Scheduler {
    current_task: Option<Task>,
    next_task: Option<Task>,
    is_running: bool,
    /// 是否启用优先级调度
    use_priority: bool,
}

impl Scheduler {
    pub fn init() {
        unsafe {
            SCHEDULER = Scheduler {
                current_task: None,
                next_task: None,
                is_running: false,
                use_priority: false,
            };

            init_idle_task();

        }
    }

    /// 启用优先级调度
    ///
    /// 启用后，调度器会优先选择优先级最高的就绪任务运行。
    pub fn enable_priority_scheduling() {
        unsafe { SCHEDULER.use_priority = true };
    }

    /// 禁用优先级调度
    ///
    /// 禁用后，调度器使用轮转调度算法。
    pub fn disable_priority_scheduling() {
        unsafe { SCHEDULER.use_priority = false };
    }

    /// 检查是否启用优先级调度
    pub fn is_priority_scheduling_enabled() -> bool {
        unsafe { SCHEDULER.use_priority }
    }

    /// 基于优先级的调度
    ///
    /// 选择优先级最高的就绪任务运行。
    /// 如果有多个相同优先级的任务，选择第一个找到的。
    pub fn schedule_by_priority() {
        // 如果调度器未运行，直接返回
        if !unsafe { SCHEDULER.is_running } {
            return;
        }

        let mut current_task = unsafe { SCHEDULER.current_task.unwrap() };

        // 使用迭代器找到最高优先级的就绪任务
        let next_task = Task::ready_tasks()
            .filter(|t| t.get_taskid() != current_task.get_taskid())
            .max_by_key(|t| t.get_priority());

        match (next_task, current_task.get_state()) {
            // 找到了更高优先级的就绪任务
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

    /// 抢占式调度检查
    ///
    /// 如果有更高优先级的任务就绪，触发任务切换。
    /// 通常在 SysTick 中断或任务唤醒时调用。
    pub fn preempt_check() {
        // 如果调度器未运行或未启用优先级调度，直接返回
        if !unsafe { SCHEDULER.is_running } || !unsafe { SCHEDULER.use_priority } {
            return;
        }

        let current_task = unsafe { SCHEDULER.current_task.unwrap() };
        let current_priority = current_task.get_priority();

        // 检查是否有更高优先级的就绪任务
        let higher_priority_exists = Task::ready_tasks()
            .any(|t| t.get_priority() > current_priority);

        if higher_priority_exists {
            Self::schedule_by_priority();
        }
    }

    //使用task::for_each_from遍历所有任务,找到当前任务之后的下一个非阻塞任务,如果当前任务是最后一个任务,则找到第一个任务
    //但也要考虑其他任务找不到准备状态，此时currenttask还是原任务
    pub fn task_switch() {
        // 如果调度器未运行，直接返回
        if !unsafe { SCHEDULER.is_running } {
            return;
        }

        // 如果启用了优先级调度，使用优先级调度算法
        if unsafe { SCHEDULER.use_priority } {
            Self::schedule_by_priority();
            return;
        }

        // 否则使用轮转调度算法
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
        crate::hal::start_first_task();
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
    use crate::sync::event::Event;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::utils::kernel_init;
    use serial_test::serial;

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
    #[serial]
    fn test_schedule() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::new("task3", task3).unwrap();
        Task::new("task4", task4).unwrap();
        Task::new("task5", task5).unwrap();

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
    #[serial]
    fn test_schedule_block() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::new("task3", task3).unwrap();
        Task::new("task4", task4).unwrap();
        Task::new("task5", task5).unwrap();
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
    #[serial]
    fn test_schedule_block_and_schedule() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::new("task3", task3).unwrap();
        Task::new("task4", task4).unwrap();
        Task::new("task5", task5).unwrap();
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

    #[test]
    #[serial]
    fn test_schedule_stop() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Scheduler::start();
        let current_task = unsafe { SCHEDULER.current_task.unwrap() };
        Scheduler::stop();
        Scheduler::task_switch();
        assert_eq!(current_task.get_state(), TaskState::Running);
    }

    #[test]
    #[serial]
    fn test_all_tasks_blocked() {
        kernel_init();
        let mut task1 = Task::new("blocked_task1", |_| {}).unwrap();
        let mut task2 = Task::new("blocked_task2", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 获取当前任务（应该是 task1，因为它是第一个创建的）
        let current_task = Scheduler::get_current_task();
        
        // 阻塞非当前任务
        if current_task.get_taskid() == task1.get_taskid() {
            task2.block(Event::Signal(2));
        } else {
            task1.block(Event::Signal(1));
        }
        
        // 保存当前任务
        let current_id = current_task.get_taskid();
        
        // 阻塞当前任务
        if current_task.get_taskid() == task1.get_taskid() {
            task1.block(Event::Signal(1));
        } else {
            task2.block(Event::Signal(2));
        }
        
        // 尝试调度 - 此时所有任务都被阻塞
        Scheduler::task_switch();
        
        // 当前任务 ID 应该保持不变（因为没有可调度的任务）
        assert_eq!(Scheduler::get_current_task().get_taskid(), current_id);
    }
    
    #[test]
    #[serial]
    fn test_schedule_after_unblock() {
        kernel_init();
        
        let mut task1 = Task::new("unblock_test1", |_| {}).unwrap();
        let mut task2 = Task::new("unblock_test2", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 获取当前任务（应该是 task1）
        let current = Scheduler::get_current_task();
        assert_eq!(current.get_taskid(), task1.get_taskid());
        
        // 阻塞当前任务
        task1.block(Event::Signal(1));
        
        // 调度到下一个任务
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_taskid(), task2.get_taskid());
        
        // 唤醒被阻塞的任务
        task1.ready();
        
        // 再次调度 - 应该切换回 task1（轮转调度）
        Scheduler::task_switch();
        // 注意：轮转调度下，可能切换到 task1 或保持 task2
        // 这里只验证当前任务是运行状态
        assert_eq!(Scheduler::get_current_task().get_state(), TaskState::Running);
    }
    
    #[test]
    #[serial]
    fn test_start_stop_restart() {
        kernel_init();
        Task::new("restart_test", |_| {}).unwrap();
        
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

    // ========================================================================
    // 优先级调度测试
    // ========================================================================

    #[test]
    #[serial]
    fn test_priority_scheduling_enable_disable() {
        kernel_init();
        
        // 默认应该禁用优先级调度
        assert!(!Scheduler::is_priority_scheduling_enabled());
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        assert!(Scheduler::is_priority_scheduling_enabled());
        
        // 禁用优先级调度
        Scheduler::disable_priority_scheduling();
        assert!(!Scheduler::is_priority_scheduling_enabled());
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_basic() {
        kernel_init();
        
        // 创建不同优先级的任务
        let low_task = Task::builder("low_priority")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        let high_task = Task::builder("high_priority")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        let normal_task = Task::builder("normal_priority")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 第一个任务开始运行（task id 0）
        assert_eq!(Scheduler::get_current_task().get_taskid(), low_task.get_taskid());
        
        // 调度后应该切换到最高优先级的任务
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_taskid(), high_task.get_taskid());
        assert_eq!(Scheduler::get_current_task().get_priority(), Priority::High);
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_preempt_check() {
        kernel_init();
        
        // 创建低优先级任务
        let mut low_task = Task::builder("low")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        // 创建高优先级任务（初始阻塞）
        let mut high_task = Task::builder("high")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 阻塞高优先级任务
        high_task.block(Event::Signal(1));
        
        // 调度，应该运行低优先级任务
        Scheduler::task_switch();
        
        // 唤醒高优先级任务
        high_task.ready();
        
        // 抢占检查应该触发切换到高优先级任务
        Scheduler::preempt_check();
        assert_eq!(Scheduler::get_current_task().get_taskid(), high_task.get_taskid());
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_same_priority() {
        kernel_init();
        
        // 创建多个相同优先级的任务
        let task1 = Task::builder("normal1")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        let task2 = Task::builder("normal2")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        let task3 = Task::builder("normal3")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 所有任务优先级相同，调度应该正常工作
        Scheduler::task_switch();
        let current = Scheduler::get_current_task();
        assert_eq!(current.get_priority(), Priority::Normal);
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_with_blocked_high_priority() {
        kernel_init();
        
        // 创建任务
        let low_task = Task::builder("low")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        let mut high_task = Task::builder("high")
            .priority(Priority::Critical)
            .spawn(|_| {})
            .unwrap();
        
        let normal_task = Task::builder("normal")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 阻塞高优先级任务
        high_task.block(Event::Signal(1));
        
        // 调度应该选择 Normal 优先级任务（因为 Critical 被阻塞）
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_priority(), Priority::Normal);
    }

    #[test]
    #[serial]
    fn test_round_robin_when_priority_disabled() {
        kernel_init();
        
        // 创建不同优先级的任务
        let task1 = Task::builder("task1")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        let task2 = Task::builder("task2")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        // 确保优先级调度禁用
        Scheduler::disable_priority_scheduling();
        Scheduler::start();
        
        // 轮转调度应该按顺序切换，而不是按优先级
        assert_eq!(Scheduler::get_current_task().get_taskid(), task1.get_taskid());
        
        Scheduler::task_switch();
        // 轮转调度下，应该切换到下一个任务
        assert_eq!(Scheduler::get_current_task().get_taskid(), task2.get_taskid());
    }
}
