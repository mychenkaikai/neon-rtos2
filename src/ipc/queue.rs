use crate::compat::Arc;
use crate::config::MAX_MQS;
use crate::error::{Result, RtosError};
use crate::hal::trigger_schedule;
use crate::kernel::scheduler::Scheduler;
use crate::sync::event::Event;
use crate::sync::signal::WaiterList;
use core::cell::RefCell;
use critical_section::Mutex;

struct QueueRegistry {
    slots: [Option<usize>; MAX_MQS],
    next_id: usize,
}

static MQ_LIST: Mutex<RefCell<QueueRegistry>> = Mutex::new(RefCell::new(QueueRegistry {
    slots: [None; MAX_MQS],
    next_id: 0,
}));

struct QueueData<T, const N: usize> {
    buffer: [Option<T>; N],
    head: usize,
    tail: usize,
    count: usize,
    send_waiters: WaiterList,
    recv_waiters: WaiterList,
}

struct QueueStorage<T, const N: usize> {
    data: Mutex<RefCell<QueueData<T, N>>>,
    id: usize,
}

impl<T, const N: usize> Drop for QueueStorage<T, N> {
    fn drop(&mut self) {
        critical_section::with(|cs| {
            let mut registry = MQ_LIST.borrow_ref_mut(cs);
            for slot in &mut registry.slots {
                if *slot == Some(self.id) {
                    *slot = None;
                    break;
                }
            }
        });
    }
}

/// 可克隆的有界消息队列。阻塞期间不持有缓冲区的独占借用或临界区。
///
/// 将克隆的句柄传入不同任务；检查条件、登记等待者和阻塞一次完成。
pub struct Mq<T, const N: usize> {
    inner: Arc<QueueStorage<T, N>>,
    id: usize,
}

impl<T, const N: usize> Clone for Mq<T, N> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone(), id: self.id }
    }
}

impl<T, const N: usize> Mq<T, N> {
    pub fn new() -> Result<Self> {
        if N == 0 { return Err(RtosError::InvalidArgument); }
        let id = critical_section::with(|cs| -> Result<usize> {
            let mut registry = MQ_LIST.borrow_ref_mut(cs);
            let slot = registry.slots.iter().position(Option::is_none).ok_or(RtosError::QueueFull)?;
            let id = registry.next_id;
            registry.next_id = id.checked_add(1).ok_or(RtosError::OutOfMemory)?;
            registry.slots[slot] = Some(id);
            Ok(id)
        })?;
        Ok(Self {
            inner: Arc::new(QueueStorage {
                data: Mutex::new(RefCell::new(QueueData {
                    buffer: [const { None }; N], head: 0, tail: 0, count: 0,
                    send_waiters: WaiterList::new(), recv_waiters: WaiterList::new(),
                })),
                id,
            }),
            id,
        })
    }

    /// 仅在内核初始化或串行测试重置时调用；不复用事件标识。
    pub(crate) fn init() {
        critical_section::with(|cs| MQ_LIST.borrow_ref_mut(cs).slots.fill(None));
    }

    pub fn try_send(&self, data: T) -> core::result::Result<(), crate::ipc::TrySendError<T>> {
        critical_section::with(|cs| {
            let mut queue = self.inner.data.borrow_ref_mut(cs);
            if queue.count == N { return Err(crate::ipc::TrySendError::Full(data)); }
            self.push_inner(&mut queue, data);
            Ok(())
        })
    }

    /// Compatibility operation for copyable messages.
    pub fn push(&self, data: T) -> bool where T: Copy {
        self.try_send(data).is_ok()
    }

    pub fn pop(&self) -> Option<T> {
        critical_section::with(|cs| {
            let mut queue = self.inner.data.borrow_ref_mut(cs);
            if queue.count == 0 { return None; }
            Some(self.pop_inner(&mut queue))
        })
    }

    pub fn push_wait(&self, data: T) -> Result<()> {
        let mut pending = Some(data);
        loop {
            let sent = critical_section::with(|cs| -> Result<bool> {
                let mut queue = self.inner.data.borrow_ref_mut(cs);
                let mut current = Scheduler::get_current_task();
                queue.send_waiters.remove(current.0);
                if queue.count < N {
                    self.push_inner(&mut queue, pending.take().expect("pending message"));
                    return Ok(true);
                }
                Self::register_waiter(&mut queue.send_waiters, current.0)?;
                current.block(Event::Mq(self.id));
                Ok(false)
            })?;
            if sent { return Ok(()); }
            trigger_schedule();
        }
    }

