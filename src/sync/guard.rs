//! RAII 互斥锁守卫
//!
//! 当 MutexGuard 被 drop 时，会自动释放锁，确保不会忘记解锁。

use super::Mutex;

/// RAII 互斥锁守卫
///
/// 持有 MutexGuard 期间，锁保持被持有状态。
/// 当 MutexGuard 离开作用域被 drop 时，锁会自动释放。
///
/// # 示例
///
/// ```rust
/// let mutex = Mutex::new()?;
/// {
///     let _guard = mutex.lock_guard();
///     // 临界区代码
///     // 离开作用域自动释放锁
/// }
/// ```
///
/// # 安全性
///
/// 即使在 panic 时，MutexGuard 也会正确释放锁，
/// 因为 Rust 的 drop 机制会在栈展开时调用 Drop::drop。
pub struct MutexGuard<'a> {
    mutex: &'a Mutex,
}

impl<'a> MutexGuard<'a> {
    /// 创建新的 MutexGuard
    ///
    /// 这是一个内部方法，用户应该通过 `Mutex::lock_guard()` 获取守卫。
    pub(crate) fn new(mutex: &'a Mutex) -> Self {
        Self { mutex }
    }

    /// 获取关联的 Mutex 引用
    pub fn mutex(&self) -> &Mutex {
        self.mutex
    }
}

impl Drop for MutexGuard<'_> {
    fn drop(&mut self) {
        // 自动释放锁
        // 忽略错误，因为在 drop 中不应该 panic
        // 如果解锁失败（例如当前任务不是锁的持有者），这通常表示程序逻辑错误
        let _ = self.mutex.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::Task;
    use crate::utils::kernel_init;

    #[test]
    fn test_mutex_guard_auto_unlock() {
        kernel_init();
        let mutex = Mutex::new().unwrap();
        Task::new("guard_test", |_| {}).unwrap();
        Scheduler::start();

        // 使用 guard 获取锁
        {
            let _guard = mutex.lock_guard();
            // 锁已被持有
        }
        // 离开作用域，锁应该已被自动释放

        // 再次获取锁应该成功
        {
            let _guard = mutex.lock_guard();
            // 如果锁没有被释放，这里会阻塞
        }
    }

    #[test]
    fn test_mutex_with_lock() {
        kernel_init();
        let mutex = Mutex::new().unwrap();
        Task::new("with_lock_test", |_| {}).unwrap();
        Scheduler::start();

        // 使用 with_lock 闭包风格
        let result = mutex.with_lock(|| {
            // 临界区代码
            42
        });

        assert_eq!(result, 42);

        // 锁应该已被释放，可以再次获取
        let result2 = mutex.with_lock(|| 100);
        assert_eq!(result2, 100);
    }

    #[test]
    fn test_mutex_guard_nested_scope() {
        kernel_init();
        let mutex = Mutex::new().unwrap();
        Task::new("nested_test", |_| {}).unwrap();
        Scheduler::start();

        let mut value = 0;

        {
            let _guard = mutex.lock_guard();
            value += 1;

            // 嵌套作用域
            {
                value += 10;
            }

            value += 100;
        }
        // 锁在这里释放

        assert_eq!(value, 111);
    }
}

