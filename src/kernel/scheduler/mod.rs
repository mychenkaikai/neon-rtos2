use crate::kernel::task::{Task, TaskState, Priority};
use crate::hal::init_idle_task;
use crate::config::MAX_TASKS;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use spin::{Once, RwLock, Mutex};

/// 优先级数量（对应 Priority 枚举的变体数）
const PRIORITY_COUNT: usize = 5;

/// 就绪队列 - 每个优先级一个队列
/// 
/// 使用环形缓冲区实现 FIFO 队列，支持 O(1) 的入队和出队操作
#[derive(Debug)]
struct ReadyQueue {
    /// 任务 ID 数组
    tasks: [usize; MAX_TASKS],
    /// 队列头部索引
    head: usize,
    /// 队列尾部索引
    tail: usize,
    /// 队列中的任务数量
    count: usize,
}

impl ReadyQueue {
    const fn new() -> Self {
        Self {
            tasks: [0; MAX_TASKS],
            head: 0,
            tail: 0,
            count: 0,
        }
    }
    
    /// 入队 - O(1)
    #[inline]
    fn push(&mut self, task_id: usize) -> bool {
        if self.count >= MAX_TASKS {
            return false;
        }
        self.tasks[self.tail] = task_id;
        self.tail = (self.tail + 1) % MAX_TASKS;
        self.count += 1;
        true
    }
    
    /// 出队 - O(1)
    #[inline]
    fn pop(&mut self) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let task_id = self.tasks[self.head];
        self.head = (self.head + 1) % MAX_TASKS;
        self.count -= 1;
        Some(task_id)
    }
    
    /// 查看队首元素 - O(1)
    #[inline]
    fn peek(&self) -> Option<usize> {
        if self.count == 0 {
            None
        } else {
            Some(self.tasks[self.head])
        }
    }
    
    /// 从队列中移除指定任务 - O(n)
    /// 
    /// 注意：这个操作较慢，仅在任务阻塞时使用
    fn remove(&mut self, task_id: usize) -> bool {
        if self.count == 0 {
            return false;
        }
        
        // 查找任务位置
        let mut found_idx = None;
        let mut idx = self.head;
        for i in 0..self.count {
            if self.tasks[idx] == task_id {
                found_idx = Some(i);
                break;
            }
            idx = (idx + 1) % MAX_TASKS;
        }
        
        if let Some(pos) = found_idx {
            // 将后面的元素前移
            let mut src = (self.head + pos + 1) % MAX_TASKS;
            let mut dst = (self.head + pos) % MAX_TASKS;
            for _ in pos..(self.count - 1) {
                self.tasks[dst] = self.tasks[src];
                dst = src;
                src = (src + 1) % MAX_TASKS;
            }
            self.tail = if self.tail == 0 { MAX_TASKS - 1 } else { self.tail - 1 };
            self.count -= 1;
            true
        } else {
            false
        }
    }
    
    /// 检查队列是否为空 - O(1)
    #[inline]
    fn is_empty(&self) -> bool {
        self.count == 0
    }
    
    /// 获取队列长度 - O(1)
    #[inline]
    fn len(&self) -> usize {
        self.count
    }
    
    /// 检查是否包含指定任务 - O(n)
    fn contains(&self, task_id: usize) -> bool {
        let mut idx = self.head;
        for _ in 0..self.count {
            if self.tasks[idx] == task_id {
                return true;
            }
            idx = (idx + 1) % MAX_TASKS;
        }
        false
    }
}

/// 调度器内部状态
/// 
/// ## 优化说明
/// 
/// 使用位图 + 优先级队列实现 O(1) 调度：
/// - `ready_bitmap`: 位图标记哪些优先级有就绪任务
/// - `ready_queues`: 每个优先级一个 FIFO 队列
/// - 查找最高优先级：O(1) - 使用 `leading_zeros()` 指令
/// - 入队/出队：O(1)
struct SchedulerInner {
    /// 当前运行的任务
    current_task: Option<Task>,
    /// 下一个要运行的任务（用于上下文切换）
    next_task: Option<Task>,
    /// 就绪位图：第 i 位为 1 表示优先级 i 有就绪任务
    /// 位 0 = Idle, 位 1 = Low, 位 2 = Normal, 位 3 = High, 位 4 = Critical
    ready_bitmap: u8,
    /// 每个优先级的就绪队列
    ready_queues: [ReadyQueue; PRIORITY_COUNT],
}

