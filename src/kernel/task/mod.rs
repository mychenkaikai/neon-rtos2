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

use core::cell::RefCell;
use critical_section::{CriticalSection, Mutex};
use crate::kernel::scheduler::Scheduler;

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
const STATE_FINISHED: u8 = 4;
const STATE_CREATED: u8 = 5;

// ============================================================================
// 全局任务列表（优化后的细粒度锁版本）
// ============================================================================

// 所有元数据在临界区内发布；中断不能等待被抢占任务持有的锁。
static TASK_LIST: [TaskControlBlock; MAX_TASKS] =
    [const { TaskControlBlock::new() }; MAX_TASKS];

#[unsafe(no_mangle)]
static mut TASK_STACKS: [Stack; MAX_TASKS] = [const {
    Stack {
        data: [0; STACK_SIZE],
    }
}; MAX_TASKS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TaskState {
    Uninit,
    Created,
    Ready,
    Running,
    Blocked(Event),
    Finished,
}

impl TaskState {
    /// 将状态编码为 u8（用于原子操作）
    #[inline]
    fn to_u8(&self) -> u8 {
        match self {
            TaskState::Created => STATE_CREATED,
            TaskState::Finished => STATE_FINISHED,
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
            STATE_CREATED => TaskState::Created,
            STATE_FINISHED => TaskState::Finished,
            STATE_UNINIT => TaskState::Uninit,
            STATE_READY => TaskState::Ready,
            STATE_RUNNING => TaskState::Running,
            STATE_BLOCKED => TaskState::Blocked(Event::None),
            _ => TaskState::Uninit,
        }
    }
}

#[repr(C)]
#[repr(align(32))]
pub struct Stack {
    pub data: [u8; STACK_SIZE],
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
/// 元数据使用 critical-section 保护的内部可变性。
/// 状态、优先级和调度队列由 Scheduler 在同一临界区提交。
/// 栈顶由 HAL 在上下文切换边界保存。
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
    base_priority: AtomicU8,
    generation: AtomicUsize,
    
    // ========== 阻塞事件（需要锁保护）==========
    /// 阻塞事件 - 仅在 Blocked 状态时有效
    /// 使用细粒度锁，因为事件信息无法用原子操作存储
    pub(crate) blocked_event: Mutex<RefCell<Option<Event>>>,
    
    // ========== 冷数据（不常访问）==========
    /// 任务名称 - 创建后不变
    name: Mutex<RefCell<&'static str>>,
    
    /// 任务函数 - 仅启动时访问一次
    /// 使用细粒度锁
    pub(crate) task_fn: Mutex<RefCell<Option<Box<dyn FnOnce() + Send + 'static>>>>,
}

#[derive(Clone, PartialEq, Copy, Debug)]
/// A handle identifies one incarnation of a task. The numeric slot returned by
/// `get_taskid` can be reused after return. Retained handles report Finished
/// and cannot change the state or priority of a replacement.
pub struct Task(pub(crate) usize, usize);

/// 获取任务列表（优化后的版本）
/// 
/// 返回静态任务列表的引用，每个 TCB 内部使用原子操作和细粒度锁
pub(super) fn get_task_list() -> &'static [TaskControlBlock; MAX_TASKS] {
    &TASK_LIST
}

// 关键：统一的包装器函数，���固定的入口地址
fn task_wrapper_entry(task_id: usize) {
    let task_list = get_task_list();
    // 使用细粒度锁获取任务函数
    let task_fn = critical_section::with(|cs| task_list[task_id].task_fn.borrow_ref_mut(cs).take());
    if let Some(func) = task_fn {
        func();
    }
    Scheduler::finish_current();
    crate::hal::trigger_schedule();
    // Retain this stack until the switch actually takes effect.
    loop { core::hint::spin_loop(); }
}

