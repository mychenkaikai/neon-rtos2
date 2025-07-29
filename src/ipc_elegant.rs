use crate::config::MAX_TASKS;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

// 通道类型枚举
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelId {
    SystemLog = 0,
    InterruptToTask = 1,
    TaskToTask = 2,
    UserChannel0 = 3,
    UserChannel1 = 4,
    UserChannel2 = 5,
    UserChannel3 = 6,
    UserChannel4 = 7,
}

const MAX_CHANNELS: usize = 8;
const CHANNEL_CAPACITY: usize = 16;

// 消息槽
#[repr(C)]
struct MessageSlot<T> {
    data: MaybeUninit<T>,
    is_valid: bool,
}

impl<T> MessageSlot<T> {
    const fn new() -> Self {
        Self {
            data: MaybeUninit::uninit(),
            is_valid: false,
        }
    }

    fn store(&mut self, value: T) {
        self.data = MaybeUninit::new(value);
        self.is_valid = true;
    }

    fn take(&mut self) -> Option<T> {
        if self.is_valid {
            self.is_valid = false;
            Some(unsafe { self.data.assume_init_read() })
        } else {
            None
        }
    }
}

// 环形缓冲区
struct RingBuffer<T> {
    slots: [MessageSlot<T>; CHANNEL_CAPACITY],
    head: AtomicUsize,
    tail: AtomicUsize,
    count: AtomicUsize,
}

