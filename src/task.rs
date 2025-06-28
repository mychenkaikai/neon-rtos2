use crate::arch::init_task_stack;
use crate::config::MAX_TASKS;
use crate::config::STACK_SIZE;

use core::cmp::PartialEq;
use core::fmt::Debug;
use core::panic;
use core::prelude::rust_2024::*;
use core::ptr::addr_of;

// 在lib.rs或main.rs中
#[unsafe(no_mangle)]
pub(crate) static mut TASK_LIST: [TCB; MAX_TASKS] = [TCB {
    stack_top: 0,
    name: "noinit",
    stackid: 0,
    state: TaskState::Uninit,
}; MAX_TASKS];

#[unsafe(no_mangle)]
static mut TASK_STACKS: [Stack; MAX_TASKS] = [const {
    Stack {
        data: [0; STACK_SIZE],
    }
}; MAX_TASKS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum BlockReason {
    Signal,
    Wait,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum TaskState {
    Uninit,
    Ready,
    Running,
    Blocked(BlockReason),
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
    pub(crate) stackid: usize,
    pub(crate) state: TaskState,
}

#[derive(Clone, PartialEq, Copy)]
pub struct Task(pub usize);

impl TCB {
    pub fn init(&mut self, name: &'static str, func: fn(usize), stackid: usize, stack_top: usize) {
        self.stack_top = stack_top;
        self.stackid = stackid;

        self.name = name;
        self.state = TaskState::Ready;

        init_task_stack(&mut self.stack_top, func, stackid);
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

    pub fn block(&mut self, reason: BlockReason) {
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

    pub fn get_stackid(&self) -> usize {
        unsafe { TASK_LIST[self.0].stackid }
    }

    pub fn get_stack_top(&self) -> usize {
        unsafe { TASK_LIST[self.0].stack_top }
    }

    pub(crate) fn reset_tasks() {
        unsafe {
            for i in 0..MAX_TASKS {
                TASK_LIST[i] = TCB {
                    stack_top: 0,
                    name: "noinit",
                    stackid: 0,
                    state: TaskState::Uninit,
                };
                TASK_STACKS[i] = Stack {
                    data: [0; STACK_SIZE],
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::assert_eq;

    fn task1(_args: usize) {
        // 简化的任务函数
    }

    fn task2(_args: usize) {
        // 简化的任务函数
    }

    #[test]
    fn test_task() {
        Task::reset_tasks();
        let task1 = Task::new("task1", task1);
        let task2 = Task::new("task2", task2);
        assert_eq!(task1.get_state(), TaskState::Ready);
        assert_eq!(task2.get_state(), TaskState::Ready);
        assert_eq!(task1.get_name(), "task1");
        assert_eq!(task2.get_name(), "task2");
        assert_eq!(task1.get_stackid(), 0);
        assert_eq!(task2.get_stackid(), 1);
        //检测栈顶是否8字节对齐
        assert_eq!(task1.get_stack_top() & !(0x0007), task1.get_stack_top());
        assert_eq!(task2.get_stack_top() & !(0x0007), task2.get_stack_top());
    }

    //检测任务数超过MAX_TASKS时，是否panic
    #[test]
    #[should_panic]
    fn test_task_overflow() {
        Task::reset_tasks();
        for _ in 0..MAX_TASKS + 1 {
            Task::new("task", task1);
        }
    }

    //检测任务状态
    #[test]
    fn test_task_state() {
        Task::reset_tasks();
        let mut task = Task::new("task", task1);
        task.run();
        assert_eq!(task.get_state(), TaskState::Running);
        task.block(BlockReason::Signal);
        assert_eq!(task.get_state(), TaskState::Blocked(BlockReason::Signal));
        task.ready();
        assert_eq!(task.get_state(), TaskState::Ready);
    }
}
