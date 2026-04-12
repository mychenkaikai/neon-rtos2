use crate::config::MAX_MQS;
use crate::error::{Result, RtosError};
use crate::hal::trigger_schedule;
use crate::kernel::scheduler::Scheduler;
use crate::sync::event::Event;
use crate::sync::signal::WaiterList;
use core::mem::MaybeUninit;

// 全局变量数组，用于给 mq 分配 id
static mut MQ_LIST: [Option<QueueInner>; MAX_MQS] = [None; MAX_MQS];

#[derive(Copy, Clone)]
struct QueueInner {
    id: usize,
}

/// 消息队列
/// 
/// 仿照 mutex，实现任务间的阻塞机制
pub struct Mq<T, const N: usize> {
    buffer: [MaybeUninit<T>; N],
    head: usize,
    tail: usize,
    count: usize,
    send_waiters: WaiterList,
    recv_waiters: WaiterList,
    id: usize,
}

impl<T, const N: usize> Mq<T, N>
where
    T: Copy + Default + Sized,
{
    /// 创建一个消息队列
    /// 
    /// # 返回值
    /// - `Ok(Mq)` - 成功创建消息队列
    /// - `Err(RtosError::QueueFull)` - 没有可用的消息队列槽位
    pub fn new() -> Result<Self> {
        let mut id: Option<usize> = None;
        unsafe {
            for i in 0..MAX_MQS {
                if MQ_LIST[i].is_none() {
                    MQ_LIST[i] = Some(QueueInner { id: i });
                    id = Some(MQ_LIST[i].unwrap().id);
                    break;
                }
            }
        }
        
        let id = id.ok_or(RtosError::QueueFull)?;

        Ok(Mq {
            buffer: [MaybeUninit::uninit(); N],
            head: 0,
            tail: 0,
            count: 0,
            send_waiters: WaiterList::new(),
            recv_waiters: WaiterList::new(),
            id,
        })
    }

    /// 初始化消息队列列表
    pub fn init() {
        unsafe {
            for i in 0..MAX_MQS {
                MQ_LIST[i] = None;
            }
        }
    }

    /// 向队列中推送数据
    /// 
    /// # 返回值
    /// - `true` - 成功推送
    /// - `false` - 队列已满或被锁定
    pub fn push(&mut self, data: T) -> bool {
        if self.count == N {
            return false;
        }

        self.push_inner(data);
        true
    }

    /// 从队列中弹出数据
    /// 
    /// # 返回值
    /// - `Some(T)` - 成功弹出数据
    /// - `None` - 队列为空或被锁定
    pub fn pop(&mut self) -> Option<T> {
        if self.count == 0 {
            return None;
        }

        Some(self.pop_inner())
    }

    pub fn push_wait(&mut self, data: T) -> Result<()> {
        loop {
            if self.count < N {
                self.push_inner(data);
                return Ok(());
            }

            self.wait_as_sender()?;
        }
    }

    pub fn pop_wait(&mut self) -> Result<T> {
        loop {
            if self.count > 0 {
                return Ok(self.pop_inner());
            }

            self.wait_as_receiver()?;
        }
    }

    /// 获取队列当前元素数量
    pub fn len(&self) -> usize {
        self.count
    }

    /// 检查队列是否为空
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 检查队列是否已满
    pub fn is_full(&self) -> bool {
        self.count == N
    }

    pub(crate) fn register_sender_waiter(&mut self, task_id: usize) -> Result<()> {
        Self::register_waiter(&mut self.send_waiters, task_id)
    }

    pub(crate) fn register_receiver_waiter(&mut self, task_id: usize) -> Result<()> {
        Self::register_waiter(&mut self.recv_waiters, task_id)
    }

    fn event(&self) -> Event {
        Event::Mq(self.id)
    }

    fn push_inner(&mut self, data: T) {
        unsafe {
            *self.buffer.get_unchecked_mut(self.tail) = MaybeUninit::new(data);
            self.count += 1;
            self.tail = (self.tail + 1) % N;
        }
        self.wake_next_receiver();
    }

    fn pop_inner(&mut self) -> T {
        let ret;
        unsafe {
            ret = self.buffer.get_unchecked(self.head).assume_init();
            self.count -= 1;
            self.head = (self.head + 1) % N;
        }
        self.wake_next_sender();
        ret
    }

    fn wait_as_sender(&mut self) -> Result<()> {
        let mut current = Scheduler::get_current_task();
        self.register_sender_waiter(current.get_taskid())?;
        current.block(self.event());
        trigger_schedule();
        Ok(())
    }

    fn wait_as_receiver(&mut self) -> Result<()> {
        let mut current = Scheduler::get_current_task();
        self.register_receiver_waiter(current.get_taskid())?;
        current.block(self.event());
        trigger_schedule();
        Ok(())
    }

    fn register_waiter(waiters: &mut WaiterList, task_id: usize) -> Result<()> {
        if waiters.contains(task_id) {
            return Ok(());
        }

        if waiters.push(task_id) {
            Ok(())
        } else {
            Err(RtosError::WaiterQueueFull)
        }
    }

    fn wake_next_sender(&mut self) -> bool {
        while let Some(task_id) = self.send_waiters.pop_front() {
            if Event::wake_task_by_id_if(task_id, self.event()) {
                return true;
            }
        }
        false
    }

    fn wake_next_receiver(&mut self) -> bool {
        while let Some(task_id) = self.recv_waiters.pop_front() {
            if Event::wake_task_by_id_if(task_id, self.event()) {
                return true;
            }
        }
        false
    }
}

