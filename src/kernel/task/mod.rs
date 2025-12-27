use crate::hal::init_task_stack;
use crate::config::MAX_TASKS;
use crate::config::STACK_SIZE;
use crate::sync::event::Event;
use crate::error::{Result, RtosError};
use crate::compat::Box;
use core::cmp::PartialEq;
use core::fmt::Debug;
use core::prelude::rust_2024::*;
use core::ptr::addr_of;

use spin::{Once, RwLock};

// 子模块
pub mod priority;
pub mod builder;
pub mod state;

// 重新导出
pub use priority::Priority;
pub use builder::TaskBuilder;
pub use state::{TypedTask, TypedTaskBuilder, TaskStateMarker, Created, Ready, Running, Blocked};

// 在lib.rs或main.rs中

static TASK_LIST: Once<RwLock<[TaskControlBlock; MAX_TASKS]>> = Once::new();

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

pub trait TaskFunction: Send + 'static + Sync {
    fn call(self: Box<Self>, task_id: usize);
}

// 为闭包实现TaskFunction
impl<F> TaskFunction for F
where
    F: FnOnce(usize) + Send + 'static + Sync,
{
    fn call(self: Box<Self>, task_id: usize) {
        (*self)(task_id);
    }
}

#[repr(C)]
pub struct TaskControlBlock {
    pub(crate) stack_top: usize,
    pub(crate) name: &'static str,
    pub(crate) taskid: usize,
    pub(crate) state: TaskState,
    pub(crate) priority: Priority,
    pub(crate) task_fn: Option<Box<dyn TaskFunction>>,
}

#[derive(Clone, PartialEq, Copy, Debug)]
pub struct Task(pub usize);

fn get_task_list() -> &'static RwLock<[TaskControlBlock; MAX_TASKS]> {
    TASK_LIST.call_once(|| RwLock::new([(); MAX_TASKS].map(|_| TaskControlBlock::default())))
}

// 关键：统一的包装器函数，有固定的入口地址
fn task_wrapper_entry(task_id: usize) {
    let mut task_list = get_task_list().write();
    if let Some(task_fn) = task_list[task_id].task_fn.take() {
        drop(task_list); // 提前释放写锁
        task_fn.call(task_id);
    }
}

impl TaskControlBlock {
    fn default() -> Self {
        Self {
            stack_top: 0,
            name: "noinit",
            taskid: 0,
            state: TaskState::Uninit,
            priority: Priority::Normal,
            task_fn: None,
        }
    }
    pub fn init_unified<F>(&mut self, name: &'static str, func: F, taskid: usize, stack_top: usize)
    where
        F: TaskFunction,
    {
        self.stack_top = stack_top;
        self.taskid = taskid;
        self.name = name;
        self.state = TaskState::Ready;
        self.task_fn = Some(Box::new(func));

        // 关键：汇编层面始终使用统一的包装器函数地址
        init_task_stack(&mut self.stack_top, task_wrapper_entry, taskid);
    }

    // pub fn init(&mut self, name: &'static str, func: fn(usize), taskid: usize, stack_top: usize) {
    //     self.stack_top = stack_top;
    //     self.taskid = taskid;

    //     self.name = name;
    //     self.state = TaskState::Ready;

    //     init_task_stack(&mut self.stack_top, func, taskid);
    // }
}

impl Task {
    pub fn new<F>(name: &'static str, func: F) -> Result<Self>
    where
        F: TaskFunction,
    {
        let mut task_list = get_task_list().write();
        for i in 0..MAX_TASKS {
            if task_list[i].state == TaskState::Uninit {
                // SAFETY: TASK_STACKS 是静态数组，我们通过 task_list 的写锁保证了
                // 同一时间只有一个任务在初始化特定的栈槽位
                let stack_top = unsafe { addr_of!(TASK_STACKS[i].data) as usize + STACK_SIZE };
                task_list[i].init_unified(name, func, i, stack_top);
                return Ok(Task(i));
            }
        }
        Err(RtosError::TaskSlotsFull)
    }

    pub fn run(&mut self) {
        get_task_list().write()[self.0].state = TaskState::Running;
    }

    pub fn ready(&mut self) {
        get_task_list().write()[self.0].state = TaskState::Ready;
    }

    pub fn block(&mut self, reason: Event) {
        get_task_list().write()[self.0].state = TaskState::Blocked(reason);
    }

    pub fn get_state(&self) -> TaskState {
        get_task_list().read()[self.0].state
    }

    pub fn get_name(&self) -> &'static str {
        get_task_list().read()[self.0].name
    }

    pub fn get_taskid(&self) -> usize {
        get_task_list().read()[self.0].taskid
    }

    pub fn get_stack_top(&self) -> usize {
        get_task_list().read()[self.0].stack_top
    }

    pub fn set_stack_top(&mut self, stack_top: usize) {
        get_task_list().write()[self.0].stack_top = stack_top;
    }