impl TaskControlBlock {
    /// 创建默认的 TCB（未初始化状态）
    const fn new() -> Self {
        Self {
            stack_top: AtomicUsize::new(0),
            state_atomic: AtomicU8::new(STATE_UNINIT),
            priority_atomic: AtomicU8::new(Priority::Normal.as_u8()),
            generation: AtomicUsize::new(0),
            base_priority: AtomicU8::new(Priority::Normal.as_u8()),
            blocked_event: Mutex::new(RefCell::new(None)),
            name: Mutex::new(RefCell::new("noinit")),
            task_fn: Mutex::new(RefCell::new(None)),
        }
    }
    
    /// 初始化任务（内部使用）
    /// 
    /// 调用者必须处于任务分配临界区
    fn init_unified<F>(&self, name: &'static str, func: F, taskid: usize, stack_top: usize)
    where
        F: FnOnce() + Send + 'static,
    {
        // 设置栈顶（需要可变引用用于 init_task_stack）
        self.stack_top.store(stack_top, Ordering::Release);
        
        critical_section::with(|cs| {
            *self.name.borrow_ref_mut(cs) = name;
            *self.task_fn.borrow_ref_mut(cs) = Some(Box::new(func));
        });

        // 初始化栈（需要获取栈顶的可变引用）
        let mut stack_top_val = self.stack_top.load(Ordering::Acquire);
        init_task_stack(&mut stack_top_val, task_wrapper_entry, taskid);
        self.stack_top.store(stack_top_val, Ordering::Release);
        
        // 调用者在优先级、栈和入口就绪后，通过调度器发布 Ready 状态。
    }
    
    /// 重置 TCB 到未初始化状态
    fn reset(&self) {
        self.state_atomic.store(STATE_UNINIT, Ordering::Release);
        self.stack_top.store(0, Ordering::Release);
        self.priority_atomic.store(Priority::Normal.as_u8(), Ordering::Release);
        self.base_priority.store(Priority::Normal.as_u8(), Ordering::Release);
        critical_section::with(|cs| *self.blocked_event.borrow_ref_mut(cs) = None);
        critical_section::with(|cs| {
            *self.task_fn.borrow_ref_mut(cs) = None;
            *self.name.borrow_ref_mut(cs) = "noinit";
        });
    }
    
    // ========== 原子访问方法 ==========
    
    /// 在同一临界区读取状态编码和阻塞原因
    #[inline]
    pub(super) fn get_state(&self, cs: CriticalSection<'_>) -> TaskState {
        let state_code = self.state_atomic.load(Ordering::Acquire);
        if state_code == STATE_BLOCKED {
            TaskState::Blocked(self.blocked_event.borrow_ref(cs).unwrap_or(Event::None))
        } else {
            TaskState::from_u8_simple(state_code)
        }
    }

    pub(super) fn set_state(&self, cs: CriticalSection<'_>, state: TaskState) {
        *self.blocked_event.borrow_ref_mut(cs) = match state {
            TaskState::Blocked(event) => Some(event),
            _ => None,
        };
        self.state_atomic.store(state.to_u8(), Ordering::Release);
    }

    pub(super) fn base_priority(&self) -> Priority {
        Priority::from_u8(self.base_priority.load(Ordering::Acquire)).unwrap()
    }

    /// 获取优先级（原子操作）- O(1)，无锁
    #[inline]
    pub(super) fn get_priority(&self) -> Priority {
        let prio = self.priority_atomic.load(Ordering::Acquire);
        Priority::from_u8(prio).unwrap_or(Priority::Normal)
    }
    
    /// 设置优先级（原子操作）- O(1)
    #[inline]
    pub(super) fn set_priority(&self, priority: Priority) {
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
        matches!(self.state_atomic.load(Ordering::Acquire), STATE_CREATED | STATE_READY | STATE_RUNNING | STATE_BLOCKED)
    }
}

impl Task {
    // Slot lookup is kernel-only. Public handles retain their original generation.
    pub(crate) fn from_slot(slot: usize) -> Self {
        Self(slot, TASK_LIST[slot].generation.load(Ordering::Acquire))
    }

