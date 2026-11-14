use crate::kernel::task::{get_task_list, Task, TaskState, Priority};
use crate::hal::init_idle_task;
use crate::config::MAX_TASKS;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::cell::RefCell;
use critical_section::{CriticalSection, Mutex};

/// 优先级数量（对应 Priority 枚举的变体数）
const PRIORITY_COUNT: usize = 5;

/// 就绪队列 - 每个优先级一个队列
/// 
/// 使用双向链表实现 FIFO 队列，支持 O(1) 的入队、出队和任意节点删除操作
#[derive(Debug)]
struct ReadyQueue {
    head: usize,
    tail: usize,
    count: usize,
    prev: [usize; MAX_TASKS],
    next: [usize; MAX_TASKS],
    in_queue: [bool; MAX_TASKS],
}

impl ReadyQueue {
    const fn new() -> Self {
        Self {
            head: usize::MAX,
            tail: usize::MAX,
            count: 0,
            prev: [usize::MAX; MAX_TASKS],
            next: [usize::MAX; MAX_TASKS],
            in_queue: [false; MAX_TASKS],
        }
    }
    
    /// 入队 - O(1)
    #[inline]
    fn push(&mut self, task_id: usize) -> bool {
        if task_id >= MAX_TASKS || self.in_queue[task_id] {
            return false;
        }
        
        self.in_queue[task_id] = true;
        self.next[task_id] = usize::MAX;
        
        if self.head == usize::MAX {
            self.head = task_id;
            self.prev[task_id] = usize::MAX;
        } else {
            self.next[self.tail] = task_id;
            self.prev[task_id] = self.tail;
        }
        self.tail = task_id;
        self.count += 1;
        true
    }
    
    /// 出队 - O(1)
    #[inline]
    fn pop(&mut self) -> Option<usize> {
        if self.head == usize::MAX {
            return None;
        }
        
        let task_id = self.head;
        self.in_queue[task_id] = false;
        
        self.head = self.next[task_id];
        if self.head == usize::MAX {
            self.tail = usize::MAX;
        } else {
            self.prev[self.head] = usize::MAX;
        }
        
        self.count -= 1;
        Some(task_id)
    }
    
    /// 查看队首元素 - O(1)
    #[inline]
    fn peek(&self) -> Option<usize> {
        if self.head == usize::MAX {
            None
        } else {
            Some(self.head)
        }
    }
    
