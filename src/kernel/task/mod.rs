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
use core::sync::atomic::{AtomicUsize, AtomicU8, Ordering};

use spin::{Once, Mutex};

// 子模块
pub mod priority;
pub mod builder;
pub mod state;

// 重新导出
pub use priority::Priority;
pub use builder::TaskBuilder;
pub use state::{TypedTask, TypedTaskBuilder, TaskStateMarker, Created, Ready, Running, Blocked};

// ============================================================================
// 任务状态编码（用于原子操作）
// ============================================================================

/// 任务状态的原子编码
/// 
/// 使用 u8 编码状态，支持原子操作：
/// - 0: Uninit
/// - 1: Ready
/// - 2: Running
/// - 3+: Blocked (高位存储事件类型，需要额外存储事件 ID)
const STATE_UNINIT: u8 = 0;
const STATE_READY: u8 = 1;
const STATE_RUNNING: u8 = 2;
const STATE_BLOCKED: u8 = 3;

// ============================================================================
// 全局任务列表（优化后的细粒度锁版本）
// ============================================================================

/// 任务列表 - 每个 TCB 独立锁
/// 
/// ## 优化说明
/// 
/// 原实现使用全局 `RwLock<[TaskControlBlock; MAX_TASKS]>`，
/// 每次访问任何任务都需要获取全局锁，导致严重的锁竞争。
/// 
/// 新实现将 TCB 分为两部分：
/// 1. **不变字段**（创建后不变）：直接存储，无需锁
/// 2. **可变字段**：使用原子操作或细粒度锁
/// 
/// 这样可以：
/// - 读取不变字段（name, taskid）：无锁，O(1)
/// - 读取可变字段（state, priority, stack_top）：原子操作，O(1)
/// - ��改可变字段：原子操作或细粒度锁，O(1)
/// - 任务创建：只需获取分配锁，不影响其他任务的访问
static TASK_LIST: Once<[TaskControlBlock; MAX_TASKS]> = Once::new();

/// 任务分配锁 - 仅用于任务创建时的槽位分配
static TASK_ALLOC_LOCK: Once<Mutex<()>> = Once::new();

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

impl TaskState {
    /// 将状态编码为 u8（用于原子操作）
    #[inline]
    fn to_u8(&self) -> u8 {
        match self {
            TaskState::Uninit => STATE_UNINIT,
            TaskState::Ready => STATE_READY,
            TaskState::Running => STATE_RUNNING,
            TaskState::Blocked(_) => STATE_BLOCKED,
        }
    }
    
    /// 从 u8 解码状态（不包含 Blocked 的事件信息）
    #[inline]
    fn from_u8_simple(value: u8) -> Self {
        match value {
            STATE_UNINIT => TaskState::Uninit,
            STATE_READY => TaskState::Ready,
            STATE_RUNNING => TaskState::Running,
            STATE_BLOCKED => TaskState::Blocked(Event::None),
            _ => TaskState::Uninit,
        }
    }
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

/// 任务控制块 - 优化后的细粒度锁版本
/// 
/// ## 内存布局优化
/// 
/// 字段按访问频率和可变性排列：
/// 1. **热数据**（调度器频繁访问）放在前面
/// 2. **原子字段**（可变但无需锁）
/// 3. **冷数据**（不常访问）放在后面
/// 
/// ## 锁策略
/// 
/// - `stack_top`: 原子操作（上下文切换时频繁访问）
/// - `state`: 原子操作 + 事件锁（状态切换频繁）
/// - `priority`: 原子操作（优先级调度时访问）
/// - `name`, `taskid`: 不变字段，无需锁
/// - `task_fn`: 细粒度锁（仅任务启动时访问一次）
#[repr(C)]
pub struct TaskControlBlock {
    // ========== 热数据（调度器频繁访问）==========
    /// 栈顶指针 - 上下文切换必须是第一个字段
    /// 使用原子操作，避免锁竞争
    pub(crate) stack_top: AtomicUsize,
    
    /// 任务状态（原子编码）
    /// 使用 u8 编码：0=Uninit, 1=Ready, 2=Running, 3=Blocked
    pub(crate) state_atomic: AtomicU8,
    