impl<T> RingBuffer<T> {
    const fn new() -> Self {
        Self {
            slots: [const { MessageSlot::new() }; CHANNEL_CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    fn push(&mut self, value: T) -> Result<(), T> {
        let count = self.count.load(Ordering::Acquire);
        if count >= CHANNEL_CAPACITY {
            return Err(value);
        }

        let tail = self.tail.load(Ordering::Acquire);
        self.slots[tail].store(value);

        self.tail
            .store((tail + 1) % CHANNEL_CAPACITY, Ordering::Release);
        self.count.fetch_add(1, Ordering::Release);
        Ok(())
    }

    fn pop(&mut self) -> Option<T> {
        let count = self.count.load(Ordering::Acquire);
        if count == 0 {
            return None;
        }

        let head = self.head.load(Ordering::Acquire);
        let value = self.slots[head].take();

        if value.is_some() {
            self.head
                .store((head + 1) % CHANNEL_CAPACITY, Ordering::Release);
            self.count.fetch_sub(1, Ordering::Release);
        }
        value
    }

    fn is_full(&self) -> bool {
        self.count.load(Ordering::Acquire) >= CHANNEL_CAPACITY
    }

    fn is_empty(&self) -> bool {
        self.count.load(Ordering::Acquire) == 0
    }
}

// 类型安全的发送者
pub struct Sender<T> {
    channel_id: ChannelId,
    _phantom: PhantomData<T>,
}

// 类型安全的接收者
pub struct Receiver<T> {
    channel_id: ChannelId,
    _phantom: PhantomData<T>,
}

impl<T> Sender<T> {
    pub fn send(&self, message: T) -> Result<(), IpcError> {
        unsafe { IpcCore::send(self.channel_id, message) }
    }

    pub fn try_send(&self, message: T) -> Result<(), IpcError> {
        unsafe { IpcCore::try_send(self.channel_id, message) }
    }
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, IpcError> {
        unsafe { IpcCore::recv(self.channel_id) }
    }

    pub fn try_recv(&self) -> Result<T, IpcError> {
        unsafe { IpcCore::try_recv(self.channel_id) }
    }
}

// IPC核心管理器
struct IpcCore {
    // 使用trait对象存储不同类型的通道
    channels: [Option<&'static mut dyn ChannelOps>; MAX_CHANNELS],
}

trait ChannelOps {
    fn send_raw(&mut self, data: *const u8, size: usize) -> Result<(), IpcError>;
    fn recv_raw(&mut self, data: *mut u8, size: usize) -> Result<(), IpcError>;
    fn try_send_raw(&mut self, data: *const u8, size: usize) -> Result<(), IpcError>;
    fn try_recv_raw(&mut self, data: *mut u8, size: usize) -> Result<(), IpcError>;
}

impl<T> ChannelOps for RingBuffer<T> {
    fn send_raw(&mut self, data: *const u8, size: usize) -> Result<(), IpcError> {
        if size != core::mem::size_of::<T>() {
            return Err(IpcError::TypeMismatch);
        }
        let value = unsafe { core::ptr::read(data as *const T) };
        self.push(value).map_err(|_| IpcError::ChannelFull)
    }

    fn recv_raw(&mut self, data: *mut u8, size: usize) -> Result<(), IpcError> {
        if size != core::mem::size_of::<T>() {
            return Err(IpcError::TypeMismatch);
        }
        let value = self.pop().ok_or(IpcError::ChannelEmpty)?;
        unsafe { core::ptr::write(data as *mut T, value) };
        Ok(())
    }

    fn try_send_raw(&mut self, data: *const u8, size: usize) -> Result<(), IpcError> {
        self.send_raw(data, size)
    }

    fn try_recv_raw(&mut self, data: *mut u8, size: usize) -> Result<(), IpcError> {
        self.recv_raw(data, size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IpcError {
    ChannelNotFound,
    ChannelFull,
    ChannelEmpty,
    TypeMismatch,
    InvalidChannel,
}

impl IpcCore {
    const fn new() -> Self {
        Self {
            channels: [const { None }; MAX_CHANNELS],
        }
    }

    unsafe fn register_channel<T: 'static>(channel_id: ChannelId, buffer: &'static mut RingBuffer<T>) {
        let channel_idx = channel_id as usize;
        if channel_idx < MAX_CHANNELS {
            IPC_CORE.channels[channel_idx] = Some(buffer as &'static mut dyn ChannelOps);
        }
    }

    fn send<T>(channel_id: ChannelId, message: T) -> Result<(), IpcError> {
        let channel_idx = channel_id as usize;
        if channel_idx >= MAX_CHANNELS {
            return Err(IpcError::InvalidChannel);
        }

        // 这里需要unsafe操作来处理trait对象
        // 在实际实现中，你可能需要使用更复杂的类型擦除机制
        unsafe {
            if let Some(channel) = &mut IPC_CORE.channels[channel_idx] {
                channel.send_raw(&message as *const T as *const u8, core::mem::size_of::<T>())
            } else {
                Err(IpcError::ChannelNotFound)
            }
        }
    }

    fn recv<T>(channel_id: ChannelId) -> Result<T, IpcError> {
        let channel_idx = channel_id as usize;
        if channel_idx >= MAX_CHANNELS {
            return Err(IpcError::InvalidChannel);
        }

        unsafe {
            if let Some(channel) = &mut IPC_CORE.channels[channel_idx] {
                let mut result = core::mem::MaybeUninit::<T>::uninit();
                channel.recv_raw(result.as_mut_ptr() as *mut u8, core::mem::size_of::<T>())?;
                Ok(result.assume_init())
            } else {
                Err(IpcError::ChannelNotFound)
            }
        }
    }

    fn try_send<T>(channel_id: ChannelId, message: T) -> Result<(), IpcError> {
        IpcCore::send(channel_id, message)
    }

    fn try_recv<T>(channel_id: ChannelId) -> Result<T, IpcError> {
        IpcCore::recv(channel_id)
    }
}

static mut IPC_CORE: IpcCore = IpcCore::new();

// 全局通道创建宏
macro_rules! create_global_channel {
    ($channel_id:expr, $msg_type:ty) => {{
        static mut BUFFER: RingBuffer<$msg_type> = RingBuffer::new();
        unsafe {
            // 使用 addr_of_mut! 避免创建可变引用
            let buffer_ptr = core::ptr::addr_of_mut!(BUFFER);
            IpcCore::register_channel($channel_id, &mut *buffer_ptr);
        }
        (
            Sender {
                channel_id: $channel_id,
                _phantom: PhantomData::<$msg_type>,
            },
            Receiver {
                channel_id: $channel_id,
                _phantom: PhantomData::<$msg_type>,
            },
        )
    }};
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_basic_functionality() {
        // 获取预定义通道
        let (log_tx, log_rx) = create_global_channel!(ChannelId::SystemLog, &'static str);
        let (int_tx, int_rx) = create_global_channel!(ChannelId::InterruptToTask, u32);

        // 测试发送消息
        assert!(log_tx.send("Hello from task").is_ok());
        assert!(int_tx.send(42).is_ok());

        // 测试接收消息
        match log_rx.try_recv() {
            Ok(msg) => {
                assert_eq!(msg, "Hello from task");
            }
            Err(e) => panic!("Failed to receive log message: {:?}", e),
        }

        match int_rx.try_recv() {
            Ok(int_val) => {
                assert_eq!(int_val, 42);
            }
            Err(e) => panic!("Failed to receive int value: {:?}", e),
        }
    }

    #[test]
    fn test_channel_empty() {
        let (_, rx) = create_global_channel!(ChannelId::UserChannel0, i32);
        
        // 测试空通道接收
        match rx.try_recv() {
            Err(IpcError::ChannelEmpty) => {}, // 期望的结果
            Ok(_) => panic!("Expected ChannelEmpty error"),
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_multiple_messages() {
        let (tx, rx) = create_global_channel!(ChannelId::UserChannel1, u8);
        
        // 发送多个消息
        for i in 0..5 {
            assert!(tx.send(i).is_ok());
        }
        
        // 接收并验证消息顺序
        for i in 0..5 {
            match rx.try_recv() {
                Ok(val) => assert_eq!(val, i),
                Err(e) => panic!("Failed to receive message {}: {:?}", i, e),
            }
        }
        
        // 验证通道现在为空
        assert!(matches!(rx.try_recv(), Err(IpcError::ChannelEmpty)));
    }

    #[test]
    fn test_different_data_types() {
        // 测试不同数据类型
        let (str_tx, str_rx) = create_global_channel!(ChannelId::UserChannel2, &'static str);
        let (bool_tx, bool_rx) = create_global_channel!(ChannelId::UserChannel3, bool);
        let (tuple_tx, tuple_rx) = create_global_channel!(ChannelId::UserChannel4, (u32, u32));
        
        // 发送不同类型的数据
        assert!(str_tx.send("test string").is_ok());
        assert!(bool_tx.send(true).is_ok());
        assert!(tuple_tx.send((10, 20)).is_ok());
        
        // 接收并验证
        assert_eq!(str_rx.try_recv().unwrap(), "test string");
        assert_eq!(bool_rx.try_recv().unwrap(), true);
        assert_eq!(tuple_rx.try_recv().unwrap(), (10, 20));
    }
}