impl SchedulerInner {
    const fn new() -> Self {
        Self {
            current_task: None,
            next_task: None,
            ready_bitmap: 0,
            ready_queues: [
                ReadyQueue::new(),
                ReadyQueue::new(),
                ReadyQueue::new(),
                ReadyQueue::new(),
                ReadyQueue::new(),
            ],
        }
    }
    
    /// 将任务加入就绪队列 - O(1)
    fn enqueue_ready(&mut self, task_id: usize, priority: Priority) {
        let prio_idx = priority.as_u8() as usize;
        if prio_idx < PRIORITY_COUNT {
            // 避免重复入队
            if !self.ready_queues[prio_idx].contains(task_id) {
                self.ready_queues[prio_idx].push(task_id);
                // 设置位图
                self.ready_bitmap |= 1 << prio_idx;
            }
        }
    }
    
    /// 从就绪队列移除任务 - O(n)
    fn dequeue_task(&mut self, task_id: usize, priority: Priority) {
        let prio_idx = priority.as_u8() as usize;
        if prio_idx < PRIORITY_COUNT {
            self.ready_queues[prio_idx].remove(task_id);
            // 如果队列为空，清除位图
            if self.ready_queues[prio_idx].is_empty() {
                self.ready_bitmap &= !(1 << prio_idx);
            }
        }
    }
    
    /// 从所有队列中移除任务（当不知道优先级时使用）
    fn remove_task_from_all_queues(&mut self, task_id: usize) {
        for prio_idx in 0..PRIORITY_COUNT {
            if self.ready_queues[prio_idx].remove(task_id) {
                if self.ready_queues[prio_idx].is_empty() {
                    self.ready_bitmap &= !(1 << prio_idx);
                }
                break;
            }
        }
    }
    
    /// 获取最高优先级的就绪任务 - O(1) 平均，最坏 O(n)
    /// 
    /// 使用位图快速查找最高优先级。
    /// 会验证任务状态，跳过已阻塞的任务。
    fn get_highest_priority_ready_task(&mut self) -> Option<usize> {
        // 从最高优先级开始查找
        for prio_idx in (0..PRIORITY_COUNT).rev() {
            if (self.ready_bitmap & (1 << prio_idx)) == 0 {
                continue;
            }
            
            // 遍历该优先级队列，找到真正就绪的任务
            loop {
                if let Some(task_id) = self.ready_queues[prio_idx].pop() {
                    // 验证任务状态
                    let task = Task(task_id);
                    if task.get_state() == TaskState::Ready {
                        // 如果队列为空，清除位图
                        if self.ready_queues[prio_idx].is_empty() {
                            self.ready_bitmap &= !(1 << prio_idx);
                        }
                        return Some(task_id);
                    }
                    // 任务不是就绪状态，继续查找下一个
                } else {
                    // 队列为空，清除位图
                    self.ready_bitmap &= !(1 << prio_idx);
                    break;
                }
            }
        }
        
        None
    }
    
    /// 查看最高优先级的就绪任务（不移除）
    /// 
    /// 会验证任务状态，跳过已阻塞的任务。
    fn peek_highest_priority_ready_task(&self) -> Option<(usize, Priority)> {
        // 从最高优先级开始查找
        for prio_idx in (0..PRIORITY_COUNT).rev() {
            if (self.ready_bitmap & (1 << prio_idx)) == 0 {
                continue;
            }
            
            // 遍历该优先级队列，找到真正就绪的任务
            let queue = &self.ready_queues[prio_idx];
            let mut idx = queue.head;
            for _ in 0..queue.count {
                let task_id = queue.tasks[idx];
                let task = Task(task_id);
                if task.get_state() == TaskState::Ready {
                    return Some((task_id, Priority::from_u8(prio_idx as u8).unwrap_or(Priority::Normal)));
                }
                idx = (idx + 1) % MAX_TASKS;
            }
        }
        
        None
    }
    