    /// 任务优先级（原子操作）
    pub(crate) priority_atomic: AtomicU8,
    
    // ========== 阻塞事件（需要锁保护）==========
    /// 阻塞事件 - 仅在 Blocked 状态时有效
    /// 使用细粒度锁，因为事件信息无法用原子操作存储
    pub(crate) blocked_event: Mutex<Option<Event>>,
    
    // ========== 冷数据（不常访问）==========
    /// 任务名称 - 创建后不变
    pub(crate) name: &'static str,
    
    /// 任务 ID - 创建后不变
    pub(crate) taskid: usize,
    
    /// 任务函数 - 仅启动时访问一次
    /// 使用细粒度锁
    pub(crate) task_fn: Mutex<Option<Box<dyn TaskFunction>>>,
}

#[derive(Clone, PartialEq, Copy, Debug)]
pub struct Task(pub usize);

/// 获取任务列表（优化后的版本）
/// 
/// 返回静态任务列表的引用，每个 TCB 内部使用原子操作和细粒度锁
fn get_task_list() -> &'static [TaskControlBlock; MAX_TASKS] {
    TASK_LIST.call_once(|| {
        [(); MAX_TASKS].map(|_| TaskControlBlock::default())
    })
}

/// 获取任务分配锁
fn get_alloc_lock() -> &'static Mutex<()> {
    TASK_ALLOC_LOCK.call_once(|| Mutex::new(()))
}

// 关键：统一的包装器函数，���固定的入口地址
fn task_wrapper_entry(task_id: usize) {
    let task_list = get_task_list();
    // 使用细粒度锁获取任务函数
    let task_fn = task_list[task_id].task_fn.lock().take();
    if let Some(func) = task_fn {
        func.call(task_id);
    }
}

impl TaskControlBlock {
    /// 创建默认的 TCB（未初始化状态）
    fn default() -> Self {
        Self {
            stack_top: AtomicUsize::new(0),
            state_atomic: AtomicU8::new(STATE_UNINIT),
            priority_atomic: AtomicU8::new(Priority::Normal.as_u8()),
            blocked_event: Mutex::new(None),
            name: "noinit",
            taskid: 0,
            task_fn: Mutex::new(None),
        }
    }
    
