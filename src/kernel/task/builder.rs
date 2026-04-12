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
///     .spawn(|_| {
///         // 任务逻辑
///     });
/// ```
pub struct TaskBuilder {
    name: &'static str,
    priority: Priority,
}

impl TaskBuilder {
    /// 创建新的任务构建器
    ///
    /// # 参数
    /// - `name`: 任务名称
    ///
    /// # 默认值
    /// - 优先级: `Priority::Normal`
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            priority: Priority::default(),
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

    /// 使用配置启动任务
    ///
    /// # 参数
    /// - `func`: 任务执行的闭包或函数
    ///
    /// # 返回值
    /// - `Ok(Task)`: 成功创建并返回任务句柄
    /// - `Err(RtosError)`: 创建失败（如任务槽已满）
    pub fn spawn<F>(self, func: F) -> Result<Task>
    where
        F: TaskFunction,
    {
        let mut task = Task::new(self.name, func)?;
        task.set_priority(self.priority);
        Ok(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_task_builder_default() {
        kernel_init();
        let builder = TaskBuilder::new("test_task");
        assert_eq!(builder.name, "test_task");
        assert_eq!(builder.priority, Priority::Normal);
    }

    #[test]
    #[serial]
    fn test_task_builder_priority() {
        kernel_init();
        let builder = TaskBuilder::new("high_priority")
            .priority(Priority::High);
        assert_eq!(builder.priority, Priority::High);
    }

    #[test]
    #[serial]
    fn test_task_builder_spawn() {
        kernel_init();
        
        let task = TaskBuilder::new("spawn_test")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
            
        assert_eq!(task.get_name(), "spawn_test");
        assert_eq!(task.get_priority(), Priority::High);
    }
}