    pub(crate) fn same_generation(self) -> bool {
        self.1 == TASK_LIST[self.0].generation.load(Ordering::Acquire)
    }

    pub(crate) fn is_live(self, cs: CriticalSection<'_>) -> bool {
        !matches!(self.state_in(cs), TaskState::Uninit | TaskState::Finished)
    }

    pub fn wake_if(self, event: Event) -> bool {
        critical_section::with(|cs| {
            if self.state_in(cs) != TaskState::Blocked(event) { return false; }
            Scheduler::set_task_state(self, TaskState::Ready);
            true
        })
    }

    /// 创建新任务
    /// 
    /// ## 性能优化
    /// 
    /// - 只在分配槽位时获取分配锁
    /// - 任务初始化使用原子操作和细粒度锁
    /// - 不影响其他任务的并发访问
    pub fn new<F>(name: &'static str, func: F) -> Result<Self>
    where
        F: FnOnce() + Send + 'static,
    {
        Self::new_with_priority(name, func, Priority::Normal)
    }

    pub(crate) fn new_with_priority<F>(name: &'static str, func: F, priority: Priority) -> Result<Self>
    where
        F: FnOnce() + Send + 'static,
    {
        Self::allocate(name, func, priority, TaskState::Ready)
    }

    fn allocate<F: FnOnce() + Send + 'static>(name: &'static str, func: F, priority: Priority, initial: TaskState) -> Result<Self> {
        critical_section::with(|cs| {
            let task_list = get_task_list();
            for i in 0..MAX_TASKS {
                let state = task_list[i].get_state(cs);
                if state == TaskState::Uninit || (state == TaskState::Finished && Scheduler::get_current_task().0 != i) {
                    // Never wrap an identity onto a still-held old handle.
                    let Some(generation) = task_list[i].generation.load(Ordering::Relaxed).checked_add(1) else { continue };
                    task_list[i].generation.store(generation, Ordering::Release);
                    // SAFETY: 槽位分配和栈初始化位于同一个临界区。
                    let stack_top = unsafe { addr_of!(TASK_STACKS[i].data) as usize + STACK_SIZE };
                    task_list[i].init_unified(name, func, i, stack_top);
                    let task = Task::from_slot(i);
                    task.set_base_priority(priority);
                    task.set_priority_raw(priority);
                    Scheduler::publish_task(cs, task, initial);
                    return Ok(task);
                }
            }
            Err(RtosError::TaskSlotsFull)
        })
    }

    /// Discard a task that has been created but has not started. Its closure
    /// is dropped outside the kernel borrow. Running and finished tasks are unaffected.
    fn cancel_unstarted(self) -> bool {
        let closure = critical_section::with(|cs| {
            if self.state_in(cs) != TaskState::Created { return None; }
            self.set_state_raw(cs, TaskState::Finished);
            Event::wake_task(Event::TaskExit(self));
            TASK_LIST[self.0].task_fn.borrow_ref_mut(cs).take()
        });
        closure.is_some()
    }

    /// Block the calling task until this task returns or its creation is cancelled.
    /// Multiple observers may join the same generation, including after slot reuse.
    pub fn join(self) -> Result<()> {
        loop {
            let finished = critical_section::with(|cs| {
                if self.state_in(cs) == TaskState::Finished {
                    return Ok(true);
                }
                let mut caller = Scheduler::get_current_task();
                if !Scheduler::is_running() || caller == self || caller.0 == 0 || self.0 == 0 {
                    return Err(RtosError::InvalidTaskState);
                }
                caller.block(Event::TaskExit(self));
                Ok(false)
            })?;
            if finished {
                return Ok(());
            }
            crate::hal::trigger_schedule();
        }
    }

    /// Suspend a ready task before dispatch. The idle task must always be runnable.
    pub fn block_if_ready(self, event: Event) -> bool {
        critical_section::with(|cs| {
            if self.0 == 0 || self.state_in(cs) != TaskState::Ready { return false; }
            Scheduler::set_task_state(self, TaskState::Blocked(event));
            true
        })
    }

    pub(crate) fn ready(&mut self) {
        Scheduler::set_task_state(*self, TaskState::Ready);
    }

    pub(crate) fn block(&mut self, reason: Event) {
        Scheduler::set_task_state(*self, TaskState::Blocked(reason));
    }

    // 只供调度器在更新就绪队列的同一个临界区内调用。
    pub(crate) fn set_state_raw(&self, cs: CriticalSection<'_>, state: TaskState) {
        get_task_list()[self.0].set_state(cs, state);
    }


    pub(crate) fn set_base_priority(&self, priority: Priority) {
        get_task_list()[self.0].base_priority.store(priority.as_u8(), Ordering::Release);
    }

    pub(crate) fn set_priority_raw(&self, priority: Priority) {
        get_task_list()[self.0].set_priority(priority);
    }

    /// 获取任务状态 - O(1)，原子操作（Blocked 状态需要锁）
    pub fn get_state(&self) -> TaskState {
        critical_section::with(|cs| self.state_in(cs))
    }

    pub(crate) fn state_in(&self, cs: CriticalSection<'_>) -> TaskState {
        if self.same_generation() { TASK_LIST[self.0].get_state(cs) } else { TaskState::Finished }
    }

    /// 在临界区读取任务名称，避免与槽位初始化竞争
    pub fn get_name(&self) -> &'static str {
        critical_section::with(|cs| if self.same_generation() { *TASK_LIST[self.0].name.borrow_ref(cs) } else { "<finished>" })
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
        let top = TASK_LIST[self.0].get_stack_top();
        if self.same_generation() { top } else { 0 }
    }