    /// 初始化任务（内部使用）
    /// 
    /// 注意：此方法假设调用者已经持有分配锁
    fn init_unified<F>(&self, name: &'static str, func: F, taskid: usize, stack_top: usize)
    where
        F: TaskFunction,
    {
        // 设置栈顶（需要可变引用用于 init_task_stack）
        self.stack_top.store(stack_top, Ordering::Release);
        
        // 设置不变字段（通过 unsafe 因为我们需要修改 &self）
        // SAFETY: 我们持有分配锁，保证没有其他线程在访问这个 TCB
        unsafe {
            let self_mut = self as *const Self as *mut Self;
            (*self_mut).taskid = taskid;
            (*self_mut).name = name;
        }
        
        // 设置任务函数
        *self.task_fn.lock() = Some(Box::new(func));
        
        // 初始化栈（需要获取栈顶的可变引用）
        let mut stack_top_val = self.stack_top.load(Ordering::Acquire);
        init_task_stack(&mut stack_top_val, task_wrapper_entry, taskid);
        self.stack_top.store(stack_top_val, Ordering::Release);
        
        // 最后设置状态为 Ready（使用 Release 保证之前的写入对其他线程可见）
        self.state_atomic.store(STATE_READY, Ordering::Release);
    }
    
    /// 重置 TCB 到未初始化状态
    fn reset(&self) {
        self.state_atomic.store(STATE_UNINIT, Ordering::Release);
        self.stack_top.store(0, Ordering::Release);
        self.priority_atomic.store(Priority::Normal.as_u8(), Ordering::Release);
        *self.blocked_event.lock() = None;
        *self.task_fn.lock() = None;
    }
    
    // ========== 原子访问方法 ==========
    
    /// 获取状态（原子操作）- O(1)，无锁
    #[inline]
    fn get_state(&self) -> TaskState {
        let state_code = self.state_atomic.load(Ordering::Acquire);
        if state_code == STATE_BLOCKED {
            // 需要获取事件信息
            let event = self.blocked_event.lock().unwrap_or(Event::None);
            TaskState::Blocked(event)
        } else {
            TaskState::from_u8_simple(state_code)
        }
    }
    
    /// 设置状态为 Ready（原子操作）- O(1)
    #[inline]
    fn set_ready(&self) {
        *self.blocked_event.lock() = None;
        self.state_atomic.store(STATE_READY, Ordering::Release);
    }
    
    /// 设置状态为 Running（原子操作）- O(1)
    #[inline]
    fn set_running(&self) {
        self.state_atomic.store(STATE_RUNNING, Ordering::Release);
    }
    
    /// 设置状态为 Blocked（需要锁保护事件）
    #[inline]
    fn set_blocked(&self, event: Event) {
        *self.blocked_event.lock() = Some(event);
        self.state_atomic.store(STATE_BLOCKED, Ordering::Release);
    }
    
    /// 获取优先级（原子操作）- O(1)，无锁
    #[inline]
    fn get_priority(&self) -> Priority {
        let prio = self.priority_atomic.load(Ordering::Acquire);
        Priority::from_u8(prio).unwrap_or(Priority::Normal)
    }
    
    /// 设置优先级（原子操作）- O(1)
    #[inline]
    fn set_priority(&self, priority: Priority) {
        self.priority_atomic.store(priority.as_u8(), Ordering::Release);
    }
    
    /// 获取栈顶（原子操作）- O(1)，无锁
    #[inline]
    fn get_stack_top(&self) -> usize {
        self.stack_top.load(Ordering::Acquire)
    }
    
    /// 设置栈顶（原子操作）- O(1)
    #[inline]
    fn set_stack_top(&self, value: usize) {
        self.stack_top.store(value, Ordering::Release);
    }
    
    /// 检查是否已初始化（原子操作）- O(1)，无锁
    #[inline]
    fn is_initialized(&self) -> bool {
        self.state_atomic.load(Ordering::Acquire) != STATE_UNINIT
    }
}

impl Task {
    /// 创建新任务
    /// 
    /// ## 性能优化
    /// 
    /// - 只在分配槽位时获取分配锁
    /// - 任务初始化使用原子操作和细粒度锁
    /// - 不影响其他任务的并发访问
    pub fn new<F>(name: &'static str, func: F) -> Result<Self>
    where
        F: TaskFunction,
    {
        let task_list = get_task_list();
        
        // 获取分配锁，保证槽位分配的原子性
        let _alloc_guard = get_alloc_lock().lock();
        
        // 查找空闲槽位
        for i in 0..MAX_TASKS {
            // 使用原子操作检查状态
            if task_list[i].state_atomic.load(Ordering::Acquire) == STATE_UNINIT {
                // SAFETY: TASK_STACKS 是静态数组，我们通过分配锁保证了
                // 同一时间只有一个任务在初始化特定的栈槽位
                let stack_top = unsafe { addr_of!(TASK_STACKS[i].data) as usize + STACK_SIZE };
                task_list[i].init_unified(name, func, i, stack_top);
                return Ok(Task(i));
            }
        }
        Err(RtosError::TaskSlotsFull)
    }

    /// 设置任务状态为 Running - O(1)，原子操作
    pub fn run(&mut self) {
        get_task_list()[self.0].set_running();
    }

    /// 设置任务状态为 Ready - O(1)，原子操作
    pub fn ready(&mut self) {
        get_task_list()[self.0].set_ready();
    }

    /// 设置任务状态为 Blocked - O(1)
    pub fn block(&mut self, reason: Event) {
        get_task_list()[self.0].set_blocked(reason);
    }

    /// 获取任务状态 - O(1)，原子操作（Blocked 状态需要锁）
    pub fn get_state(&self) -> TaskState {
        get_task_list()[self.0].get_state()
    }

    /// 获取任务名称 - O(1)，无锁（不变字段）
    pub fn get_name(&self) -> &'static str {
        get_task_list()[self.0].name
    }

