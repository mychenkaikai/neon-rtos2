

use crate::config::STACK_SIZE;
use crate::arch::init_task_stack;

pub(crate) enum TaskState {
    Ready,
    Running,
    Blocked(BlockReason),
}

#[repr(C)]
pub struct TCB {
    // 任务控制块的字段
    pub(crate) stack_top: usize,
    pub(crate) name: &'static str,
    pub(crate) stack: [u8; STACK_SIZE],
    pub(crate) state: TaskState,
}

pub struct Task {
    tcb: TCB,
    
}

impl Task {
    pub fn new(name: &'static str, func: fn(usize)) -> Self {
        let stack = [0; STACK_SIZE];

        let tcb = TCB {
            stack_top: stack.len() as usize,
            name: name, 
            stack: stack,
            state: TaskState::Ready,
        };

        init_task_stack(&mut tcb.stack_top, func, 0);

        Self { tcb }
    }

    pub fn run(&self) {
        self.tcb.state = TaskState::Running;
    }

    fn block(&self, reason: BlockReason) {
        self.tcb.state = TaskState::Blocked(reason);
    }
    
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task1(args: usize) {
        println!("task1");
    }

    fn task2(args: usize) {
        println!("task2");
    }

    #[test]
    fn test_task() {

        let tasks=[
            Task::new("task1", task1),
            Task::new("task2", task2),
        ];

        for task in tasks {
            task.run();
        }

        assert_eq!(tasks[0].tcb.state, TaskState::Running);
        assert_eq!(tasks[1].tcb.state, TaskState::Ready);
    }

    #[test]
    fn test_task_block() {
        let task = Task::new("task1", task1);
        task.block(BlockReason::Signal);
        assert_eq!(task.tcb.state, TaskState::Blocked(BlockReason::Signal));
    }

    #[test]
    fn test_task_unblock() {
        let task = Task::new("task1", task1);
        task.unblock(BlockReason::Signal);
        assert_eq!(task.tcb.state, TaskState::Running);
    }

    #[test]
    fn test_task_stack() {
        let task = Task::new("task1", task1);
        //stack_top应该8字节对齐
        assert_eq!(task.tcb.stack_top & (!(0x0007)), task.tcb.stack_top);
        assert_eq!(task.tcb.stack.len(), STACK_SIZE);
    }
}

