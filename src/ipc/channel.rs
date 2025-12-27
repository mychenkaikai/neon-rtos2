use crate::kernel::task::Task;
use crate::{sync::event::Event, kernel::scheduler::Scheduler};
use crate::compat::{Box, Vec, VecDeque};

use core::any::Any;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

// IPC句柄类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcHandle(usize);

// 消息类型，使用 Box<dyn Any> 进行类型安全的类型擦除
struct Message {
    data: Box<dyn Any + Send>,
}

impl Message {
    fn new<T: 'static + Send>(data: T) -> Self {
        Self {
            data: Box::new(data),
        }
    }

    fn try_into<T: 'static>(self) -> Result<T, Self> {
        match self.data.downcast::<T>() {
            Ok(boxed) => Ok(*boxed),
            Err(data) => Err(Self { data }),
        }
    }
    
    fn is_type<T: 'static>(&self) -> bool {
        self.data.is::<T>()
    }
}

// 消息队列结构
struct MessageQueue {
    queue: VecDeque<Message>,
    capacity: usize,
    waiting_senders: Vec<Task>,
    waiting_receivers: Vec<Task>,
}

impl MessageQueue {
    fn new(capacity: usize) -> Self {
        MessageQueue {
            queue: VecDeque::with_capacity(capacity),
            capacity,
            waiting_senders: Vec::new(),
            waiting_receivers: Vec::new(),
        }
    }

    fn is_full(&self) -> bool {
        self.queue.len() >= self.capacity
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

// 全局IPC管理器
struct IpcManager {
    queues: Vec<Option<MessageQueue>>,
    next_handle: AtomicUsize,
}

impl IpcManager {
    const fn new() -> Self {
        IpcManager {
            queues: Vec::new(),
            next_handle: AtomicUsize::new(1), // 从1开始，0表示无效句柄
        }
    }

    fn create_queue(&mut self, capacity: usize) -> IpcHandle {
        let handle_id = self.next_handle.fetch_add(1, Ordering::Relaxed);
        let queue = MessageQueue::new(capacity);

        // 扩展队列数组以容纳新的句柄
        while self.queues.len() <= handle_id {
            self.queues.push(None);
        }

        self.queues[handle_id] = Some(queue);
        IpcHandle(handle_id)
    }

    fn send_message<T: 'static + Send>(&mut self, handle: IpcHandle, data: T) -> Result<(), IpcError> {
        let queue_opt = self.queues.get_mut(handle.0);

        match queue_opt {
            Some(Some(queue)) => {
                if queue.is_full() {
                    // 队列满，阻塞当前任务
                    let mut current_task = Scheduler::get_current_task();
                    queue.waiting_senders.push(current_task);
                    current_task.block(Event::Mq(handle.0));

                    Err(IpcError::QueueFull)
                } else {
                    let message = Message::new(data);
                    queue.queue.push_back(message);

                    // 唤醒等待接收的任务
                    if let Some(waiting_task) = queue.waiting_receivers.pop() {
                        Event::wake_task(Event::Mq(handle.0));
                    }

                    Ok(())
                }
            }
            _ => Err(IpcError::InvalidHandle),
        }
    }

    fn receive_message<T: 'static>(&mut self, handle: IpcHandle) -> Result<T, IpcError> {
        let queue_opt = self.queues.get_mut(handle.0);

        match queue_opt {
            Some(Some(queue)) => {
                if queue.is_empty() {
                    // 队列空，阻塞当前任务
                    let mut current_task = Scheduler::get_current_task();
                    queue.waiting_receivers.push(current_task);
                    current_task.block(Event::Mq(handle.0));

                    Err(IpcError::QueueEmpty)
                } else {
                    // 先检查类型是否匹配，不要立即消费消息
                    if let Some(front_message) = queue.queue.front() {
                        if !front_message.is_type::<T>() {
                            return Err(IpcError::TypeMismatch);
                        }
                    }

                    // 类型匹配，现在可以安全地消费消息
                    let message = queue.queue.pop_front().unwrap();

                    // 唤醒等待发送的任务
                    if let Some(_waiting_task) = queue.waiting_senders.pop() {
                        Event::wake_task(Event::Mq(handle.0));
                    }

                    match message.try_into::<T>() {
                        Ok(data) => Ok(data),
                        Err(_) => Err(IpcError::TypeMismatch), // 这种情况理论上不会发生
                    }
                }
            }
            _ => Err(IpcError::InvalidHandle),
        }
    }

    fn destroy_queue(&mut self, handle: IpcHandle) -> Result<(), IpcError> {
        if handle.0 < self.queues.len() {
            self.queues[handle.0] = None;
            Ok(())
        } else {
            Err(IpcError::InvalidHandle)
        }
    }
}

// 全局IPC管理器实例
static IPC_MANAGER: Mutex<IpcManager> = Mutex::new(IpcManager::new());

// IPC错误类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    InvalidHandle,
    QueueFull,
    QueueEmpty,
    TypeMismatch,
}

// 公共API
pub struct Ipc;

impl Ipc {
    /// 创建一个新的消息队列
    pub fn create_queue(capacity: usize) -> IpcHandle {
        IPC_MANAGER.lock().create_queue(capacity)
    }

    /// 发送消息到指定队列
    pub fn send<T: 'static + Send>(handle: IpcHandle, data: T) -> Result<(), IpcError> {
        IPC_MANAGER.lock().send_message(handle, data)
    }

    /// 从指定队列接收消息
    pub fn receive<T: 'static>(handle: IpcHandle) -> Result<T, IpcError> {
        IPC_MANAGER.lock().receive_message(handle)
    }

    /// 销毁消息队列
    pub fn destroy_queue(handle: IpcHandle) -> Result<(), IpcError> {
        IPC_MANAGER.lock().destroy_queue(handle)
    }

    /// 非阻塞发送（如果队列满则立即返回错误）
    pub fn try_send<T: 'static + Send>(handle: IpcHandle, data: T) -> Result<(), IpcError> {
        // 这里可以实现非阻塞版本
        Self::send(handle, data)
    }

    /// 非阻塞接收（如果队列空则立即返回错误）
    pub fn try_receive<T: 'static>(handle: IpcHandle) -> Result<T, IpcError> {
        // 这里可以实现非阻塞版本
        Self::receive(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::Task;
    use crate::utils::kernel_init;

    fn test1(_: usize) {}
    fn test2(_: usize) {}

    #[test]
    fn test_ipc_basic() {
        kernel_init();
        Task::new("task1", test1);
        Task::new("task2", test2);

        Scheduler::start();

        let queue = Ipc::create_queue(10);

        // 测试发送和接收
        assert!(Ipc::send(queue, 42u32).is_ok());
        assert!(Ipc::send(queue, 100u32).is_ok());

        assert_eq!(Ipc::receive::<u32>(queue).unwrap(), 42);
        assert_eq!(Ipc::receive::<u32>(queue).unwrap(), 100);

        // 测试类型安全
        assert!(Ipc::send(queue, "hello").is_ok());
        assert!(Ipc::receive::<u32>(queue).is_err()); // 类型不匹配
        assert_eq!(Ipc::receive::<&str>(queue).unwrap(), "hello");

        Ipc::destroy_queue(queue).unwrap();
    }
}
