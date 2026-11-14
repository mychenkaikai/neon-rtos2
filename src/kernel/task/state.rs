//! # 类型状态模式 (Type State Pattern)
//!
//! 创建句柄占用任务槽。启动后，类型化句柄记录观察时的状态；
//! 调度器切换、阻塞和事件唤醒均可使快照过期。
//! 需要最新状态时使用 `Task::into_typed()`。

use crate::error::{Result, RtosError};
use super::TaskState;
use crate::sync::event::Event;
use super::{Task, Priority};

// ============================================================================
// 状态标记类型
// ============================================================================

/// 任务状态标记 trait
/// 
/// 所有状态类型都必须实现此 trait
pub trait TaskStateMarker: private::Sealed {}

/// 已创建状态 - 任务已创建但尚未启动
#[derive(Debug)]
pub struct Created(Task);

impl Drop for Created {
    fn drop(&mut self) {
        self.0.cancel_unstarted();
    }
}

/// 就绪状态 - 任务已准备好运行，等待调度
#[derive(Debug, Clone, Copy)]
pub struct Ready;

/// 运行状态 - 任务正在 CPU 上执行
#[derive(Debug, Clone, Copy)]
pub struct Running;

/// 阻塞状态 - 任务正在等待某个事件
#[derive(Debug, Clone, Copy)]
pub struct Blocked;

// 实现状态标记 trait
impl TaskStateMarker for Created {}
impl TaskStateMarker for Ready {}
impl TaskStateMarker for Running {}
impl TaskStateMarker for Blocked {}

// 私有模块用于封闭 trait
mod private {
    pub trait Sealed {}
    impl Sealed for super::Created {}
    impl Sealed for super::Ready {}
    impl Sealed for super::Running {}
    impl Sealed for super::Blocked {}
}

// ============================================================================
// 类型安全的任务句柄
// ============================================================================

/// 类型安全的任务句柄
///
/// 泛型参数 `S` 表示创建或读取句柄时观察到的状态。
///
/// # 类型参数
///
/// - `S`: 任务的当前状态，必须实现 `TaskStateMarker`
///
/// # 示例
///
/// ```rust,no_run
/// # use neon_rtos2::kernel::task::state::*;
/// # use neon_rtos2::prelude::*;
/// # fn main() -> core::result::Result<(), RtosError> {
/// # kernel_init();
/// // 创建任务
/// let task = TypedTask::<Created>::new("task", || {})?;
///
/// // 状态转换
/// let task = task.start()?;  // Created -> Ready
/// let snapshot = task.into_inner().into_typed()?;
///
/// // 编译错误示例：
/// // let task = TypedTask::<Running>::new(...); // 错误：只能创建 Created 状态
/// // task.start(); // 错误：Running 没有 start 方法
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct TypedTask<S: TaskStateMarker> {
    /// 内部任务句柄
    pub(crate) inner: Task,
    /// 阻塞事件（仅 Blocked 状态使用）
    pub(crate) blocked_event: Option<Event>,
    /// 状态标记（零大小类型，不占用内存）
    pub(crate) _state: S,
}

// ============================================================================
// Created 状态实现
// ============================================================================