    // Validate the saved frame at the HAL boundary. This detects a bad SP;
    // it cannot undo stores already made by an overflowing task.
    #[cfg(target_os = "none")]
    pub(crate) fn check_frame(&self, sp: usize) {
        #[cfg(target_arch = "arm")]
        let (frame_bytes, align) = (64, 8);
        #[cfg(target_arch = "riscv32")]
        let (frame_bytes, align) = (crate::hal::riscv::CONTEXT_FRAME_SIZE, 16);
        let bottom = self.get_stack_bottom();
        assert!(sp >= bottom + 32 && sp <= bottom + STACK_SIZE - frame_bytes && sp % align == 0,
            "invalid context frame for task {}", self.0);
    }

    /// 设置栈顶指针 - O(1)，原子操作
    pub(crate) fn set_stack_top(&mut self, stack_top: usize) {
        get_task_list()[self.0].set_stack_top(stack_top);
    }

    /// 获取任务栈底部地址（最低地址，用于 MPU 保护）
    /// O(1)，静态数组地址计算
    pub fn get_stack_bottom(&self) -> usize {
        unsafe { addr_of!(TASK_STACKS[self.0].data) as usize }
    }

    /// 获取任务优先级 - O(1)，原子操作
    ///
    /// # 返回值
    /// 任务的当前优先级
    pub fn get_priority(&self) -> Priority {
        let priority = TASK_LIST[self.0].get_priority();
        if self.same_generation() { priority } else { Priority::Idle }
    }

    /// 设置任务优先级 - O(1)，原子操作
    ///
    /// # 参数
    /// - `priority`: 新的优先级
    pub fn set_priority(&mut self, priority: Priority) {
        Scheduler::set_task_priority(*self, priority);
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
        critical_section::with(|cs| (self.state_in(cs), self.get_priority(), self.get_stack_top()))
    }