    /// 获取任务优先级
    ///
    /// # 返回值
    /// 任务的当前优先级
    pub fn get_priority(&self) -> Priority {
        get_task_list().read()[self.0].priority
    }

    /// 设置任务优先级
    ///
    /// # 参数
    /// - `priority`: 新的优先级
    pub fn set_priority(&mut self, priority: Priority) {
        get_task_list().write()[self.0].priority = priority;
    }

    pub(crate) fn init() {
        for i in 0..MAX_TASKS {
            get_task_list().write()[i] = TaskControlBlock::default();
        }
        // TASK_STACKS 是静态数组，需要 unsafe 访问
        // 这是必要的 unsafe，因为我们需要重置静态可变数组
        unsafe {
            for i in 0..MAX_TASKS {
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
        for i in 0..MAX_TASKS {
            if get_task_list().read()[i].state != TaskState::Uninit {
                f(&mut Task(i), i);
            }
        }
    }

    //从给定id开始循环遍历所有任务到id本身,如果id是最后一个任务,则从0开始
    pub fn for_each_from<F>(start: usize, mut f: F)
    where
        F: FnMut(&mut Task, usize) -> (),
    {
        for i in start..MAX_TASKS {
            if get_task_list().read()[i].state != TaskState::Uninit {
                f(&mut Task(i), i);
            }
        }
        for i in 0..start {
            if get_task_list().read()[i].state != TaskState::Uninit {
                f(&mut Task(i), i);
            }
        }
    }

    /// 返回所有已初始化任务的迭代器
    ///
    /// # 示例
    /// ```rust
    /// use neon_rtos2::kernel::task::{Task, TaskState};
    ///
    /// // 统计就绪任务数量
    /// let ready_count = Task::iter()
    ///     .filter(|t| t.get_state() == TaskState::Ready)
    ///     .count();
    ///
    /// // 遍历所有任务
    /// Task::iter()
    ///     .for_each(|t| println!("Task: {}", t.get_name()));
    /// ```
    pub fn iter() -> TaskIter {
        TaskIter::new()
    }

    /// 返回所有就绪任务的迭代器
    pub fn ready_tasks() -> impl Iterator<Item = Task> {
        Self::iter().filter(|t| t.get_state() == TaskState::Ready)
    }

    /// 返回所有阻塞任务的迭代器
    pub fn blocked_tasks() -> impl Iterator<Item = Task> {
        Self::iter().filter(|t| matches!(t.get_state(), TaskState::Blocked(_)))
    }

    /// 创建任务构建器
    ///
    /// # 参数
    /// - `name`: 任务名称
    ///
    /// # 示例
    /// ```rust
    /// use neon_rtos2::kernel::task::{Task, Priority};
    ///
    /// let task = Task::builder("my_task")
    ///     .priority(Priority::High)
    ///     .spawn(|_| { /* ... */ });
    /// ```
    pub fn builder(name: &'static str) -> TaskBuilder {
        TaskBuilder::new(name)
    }

    /// 尝试转换为类型安全的任务句柄
    ///
    /// 根据任务当前状态返回对应的 `TypedTask` 变体。
    /// 这是从普通 `Task` 转换到类型状态系统的推荐方式。
    ///
    /// # 返回值
    ///
    /// - `Ok(TypedTaskAny)`: 成功转换，包含对应状态的 `TypedTask`
    /// - `Err(RtosError::InvalidTaskState)`: 任务处于未初始化状态
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// use neon_rtos2::kernel::task::state::TypedTaskAny;
    ///
    /// let task = Task::new("my_task", |_| {})?;
    ///
    /// match task.into_typed()? {
    ///     TypedTaskAny::Ready(ready_task) => {
    ///         println!("Task is ready");
    ///         let running_task = ready_task.run();
    ///     }
    ///     TypedTaskAny::Running(running_task) => {
    ///         println!("Task is running");
    ///     }
    ///     TypedTaskAny::Blocked(blocked_task) => {
    ///         println!("Task is blocked on: {:?}", blocked_task.blocked_event());
    ///     }
    ///     TypedTaskAny::Created(created_task) => {
    ///         println!("Task is created");
    ///     }
    /// }
    /// ```
    pub fn into_typed(self) -> Result<state::TypedTaskAny> {
        use state::{TypedTask, TypedTaskAny, Created, Ready, Running, Blocked};
        use core::marker::PhantomData;

        match self.get_state() {
            TaskState::Uninit => Err(RtosError::InvalidTaskState),
            TaskState::Ready => Ok(TypedTaskAny::Ready(TypedTask {
                inner: self,
                blocked_event: None,
                _state: PhantomData,
            })),
            TaskState::Running => Ok(TypedTaskAny::Running(TypedTask {
                inner: self,
                blocked_event: None,
                _state: PhantomData,
            })),
            TaskState::Blocked(event) => Ok(TypedTaskAny::Blocked(TypedTask {
                inner: self,
                blocked_event: Some(event),
                _state: PhantomData,
            })),
        }
    }
}

/// 任务迭代器
///
/// 遍历所有已初始化的任务
pub struct TaskIter {
    current: usize,
}

impl TaskIter {
    /// 创建新的任务迭代器
    fn new() -> Self {
        Self { current: 0 }
    }
}

impl Iterator for TaskIter {
    type Item = Task;

    fn next(&mut self) -> Option<Self::Item> {
        let task_list = get_task_list().read();
        while self.current < MAX_TASKS {
            let idx = self.current;
            self.current += 1;
            if task_list[idx].state != TaskState::Uninit {
                return Some(Task(idx));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use serial_test::serial;

    fn task1(_args: usize) {
        // 简化的任务函数
    }

    fn task2(_args: usize) {
        // 简化的任务函数
    }

    #[test]
    #[serial]
    fn test_task() {
        kernel_init();
        let task1 = Task::new("task1", task1).unwrap();
        let task2 = Task::new("task2", task2).unwrap();
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
    #[serial]
    fn test_task_overflow() {
        kernel_init();
        for _ in 0..MAX_TASKS {
            Task::new("task", task1).unwrap();
        }
        assert_eq!(Task::new("task", task1).err(), Some(RtosError::TaskSlotsFull));
    }

    //检测任务状态
    #[test]
    #[serial]
    fn test_task_state() {
        kernel_init();
        let mut task = Task::new("task", task1).unwrap();
        task.run();
        assert_eq!(task.get_state(), TaskState::Running);
        task.block(Event::Signal(1));
        assert_eq!(task.get_state(), TaskState::Blocked(Event::Signal(1)));
        task.ready();
        assert_eq!(task.get_state(), TaskState::Ready);
    }

    #[test]
    #[serial]
    fn test_task_for_each_from() {
        kernel_init();
        //使用一个cnt来记录遍历的次数，cnt为0的时候，应该是task1，cnt为1的时候，应该是task2
        let mut cnt = 0;
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
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
    #[serial]
    fn test_task_for_each() {
        //使用一个cnt来记录遍历的次数，cnt为0的时候，应该是task1，cnt为1的时候，应该是task2
        let mut cnt = 0;
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
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

    #[test]
    #[serial]
    fn test_task_state_transitions() {
        kernel_init();
        let mut task = Task::new("transition_task", |_| {}).unwrap();

        // 测试所有状态转换
        assert_eq!(task.get_state(), TaskState::Ready);

        task.run();
        assert_eq!(task.get_state(), TaskState::Running);

        task.block(Event::Signal(1));
        assert_eq!(task.get_state(), TaskState::Blocked(Event::Signal(1)));

        task.ready();
        assert_eq!(task.get_state(), TaskState::Ready);
    }

    #[test]
    #[serial]
    fn test_task_stack_manipulation() {
        kernel_init();
        let mut task = Task::new("stack_task", |_| {}).unwrap();

        let original_stack_top = task.get_stack_top();
        assert_ne!(original_stack_top, 0);

        // 测试修改栈顶
        let new_stack_top = original_stack_top + 16;
        task.set_stack_top(new_stack_top);
        assert_eq!(task.get_stack_top(), new_stack_top);
    }

    #[test]
    #[serial]
    fn test_for_each_with_no_tasks() {
        // 重新初始化任务列表，不创建任务
        Task::init();

        let mut count = 0;
        Task::for_each(|_, _| {
            count += 1;
        });

        // 应该没有任务被遍历
        assert_eq!(count, 0);
    }

    #[test]
    #[serial]
    fn test_for_each_from_with_gap() {
        kernel_init();

        // 创建几个任务，但中间有空隙
        Task::new("task_1", |_| {}).unwrap();
        // task_2位置空出来
        let task3 = Task::new("task_3", |_| {}).unwrap();

        let mut count = 0;
        let mut found_task3 = false;

        Task::for_each_from(0, |task, _| {
            count += 1;
            if task.get_taskid() == task3.get_taskid() {
                found_task3 = true;
            }
        });

        assert_eq!(count, 2); // 只应遍历两个任务
        assert!(found_task3); // 应该找到task3
    }

    #[test]
    #[serial]
    fn test_for_each_from_with_wrap_around() {
        kernel_init();

        // 创建几个任务
        let task1 = Task::new("task_1", |_| {}).unwrap();
        let task2 = Task::new("task_2", |_| {}).unwrap();
        let task3 = Task::new("task_3", |_| {}).unwrap();

        let mut count = 0;
        let mut found_task1 = false;
        let mut found_task2 = false;
        let mut found_task3 = false;
        Task::for_each_from(2, |task, _| {
            count += 1;
            if task.get_taskid() == task1.get_taskid() {
                found_task1 = true;
            }
        });
    }
}