impl TypedTask<Created> {
    /// 创建新任务
    ///
    /// 新创建的任务处于 `Created` 状态，需要调用 `start()` 启动。
    /// 
    /// 创建会保留一个任务槽和入口闭包。调用 start 才会进入就绪队列，
    /// 丢弃未启动的创建句柄会释放预留的槽。
    ///
    /// # 参数
    ///
    /// - `name`: 任务名��
    /// - `func`: 任务函数
    ///
    /// # 返回值
    ///
    /// 成功返回 `TypedTask<Created>`，失败返回错误
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// # use neon_rtos2::kernel::task::state::*;
    /// # use neon_rtos2::prelude::*;
    /// # fn main() -> core::result::Result<(), RtosError> {
    /// # kernel_init();
    /// let task = TypedTask::<Created>::new("my_task", || {
    ///     loop {
    ///         // 任务逻辑
    ///     }
    /// })?;
    /// // 此时任务已创建，但类型系统标记为 Created
    /// // 可以在 start() 之前进行配置
    /// let task = task.start()?; // 现在类型变为 Ready
    /// # Ok(())
    /// # }
    /// ```
    pub fn new<F>(name: &'static str, func: F) -> Result<Self>
    where
        F: FnOnce() + Send + 'static,
    {
        let inner = Task::allocate(name, func, Priority::Normal, super::TaskState::Created)?;
        Ok(Self {
            inner,
            blocked_event: None,
            _state: Created(inner),
        })
    }
    
    /// 获取任务优先级
    /// 
    /// 可以在 `start()` 之前查看或修改优先级
    pub fn priority(&self) -> Priority {
        self.inner.get_priority()
    }
    
    /// 设置任务优先级
    /// 
    /// 可以在 `start()` 之前设置优先级，这样任务启动时就具有正确的优先级
    /// 
    /// # 示例
    /// 
    /// ```rust,no_run
    /// # use neon_rtos2::kernel::task::state::*;
    /// # use neon_rtos2::prelude::*;
    /// # fn main() -> core::result::Result<(), RtosError> {
    /// # kernel_init();
    /// let mut task = TypedTask::<Created>::new("high_priority_task", || {})?;
    /// task.set_priority(Priority::High);
    /// let task = task.start()?; // 启动时已经是高优先级
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_priority(&mut self, priority: Priority) {
        self.inner.set_priority(priority);
    }

    /// 使用 Builder 模式创建任务
    ///
    /// # 参数
    ///
    /// - `name`: 任务名称
    ///
    /// # 返回值
    ///
    /// 返回 `TypedTaskBuilder` 用于链式配置
    pub fn builder(name: &'static str) -> TypedTaskBuilder {
        TypedTaskBuilder::new(name)
    }

    /// 启动任务（Created -> Ready）
    ///
    /// 将任务从 `Created` 状态转换为 `Ready` 状态，
    /// 使其可以被调度器调度执行。
    ///
    /// # 返回值
    ///
    /// 返回 `TypedTask<Ready>`
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// # use neon_rtos2::kernel::task::state::*;
    /// # use neon_rtos2::prelude::*;
    /// # fn main() -> core::result::Result<(), RtosError> {
    /// # kernel_init();
    /// let task = TypedTask::<Created>::new("task", || {})?;
    /// let task = task.start()?; // 现在是 Ready 状态
    /// # Ok(())
    /// # }
    /// ```
    pub fn start(mut self) -> Result<TypedTask<Ready>> {
        critical_section::with(|cs| {
            if self.inner.state_in(cs) != TaskState::Created {
                return Err(RtosError::InvalidTaskState);
            }
            self.inner.ready();
            Ok(())
        })?;
        Ok(TypedTask {
            inner: self.inner,
            blocked_event: None,
            _state: Ready,
        })
    }

    /// 获取任务 ID
    pub fn id(&self) -> usize {
        self.inner.get_taskid()
    }

    /// 获取任务名称
    pub fn name(&self) -> &'static str {
        self.inner.get_name()
    }
}

// ============================================================================
// Ready 状态实现
// ============================================================================

impl TypedTask<Ready> {
    /// 获取任务 ID
    pub fn id(&self) -> usize {
        self.inner.get_taskid()
    }

    /// 获取任务名称
    pub fn name(&self) -> &'static str {
        self.inner.get_name()
    }

    /// 获取任务优先级
    pub fn priority(&self) -> Priority {
        self.inner.get_priority()
    }

    /// 设置任务优先级
    pub fn set_priority(&mut self, priority: Priority) {
        self.inner.set_priority(priority);
    }
}

// ============================================================================
// Running 状态实现
// ============================================================================

impl TypedTask<Running> {
    /// 获取任务 ID
    pub fn id(&self) -> usize {
        self.inner.get_taskid()
    }

    /// 获取任务名称
    pub fn name(&self) -> &'static str {
        self.inner.get_name()
    }

    /// 获取任务优先级
    pub fn priority(&self) -> Priority {
        self.inner.get_priority()
    }
}

// ============================================================================
// Blocked 状态实现
// ============================================================================

impl TypedTask<Blocked> {
    /// 获取阻塞事件
    pub fn blocked_event(&self) -> Option<Event> {
        self.blocked_event
    }