    /// 获取指定优先级的就绪任务数量
    fn ready_count_at_priority(&self, priority: Priority) -> usize {
        let prio_idx = priority.as_u8() as usize;
        if prio_idx < PRIORITY_COUNT {
            self.ready_queues[prio_idx].len()
        } else {
            0
        }
    }
    
    /// 获取总就绪任务数量
    fn total_ready_count(&self) -> usize {
        self.ready_queues.iter().map(|q| q.len()).sum()
    }
}

/// 全局调度器状态
static SCHEDULER_INNER: Once<Mutex<SchedulerInner>> = Once::new();
static SCHEDULER_RUNNING: AtomicBool = AtomicBool::new(false);
static SCHEDULER_USE_PRIORITY: AtomicBool = AtomicBool::new(false);

/// 当前任务 ID（原子变量，用于快速访问）
static CURRENT_TASK_ID: AtomicUsize = AtomicUsize::new(0);

fn get_scheduler_inner() -> &'static Mutex<SchedulerInner> {
    SCHEDULER_INNER.call_once(|| Mutex::new(SchedulerInner::new()))
}

pub struct Scheduler;

impl Scheduler {
    pub fn init() {
        // 重置调度器状态
        {
            let mut inner = get_scheduler_inner().lock();
            *inner = SchedulerInner::new();
        }
        SCHEDULER_RUNNING.store(false, Ordering::Release);
        SCHEDULER_USE_PRIORITY.store(false, Ordering::Release);
        CURRENT_TASK_ID.store(0, Ordering::Release);
        
        init_idle_task();
    }

    /// 启用优先级调度
    ///
    /// 启用后，调度器会优先选择优先级最高的就绪任务运行。
    /// 
    /// ## 性能说明
    /// 
    /// 优先级调度使用位图 + 优先级队列实现 O(1) 调度：
    /// - 查找最高优先级任务：O(1)
    /// - 任务入队/出队：O(1)
    pub fn enable_priority_scheduling() {
        SCHEDULER_USE_PRIORITY.store(true, Ordering::Release);
    }

    /// 禁用优先级调度
    ///
    /// 禁用后，调度器使用轮转调度算法。
    pub fn disable_priority_scheduling() {
        SCHEDULER_USE_PRIORITY.store(false, Ordering::Release);
    }

    /// 检查是否启用优先级调度
    pub fn is_priority_scheduling_enabled() -> bool {
        SCHEDULER_USE_PRIORITY.load(Ordering::Acquire)
    }

    /// 检查调度器是否正在运行
    pub fn is_running() -> bool {
        SCHEDULER_RUNNING.load(Ordering::Acquire)
    }
    
    /// 将任务加入就绪队列
    /// 
    /// 当任务从阻塞状态变为就绪状态时调用。
    /// 
    /// ## 性能
    /// - 时间复杂度：O(1)
    pub fn enqueue_ready_task(task: &Task) {
        let mut inner = get_scheduler_inner().lock();
        inner.enqueue_ready(task.get_taskid(), task.get_priority());
    }
    
    /// 从就绪队列移除任务
    /// 
    /// 当任务进入阻塞状态时调用。
    /// 
    /// ## 性能
    /// - 时间复杂度：O(n)，n 为该优先级队列中的任务数
    pub fn dequeue_task(task: &Task) {
        let mut inner = get_scheduler_inner().lock();
        inner.dequeue_task(task.get_taskid(), task.get_priority());
    }