    /// 初始化任务系统
    pub(crate) fn init() {
        let task_list = get_task_list();
        critical_section::with(|_| {
        
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
        });
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
                f(&mut Task::from_slot(i), i);
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
                f(&mut Task::from_slot(i), i);
            }
        }
        for i in 0..start {
            if task_list[i].is_initialized() {
                f(&mut Task::from_slot(i), i);
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
    ///     .spawn(|| { /* ... */ });
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
    /// let task = Task::new("my_task", || {})?;
    ///
    /// match task.into_typed()? {
    ///     TypedTaskAny::Ready(ready_task) => {
    ///         println!("Task is ready");
    ///         assert_eq!(ready_task.id(), task.get_taskid());
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
        use state::{TypedTask, TypedTaskAny};


        match self.get_state() {
            TaskState::Uninit | TaskState::Finished => Err(RtosError::InvalidTaskState),
            TaskState::Created => Ok(TypedTaskAny::Created(self)),
            TaskState::Ready => Ok(TypedTaskAny::Ready(TypedTask {
                inner: self,
                blocked_event: None,
                _state: state::Ready,
            })),
            TaskState::Running => Ok(TypedTaskAny::Running(TypedTask {
                inner: self,
                blocked_event: None,
                _state: state::Running,
            })),
            TaskState::Blocked(event) => Ok(TypedTaskAny::Blocked(TypedTask {
                inner: self,
                blocked_event: Some(event),
                _state: state::Blocked,
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
                return Some(Task::from_slot(idx));
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
                    state: Task::from_slot(i).get_state(),
                    priority: task_list[i].get_priority(),
                    name: Task::from_slot(i).get_name(),
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

    fn task1() {
        // 简化的任务函数
    }

    fn task2() {
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
        assert_eq!(task1.get_taskid(), 1);
        assert_eq!(task2.get_taskid(), 2);
        //检测栈顶是否8字节对齐
        assert_eq!(task1.get_stack_top() & !(0x0007), task1.get_stack_top());
        assert_eq!(task2.get_stack_top() & !(0x0007), task2.get_stack_top());
    }

    //检测任务数超过MAX_TASKS时，是否panic
    #[test]
    #[serial]
    fn test_task_overflow() {
        kernel_init();
        for _ in 1..MAX_TASKS {
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
        Scheduler::start();
        Scheduler::task_switch();
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
        let mut cnt = 0;
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::for_each_from(1, |task, id| {
            if cnt == 0 {
                assert_eq!(task.get_name(), "task1");
                assert_eq!(id, 1);
                cnt += 1;
            } else if cnt == 1 {
                assert_eq!(task.get_name(), "task2");
                assert_eq!(id, 2);
                cnt += 1;
            }
        });
        cnt = 0;
        Task::for_each_from(2, |task, id| {
            if cnt == 0 {
                assert_eq!(task.get_name(), "task2");
                assert_eq!(id, 2);
                cnt += 1;
            } else if cnt == 1 {
                // it wraps around and first encounters "idle"
                assert_eq!(task.get_name(), "idle");
                assert_eq!(id, 0);
                cnt += 1;
            } else if cnt == 2 {
                assert_eq!(task.get_name(), "task1");
                assert_eq!(id, 1);
                cnt += 1;
            }
        });
    }

    #[test]
    #[serial]
    fn test_task_for_each() {
        let mut cnt = 0;
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::for_each(|task, id| {
            if cnt == 0 {
                assert_eq!(task.get_name(), "idle");
                assert_eq!(id, 0);
                cnt += 1;
            } else if cnt == 1 {
                assert_eq!(task.get_name(), "task1");
                assert_eq!(id, 1);
                cnt += 1;
            } else if cnt == 2 {
                assert_eq!(task.get_name(), "task2");
                assert_eq!(id, 2);
                cnt += 1;
            }
        });
    }

    #[test]
    #[serial]
    fn test_task_state_transitions() {
        kernel_init();
        let mut task = Task::new("transition_task", || {}).unwrap();

        // 测试所有状态转换
        assert_eq!(task.get_state(), TaskState::Ready);

        Scheduler::start();
        Scheduler::task_switch();
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
        let mut task = Task::new("stack_task", || {}).unwrap();

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
        Task::new("task_1", || {}).unwrap();
        // task_2位置空出来
        let task3 = Task::new("task_3", || {}).unwrap();

        let mut count = 0;
        let mut found_task3 = false;

        Task::for_each_from(1, |task, _| {
            count += 1;
            if task.get_taskid() == task3.get_taskid() {
                found_task3 = true;
            }
        });

        assert_eq!(count, 3); // idle (if wrapped), task_1, task_3
        assert!(found_task3); // 应该找到task3
    }

    #[test]
    #[serial]
    fn test_for_each_from_with_wrap_around() {
        kernel_init();

        // 创建几个任务
        let task1 = Task::new("task_1", || {}).unwrap();
        let task2 = Task::new("task_2", || {}).unwrap();
        let task3 = Task::new("task_3", || {}).unwrap();

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
        
        Task::new("task1", || {}).unwrap();
        Task::new("task2", || {}).unwrap();
        Task::new("task3", || {}).unwrap();
        
        // 使用快照迭代器
        let snapshot_iter = Task::snapshot_iter();
        assert_eq!(snapshot_iter.len(), 4); // idle + 3 tasks
        
        // 遍历快照
        let snapshots: Vec<_> = Task::snapshot_iter().collect();
        assert_eq!(snapshots.len(), 4);
        assert_eq!(snapshots[0].name, "idle");
        assert_eq!(snapshots[1].name, "task1");
        assert_eq!(snapshots[2].name, "task2");
        assert_eq!(snapshots[3].name, "task3");
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_empty() {
        kernel_init();
        
        let snapshot_iter = Task::snapshot_iter();
        assert!(!snapshot_iter.is_empty()); // idle task exists
        assert_eq!(snapshot_iter.len(), 1);
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_ready_tasks() {
        kernel_init();
        
        let mut task1 = Task::new("ready_task", || {}).unwrap();
        let mut task2 = Task::new("running_task", || {}).unwrap();
        let mut task3 = Task::new("blocked_task", || {}).unwrap();
        
        // 设置不同状态
        Scheduler::start();
        Scheduler::task_switch();
        Scheduler::task_switch();
        task3.block(Event::Signal(1));
        
        // 使用快照迭代器获取就绪任务
        let ready_snapshots: Vec<_> = Task::snapshot_iter()
            .ready_snapshots()
            .collect();
        
        assert_eq!(ready_snapshots.len(), 2);
        assert_eq!(ready_snapshots[0].name, "idle");
        assert_eq!(ready_snapshots[1].name, "ready_task");
        assert_eq!(ready_snapshots[1].state, TaskState::Ready);
    }

    #[test]
    #[serial]
    fn test_snapshot_iter_highest_priority() {
        kernel_init();
        
        let mut low_task = Task::builder("low_task")
            .priority(Priority::Low)
            .spawn(|| {})
            .unwrap();
        
        let high_task = Task::builder("high_task")
            .priority(Priority::High)
            .spawn(|| {})
            .unwrap();
        
        let normal_task = Task::builder("normal_task")
            .priority(Priority::Normal)
            .spawn(|| {})
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
        
        let mut task1 = Task::new("task1", || {}).unwrap();
        let mut task2 = Task::new("task2", || {}).unwrap();
        
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
        
        Task::new("task1", || {}).unwrap();
        Task::new("task2", || {}).unwrap();
        
        let mut iter = Task::snapshot_iter();
        assert_eq!(iter.len(), 3);
        
        iter.next();
        assert_eq!(iter.len(), 2);
        
        iter.next();
        assert_eq!(iter.len(), 1);

        iter.next();
        assert_eq!(iter.len(), 0);
    }
}