    /// 获取任务 ID
    pub fn id(&self) -> usize {
        self.inner.get_taskid()
    }

    /// 获取任务名称
    pub fn name(&self) -> &'static str {
        self.inner.get_name()
    }
}

// ============================================================================
// 通用实现（所有状态共享）
// ============================================================================

impl<S: TaskStateMarker> TypedTask<S> {
    /// 获取内部任务句柄的引用
    ///
    /// 用于需要访问底层 `Task` 的场景
    pub fn inner(&self) -> &Task {
        &self.inner
    }

}

pub trait StartedState: TaskStateMarker {}
impl StartedState for Ready {}
impl StartedState for Running {}
impl StartedState for Blocked {}

impl<S: StartedState> TypedTask<S> {
    /// 消费自身，返回内部任务句柄
    ///
    /// 用于需要脱离类型状态系统的场景
    pub fn into_inner(self) -> Task {
        self.inner
    }
}

// ============================================================================
// Builder 模式
// ============================================================================

/// 类型安全任务构建器
///
/// 提供链式 API 创建任务
///
/// # 示例
///
/// ```rust,no_run
/// # use neon_rtos2::prelude::*;
/// # use neon_rtos2::kernel::task::state::TypedTask;
/// # fn main() -> core::result::Result<(), RtosError> {
/// # kernel_init();
/// let task = TypedTask::builder("my_task")
///     .priority(Priority::High)
///     .spawn(|| {
///         // 任务逻辑
///     })?;
/// # Ok(())
/// # }
/// ```
pub struct TypedTaskBuilder {
    name: &'static str,
    priority: Priority,
}

impl TypedTaskBuilder {
    /// 创建新的构建器
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            priority: Priority::Normal,
        }
    }

    /// 设置任务优先级
    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// 创建并启动任务
    ///
    /// 返回 `TypedTask<Ready>` 状态的任务
    /// 
    /// ## 流程
    /// 1. 创建 `TypedTask<Created>`
    /// 2. 在 `Created` 状态设置优先级
    /// 3. 调用 `start()` 转换为 `Ready` 状态
    pub fn spawn<F>(self, func: F) -> Result<TypedTask<Ready>>
    where
        F: FnOnce() + Send + 'static,
    {
        let mut task = TypedTask::<Created>::new(self.name, func)?;
        // 在 Created 状态设置优先级（更符合语义）
        task.set_priority(self.priority);
        task.start()
    }

    /// 只创建任务，不启动
    ///
    /// 返回 `TypedTask<Created>` 状态的任务，已设置优先级
    /// 
    /// # 示例
    /// 
    /// ```rust,no_run
    /// # use neon_rtos2::prelude::*;
    /// # use neon_rtos2::kernel::task::state::TypedTask;
    /// # fn main() -> core::result::Result<(), RtosError> {
    /// # kernel_init();
    /// let task = TypedTask::builder("my_task")
    ///     .priority(Priority::High)
    ///     .build(|| {})?;
    /// 
    /// // 任务已创建并设置了优先级，但还未启动
    /// assert_eq!(task.priority(), Priority::High);
    /// 
    /// // 稍后启动
    /// let task = task.start()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build<F>(self, func: F) -> Result<TypedTask<Created>>
    where
        F: FnOnce() + Send + 'static,
    {
        let mut task = TypedTask::<Created>::new(self.name, func)?;
        task.set_priority(self.priority);
        Ok(task)
    }
}

// ============================================================================
// 从普通 Task 转换
// ============================================================================

/// 任意状态的类型安全任务
///
/// 用于从普通 `Task` 转换时，根据实际状态返回对应的 `TypedTask` 变体
///
/// # 示例
///
/// ```rust,no_run
/// # use neon_rtos2::prelude::*;
/// # use neon_rtos2::kernel::task::state::TypedTaskAny;
/// # fn main() -> core::result::Result<(), RtosError> {
/// # kernel_init();
/// let task = Task::new("task", || {})?;
/// match task.into_typed()? {
///     TypedTaskAny::Ready(ready_task) => {
///         // 处理就绪状态的任务
///     }
///     TypedTaskAny::Running(running_task) => {
///         // 处理运行状态的任务
///     }
///     TypedTaskAny::Blocked(blocked_task) => {
///         // 处理阻塞状态的任务
///     }
///     TypedTaskAny::Created(_) => {
///         // 处理创建状态的任务
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub enum TypedTaskAny {
    /// 已创建状态
    Created(Task),
    /// 就绪状态
    Ready(TypedTask<Ready>),
    /// 运行状态
    Running(TypedTask<Running>),
    /// 阻塞状态
    Blocked(TypedTask<Blocked>),
}