    /// 获取任务 ID - O(1)，无锁（不变字段）
    /// 
    /// 注意：实际上 task ID 就是 self.0，这个方法保持向后兼容
    #[inline]
    pub fn get_taskid(&self) -> usize {
        self.0
    }

    /// 获取栈顶指针 - O(1)，原子操作
    pub fn get_stack_top(&self) -> usize {
        get_task_list()[self.0].get_stack_top()
    }

    /// 设置栈顶指针 - O(1)，原子操作
    pub fn set_stack_top(&mut self, stack_top: usize) {
        get_task_list()[self.0].set_stack_top(stack_top);
    }

    /// 获取任务优先级 - O(1)，原子操作
    ///
    /// # 返回值
    /// 任务的当前优先级
    pub fn get_priority(&self) -> Priority {
        get_task_list()[self.0].get_priority()
    }

    /// 设置任务优先级 - O(1)，原子操作
    ///
    /// # 参数
    /// - `priority`: 新的优先级
    pub fn set_priority(&mut self, priority: Priority) {
        get_task_list()[self.0].set_priority(priority);
    }
    
    /// 批量获取任务信息 - 减少多次访问的开销
    /// 
    /// ## 优化说明
    /// 
    /// 当需要同时获取多个字段时，使用此方法可以减少原子操作次数
    /// 
    /// # 返回值
    /// 返回 (state, priority, stack_top) 元组
    #[inline]
    pub fn get_info(&self) -> (TaskState, Priority, usize) {
        let tcb = &get_task_list()[self.0];
        (tcb.get_state(), tcb.get_priority(), tcb.get_stack_top())
    }