impl<T, const N: usize> Drop for Mq<T, N> {
    /// 当 Mq 被 drop 时，自动释放槽位
    ///
    /// 这允许槽位被后续的 Mq::new() 重用
    fn drop(&mut self) {
        unsafe {
            MQ_LIST[self.id] = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::{Task, TaskState};
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_mq() {
        kernel_init();

        // 使用非可变静态数组
        let mut mq: Mq<u32, 10> = Mq::<u32, 10>::new().unwrap();

        assert_eq!(mq.push(1), true);
        assert_eq!(mq.push(2), true);
        assert_eq!(mq.push(3), true);
        assert_eq!(mq.push(4), true);
        assert_eq!(mq.push(5), true);
        assert_eq!(mq.push(6), true);
        assert_eq!(mq.push(7), true);
        assert_eq!(mq.push(8), true);
        assert_eq!(mq.push(9), true);
        assert_eq!(mq.push(10), true);
        assert_eq!(mq.push(11), false);

        assert_eq!(mq.pop(), Some(1));
        assert_eq!(mq.pop(), Some(2));
        assert_eq!(mq.pop(), Some(3));
        assert_eq!(mq.pop(), Some(4));
        assert_eq!(mq.pop(), Some(5));
        assert_eq!(mq.pop(), Some(6));
        assert_eq!(mq.pop(), Some(7));
        assert_eq!(mq.pop(), Some(8));
        assert_eq!(mq.pop(), Some(9));
        assert_eq!(mq.pop(), Some(10));
        assert_eq!(mq.pop(), None);
    }

    // 测试两个任务同时 push 和 pop
    #[test]
    #[serial]
    fn test_mq_multi_task() {
        kernel_init();

        let mut mq: Mq<u32, 10> = Mq::<u32, 10>::new().unwrap();

        // 任务要空着，不能有参数
        Task::new("task1", |_| {}).unwrap();
        Task::new("task2", |_| {}).unwrap();
        Scheduler::start();

        mq.push(1);
        mq.push(2);

        Scheduler::task_switch();

        assert_eq!(mq.pop(), Some(1));
        assert_eq!(mq.pop(), Some(2));
    }

    // 测试两个任务同时 push 和 pop，但是一个任务先 push，一个任务后 push
    #[test]
    #[serial]
    fn test_mq_multi_task_push_pop() {
        kernel_init();

        let mut mq: Mq<u32, 10> = Mq::<u32, 10>::new().unwrap();
        Task::new("task1", |_| {}).unwrap();
        Task::new("task2", |_| {}).unwrap();
        Scheduler::start();
        // 测试可能冲突的情况
        mq.push(1);
        mq.push(2);
        Scheduler::task_switch();
        assert_eq!(mq.pop(), Some(1));
        assert_eq!(mq.pop(), Some(2));
    }

    #[test]
    #[serial]
    fn test_mq_len_and_empty() {
        kernel_init();
        
        let mut mq: Mq<u32, 5> = Mq::<u32, 5>::new().unwrap();
        
        assert_eq!(mq.len(), 0);
        assert!(mq.is_empty());
        assert!(!mq.is_full());
        
        mq.push(1);
        mq.push(2);
        
        assert_eq!(mq.len(), 2);
        assert!(!mq.is_empty());
        assert!(!mq.is_full());
        
        mq.push(3);
        mq.push(4);
        mq.push(5);
        
        assert_eq!(mq.len(), 5);
        assert!(mq.is_full());
    }

    #[test]
    #[serial]
    fn test_mq_slots_full() {
        kernel_init();
        
        // 创建最大数量的消息队列
        let mut queues = Vec::new();
        for i in 0..MAX_MQS {
            let mq: Result<Mq<u32, 4>> = Mq::new();
            assert!(mq.is_ok(), "Mq {} should be created successfully", i);
            queues.push(mq.unwrap());
        }
        
        // 再创建一个应该失败
        let result: Result<Mq<u32, 4>> = Mq::new();
        assert_eq!(result.err(), Some(RtosError::QueueFull));
    }

    #[test]
    #[serial]
    fn test_mq_push_wakes_one_waiting_receiver() {
        kernel_init();

        let mut mq: Mq<u32, 2> = Mq::new().unwrap();
        let mut receiver1 = Task::new("receiver1", |_| {}).unwrap();
        let mut receiver2 = Task::new("receiver2", |_| {}).unwrap();

        mq.register_receiver_waiter(receiver1.get_taskid()).unwrap();
        mq.register_receiver_waiter(receiver2.get_taskid()).unwrap();

        receiver1.block(Event::Mq(mq.id));
        receiver2.block(Event::Mq(mq.id));

        assert!(mq.push(7));
        assert_eq!(receiver1.get_state(), TaskState::Ready);
        assert_eq!(receiver2.get_state(), TaskState::Blocked(Event::Mq(mq.id)));
    }

    #[test]
    #[serial]
    fn test_mq_pop_wakes_one_waiting_sender() {
        kernel_init();

        let mut mq: Mq<u32, 1> = Mq::new().unwrap();
        let mut sender1 = Task::new("sender1", |_| {}).unwrap();
        let mut sender2 = Task::new("sender2", |_| {}).unwrap();

        assert!(mq.push(11));

        mq.register_sender_waiter(sender1.get_taskid()).unwrap();
        mq.register_sender_waiter(sender2.get_taskid()).unwrap();

        sender1.block(Event::Mq(mq.id));
        sender2.block(Event::Mq(mq.id));

        assert_eq!(mq.pop(), Some(11));
        assert_eq!(sender1.get_state(), TaskState::Ready);
        assert_eq!(sender2.get_state(), TaskState::Blocked(Event::Mq(mq.id)));
    }
}