impl TypedTaskAny {
    /// 尝试转换为 Ready 状态
    ///
    /// # 返回值
    ///
    /// 如果任务处于 Ready 状态，返回 `Some(TypedTask<Ready>)`，否则返回 `None`
    pub fn into_ready(self) -> Option<TypedTask<Ready>> {
        match self {
            TypedTaskAny::Ready(task) => Some(task),
            _ => None,
        }
    }

    /// 尝试转换为 Running 状态
    pub fn into_running(self) -> Option<TypedTask<Running>> {
        match self {
            TypedTaskAny::Running(task) => Some(task),
            _ => None,
        }
    }

    /// 尝试转换为 Blocked 状态
    pub fn into_blocked(self) -> Option<TypedTask<Blocked>> {
        match self {
            TypedTaskAny::Blocked(task) => Some(task),
            _ => None,
        }
    }

    /// 尝试转换为 Created 状态
    pub fn into_created(self) -> Option<Task> {
        match self {
            TypedTaskAny::Created(task) => Some(task),
            _ => None,
        }
    }

    /// 获取任务 ID
    pub fn id(&self) -> usize {
        match self {
            TypedTaskAny::Created(task) => task.get_taskid(),
            TypedTaskAny::Ready(task) => task.id(),
            TypedTaskAny::Running(task) => task.id(),
            TypedTaskAny::Blocked(task) => task.id(),
        }
    }

    /// 获取任务名称
    pub fn name(&self) -> &'static str {
        match self {
            TypedTaskAny::Created(task) => task.get_name(),
            TypedTaskAny::Ready(task) => task.name(),
            TypedTaskAny::Running(task) => task.name(),
            TypedTaskAny::Blocked(task) => task.name(),
        }
    }
}

impl TryFrom<Task> for TypedTask<Ready> {
    type Error = RtosError;