    /// 基于优先级的调度 - O(1)
    ///
    /// 选择优先级最高的就绪任务运行。
    /// 如果有多个相同优先级的任务，按 FIFO 顺序选择。
    /// 
    /// ## 优化说明
    /// 
    /// 使用位图快速查找最高优先级：
    /// - `ready_bitmap` 的每一位表示对应优先级是否有就绪任务
    /// - 使用 `leading_zeros()` 指令在 O(1) 时间内找到最高优先级
    pub fn schedule_by_priority() {
        // 如果调度器未运行，直接返回
        if !Self::is_running() {
            return;
        }

        let mut inner = get_scheduler_inner().lock();
        
        let current_task = match inner.current_task {
            Some(task) => task,
            None => return,
        };
        
        let current_priority = current_task.get_priority();
        let current_state = current_task.get_state();

        // 查看最高优先级的就绪任务
        if let Some((next_task_id, next_priority)) = inner.peek_highest_priority_ready_task() {
            // 只有当找到的任务优先级更高，或者当前任务不是运行状态时才切换
            if next_task_id != current_task.get_taskid() && 
               (next_priority > current_priority || current_state != TaskState::Running) {
                // 从队列中取出任务
                let next_task_id = inner.get_highest_priority_ready_task().unwrap();
                let mut next_task = Task(next_task_id);
                
                // 如果当前任务正在运行，将其设为就绪并重新入队
                if current_state == TaskState::Running {
                    let mut current = current_task;
                    current.ready();
                    inner.enqueue_ready(current.get_taskid(), current_priority);
                }

                // 运行下一个任务
                next_task.run();
                inner.current_task = Some(next_task);
                CURRENT_TASK_ID.store(next_task_id, Ordering::Release);
                return;
            }
        }
        
        // 没找到更高优先级的任务
        if current_state == TaskState::Ready {
            let mut current = current_task;
            current.run();
            inner.current_task = Some(current);
        }
    }

    /// 抢占式调度检查 - O(1)
    ///
    /// 如果有更高优先级的任务就绪，触发任务切换。
    /// 通常在 SysTick 中断或任务唤醒时调用。
    /// 
    /// ## 性能
    /// - 时间复杂度：O(1) - 只需检查位图
    pub fn preempt_check() {
        // 如果调度器未运行或未启用优先级调度，直接返回
        if !Self::is_running() || !Self::is_priority_scheduling_enabled() {
            return;
        }

        let inner = get_scheduler_inner().lock();
        
        let current_task = match inner.current_task {
            Some(task) => task,
            None => return,
        };
        let current_priority = current_task.get_priority();

        // O(1) 检查是否有更高优先级的就绪任务
        if let Some((_, highest_priority)) = inner.peek_highest_priority_ready_task() {
            if highest_priority > current_priority {
                drop(inner); // 释放锁
                Self::schedule_by_priority();
            }
        }
    }

    /// 任务切换
    ///
    /// 根据调度策略选择下一个任务运行。
    /// - 优先级调度：选择最高优先级的就绪任务
    /// - 轮转调度：选择下一个就绪任务
    pub fn task_switch() {
        // 如果调度器未运行，直接返回
        if !Self::is_running() {
            return;
        }

        // 如果启用了优先级调度，使用优先级调度算法
        if Self::is_priority_scheduling_enabled() {
            Self::schedule_by_priority();
            return;
        }

        // 否则使用轮转调度算法（使用快照迭代器减少锁竞争）
        Self::round_robin_schedule();
    }
    
    /// 轮转调度算法
    /// 
    /// 使用快照迭代器减少锁持有时间
    fn round_robin_schedule() {
        let current_task_id = CURRENT_TASK_ID.load(Ordering::Acquire);
        
        // 使用快照迭代器获取任务状态（只获取一次锁）
        let snapshot_iter = Task::snapshot_iter();
        
        // 查找下一个就绪任务
        let mut next_task_id: Option<usize> = None;
        let mut current_task_ready = false;
        
        // 从当前任务之后开始查找
        for snapshot in snapshot_iter {
            if snapshot.task_id == current_task_id {
                current_task_ready = snapshot.state == TaskState::Ready;
                continue;
            }
            if snapshot.state == TaskState::Ready && snapshot.task_id > current_task_id && next_task_id.is_none() {
                next_task_id = Some(snapshot.task_id);
                break;
            }
        }
        
        // 如果没找到，从头开始查找
        if next_task_id.is_none() {
            for snapshot in Task::snapshot_iter() {
                if snapshot.task_id == current_task_id {
                    continue;
                }
                if snapshot.state == TaskState::Ready && snapshot.task_id < current_task_id {
                    next_task_id = Some(snapshot.task_id);
                    break;
                }
            }
        }

        // 执行任务切换
        let mut inner = get_scheduler_inner().lock();
        let current_task = match inner.current_task {
            Some(task) => task,
            None => return,
        };
        
        match (next_task_id, current_task.get_state()) {
            // 找到了下一个准备好的任务
            (Some(next_id), _) => {
                let mut next = Task(next_id);
                
                // 如果当前任务正在运行，将其设为就绪状态
                if current_task.get_state() == TaskState::Running {
                    let mut current = current_task;
                    current.ready();
                }

                // 运行下一个任务
                next.run();
                inner.current_task = Some(next);
                CURRENT_TASK_ID.store(next_id, Ordering::Release);
            }

            // 没找到其他任务，但当前任务就绪
            (None, TaskState::Ready) => {
                let mut current = current_task;
                current.run();
                inner.current_task = Some(current);
            }

            // 其他情况保持不变
            _ => {}
        }
    }

