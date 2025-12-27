//! # 异步执行器
//!
//! 提供简单的异步任务执行器，适合嵌入式环境使用。
//!
//! ## 设计原则
//!
//! - **简单**: 单线程执行，无需复杂的同步
//! - **轻量**: 最小化内存占用
//! - **可预测**: 确定性的执行顺序

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use crate::compat::{Box, VecDeque};
use super::waker::TaskWaker;

/// 异步任务包装器
///
/// 将 Future 包装为可执行的任务
pub struct AsyncTask {
    /// 被包装的 Future
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    /// 任务 ID，用于创建 Waker
    task_id: usize,
}

impl AsyncTask {
    /// 创建新的异步任务
    pub fn new<F>(future: F, task_id: usize) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            future: Box::pin(future),
            task_id,
        }
    }
}

/// 异步执行器
///
/// 管理和执行异步任务的简单执行器。
///
/// # 示例
///
/// ```rust,ignore
/// let mut executor = Executor::new();
///
/// executor.spawn(async {
///     // 异步任务逻辑
/// });
///
/// executor.run();
/// ```
pub struct Executor {
    /// 就绪队列
    ready_queue: VecDeque<AsyncTask>,
    /// 下一个任务 ID
    next_task_id: usize,
}

impl Executor {
    /// 创建新的执行器
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            next_task_id: 0,
        }
    }

    /// 添加异步任务
    ///
    /// # 参数
    /// - `future`: 要执行的 Future
    ///
    /// # 返回值
    /// 任务 ID
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let task_id = executor.spawn(async {
    ///     // 异步逻辑
    /// });
    /// ```
    pub fn spawn<F>(&mut self, future: F) -> usize
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        
        let task = AsyncTask::new(future, task_id);
        self.ready_queue.push_back(task);
        
        task_id
    }

    /// 运行执行器
    ///
    /// 持续执行就绪队列中的任务，直到所有任务完成。
    ///
    /// # 注意
    ///
    /// 在嵌入式环境中，通常不会返回，而是在空闲时进入低功耗模式。
    pub fn run(&mut self) {
        while let Some(mut task) = self.ready_queue.pop_front() {
            let waker = TaskWaker::new(task.task_id);
            let mut cx = Context::from_waker(&waker);
            
            match task.future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {
                    // 任务完成，不再重新入队
                }
                Poll::Pending => {
                    // 任务未完成，重新入队等待下次调度
                    self.ready_queue.push_back(task);
                }
            }
        }
    }

    /// 执行一轮调度
    ///
    /// 只执行一次就绪队列中的任务，然后返回。
    /// 适合与 RTOS 调度器集成使用。
    ///
    /// # 返回值
    /// - `true`: 还有待执行的任务
    /// - `false`: 所有任务已完成
    pub fn poll_once(&mut self) -> bool {
        if let Some(mut task) = self.ready_queue.pop_front() {
            let waker = TaskWaker::new(task.task_id);
            let mut cx = Context::from_waker(&waker);
            
            match task.future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {
                    // 任务完成
                }
                Poll::Pending => {
                    // 任务未完成，重新入队
                    self.ready_queue.push_back(task);
                }
            }
        }
        
        !self.ready_queue.is_empty()
    }

    /// 获取就绪队列中的任务数量
    pub fn pending_count(&self) -> usize {
        self.ready_queue.len()
    }

    /// 检查执行器是否为空
    pub fn is_empty(&self) -> bool {
        self.ready_queue.is_empty()
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_executor_creation() {
        let executor = Executor::new();
        assert!(executor.is_empty());
        assert_eq!(executor.pending_count(), 0);
    }

    #[test]
    fn test_executor_spawn() {
        let mut executor = Executor::new();
        
        let task_id = executor.spawn(async {});
        assert_eq!(task_id, 0);
        assert_eq!(executor.pending_count(), 1);
        
        let task_id2 = executor.spawn(async {});
        assert_eq!(task_id2, 1);
        assert_eq!(executor.pending_count(), 2);
    }

    #[test]
    fn test_executor_run_simple() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        
        let mut executor = Executor::new();
        
        executor.spawn(async {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        });
        
        executor.spawn(async {
            COUNTER.fetch_add(10, Ordering::SeqCst);
        });
        
        executor.run();
        
        assert_eq!(COUNTER.load(Ordering::SeqCst), 11);
        assert!(executor.is_empty());
    }

    #[test]
    fn test_executor_poll_once() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        
        let mut executor = Executor::new();
        
        executor.spawn(async {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        });
        
        executor.spawn(async {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        });
        
        // 第一次 poll
        let has_more = executor.poll_once();
        assert!(has_more); // 还有一个任务
        
        // 第二次 poll
        let has_more = executor.poll_once();
        assert!(!has_more); // 所有任务完成
        
        assert_eq!(COUNTER.load(Ordering::SeqCst), 2);
    }
}

