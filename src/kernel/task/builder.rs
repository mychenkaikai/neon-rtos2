//! 任务构建器
//!
//! 提供链式 API 创建任务，支持设置优先级和栈大小。

use super::{Task, TaskFunction};
use super::priority::Priority;
use crate::config::STACK_SIZE;
use crate::error::Result;

/// 任务构建器
///
/// 使用 Builder 模式创建任务，提供更灵活的配置选项。
///
/// # 示例
///
/// ```rust
/// use neon_rtos2::kernel::task::{Task, Priority};
///
/// // 使用默认配置
/// let task = Task::builder("simple_task")
///     .spawn(|_| {
///         // 任务逻辑
///     });
///
/// // 自定义配置
/// let task = Task::builder("custom_task")
///     .priority(Priority::High)
///     .stack_size(8192)
///     .spawn(|_| {
///         // 任务逻辑
///     });
/// ```
pub struct TaskBuilder {
    name: &'static str,
    priority: Priority,
    /// 栈大小配置
    /// 
    /// 注意：当前版本使用固定栈大小（由 `config::STACK_SIZE` 定义），
    /// 此字段预留给未来动态栈分配功能扩展使用。
    #[allow(dead_code)]
    stack_size: usize,
}

impl TaskBuilder {
    /// 创建新的任务构建器
    ///
    /// # 参数
    /// - `name`: 任务名称
    ///
    /// # 默认值
    /// - 优先级: `Priority::Normal`
    /// - 栈大小: `STACK_SIZE` (配置文件中定义)
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            priority: Priority::default(),
            stack_size: STACK_SIZE,
        }
    }

    /// 设置任务优先级
    ///
    /// # 参数
    /// - `priority`: 任务优先级
    ///
    /// # 示例
    /// ```rust
    /// use neon_rtos2::kernel::task::{Task, Priority};
    ///
    /// Task::builder("high_priority_task")
    ///     .priority(Priority::High)
    ///     .spawn(|_| {});
    /// ``````
    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// 设置栈大小
    ///
    /// # 参数
    /// - `size`: 栈大小（字节）
    ///
    /// # 注意
    /// 栈大小应该是 8 字节对齐的，如果不是，会自动向上对齐。
    ///
    /// # 示例
    /// ```rust
    /// use neon_rtos2::kernel::task::Task;
    ///
    /// Task::builder("large_stack_task")
    ///     .stack_size(8192)
    ///     .spawn(|_| {});
    /// ```
    pub fn stack_size(mut self, size: usize) -> Self {
        // 确保 8 字节对齐
        self.stack_size = (size + 7) & !7;
        self
    }

    /// 获取配置的任务名称
    pub fn get_name(&self) -> &str {
        self.name
    }

    /// 获取配置的优先级
    pub fn get_priority(&self) -> Priority {
        self.priority
    }

    /// 获取配置的栈大小
    pub fn get_stack_size(&self) -> usize {
        self.stack_size
    }

    /// 创建并启动任务
    ///
    /// # 参数
    /// - `func`: 任务函数，可以是函数指针或闭包
    ///
    /// # 返回值
    /// - `Ok(Task)`: 成功创建的任务句柄
    /// - `Err(RtosError::TaskSlotsFull)`: 没有可用的任务槽位
    ///
    /// # 示例
    /// ```rust
    /// use neon_rtos2::kernel::task::{Task, Priority};
    ///
    /// let task = Task::builder("my_task")
    ///     .priority(Priority::High)
    ///     .spawn(|task_id| {
    ///         loop {
    ///             // 任务逻辑
    ///         }
    ///     });
    /// ```
    pub fn spawn<F>(self, func: F) -> Result<Task>
    where
        F: TaskFunction,
    {
        // 创建任务并设置优先级
        let mut task = Task::new(self.name, func)?;
        task.set_priority(self.priority);
        Ok(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;

    #[test]
    fn test_task_builder_default() {
        kernel_init();
        
        let builder = TaskBuilder::new("test_task");
        assert_eq!(builder.get_name(), "test_task");
        assert_eq!(builder.get_priority(), Priority::Normal);
        assert_eq!(builder.get_stack_size(), STACK_SIZE);
    }

    #[test]
    fn test_task_builder_custom_priority() {
        kernel_init();
        
        let builder = TaskBuilder::new("high_task")
            .priority(Priority::High);
        
        assert_eq!(builder.get_priority(), Priority::High);
    }

    #[test]
    fn test_task_builder_custom_stack_size() {
        kernel_init();
        
        let builder = TaskBuilder::new("large_stack")
            .stack_size(8192);
        
        assert_eq!(builder.get_stack_size(), 8192);
    }

    #[test]
    fn test_task_builder_stack_alignment() {
        kernel_init();
        
        // 测试非对齐的栈大小会被自动对齐
        let builder = TaskBuilder::new("align_test")
            .stack_size(1000); // 不是 8 的倍数
        
        assert_eq!(builder.get_stack_size() % 8, 0);
        assert!(builder.get_stack_size() >= 1000);
    }

    #[test]
    fn test_task_builder_chain() {
        kernel_init();
        
        let builder = TaskBuilder::new("chain_test")
            .priority(Priority::Critical)
            .stack_size(16384);
        
        assert_eq!(builder.get_name(), "chain_test");
        assert_eq!(builder.get_priority(), Priority::Critical);
        assert_eq!(builder.get_stack_size(), 16384);
    }

    #[test]
    fn test_task_builder_spawn() {
        kernel_init();
        
        let task = Task::builder("spawn_test")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        assert_eq!(task.get_name(), "spawn_test");
        assert_eq!(task.get_priority(), Priority::High);
    }

    #[test]
    fn test_task_builder_spawn_with_closure() {
        kernel_init();
        
        let task = Task::builder("closure_test")
            .spawn(|task_id| {
                let _ = task_id; // 使用 task_id
            })
            .unwrap();
        
        assert_eq!(task.get_name(), "closure_test");
    }
}

