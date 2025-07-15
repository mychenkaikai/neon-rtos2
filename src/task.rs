use crate::arch::init_task_stack;
use crate::config::MAX_TASKS;
use crate::config::STACK_SIZE;
use crate::event::Event;
use core::cmp::PartialEq;
use core::fmt::Debug;
use core::panic;
use core::prelude::rust_2024::*;
use core::ptr::addr_of;

// 在lib.rs或main.rs中

static mut TASK_LIST: [TCB; MAX_TASKS] = [TCB {
    stack_top: 0,
    name: "noinit",
    taskid: 0,
    state: TaskState::Uninit,
}; MAX_TASKS];

#[unsafe(no_mangle)]
static mut TASK_STACKS: [Stack; MAX_TASKS] = [const {
    Stack {
        data: [0; STACK_SIZE],
    }
}; MAX_TASKS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TaskState {
    Uninit,
    Ready,
    Running,
    Blocked(Event),
}

#[repr(C)]
#[repr(align(8))]
pub struct Stack {
    pub data: [u8; STACK_SIZE],
}

#[repr(C)]
#[derive(Clone, PartialEq, Copy)]
pub struct TCB {
    // 任务控制块的字段
    pub(crate) stack_top: usize,
    pub(crate) name: &'static str,
    pub(crate) taskid: usize,
    pub(crate) state: TaskState,
}

#[derive(Clone, PartialEq, Copy)]
pub struct Task(pub usize);

impl TCB {
    pub fn init(&mut self, name: &'static str, func: fn(usize), taskid: usize, stack_top: usize) {
        self.stack_top = stack_top;
        self.taskid = taskid;

        self.name = name;
        self.state = TaskState::Ready;

        init_task_stack(&mut self.stack_top, func, taskid);
    }
}

impl Task {
    pub fn new(name: &'static str, func: fn(usize)) -> Self {
        unsafe {
            for i in 0..MAX_TASKS {
                if TASK_LIST[i].state == TaskState::Uninit {
                    TASK_LIST[i].init(
                        name,
                        func,
                        i,
                        addr_of!(TASK_STACKS[i].data) as usize + STACK_SIZE,
                    );
                    return Task(i);
                }
            }
            panic!("No free task slot");
        }
    }

    pub fn run(&mut self) {
        unsafe {
            TASK_LIST[self.0].state = TaskState::Running;
        }
    }
    pub fn ready(&mut self) {
        unsafe {
            TASK_LIST[self.0].state = TaskState::Ready;
        }
    }

    pub fn block(&mut self, reason: Event) {
        unsafe {
            TASK_LIST[self.0].state = TaskState::Blocked(reason);
        }
    }

    pub fn get_state(&self) -> TaskState {
        unsafe { TASK_LIST[self.0].state }
    }

    pub fn get_name(&self) -> &'static str {
        unsafe { TASK_LIST[self.0].name }
    }

    pub fn get_taskid(&self) -> usize {
        unsafe { TASK_LIST[self.0].taskid }
    }

    pub fn get_stack_top(&self) -> usize {
        unsafe { TASK_LIST[self.0].stack_top }
    }

    pub fn set_stack_top(&mut self, stack_top: usize) {
        unsafe {
            TASK_LIST[self.0].stack_top = stack_top;
        }
    }

    pub(crate) fn init() {
        unsafe {
            for i in 0..MAX_TASKS {
                TASK_LIST[i] = TCB {
                    stack_top: 0,
                    name: "noinit",
                    taskid: 0,
                    state: TaskState::Uninit,
                };
                TASK_STACKS[i] = Stack {
                    data: [0; STACK_SIZE],
                };
            }
        }
    }

    /// 遍历所有初始化的任务，对每个任务执行函数f,遍历的时候显示当前id
    pub fn for_each<F>(mut f: F)
    where
        F: FnMut(&mut Task, usize) -> (),
    {
        unsafe {
            for i in 0..MAX_TASKS {
                if TASK_LIST[i].state != TaskState::Uninit {
                    f(&mut Task(i), i);
                }
            }
        }
    }

    //从给定id开始循环遍历所有任务到id本身,如果id是最后一个任务,则从0开始
    pub fn for_each_from<F>(start: usize, mut f: F)
    where
        F: FnMut(&mut Task, usize) -> (),
    {
        unsafe {
            for i in start..MAX_TASKS {
                if TASK_LIST[i].state != TaskState::Uninit {
                    f(&mut Task(i), i);
                }
            }
            for i in 0..start {
                if TASK_LIST[i].state != TaskState::Uninit {
                    f(&mut Task(i), i);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;

    fn task1(_args: usize) {
        // 简化的任务函数
    }

    fn task2(_args: usize) {
        // 简化的任务函数
    }

    #[test]
    fn test_task() {
        kernel_init();
        let task1 = Task::new("task1", task1);
        let task2 = Task::new("task2", task2);
        assert_eq!(task1.get_state(), TaskState::Ready);
        assert_eq!(task2.get_state(), TaskState::Ready);
        assert_eq!(task1.get_name(), "task1");
        assert_eq!(task2.get_name(), "task2");
        assert_eq!(task1.get_taskid(), 0);
        assert_eq!(task2.get_taskid(), 1);
        //检测栈顶是否8字节对齐
        assert_eq!(task1.get_stack_top() & !(0x0007), task1.get_stack_top());
        assert_eq!(task2.get_stack_top() & !(0x0007), task2.get_stack_top());
    }

    //检测任务数超过MAX_TASKS时，是否panic
    #[test]
    #[should_panic]
    fn test_task_overflow() {
        kernel_init();
        for _ in 0..MAX_TASKS + 1 {
            Task::new("task", task1);
        }
    }

    //检测任务状态
    #[test]
    fn test_task_state() {
        kernel_init();
        let mut task = Task::new("task", task1);
        task.run();
        assert_eq!(task.get_state(), TaskState::Running);
        task.block(Event::Signal(1));
        assert_eq!(task.get_state(), TaskState::Blocked(Event::Signal(1)));
        task.ready();
        assert_eq!(task.get_state(), TaskState::Ready);
    }

    #[test]
    fn test_task_for_each_from() {
        kernel_init();
        //使用一个cnt来记录遍历的次数，cnt为0的时候，应该是task1，cnt为1的时候，应该是task2
        let mut cnt = 0;
        Task::new("task1", task1);
        Task::new("task2", task2);
        Task::for_each_from(0, |task, id| {
            if cnt == 0 {
                assert_eq!(task.get_name(), "task1");
                assert_eq!(id, 0);
                cnt += 1;
            } else if cnt == 1 {
                assert_eq!(task.get_name(), "task2");
                assert_eq!(id, 1);
                cnt += 1;
            }
        });
        cnt = 0;
        Task::for_each_from(1, |task, id| {
            if cnt == 0 {
                assert_eq!(task.get_name(), "task2");
                assert_eq!(id, 1);
                cnt += 1;
            } else if cnt == 1 {
                assert_eq!(task.get_name(), "task1");
                assert_eq!(id, 0);
                cnt += 1;
            }
        });
    }

    #[test]
    fn test_task_for_each() {
        //使用一个cnt来记录遍历的次数，cnt为0的时候，应该是task1，cnt为1的时候，应该是task2
        let mut cnt = 0;
        kernel_init();
        Task::new("task1", task1);
        Task::new("task2", task2);
        Task::for_each(|task, id| {
            if cnt == 0 {
                assert_eq!(task.get_name(), "task1");
                assert_eq!(id, 0);
                cnt += 1;
            } else if cnt == 1 {
                assert_eq!(task.get_name(), "task2");
                assert_eq!(id, 1);
                cnt += 1;
            }
        });
    }
}
