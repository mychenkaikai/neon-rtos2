use crate::compat::{Box, Vec, VecDeque};

use core::any::Any;

use core::cell::RefCell;
use critical_section::Mutex;

// IPC句柄类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcHandle(usize, u64);

struct MessageQueue {
    messages: VecDeque<Box<dyn Any + Send>>,
    capacity: usize,
    generation: u64,
}

struct IpcManager {
    queues: Vec<Option<MessageQueue>>,
    next_generation: u64,
}

impl IpcManager {
    fn get(&mut self, handle: IpcHandle) -> Result<&mut MessageQueue, IpcError> {
        self.queues
            .get_mut(handle.0)
            .and_then(Option::as_mut)
            .filter(|q| q.generation == handle.1)
            .ok_or(IpcError::InvalidHandle)
    }
}

static IPC_MANAGER: Mutex<RefCell<IpcManager>> = Mutex::new(RefCell::new(IpcManager {
    queues: Vec::new(),
    next_generation: 0,
}));

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
    pub(crate) fn init() {
        // Keep generations monotonic across host kernel resets.
        let old = critical_section::with(|cs| {
            core::mem::take(&mut IPC_MANAGER.borrow_ref_mut(cs).queues)
        });
        drop(old);
    }

    /// 创建一个新的消息队列
    pub fn create_queue(capacity: usize) -> IpcHandle {
        let messages = VecDeque::with_capacity(capacity);
        critical_section::with(|cs| {
            let mut manager = IPC_MANAGER.borrow_ref_mut(cs);
            let generation = manager.next_generation;
            manager.next_generation = generation.checked_add(1).expect("IPC IDs exhausted");
            let queue = Some(MessageQueue {
                messages,
                capacity,
                generation,
            });
            let slot = manager
                .queues
                .iter()
                .position(Option::is_none)
                .unwrap_or(manager.queues.len());
            if slot == manager.queues.len() {
                manager.queues.push(queue);
            } else {
                manager.queues[slot] = queue;
            }
            IpcHandle(slot, generation)
        })
    }

    /// 非阻塞发送消息；队列满时返回 QueueFull，不改变调用任务状态。
    pub fn send<T: 'static + Send>(handle: IpcHandle, data: T) -> Result<(), IpcError> {
        let mut data = Some(data);
        critical_section::with(|cs| {
            let mut manager = IPC_MANAGER.borrow_ref_mut(cs);
            let queue = manager.get(handle)?;
            if queue.messages.len() == queue.capacity {
                return Err(IpcError::QueueFull);
            }
            queue.messages.push_back(Box::new(data.take().unwrap()));
            Ok(())
        })
    }

    /// 非阻塞接收消息；队列空时返回 QueueEmpty，不改变调用任务状态。
    pub fn receive<T: 'static>(handle: IpcHandle) -> Result<T, IpcError> {
        let message = critical_section::with(|cs| {
            let mut manager = IPC_MANAGER.borrow_ref_mut(cs);
            let queue = manager.get(handle)?;
            if !queue
                .messages
                .front()
                .ok_or(IpcError::QueueEmpty)?
                .is::<T>()
            {
                return Err(IpcError::TypeMismatch);
            }
            Ok(queue.messages.pop_front().unwrap())
        })?;
        message
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| IpcError::TypeMismatch)
    }

    /// 销毁消息队列
    pub fn destroy_queue(handle: IpcHandle) -> Result<(), IpcError> {
        let removed = critical_section::with(|cs| {
            let mut manager = IPC_MANAGER.borrow_ref_mut(cs);
            manager.get(handle)?;
            Ok(manager.queues[handle.0].take())
        })?;
        // Message destructors may call back into IPC.
        drop(removed);
        Ok(())
    }

    /// 非阻塞发送（如果队列满则立即返回错误）
    pub fn try_send<T: 'static + Send>(handle: IpcHandle, data: T) -> Result<(), IpcError> {
        Self::send(handle, data)
    }

    /// 非阻塞接收（如果队列空则立即返回错误）
    pub fn try_receive<T: 'static>(handle: IpcHandle) -> Result<T, IpcError> {
        Self::receive(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::Task;
    use crate::utils::kernel_init;

    fn test1() {}
    fn test2() {}

    #[test]
    #[serial_test::serial]
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

    #[test]
    #[serial_test::serial]
    fn try_operations_never_block_the_caller() {
        kernel_init();
        Scheduler::start();
        let task = Scheduler::get_current_task();
        let handle = Ipc::create_queue(1);
        assert_eq!(Ipc::try_receive::<u32>(handle), Err(IpcError::QueueEmpty));
        assert_eq!(task.get_state(), crate::kernel::task::TaskState::Running);
        Ipc::try_send(handle, 1u32).unwrap();
        assert_eq!(Ipc::try_send(handle, 2u32), Err(IpcError::QueueFull));
        assert_eq!(task.get_state(), crate::kernel::task::TaskState::Running);
        Ipc::destroy_queue(handle).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn recycling_ipc_slots_and_reentrant_payload_drop() {
        struct OnDrop(IpcHandle);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                Ipc::send(self.0, 42u32).unwrap();
            }
        }
        let sink = Ipc::create_queue(2);
        let old = Ipc::create_queue(1);
        Ipc::send(old, OnDrop(sink)).unwrap();
        assert_eq!(Ipc::send(old, OnDrop(sink)), Err(IpcError::QueueFull));
        Ipc::destroy_queue(old).unwrap();
        assert_eq!(Ipc::receive::<u32>(sink), Ok(42));
        assert_eq!(Ipc::receive::<u32>(sink), Ok(42));
        let next = Ipc::create_queue(1);
        assert_eq!(next.0, old.0);
        assert_eq!(Ipc::destroy_queue(old), Err(IpcError::InvalidHandle));
        Ipc::send(next, 1u32).unwrap();
        Ipc::destroy_queue(next).unwrap();
        Ipc::destroy_queue(sink).unwrap();
    }
}