    /// 从队列中移除指定任务 - O(1)
    fn remove(&mut self, task_id: usize) -> bool {
        if task_id >= MAX_TASKS || !self.in_queue[task_id] {
            return false;
        }
        
        self.in_queue[task_id] = false;
        
        let p = self.prev[task_id];
        let n = self.next[task_id];
        
        if p != usize::MAX {
            self.next[p] = n;
        } else {
            self.head = n;
        }
        
        if n != usize::MAX {
            self.prev[n] = p;
        } else {
            self.tail = p;
        }
        
        self.count -= 1;
        true
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
    
    /// 检查是否包含指定任务 - O(1)
    #[inline]
    fn contains(&self, task_id: usize) -> bool {
        if task_id < MAX_TASKS {
            self.in_queue[task_id]
        } else {
            false
        }
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
    // Each blocked task can wait on at most one mutex owner.
    waiting_on: [Option<usize>; MAX_TASKS],
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
            waiting_on: [None; MAX_TASKS],
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
            if self.ready_queues[prio_idx].push(task_id) {
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
                    let task = Task::from_slot(task_id);
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
            let mut curr = queue.head;
            while curr != usize::MAX {
                let task_id = curr;
                let task = Task::from_slot(task_id);
                if task.get_state() == TaskState::Ready {
                    return Some((task_id, Priority::from_u8(prio_idx as u8).unwrap_or(Priority::Normal)));
                }
                curr = queue.next[curr];
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

// 内核控制状态只在短临界区中访问。不能使用会被中断抢占的自旋锁。
static SCHEDULER_INNER: Mutex<RefCell<SchedulerInner>> =
    Mutex::new(RefCell::new(SchedulerInner::new()));
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static SCHEDULER_RUNNING: AtomicBool = AtomicBool::new(false);
static SCHEDULER_USE_PRIORITY: AtomicBool = AtomicBool::new(false);
static CURRENT_TASK_ID: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn set_current_task_id_for_test(id: usize) {
    CURRENT_TASK_ID.store(id, Ordering::Release);
}

impl SchedulerInner {
    fn apply_priority(&mut self, cs: CriticalSection<'_>, slot: usize, priority: Priority) {
        let tcb = &get_task_list()[slot];
        let old = tcb.get_priority();
        if old == priority { return; }
        if tcb.get_state(cs) == TaskState::Ready {
            self.dequeue_task(slot, old);
            self.enqueue_ready(slot, priority);
        }
        tcb.set_priority(priority);
    }

    // Rebuild from base priorities so removing an edge also revokes transitive donations.
    // No allocation. A simple chain has at most MAX_TASKS - 1 edges.
    fn refresh_inheritance(&mut self, cs: CriticalSection<'_>) {
        let mut effective: [Priority; MAX_TASKS] = core::array::from_fn(|id| get_task_list()[id].base_priority());
        for _ in 1..MAX_TASKS {
            let mut changed = false;
            for (waiter, owner) in self.waiting_on.iter().enumerate() {
                if let Some(owner) = *owner {
                    if effective[waiter] > effective[owner] {
                        effective[owner] = effective[waiter];
                        changed = true;
                    }
                }
            }
            if !changed { break; }
        }
        for (id, priority) in effective.into_iter().enumerate() {
            self.apply_priority(cs, id, priority);
        }
    }

    fn set_state(&mut self, cs: CriticalSection<'_>, task: Task, state: TaskState) {
        if self.waiting_on[task.0].is_some() && task.state_in(cs) != state {
            self.waiting_on[task.0] = None;
            self.refresh_inheritance(cs);
        }
        self.remove_task_from_all_queues(task.0);
        task.set_state_raw(cs, state);
        if state == TaskState::Ready {
            self.enqueue_ready(task.0, task.get_priority());
        }
    }

    fn switch_to(&mut self, cs: CriticalSection<'_>, next: Task) {
        if let Some(current) = self.current_task {
            if current != next && current.state_in(cs) == TaskState::Running {
                self.set_state(cs, current, TaskState::Ready);
            }
        }
        self.set_state(cs, next, TaskState::Running);
        self.current_task = Some(next);
        CURRENT_TASK_ID.store(next.0, Ordering::Release);
    }
}

pub struct Scheduler;

impl Scheduler {
    pub(crate) fn init() {
        critical_section::with(|cs| {
            *SCHEDULER_INNER.borrow_ref_mut(cs) = SchedulerInner::new();
            SCHEDULER_RUNNING.store(false, Ordering::Release);
            SCHEDULER_USE_PRIORITY.store(false, Ordering::Release);
            CURRENT_TASK_ID.store(0, Ordering::Release);
            init_idle_task();
            INITIALIZED.store(true, Ordering::Release);
        });
    }

    pub fn enable_priority_scheduling() {
        SCHEDULER_USE_PRIORITY.store(true, Ordering::Release);
    }

    pub fn disable_priority_scheduling() {
        SCHEDULER_USE_PRIORITY.store(false, Ordering::Release);
    }

    pub fn is_priority_scheduling_enabled() -> bool {
        SCHEDULER_USE_PRIORITY.load(Ordering::Acquire)
    }

    pub fn is_running() -> bool {
        SCHEDULER_RUNNING.load(Ordering::Acquire)
    }

    pub(crate) fn publish_task(cs: CriticalSection<'_>, task: Task, initial: TaskState) {
        SCHEDULER_INNER.borrow_ref_mut(cs).set_state(cs, task, initial);
    }

    pub(crate) fn finish_current() {
        critical_section::with(|cs| {
            let mut inner = SCHEDULER_INNER.borrow_ref_mut(cs);
            let current = inner.current_task.expect("task returned before scheduler start");
            assert_ne!(current.0, 0, "idle task returned");
            let mut changed = inner.waiting_on[current.0].take().is_some();
            for owner in &mut inner.waiting_on {
                if *owner == Some(current.0) { *owner = None; changed = true; }
            }
            if changed { inner.refresh_inheritance(cs); }
            inner.set_state(cs, current, TaskState::Finished);
            drop(inner);
            crate::sync::event::Event::wake_task(crate::sync::event::Event::TaskExit(current));
        });
    }

    // Task、Event 和优先级继承统一经过这里，状态和队列一次提交。
    pub(crate) fn set_task_state(task: Task, state: TaskState) {
        critical_section::with(|cs| {
            if task.is_live(cs) { SCHEDULER_INNER.borrow_ref_mut(cs).set_state(cs, task, state); }
        });
    }

    // All matching waiters leave their blocked state under the caller's single
    // critical section. Revoke their donations together, before FIFO insertion
    // at the final effective priorities. Other wait events keep their donations.
    pub(crate) fn wake_tasks_if(
        cs: CriticalSection<'_>,
        ids: impl Iterator<Item = usize> + Clone,
        event: crate::sync::event::Event,
    ) -> usize {
        let mut inner = SCHEDULER_INNER.borrow_ref_mut(cs);
        let blocked = TaskState::Blocked(event);
        let mut revoked = false;
        for id in ids.clone() {
            if id < MAX_TASKS && inner.waiting_on[id].is_some() && get_task_list()[id].get_state(cs) == blocked {
                inner.waiting_on[id] = None;
                revoked = true;
            }
        }
        if revoked {
            inner.refresh_inheritance(cs);
        }
        let mut count = 0;
        for id in ids {
            if id < MAX_TASKS && get_task_list()[id].get_state(cs) == blocked {
                let task = &get_task_list()[id];
                // Blocked tasks have already been removed from the ready queues.
                task.set_state(cs, TaskState::Ready);
                inner.enqueue_ready(id, task.get_priority());
                count += 1;
            }
        }
        count
    }

    pub(crate) fn set_task_priority(task: Task, priority: Priority) {
        critical_section::with(|cs| {
            let mut inner = SCHEDULER_INNER.borrow_ref_mut(cs);
            if !task.is_live(cs) { return; }
            task.set_base_priority(priority);
            if inner.waiting_on.iter().any(Option::is_some) {
                inner.refresh_inheritance(cs);
            } else {
                inner.apply_priority(cs, task.0, priority);
            }
        });

    }

    pub(crate) fn donate_to_owner(waiter: Task, owner: Task) {
        if waiter == owner { return; }
        critical_section::with(|cs| {
            let mut inner = SCHEDULER_INNER.borrow_ref_mut(cs);
            if !owner.is_live(cs) || !waiter.is_live(cs) { return; }
            inner.waiting_on[waiter.0] = Some(owner.0);
            inner.refresh_inheritance(cs);
        });
    }

    /// 由 HAL 保存当前上下文后调用；同优先级任务按 FIFO 轮转。
    fn select_by_priority() {
        critical_section::with(|cs| {
            if !Self::is_running() {
                return;
            }
            let mut inner = SCHEDULER_INNER.borrow_ref_mut(cs);
            let Some(current) = inner.current_task else { return };
            if let Some((_, priority)) = inner.peek_highest_priority_ready_task() {
                if current.get_state() != TaskState::Running || priority >= current.get_priority() {
                    if let Some(id) = inner.get_highest_priority_ready_task() {
                        inner.switch_to(cs, Task::from_slot(id));
                    }
                }
            } else if current.get_state() == TaskState::Ready {
                inner.switch_to(cs, current);
            }
        });
    }

    /// 仅请求硬件切换。当前任务 ID 必须保持到 HAL 保存旧上下文之后。
    pub fn preempt_check() {
        let preempt = critical_section::with(|cs| {
            if !Self::is_running() || !Self::is_priority_scheduling_enabled() {
                return false;
            }
            let inner = SCHEDULER_INNER.borrow_ref(cs);
            let Some(current) = inner.current_task else { return false };
            inner.peek_highest_priority_ready_task().is_some_and(|(_, priority)| {
                current.get_state() != TaskState::Running || priority > current.get_priority()
            })
        });
        if preempt {
            crate::hal::trigger_schedule();
        }
    }

    /// HAL 上下文切换入口。主机测试可显式调用以模拟切换。
    pub fn task_switch() {
        #[cfg(target_os = "none")]
        crate::hal::trigger_schedule();
        #[cfg(not(target_os = "none"))]
        Self::select_after_save();
    }

    pub fn schedule_by_priority() {
        #[cfg(target_os = "none")]
        crate::hal::trigger_schedule();
        #[cfg(not(target_os = "none"))]
        Self::select_by_priority();
    }

    pub(crate) fn select_after_save() {
        if Self::is_priority_scheduling_enabled() {
            Self::select_by_priority();
        } else {
            Self::round_robin_schedule();
        }
    }

    fn round_robin_schedule() {
        critical_section::with(|cs| {
            if !Self::is_running() { return; }
            let mut inner = SCHEDULER_INNER.borrow_ref_mut(cs);
            let Some(current) = inner.current_task else { return };
            for offset in 1..MAX_TASKS {
                let candidate = Task::from_slot((current.0 + offset) % MAX_TASKS);
                if candidate.get_state() == TaskState::Ready {
                    inner.switch_to(cs, candidate);
                    return;
                }
            }
            if current.get_state() == TaskState::Ready {
                inner.switch_to(cs, current);
            }
        });
    }

    pub fn start() {
        let started = critical_section::with(|cs| {
            if Self::is_running() { return false; }
            assert!(INITIALIZED.load(Ordering::Acquire), "kernel_init must precede scheduler startup");
            let mut inner = SCHEDULER_INNER.borrow_ref_mut(cs);
            // stop/start 也重建一致的队列，不留下多个 Running 任务。
            *inner = SchedulerInner::new();
            for task in Task::iter() {
                if matches!(task.get_state(), TaskState::Ready | TaskState::Running) {
                    inner.set_state(cs, task, TaskState::Ready);
                }
            }
            inner.switch_to(cs, Task::from_slot(0));
            SCHEDULER_RUNNING.store(true, Ordering::Release);
            true
        });
        if started {
            crate::hal::start_first_task();
        }
    }

    #[cfg(not(target_os = "none"))]
    pub fn stop() {
        SCHEDULER_RUNNING.store(false, Ordering::Release);
    }

    pub fn get_current_task() -> Task {
        Task::from_slot(CURRENT_TASK_ID.load(Ordering::Acquire))
    }

    pub fn get_current_task_locked() -> Task {
        critical_section::with(|cs| SCHEDULER_INNER.borrow_ref(cs).current_task.unwrap())
    }

    pub fn ready_task_stats() -> ReadyTaskStats {
        critical_section::with(|cs| {
            let inner = SCHEDULER_INNER.borrow_ref(cs);
            ReadyTaskStats {
                total: inner.total_ready_count(),
                by_priority: core::array::from_fn(|i| inner.ready_queues[i].len()),
            }
        })
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

    fn task1() {
        // 简化的任务函数
    }

    fn task2() {
        // 简化的任务函数
    }

    fn task3() {
        // 简化的任务函数
    }

    fn task4() {
        // 简化的任务函数
    }

    fn task5() {
        // 简化的任务函数
    }

    #[test]
    fn test_ready_queue_operations() {
        let mut q = ReadyQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);

        // push elements
        assert!(q.push(1));
        assert!(q.push(2));
        assert!(q.push(3));
        assert_eq!(q.len(), 3);
        assert!(!q.is_empty());
        
        // contains
        assert!(q.contains(1));
        assert!(q.contains(2));
        assert!(q.contains(3));
        assert!(!q.contains(4));
        
        // peek
        assert_eq!(q.peek(), Some(1));
        
        // duplicate push should fail
        assert!(!q.push(2));
        assert_eq!(q.len(), 3);
        
        // remove middle
        assert!(q.remove(2));
        assert_eq!(q.len(), 2);
        assert!(!q.contains(2));
        
        // pop
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.len(), 1);
        
        // remove tail
        assert!(q.remove(3));
        assert_eq!(q.len(), 0);
        assert!(q.is_empty());
        
        // pop from empty
        assert_eq!(q.pop(), None);
        
        // out of bounds remove
        assert!(!q.remove(MAX_TASKS));
        
        // push after empty
        assert!(q.push(4));
        assert_eq!(q.peek(), Some(4));
        assert_eq!(q.len(), 1);
        assert!(q.remove(4));
        assert_eq!(q.len(), 0);
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
        assert_eq!(ready_count, 5); // idle + 4 remaining tasks
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
        assert_eq!(ready_count, 5);
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
        assert_eq!(ready_count, 5);
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
        let mut task1 = Task::new("blocked_task1", || {}).unwrap();
        let mut task2 = Task::new("blocked_task2", || {}).unwrap();
        
        Scheduler::start();
        
        // 获取当前任务（应该是 task1，因为它是第一个创建的）
        let current_task = Scheduler::get_current_task();
        
        println!("current_task is {}, task1 is {}, task2 is {}", current_task.get_taskid(), task1.get_taskid(), task2.get_taskid());
        println!("before blocking: idle state {:?}, task1 state {:?}, task2 state {:?}", Task::from_slot(0).get_state(), task1.get_state(), task2.get_state());
        
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
        
        println!("after blocking: idle state {:?}, task1 state {:?}, task2 state {:?}", Task::from_slot(0).get_state(), task1.get_state(), task2.get_state());
        
        // 尝试调度 - 此时所有任务都被阻塞
        Scheduler::task_switch();
        
        println!("after switch: current_task is {}", Scheduler::get_current_task().get_taskid());
        
        // 当前任务 ID 应该保持不变（因为没有可调度的任务）
        assert_eq!(Scheduler::get_current_task().get_taskid(), current_id);
    }
    
    #[test]
    #[serial]
    fn test_schedule_after_unblock() {
        kernel_init();
        
        let mut task1 = Task::new("unblock_test1", || {}).unwrap();
        let task2 = Task::new("unblock_test2", || {}).unwrap();
        
        Scheduler::start();
        
        // idle runs first. switch to task1.
        Scheduler::task_switch();
        
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
        
        // 再次调度 - 应该切换回 idle (由于轮转)
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_state(), TaskState::Running);
    }
    
    #[test]
    #[serial]
    fn test_start_stop_restart() {
        kernel_init();
        Task::new("restart_test", || {}).unwrap();
        
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
        let _low_task = Task::builder("low_priority")
            .priority(Priority::Low)
            .spawn(|| {})
            .unwrap();
        
        let high_task = Task::builder("high_priority")
            .priority(Priority::High)
            .spawn(|| {})
            .unwrap();
        
        let _normal_task = Task::builder("normal_priority")
            .priority(Priority::Normal)
            .spawn(|| {})
            .unwrap();
        
        // 启用优先级调度
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        
        // 第一个任务开始运行（task id 0, idle_task）
        assert_eq!(Scheduler::get_current_task().get_taskid(), 0);
        
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
            .spawn(|| {})
            .unwrap();
        
        // 创建高优先级任务（初始阻塞）
        let mut high_task = Task::builder("high")
            .priority(Priority::High)
            .spawn(|| {})
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
        let before = Scheduler::get_current_task();
        Scheduler::preempt_check();
        assert_eq!(Scheduler::get_current_task(), before);
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task().get_taskid(), high_task.get_taskid());
    }

    #[test]
    #[serial]
    fn test_priority_scheduling_same_priority() {
        kernel_init();
        
        // 创建多个相同优先级的任务
        let _task1 = Task::builder("normal1")
            .priority(Priority::Normal)
            .spawn(|| {})
            .unwrap();
        
        let _task2 = Task::builder("normal2")
            .priority(Priority::Normal)
            .spawn(|| {})
            .unwrap();
        
        let _task3 = Task::builder("normal3")
            .priority(Priority::Normal)
            .spawn(|| {})
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
            .spawn(|| {})
            .unwrap();
        
        let mut high_task = Task::builder("high")
            .priority(Priority::Critical)
            .spawn(|| {})
            .unwrap();
        
        let _normal_task = Task::builder("normal")
            .priority(Priority::Normal)
            .spawn(|| {})
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
            .spawn(|| {})
            .unwrap();
        
        let task2 = Task::builder("task2")
            .priority(Priority::High)
            .spawn(|| {})
            .unwrap();
        
        // 确保优先级调度禁用
        Scheduler::disable_priority_scheduling();
        Scheduler::start();
        
        // idle runs first
        assert_eq!(Scheduler::get_current_task().get_taskid(), 0);
        
        Scheduler::task_switch();
        // 轮转调度应该按顺序切换，而不是按优先级
        assert_eq!(Scheduler::get_current_task().get_taskid(), task1.get_taskid());
        
        Scheduler::task_switch();
        // 轮转调度下，应该切换到下一个任务
        assert_eq!(Scheduler::get_current_task().get_taskid(), task2.get_taskid());
    }

    fn assert_ready_invariant() {
        let stats = Scheduler::ready_task_stats();
        let mut expected = [0; PRIORITY_COUNT];
        for task in Task::ready_tasks() {
            expected[task.get_priority().as_u8() as usize] += 1;
        }
        assert_eq!(stats.by_priority, expected);
        assert_eq!(stats.total, expected.iter().sum());
    }

    #[test]
    #[serial]
    fn a_returning_tasks_stack_is_only_reusable_after_switch() {
        use crate::kernel::time::{systick::{Deadline, Systick}, timer::Timer};
        kernel_init();
        let mut old = Task::builder("retiring").priority(Priority::Critical).spawn(|| {}).unwrap();
        for _ in 2..MAX_TASKS { Task::new("occupied", || {}).unwrap(); }
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        Scheduler::select_after_save();
        assert_eq!(Scheduler::get_current_task(), old);
        let event = Event::Signal(7);
        let _outstanding = Timer::register_task_timeout(Deadline::after(1), old, event).unwrap();
        Scheduler::finish_current();
        assert_eq!(Task::new("too early", || {}), Err(crate::error::RtosError::TaskSlotsFull));
        Scheduler::select_after_save();
        let mut next = Task::new("replacement", || {}).unwrap();
        assert_ne!(old, next);
        assert_eq!(old.0, next.0);
        next.block(event);
        old.ready();
        Systick::add_current_time(1);
        Timer::timer_check_and_send_event();
        assert_eq!(next.get_state(), TaskState::Blocked(event));
        assert_eq!(old.get_state(), TaskState::Finished);
        assert_ready_invariant();
    }

    #[test]
    #[serial]
    fn batch_wake_preserves_fifo_and_unrelated_donations() {
        kernel_init();
        let owner = Task::builder("owner").priority(Priority::Low).spawn(|| {}).unwrap();
        let mut first = Task::builder("first").priority(Priority::High).spawn(|| {}).unwrap();
        let mut second = Task::builder("second").priority(Priority::High).spawn(|| {}).unwrap();
        let mut other = Task::builder("other").priority(Priority::Normal).spawn(|| {}).unwrap();
        Scheduler::enable_priority_scheduling();
        Scheduler::start();

        let event = Event::Mutex(7);
        let other_event = Event::Mutex(8);
        for (task, reason) in [(&mut second, event), (&mut first, event), (&mut other, other_event)] {
            task.block(reason);
            Scheduler::donate_to_owner(*task, owner);
        }
        assert_eq!(owner.get_priority(), Priority::High);

        let ids = [second.0, MAX_TASKS, other.0, second.0, first.0, owner.0];
        let count = critical_section::with(|cs| Scheduler::wake_tasks_if(cs, ids.into_iter(), event));
        assert_eq!(count, 2);
        assert_eq!(other.get_state(), TaskState::Blocked(other_event));
        assert_eq!(owner.get_priority(), Priority::Normal);
        assert_ready_invariant();
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), second);
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), first);
        Event::wake_task(other_event);
        assert_eq!(owner.get_priority(), Priority::Low);
        assert_ready_invariant();
    }

    #[test]
    #[serial]
    fn ready_queue_tracks_spawn_block_wake_and_priority_changes() {
        kernel_init();
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        let mut task = Task::builder("late").priority(Priority::Low).spawn(|| {}).unwrap();
        assert_ready_invariant();
        task.block(Event::Signal(99));
        assert_ready_invariant();
        Scheduler::task_switch();
        assert!(Event::wake_task_by_id_if(task.0, Event::Signal(99)));
        assert_ready_invariant();
        task.set_priority(Priority::Critical);
        assert_ready_invariant();
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), task);
        assert_ready_invariant();
        task.block(Event::Signal(99));
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), Task::from_slot(0));
        assert_ready_invariant();
        Event::wake_task(Event::Signal(99));
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), task);
        assert_ready_invariant();
    }

    #[test]
    #[serial]
    fn equal_priority_tasks_rotate_without_running_lower_priority() {
        kernel_init();
        let first = Task::builder("first").priority(Priority::High).spawn(|| {}).unwrap();
        let second = Task::builder("second").priority(Priority::High).spawn(|| {}).unwrap();
        Task::builder("low").priority(Priority::Low).spawn(|| {}).unwrap();
        Scheduler::enable_priority_scheduling();
        Scheduler::start();
        for expected in [first, second, first, second] {
            Scheduler::task_switch();
            assert_eq!(Scheduler::get_current_task(), expected);
            assert_ready_invariant();
        }
    }

    #[test]
    #[serial]
    fn switching_policy_and_reprioritizing_preserves_queues() {
        kernel_init();
        let mut first = Task::new("first", || {}).unwrap();
        let second = Task::new("second", || {}).unwrap();
        Scheduler::start();
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), first);
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), second);
        first.set_priority(Priority::High);
        Scheduler::enable_priority_scheduling();
        Scheduler::task_switch();
        assert_eq!(Scheduler::get_current_task(), first);
        assert_ready_invariant();
    }

}