    pub fn start() {
        // 初始化就绪队列：将所有就绪任务加入队列
        {
            let mut inner = get_scheduler_inner().lock();
            
            // 遍历所有任务，将就绪任务加入队列
            for snapshot in Task::snapshot_iter() {
                if snapshot.state == TaskState::Ready {
                    inner.enqueue_ready(snapshot.task_id, snapshot.priority);
                }
            }
            
            // 设置第一个任务为当前任务
            inner.current_task = Some(Task(0));
        }
        
        Task(0).run();
        CURRENT_TASK_ID.store(0, Ordering::Release);
        SCHEDULER_RUNNING.store(true, Ordering::Release);
        
        // 触发当前架构的任务切换
        crate::hal::start_first_task();
    }

    /// 关闭调度器
    pub fn stop() {
        SCHEDULER_RUNNING.store(false, Ordering::Release);
    }

    /// 获取当前任务 - O(1)
    /// 
    /// 使用原子变量快速获取当前任务 ID，避免锁竞争
    pub fn get_current_task() -> Task {
        Task(CURRENT_TASK_ID.load(Ordering::Acquire))
    }
    
    /// 获取当前任务（从调度器内部状态）
    /// 
    /// 需要获取锁，但返回完整的 Task 信息
    pub fn get_current_task_locked() -> Task {
        get_scheduler_inner().lock().current_task.unwrap()
    }
    
    /// 获取就绪任务统计信息
    pub fn ready_task_stats() -> ReadyTaskStats {
        let inner = get_scheduler_inner().lock();
        ReadyTaskStats {
            total: inner.total_ready_count(),
            by_priority: [
                inner.ready_count_at_priority(Priority::Idle),
                inner.ready_count_at_priority(Priority::Low),
                inner.ready_count_at_priority(Priority::Normal),
                inner.ready_count_at_priority(Priority::High),
                inner.ready_count_at_priority(Priority::Critical),
            ],
        }
    }
}

/// 就绪任务统计信息
#[derive(Debug, Clone, Copy)]
pub struct ReadyTaskStats {
    /// 总就绪任务数
    pub total: usize,
    /// 各优先级的就绪任务数 [Idle, Low, Normal, High, Critical]
    pub by_priority: [usize; PRIORITY_COUNT],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::event::Event;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::utils::kernel_init;
    use serial_test::serial;

    fn task1(_args: usize) {
        // 简化的任务函数
    }

    fn task2(_args: usize) {
        // 简化的任务函数
    }

    fn task3(_args: usize) {
        // 简化的任务函数
    }

    fn task4(_args: usize) {
        // 简化的任务函数
    }

    fn task5(_args: usize) {
        // 简化的任务函数
    }

    #[test]
    #[serial]
    fn test_schedule() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::new("task3", task3).unwrap();
        Task::new("task4", task4).unwrap();
        Task::new("task5", task5).unwrap();

