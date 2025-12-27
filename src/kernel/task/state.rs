//! # 类型状态模式 (Type State Pattern)
//!
//! 使用 Rust 类型系统在编译期保证任务状态转换的正确性。
//!
//! ## 设计理念
//!
//! 传统的任务状态管理使用枚举表示状态，状态转换的正确性只能在运行时检查。
//! 类型状态模式将状态编码到类型系统中，让编译器在编译期就能捕获非法的状态转换。
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! use neon_rtos2::kernel::task::state::*;
//!
//! // 创建任务（Created 状态）
//! let task = TypedTask::<Created>::new("my_task", |_| {
//!     loop { /* 任务逻辑 */ }
//! })?;
//!
//! // 启动任务（Created -> Ready）
//! let task = task.start();
//!
//! // 运行任务（Ready -> Running）
//! let task = task.run();
//!
//! // 以下代码无法编译！
//! // task.start(); // 错误：Running 状态没有 start 方法
//! ```
//!
//! ## 状态转换图
//!
//! ```text
//!                    ┌─────────┐
//!                    │ Created │
//!                    └────┬────┘
//!                         │ start()
//!                         ▼
//!     ┌──────────────►┌─────��─┐◄──────────────┐
//!     │               │ Ready │               │
//!     │               └───┬───┘               │
//!     │                   │ run()             │
//!     │                   ▼                   │
//!     │  yield_now() ┌─────────┐  wake()      │
//!     └──────────────│ Running │──────────────┘
//!                    └────┬────┘
//!                         │ block()
//!                         ▼
//!                    ┌─────────┐
//!                    │ Blocked │
//!                    └────┬────┘
//!                         │ wake()
//!                         ▼
//!                      (Ready)
//! ```

use core::marker::PhantomData;
use crate::error::{Result, RtosError};
use crate::sync::event::Event;
use super::{Task, TaskFunction, Priority};

// ============================================================================
// 状态标记类型
// ============================================================================

/// 任务状态标记 trait
/// 
/// 所有状态类型都必须实现此 trait
pub trait TaskStateMarker: private::Sealed {}

/// 已创建状态 - 任务已创建但尚未启动
#[derive(Debug, Clone, Copy)]
pub struct Created;

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
/// 通过泛型参数 `S` 编码任务的当前状态，
/// 编译器会在编译期检查状态转换的合法性。
///
/// # 类型参数
///
/// - `S`: 任务的当前状态，必须实现 `TaskStateMarker`
///
/// # 示例
///
/// ```rust,ignore
/// // 创建任务
/// let task = TypedTask::<Created>::new("task", |_| {})?;
///
/// // 状态转换
/// let task = task.start();  // Created -> Ready
/// let task = task.run();    // Ready -> Running
///
/// // 编译错误示例：
/// // let task = TypedTask::<Running>::new(...); // 错误：只能创建 Created 状态
/// // task.start(); // 错误：Running 没有 start 方法
/// ```
#[derive(Debug)]
pub struct TypedTask<S: TaskStateMarker> {
    /// 内部任务句柄
    inner: Task,
    /// 阻塞事件（仅 Blocked 状态使用）
    blocked_event: Option<Event>,
    /// 状态标记（零大小类型，不占用内存）
    _state: PhantomData<S>,
}

// ============================================================================
// Created 状态实现
// ============================================================================