    /// 初始化任务系统
    pub(crate) fn init() {
        let task_list = get_task_list();
        let _alloc_guard = get_alloc_lock().lock();
        
        for i in 0..MAX_TASKS {
            task_list[i].reset();
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

    /// 遍历所有初始化的任务，对每个任务执行函数f
    /// 
    /// ## 性能优化
    /// 
    /// 使用原子操作检查状态，无需获取全局锁
    pub fn for_each<F>(mut f: F)
    where
        F: FnMut(&mut Task, usize) -> (),
    {
        let task_list = get_task_list();
        for i in 0..MAX_TASKS {
            if task_list[i].is_initialized() {
                f(&mut Task(i), i);
            }
        }
    }

    /// 从给定id开始循环遍历所有任务到id本身
    /// 
    /// ## 性能优化
    /// 
    /// 使用原子操作检查状态，无需获取全局锁
    pub fn for_each_from<F>(start: usize, mut f: F)
    where
        F: FnMut(&mut Task, usize) -> (),
    {
        let task_list = get_task_list();
        for i in start..MAX_TASKS {
            if task_list[i].is_initialized() {
                f(&mut Task(i), i);
            }
        }
        for i in 0..start {
            if task_list[i].is_initialized() {
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

    /// 返回任务快照迭代器
    /// 
    /// 与 `iter()` 不同，快照迭代器在创建时一次性获取所有任务信息，
    /// 迭代过程中不再需要获取锁，适用于需要遍历大量任务的场景。
    /// 
    /// ## 优化说明
    /// 
    /// - `iter()`: 每次 `next()` 都获取读锁，适合少量遍历
    /// - `snapshot_iter()`: 创建时获取一次锁，适合大量遍历或调度器使用
    /// 
    /// # 示例
    /// ```rust,no_run
    /// use neon_rtos2::kernel::task::{Task, TaskState};
    /// 
    /// // 使用快照迭代器统计就绪任务（无锁��历）
    /// let ready_count = Task::snapshot_iter()
    ///     .filter(|s| s.state == TaskState::Ready)
    ///     .count();
    /// 
    /// // 找到最高优先级的就绪任务
    /// if let Some(snapshot) = Task::snapshot_iter().highest_priority_ready() {
    ///     println!("Highest priority ready task: {}", snapshot.name);
    /// }
    /// ```
    pub fn snapshot_iter() -> TaskSnapshotIter {
        TaskSnapshotIter::new()
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
    /// ```rust,no_run
    /// use neon_rtos2::kernel::task::{Task, Priority};
    /// # fn main() -> Result<(), neon_rtos2::error::RtosError> {
    /// let task = Task::builder("my_task")
    ///     .priority(Priority::High)
    ///     .spawn(|_| { /* ... */ });
    /// # Ok(())
    /// # }
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
    /// ```rust,no_run
    /// # use neon_rtos2::kernel::task::{Task, TaskState};
    /// # use neon_rtos2::kernel::task::state::TypedTaskAny;
    /// # fn main() -> Result<(), neon_rtos2::error::RtosError> {
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
    /// # Ok(())
    /// # }
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
/// 
/// ## 性能优化
/// 
/// 新实现使用原子操作检查状态，无需获取全局锁。
/// 每次 `next()` 只需要一次原子读取，性能大幅提升。
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
        let task_list = get_task_list();
        while self.current < MAX_TASKS {
            let idx = self.current;
            self.current += 1;
            // 使用原子操作检查状态，无需锁
            if task_list[idx].is_initialized() {
                return Some(Task(idx));
            }
        }
        None
    }
}

/// 任务快照信息
/// 
/// 包含任务的基本信息快照，用于减少锁竞争
#[derive(Clone, Copy, Debug)]
pub struct TaskSnapshot {
    /// 任务 ID
    pub task_id: usize,
    /// 任务状态
    pub state: TaskState,
    /// 任务优先级
    pub priority: Priority,
    /// 任务名称
    pub name: &'static str,
}

/// 任务快照迭代器
/// 
/// 在创建时一次性获取所有任务的快照，迭代过程中完全无锁。
/// 
/// ## 优化说明
/// 
/// - 创建时遍历任务列表，使用原子操作读取状态
/// - 迭代过程中完全无锁
/// - 适用于调度器遍历、任务统计等场景
/// - 快照可能与实际状态有短暂不一致（最终一致性）
/// 
/// ## 使用示例
/// 
/// ```rust,no_run
/// use neon_rtos2::kernel::task::{Task, TaskState};
/// 
/// // 使用快照迭代器统计就绪任务
/// let ready_count = Task::snapshot_iter()
///     .filter(|s| s.state == TaskState::Ready)
///     .count();
/// 
/// // 找到最高优先级的就绪任务
/// let highest_priority_task = Task::snapshot_iter()
///     .filter(|s| s.state == TaskState::Ready)
///     .max_by_key(|s| s.priority);
/// ```
pub struct TaskSnapshotIter {
    /// 任务快照数组
    snapshots: [Option<TaskSnapshot>; MAX_TASKS],
    /// 当前迭代位置
    current: usize,
    /// 有效快照数量
    count: usize,
}

impl TaskSnapshotIter {
    /// 创建新的快照迭代器
    /// 
    /// 遍历任务列表，使用原子操作读取所有已初始化任务的信息
    pub fn new() -> Self {
        let task_list = get_task_list();
        let mut snapshots = [None; MAX_TASKS];
        let mut count = 0;
        
        for i in 0..MAX_TASKS {
            // 使用原子操作检查状态
            if task_list[i].is_initialized() {
                snapshots[count] = Some(TaskSnapshot {
                    task_id: i,
                    state: task_list[i].get_state(),
                    priority: task_list[i].get_priority(),
                    name: task_list[i].name,
                });
                count += 1;
            }
        }
        
        Self { snapshots, current: 0, count }
    }
    
    /// 获取快照总数量（不受迭代影响）
    #[inline]
    pub fn total_count(&self) -> usize {
        self.count
    }
    
    /// 检查是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    
    /// 获取剩余元素数量
    #[inline]
    pub fn remaining(&self) -> usize {
        self.count - self.current
    }
    
    /// 获取所有就绪任务的快照
    pub fn ready_snapshots(self) -> impl Iterator<Item = TaskSnapshot> {
        self.filter(|s| s.state == TaskState::Ready)
    }
    
    /// 获取所有阻塞任务的快照
    pub fn blocked_snapshots(self) -> impl Iterator<Item = TaskSnapshot> {
        self.filter(|s| matches!(s.state, TaskState::Blocked(_)))
    }
    
    /// 找到最高优先级的就绪任务
    pub fn highest_priority_ready(&self) -> Option<TaskSnapshot> {
        self.snapshots[..self.count]
            .iter()
            .filter_map(|s| *s)
            .filter(|s| s.state == TaskState::Ready)
            .max_by_key(|s| s.priority)
    }
}

impl Default for TaskSnapshotIter {
    fn default() -> Self {
        Self::new()
    }
}

impl Iterator for TaskSnapshotIter {
    type Item = TaskSnapshot;
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.count {
            let snapshot = self.snapshots[self.current];
            self.current += 1;
            snapshot
        } else {
            None
        }
    }
    
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.count - self.current;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for TaskSnapshotIter {
    fn len(&self) -> usize {
        self.count - self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use serial_test::serial;
    extern crate std;
    use std::vec::Vec;

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

    // ========================================================================
    // 快照迭代器测试
    // ========================================================================

    #[test]
    #[serial]
    fn test_snapshot_iter_basic() {
        kernel_init();
        
        Task::new("task1", |_| {}).unwrap();
        Task::new("task2", |_| {}).unwrap();
        Task::new("task3", |_| {}).unwrap();
        
        // 使用快照迭代器
        let snapshot_iter = Task::snapshot_iter();
        assert_eq!(snapshot_iter.len(), 3);
        
        // 遍历快照
        let snapshots: Vec<_> = Task::snapshot_iter().collect();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].name, "task1");
        assert_eq!(snapshots[1].name, "task2");
        assert_eq!(snapshots[2].name, "task3");
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_empty() {
        kernel_init();
        
        let snapshot_iter = Task::snapshot_iter();
        assert!(snapshot_iter.is_empty());
        assert_eq!(snapshot_iter.len(), 0);
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_ready_tasks() {
        kernel_init();
        
        let mut task1 = Task::new("ready_task", |_| {}).unwrap();
        let mut task2 = Task::new("running_task", |_| {}).unwrap();
        let mut task3 = Task::new("blocked_task", |_| {}).unwrap();
        
        // 设置不同状态
        task2.run();
        task3.block(Event::Signal(1));
        
        // 使用快照迭代器获取就绪任务
        let ready_snapshots: Vec<_> = Task::snapshot_iter()
            .ready_snapshots()
            .collect();
        
        assert_eq!(ready_snapshots.len(), 1);
        assert_eq!(ready_snapshots[0].name, "ready_task");
        assert_eq!(ready_snapshots[0].state, TaskState::Ready);
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_highest_priority() {
        kernel_init();
        
        let mut low_task = Task::builder("low_task")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        let high_task = Task::builder("high_task")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        let normal_task = Task::builder("normal_task")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        // 找到最高优先级的就绪任务
        let highest = Task::snapshot_iter().highest_priority_ready();
        assert!(highest.is_some());
        assert_eq!(highest.unwrap().name, "high_task");
        assert_eq!(highest.unwrap().priority, Priority::High);
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_blocked_tasks() {
        kernel_init();
        
        let mut task1 = Task::new("task1", |_| {}).unwrap();
        let mut task2 = Task::new("task2", |_| {}).unwrap();
        
        // 阻塞一个任务
        task1.block(Event::Signal(42));
        
        // 使用快照迭代器获取阻塞任务
        let blocked_snapshots: Vec<_> = Task::snapshot_iter()
            .blocked_snapshots()
            .collect();
        
        assert_eq!(blocked_snapshots.len(), 1);
        assert_eq!(blocked_snapshots[0].name, "task1");
        assert!(matches!(blocked_snapshots[0].state, TaskState::Blocked(_)));
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_exact_size() {
        kernel_init();
        
        Task::new("task1", |_| {}).unwrap();
        Task::new("task2", |_| {}).unwrap();
        
        let mut iter = Task::snapshot_iter();
        assert_eq!(iter.len(), 2);
        
        iter.next();
        assert_eq!(iter.len(), 1);
        
        iter.next();
        assert_eq!(iter.len(), 0);
    }
}