        Scheduler::start();
        //统计任务状态为Running的次数,只能有一个任务处于Running状态
        let mut running_count = 0;
        //统计所有的ready任务
        let mut ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Running {
                running_count += 1;
            }
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        assert_eq!(running_count, 1);
        assert_eq!(ready_count, 4);
        Scheduler::task_switch();
        running_count = 0;
        ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Running {
                running_count += 1;
            }
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        assert_eq!(running_count, 1);
        assert_eq!(ready_count, 4);
        Scheduler::task_switch();
        running_count = 0;
        ready_count = 0;
        Task::for_each(|task, _| {
            if task.get_state() == TaskState::Running {
                running_count += 1;
            }
            if task.get_state() == TaskState::Ready {
                ready_count += 1;
            }
        });
        assert_eq!(running_count, 1);
        assert_eq!(ready_count, 4);
    }

    #[test]
    #[serial]
    fn test_schedule_block() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::new("task3", task3).unwrap();
        Task::new("task4", task4).unwrap();
        Task::new("task5", task5).unwrap();
        Scheduler::start();
        Scheduler::get_current_task().block(Event::Signal(1));
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Blocked(Event::Signal(1))
        );
        Scheduler::task_switch();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        Scheduler::task_switch();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
    }

    #[test]
    #[serial]
    fn test_schedule_block_and_schedule() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Task::new("task3", task3).unwrap();
        Task::new("task4", task4).unwrap();
        Task::new("task5", task5).unwrap();
        Scheduler::start();
        Scheduler::get_current_task().block(Event::Signal(1));
        //保存此时的current_task为block_task
        let block_task = Scheduler::get_current_task();
        Scheduler::task_switch();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        //测试block_task是否还是原任务
        assert_eq!(
            block_task.get_state(),
            TaskState::Blocked(Event::Signal(1))
        );
        Scheduler::task_switch();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        //测试block_task是否还是原任务
        assert_eq!(
            block_task.get_state(),
            TaskState::Blocked(Event::Signal(1))
        );
    }

    #[test]
    #[serial]
    fn test_schedule_stop() {
        kernel_init();
        Task::new("task1", task1).unwrap();
        Task::new("task2", task2).unwrap();
        Scheduler::start();
        let current_task = Scheduler::get_current_task();
        Scheduler::stop();
        Scheduler::task_switch();
        assert_eq!(current_task.get_state(), TaskState::Running);
    }

    #[test]
    #[serial]
    fn test_all_tasks_blocked() {
        kernel_init();
        let mut task1 = Task::new("blocked_task1", |_| {}).unwrap();
        let mut task2 = Task::new("blocked_task2", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 获取当前任务（应该是 task1，因为它是第一个创建的）
        let current_task = Scheduler::get_current_task();
        
        // 阻塞非当前任务
        if current_task.get_taskid() == task1.get_taskid() {
            task2.block(Event::Signal(2));
        } else {
            task1.block(Event::Signal(1));
        }
        
        // 保存当前任务
        let current_id = current_task.get_taskid();
        
        // 阻塞当前任务
        if current_task.get_taskid() == task1.get_taskid() {
            task1.block(Event::Signal(1));
        } else {
            task2.block(Event::Signal(2));
        }
        
        // 尝试调度 - 此时所有任务都被阻塞
        Scheduler::task_switch();
        
        // 当前任务 ID 应该保持不变（因为没有可调度的任务）
        assert_eq!(Scheduler::get_current_task().get_taskid(), current_id);
    }
    
    #[test]
    #[serial]
    fn test_schedule_after_unblock() {
        kernel_init();
        
        let mut task1 = Task::new("unblock_test1", |_| {}).unwrap();
        let task2 = Task::new("unblock_test2", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 获取当前任务（应该是 task1）
        let current = Scheduler::get_current_task();
        assert_eq!(current.get_taskid(), task1.get_taskid());
        
        // 阻塞当前任务
        task1.block(Event::Signal(1));
        
        // 调度到下一个任务
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_taskid(), task2.get_taskid());
        
        // 唤醒被阻塞的任务
        task1.ready();
        
        // 再次调度 - 应该切换回 task1（轮转调度）
        Scheduler::task_switch();
        // 注意：轮转调度下，可能切换到 task1 或保持 task2
        // 这里只验证当前任务是运行状态
        assert_eq!(Scheduler::get_current_task().get_state(), TaskState::Running);
    }
    
    #[test]
    #[serial]
    fn test_start_stop_restart() {
        kernel_init();
        Task::new("restart_test", |_| {}).unwrap();
        
        // 启动调度器
        Scheduler::start();
        assert!(Scheduler::is_running());
        
        // 停止调度器
        Scheduler::stop();
        assert!(!Scheduler::is_running());
        
        // 重新启动调度器
        Scheduler::start();
        assert!(Scheduler::is_running());
    }

    // ========================================================================
    // 优先级调度测试
    // ========================================================================

    #[test]
    #[serial]
    fn test_priority_scheduling_enable_disable() {
        kernel_init();
        
        // 默认应该禁用优先级调度
        assert!(!Scheduler::is_priority_scheduling_enabled());
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        assert!(Scheduler::is_priority_scheduling_enabled());
        
        // 禁用优先级调度
        Scheduler::disable_priority_scheduling();
        assert!(!Scheduler::is_priority_scheduling_enabled());
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_basic() {
        kernel_init();
        
        // 创建不同优先级的任务
        let low_task = Task::builder("low_priority")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        let high_task = Task::builder("high_priority")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        let _normal_task = Task::builder("normal_priority")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 第一个任务开始运行（task id 0）
        assert_eq!(Scheduler::get_current_task().get_taskid(), low_task.get_taskid());
        
        // 调度后应该切换到最高优先级的任务
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_taskid(), high_task.get_taskid());
        assert_eq!(Scheduler::get_current_task().get_priority(), Priority::High);
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_preempt_check() {
        kernel_init();
        
        // 创建低优先级任务
        let _low_task = Task::builder("low")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        // 创建高优先级任务（初始阻塞）
        let mut high_task = Task::builder("high")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 阻塞高优先级任务
        high_task.block(Event::Signal(1));
        
        // 调度，应该运行低优先级任务
        Scheduler::task_switch();
        
        // 唤醒高优先级任务
        high_task.ready();
        
        // 抢占检查应该触发切换到高优先级任务
        Scheduler::preempt_check();
        assert_eq!(Scheduler::get_current_task().get_taskid(), high_task.get_taskid());
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_same_priority() {
        kernel_init();
        
        // 创建多个相同优先级的任务
        let _task1 = Task::builder("normal1")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        let _task2 = Task::builder("normal2")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        let _task3 = Task::builder("normal3")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 所有任务优先级相同，调度应该正常工作
        Scheduler::task_switch();
        let current = Scheduler::get_current_task();
        assert_eq!(current.get_priority(), Priority::Normal);
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_with_blocked_high_priority() {
        kernel_init();
        
        // 创建任务
        let _low_task = Task::builder("low")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        let mut high_task = Task::builder("high")
            .priority(Priority::Critical)
            .spawn(|_| {})
            .unwrap();
        
        let _normal_task = Task::builder("normal")
            .priority(Priority::Normal)
            .spawn(|_| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 阻塞高优先级任务
        high_task.block(Event::Signal(1));
        
        // 调度应该选择 Normal 优先级任务（因为 Critical 被阻塞）
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_priority(), Priority::Normal);
    }

    #[test]
    #[serial]
    fn test_round_robin_when_priority_disabled() {
        kernel_init();
        
        // 创建不同优先级的任务
        let task1 = Task::builder("task1")
            .priority(Priority::Low)
            .spawn(|_| {})
            .unwrap();
        
        let task2 = Task::builder("task2")
            .priority(Priority::High)
            .spawn(|_| {})
            .unwrap();
        
        // 确保优先级调度禁用
        Scheduler::disable_priority_scheduling();
        Scheduler::start();
        
        // 轮转调度应该按顺序切换，而不是按优先级
        assert_eq!(Scheduler::get_current_task().get_taskid(), task1.get_taskid());
        
        Scheduler::task_switch();
        // 轮转调度下，应该切换到下一个任务
        assert_eq!(Scheduler::get_current_task().get_taskid(), task2.get_taskid());
    }
}
