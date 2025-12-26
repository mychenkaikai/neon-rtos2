use crate::config::MAX_MQS;
use crate::sync::event::Event;
use crate::kernel::task::Task;
use crate::error::{Result, RtosError};
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
    locked: bool,
    owner: Option<Task>,
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
            locked: false,
            owner: None,
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
        if self.locked {
            if let Some(mut owner) = self.owner {
                owner.block(Event::Mq(self.id));
            }
            return false;
        }

        if self.count == N {
            return false;
        }

        unsafe {
            // 直接写入 tail 位置
            *self.buffer.get_unchecked_mut(self.tail) = MaybeUninit::new(data);

            self.count += 1;
            self.tail = (self.tail + 1) % N;
        }
        // 设置 owner 为空
        self.owner = None;
        self.locked = false;
        // 唤醒被阻塞的 task
        Event::wake_task(Event::Mq(self.id));
        true
    }

    /// 从队列中弹出数据
    /// 
    /// # 返回值
    /// - `Some(T)` - 成功弹出数据
    /// - `None` - 队列为空或被锁定
    pub fn pop(&mut self) -> Option<T> {
        if self.locked {
            if let Some(mut owner) = self.owner {
                owner.block(Event::Mq(self.id));
            }
            return None;
        }

        if self.count == 0 {
            return None;
        }

        let ret;
        unsafe {
            // 直接从 head 位置读取
            ret = Some(self.buffer.get_unchecked(self.head).assume_init());

            self.count -= 1;
            self.head = (self.head + 1) % N;
        }
        self.owner = None;
        self.locked = false;
        // 唤醒被阻塞的 task
        Event::wake_task(Event::Mq(self.id));
        ret
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
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::Task;
    use crate::utils::kernel_init;

    #[test]
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
}