    fn try_from(task: Task) -> Result<Self> {
        task.into_typed()?.into_ready().ok_or(RtosError::InvalidTaskState)
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_typed_task_creation() {
        kernel_init();
        
        let task = TypedTask::<Created>::new("test_task", || {}).unwrap();
        assert_eq!(task.name(), "test_task");
    }

    #[test]
    #[serial]
    fn test_typed_task_state_transitions() {
        kernel_init();
        
        let task = TypedTask::<Created>::new("transition", || {}).unwrap().start().unwrap().into_inner();
        assert!(task.block_if_ready(Event::Signal(1)));
        assert!(!task.wake_if(Event::Timer(1)));
        assert!(task.wake_if(Event::Signal(1)));
        Scheduler::start();
        Scheduler::task_switch();
        assert!(task.into_typed().unwrap().into_running().is_some());
    }

    #[test]
    #[serial]
    fn test_typed_task_builder() {
        kernel_init();
        
        let task = TypedTask::builder("builder_task")
            .priority(Priority::High)
            .spawn(|| {})
            .unwrap();
        
        assert_eq!(task.name(), "builder_task");
        assert_eq!(task.priority(), Priority::High);
    }

    #[test]
    #[serial]
    fn test_typed_task_running_to_blocked() {
        kernel_init();
        
        let task = TypedTask::<Created>::new("block_task", || {}).unwrap();
        let mut task = task.start().unwrap().into_inner();
        Scheduler::start();
        Scheduler::task_switch();
        
        // Running -> Blocked
        task.block(Event::Timer(100));
        let task = task.into_typed().unwrap().into_blocked().unwrap();
        assert_eq!(task.blocked_event(), Some(Event::Timer(100)));
    }

    #[test]
    #[serial]
    fn test_typed_task_into_inner() {
        kernel_init();
        
        let typed_task = TypedTask::<Created>::new("inner_task", || {}).unwrap();
        let inner = typed_task.start().unwrap().into_inner();
        
        assert_eq!(inner.get_name(), "inner_task");
    }

    #[test]
    #[serial]
    fn test_typed_task_from_task() {
        kernel_init();
        
        let task = Task::new("from_task", || {}).unwrap();
        let typed_task: TypedTask<Ready> = task.try_into().unwrap();
        
        assert_eq!(typed_task.name(), "from_task");
    }

    #[test]
    #[serial]
    fn test_task_into_typed_ready() {
        kernel_init();
        
        let task = Task::new("typed_ready", || {}).unwrap();
        let typed = task.into_typed().unwrap();
        
        match typed {
            TypedTaskAny::Ready(ready_task) => {
                assert_eq!(ready_task.name(), "typed_ready");
            }
            _ => panic!("Expected Ready state"),
        }
    }

    #[test]
    #[serial]
    fn test_task_into_typed_running() {
        kernel_init();
        
        let task = Task::new("typed_running", || {}).unwrap();
        Scheduler::start();
        Scheduler::task_switch();
        
        let typed = task.into_typed().unwrap();
        
        match typed {
            TypedTaskAny::Running(running_task) => {
                assert_eq!(running_task.name(), "typed_running");
            }
            _ => panic!("Expected Running state"),
        }
    }

    #[test]
    #[serial]
    fn test_task_into_typed_blocked() {
        kernel_init();
        
        let mut task = Task::new("typed_blocked", || {}).unwrap();
        task.block(Event::Signal(42));
        
        let typed = task.into_typed().unwrap();
        
        match typed {
            TypedTaskAny::Blocked(blocked_task) => {
                assert_eq!(blocked_task.name(), "typed_blocked");
                assert_eq!(blocked_task.blocked_event(), Some(Event::Signal(42)));
            }
            _ => panic!("Expected Blocked state"),
        }
    }

    #[test]
    #[serial]
    fn test_typed_task_any_conversions() {
        kernel_init();
        
        // Test into_ready
        let task = Task::new("conv_ready", || {}).unwrap();
        let typed = task.into_typed().unwrap();
        assert!(typed.into_ready().is_some());
        
        // Test into_running
        let task = Task::new("conv_running", || {}).unwrap();
        Scheduler::start();
        Scheduler::task_switch();
        Scheduler::task_switch();
        let typed = task.into_typed().unwrap();
        assert!(typed.into_running().is_some());
        
        // Test into_blocked
        let mut task = Task::new("conv_blocked", || {}).unwrap();
        task.block(Event::Timer(1));
        let typed = task.into_typed().unwrap();
        assert!(typed.into_blocked().is_some());
    }

    #[test]
    #[serial]
    fn test_typed_task_any_accessors() {
        kernel_init();
        
        let task = Task::new("accessor_test", || {}).unwrap();
        let task_id = task.get_taskid();
        let typed = task.into_typed().unwrap();
        
        assert_eq!(typed.id(), task_id);
        assert_eq!(typed.name(), "accessor_test");
    }

    #[test]
    #[serial]
    fn test_typed_task_created_priority() {
        kernel_init();
        
        // 测试在 Created 状态设置优先级
        let mut task = TypedTask::<Created>::new("priority_test", || {}).unwrap();
        
        // 默认优先级是 Normal
        assert_eq!(task.priority(), Priority::Normal);
        
        // 在 Created 状态设置优先级
        task.set_priority(Priority::High);
        assert_eq!(task.priority(), Priority::High);
        
        // 启动后优先级保持不变
        let ready_task = task.start().unwrap();
        assert_eq!(ready_task.priority(), Priority::High);
    }

    #[test]
    #[serial]
    fn test_typed_task_builder_priority_in_created() {
        kernel_init();
        
        // 使用 builder 的 build() 方法，验证优先级在 Created 状态就已设置
        let task = TypedTask::builder("builder_priority_test")
            .priority(Priority::Critical)
            .build(|| {})
            .unwrap();
        
        // 在 Created 状态就应该有正确的优先级
        assert_eq!(task.priority(), Priority::Critical);
        
        // 启动后优先级保持不变
        let ready_task = task.start().unwrap();
        assert_eq!(ready_task.priority(), Priority::Critical);
    }
}