impl TypedTask<Created> {
    /// 创建新任务
    ///
    /// 新创建的任务处于 `Created` 状态，需要调用 `start()` 启动。
    ///
    /// # 参数
    ///
    /// - `name`: 任务名称
    /// - `func`: 任务函数
    ///
    /// # 返回值
    ///
    /// 成功返回 `TypedTask<Created>`，失败返回错误
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let task = TypedTask::<Created>::new("my_task", |_| {
    ///     loop {
    ///         // 任务逻辑
    ///     }
    /// })?;
    /// ```
    pub fn new<F>(name: &'static str, func: F) -> Result<Self>
    where
        F: TaskFunction,
    {
        let inner = Task::new(name, func)?;
        Ok(Self {
            inner,
            blocked_event: None,
            _state: PhantomData,
        })
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
    /// ```rust,ignore
    /// let task = TypedTask::<Created>::new("task", |_| {})?;
    /// let task = task.start(); // 现在是 Ready 状态
    /// ```
    pub fn start(self) -> TypedTask<Ready> {
        // 任务创建时已经是 Ready 状态，这里只是类型转换
        TypedTask {
            inner: self.inner,
            blocked_event: None,
            _state: PhantomData,
        }
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
    /// 运行任务（Ready -> Running）
    ///
    /// 将任务从 `Ready` 状态转换为 `Running` 状态。
    /// 通常由调度器调用。
    ///
    /// # 返回值
    ///
    /// 返回 `TypedTask<Running>`
    pub fn run(mut self) -> TypedTask<Running> {
        self.inner.run();
        TypedTask {
            inner: self.inner,
            blocked_event: None,
            _state: PhantomData,
        }
    }

    /// 阻塞任务（Ready -> Blocked）
    ///
    /// 将任务从 `Ready` 状态转换为 `Blocked` 状态，
    /// 等待指定的事件。
    ///
    /// # 参数
    ///
    /// - `event`: 等待的事件
    ///
    /// # 返回值
    ///
    /// 返回 `TypedTask<Blocked>`
    pub fn block(mut self, event: Event) -> TypedTask<Blocked> {
        self.inner.block(event);
        TypedTask {
            inner: self.inner,
            blocked_event: Some(event),
            _state: PhantomData,
        }
    }

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
    /// 让出 CPU（Running -> Ready）
    ///
    /// 主动让出 CPU，将任务从 `Running` 状态转换为 `Ready` 状态。
    ///
    /// # 返回值
    ///
    /// 返回 `TypedTask<Ready>`
    pub fn yield_now(mut self) -> TypedTask<Ready> {
        self.inner.ready();
        TypedTask {
            inner: self.inner,
            blocked_event: None,
            _state: PhantomData,
        }
    }

    /// 阻塞任务（Running -> Blocked）
    ///
    /// 将任务从 `Running` 状态转换为 `Blocked` 状态，
    /// 等待指定的事件。
    ///
    /// # 参数
    ///
    /// - `event`: 等待的事件
    ///
    /// # 返回值
    ///
    /// 返回 `TypedTask<Blocked>`
    pub fn block(mut self, event: Event) -> TypedTask<Blocked> {
        self.inner.block(event);
        TypedTask {
            inner: self.inner,
            blocked_event: Some(event),
            _state: PhantomData,
        }
    }

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
    /// 唤醒任务（Blocked -> Ready）
    ///
    /// 将任务从 `Blocked` 状态转换为 `Ready` 状态。
    /// 通常在等待的事件发生时调用。
    ///
    /// # 返回值
    ///
    /// 返回 `TypedTask<Ready>`
    pub fn wake(mut self) -> TypedTask<Ready> {
        self.inner.ready();
        TypedTask {
            inner: self.inner,
            blocked_event: None,
            _state: PhantomData,
        }
    }

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

    /// 获取内部任务句柄的可变引用
    pub fn inner_mut(&mut self) -> &mut Task {
        &mut self.inner
    }

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
/// ```rust,ignore
/// let task = TypedTask::builder("my_task")
///     .priority(Priority::High)
///     .spawn(|_| {
///         // 任务逻辑
///     })?;
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
    pub fn spawn<F>(self, func: F) -> Result<TypedTask<Ready>>
    where
        F: TaskFunction,
    {
        let task = TypedTask::<Created>::new(self.name, func)?;
        let mut ready_task = task.start();
        ready_task.set_priority(self.priority);
        Ok(ready_task)
    }

    /// 只创建任务，不启动
    ///
    /// 返回 `TypedTask<Created>` 状态的任务
    pub fn build<F>(self, func: F) -> Result<TypedTask<Created>>
    where
        F: TaskFunction,
    {
        TypedTask::<Created>::new(self.name, func)
    }
}

// ============================================================================
// 从普通 Task 转换
// ============================================================================

impl From<Task> for TypedTask<Ready> {
    /// 从普通 Task 转换为 TypedTask<Ready>
    ///
    /// 假设传入的 Task 处于 Ready 状态
    fn from(task: Task) -> Self {
        Self {
            inner: task,
            blocked_event: None,
            _state: PhantomData,
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_typed_task_creation() {
        kernel_init();
        
        let task = TypedTask::<Created>::new("test_task", |_| {}).unwrap();
        assert_eq!(task.name(), "test_task");
    }

    #[test]
    #[serial]
    fn test_typed_task_state_transitions() {
        kernel_init();
        
        // Created -> Ready
        let task = TypedTask::<Created>::new("transition_task", |_| {}).unwrap();
        let task = task.start();
        
        // Ready -> Running
        let task = task.run();
        
        // Running -> Ready (yield)
        let task = task.yield_now();
        
        // Ready -> Blocked
        let task = task.block(Event::Signal(1));
        assert_eq!(task.blocked_event(), Some(Event::Signal(1)));
        
        // Blocked -> Ready (wake)
        let _task = task.wake();
    }

    #[test]
    #[serial]
    fn test_typed_task_builder() {
        kernel_init();
        
        let task = TypedTask::builder("builder_task")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        assert_eq!(task.name(), "builder_task");
        assert_eq!(task.priority(), Priority::High);
    }

    #[test]
    #[serial]
    fn test_typed_task_running_to_blocked() {
        kernel_init();
        
        let task = TypedTask::<Created>::new("block_task", |_| {}).unwrap();
        let task = task.start();
        let task = task.run();
        
        // Running -> Blocked
        let task = task.block(Event::Timer(100));
        assert_eq!(task.blocked_event(), Some(Event::Timer(100)));
    }

    #[test]
    #[serial]
    fn test_typed_task_into_inner() {
        kernel_init();
        
        let typed_task = TypedTask::<Created>::new("inner_task", |_| {}).unwrap();
        let inner = typed_task.into_inner();
        
        assert_eq!(inner.get_name(), "inner_task");
    }

    #[test]
    #[serial]
    fn test_typed_task_from_task() {
        kernel_init();
        
        let task = Task::new("from_task", |_| {}).unwrap();
        let typed_task: TypedTask<Ready> = task.into();
        
        assert_eq!(typed_task.name(), "from_task");
    }
}