    pub fn pop_wait(&self) -> Result<T> {
        loop {
            let value = critical_section::with(|cs| -> Result<Option<T>> {
                let mut queue = self.inner.data.borrow_ref_mut(cs);
                let mut current = Scheduler::get_current_task();
                queue.recv_waiters.remove(current.0);
                if queue.count > 0 { return Ok(Some(self.pop_inner(&mut queue))); }
                Self::register_waiter(&mut queue.recv_waiters, current.0)?;
                current.block(Event::Mq(self.id));
                Ok(None)
            })?;
            if let Some(value) = value { return Ok(value); }
            trigger_schedule();
        }
    }

    pub fn len(&self) -> usize {
        critical_section::with(|cs| self.inner.data.borrow_ref(cs).count)
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn is_full(&self) -> bool { self.len() == N }

    #[cfg(test)]
    fn register_sender_waiter(&self, task_id: usize) -> Result<()> {
        critical_section::with(|cs| Self::register_waiter(&mut self.inner.data.borrow_ref_mut(cs).send_waiters, task_id))
    }

    #[cfg(test)]
    fn register_receiver_waiter(&self, task_id: usize) -> Result<()> {
        critical_section::with(|cs| Self::register_waiter(&mut self.inner.data.borrow_ref_mut(cs).recv_waiters, task_id))
    }

    fn push_inner(&self, queue: &mut QueueData<T, N>, value: T) {
        queue.buffer[queue.tail] = Some(value);
        queue.tail = (queue.tail + 1) % N;
        queue.count += 1;
        self.wake_next(&mut queue.recv_waiters);
    }

    fn pop_inner(&self, queue: &mut QueueData<T, N>) -> T {
        let value = queue.buffer[queue.head].take().expect("occupied queue slot");
        queue.head = (queue.head + 1) % N;
        queue.count -= 1;
        self.wake_next(&mut queue.send_waiters);
        value
    }

    fn register_waiter(waiters: &mut WaiterList, id: usize) -> Result<()> {
        if waiters.contains(id) || waiters.push(id) { Ok(()) }
        else { Err(RtosError::WaiterQueueFull) }
    }

    fn wake_next(&self, waiters: &mut WaiterList) {
        while let Some(id) = waiters.pop_front() {
            if Event::wake_task_by_id_if(id, Event::Mq(self.id)) { break; }
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

    #[test]
    #[serial]
    fn full_queue_returns_the_owned_message() {
        kernel_init();
        let queue = Mq::<Box<u32>, 1>::new().unwrap();
        queue.try_send(Box::new(1)).unwrap();
        let frame = Box::new(2);
        let address = core::ptr::from_ref(&*frame);
        let Err(crate::ipc::TrySendError::Full(frame)) = queue.try_send(frame) else { panic!() };
        assert_eq!(core::ptr::from_ref(&*frame), address);
        assert_eq!(*queue.pop().unwrap(), 1);
        queue.try_send(frame).unwrap();
        assert_eq!(core::ptr::from_ref(&*queue.pop().unwrap()), address);
    }

    // 测试两个任务同时 push 和 pop
    #[test]
    #[serial]
    fn test_mq_multi_task() {
        kernel_init();

        let mut mq: Mq<u32, 10> = Mq::<u32, 10>::new().unwrap();

        // 任务要空着，不能有参数
        Task::new("task1", || {}).unwrap();
        Task::new("task2", || {}).unwrap();
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
        Task::new("task1", || {}).unwrap();
        Task::new("task2", || {}).unwrap();
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
        let mut receiver1 = Task::new("receiver1", || {}).unwrap();
        let mut receiver2 = Task::new("receiver2", || {}).unwrap();

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
        let mut sender1 = Task::new("sender1", || {}).unwrap();
        let mut sender2 = Task::new("sender2", || {}).unwrap();

        assert!(mq.push(11));

        mq.register_sender_waiter(sender1.get_taskid()).unwrap();
        mq.register_sender_waiter(sender2.get_taskid()).unwrap();

        sender1.block(Event::Mq(mq.id));
        sender2.block(Event::Mq(mq.id));

        assert_eq!(mq.pop(), Some(11));
        assert_eq!(sender1.get_state(), TaskState::Ready);
        assert_eq!(sender2.get_state(), TaskState::Blocked(Event::Mq(mq.id)));
    }

    #[test]
    #[serial]
    fn cloned_queue_shares_data_and_releases_slot_only_after_last_handle() {
        kernel_init();
        let queue = Mq::<u32, 1>::new().unwrap();
        let receiver = queue.clone();
        assert!(queue.push(42));
        drop(queue);
        assert_eq!(receiver.pop(), Some(42));
        let mut queues = Vec::new();
        for _ in 1..MAX_MQS { queues.push(Mq::<u32, 1>::new().unwrap()); }
        assert!(Mq::<u32, 1>::new().is_err());
        drop(receiver);
        assert!(Mq::<u32, 1>::new().is_ok());
        assert_eq!(Mq::<u32, 0>::new().err(), Some(RtosError::InvalidArgument));
    }

}
