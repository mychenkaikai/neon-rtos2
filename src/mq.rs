use crate::config::MAX_MQS;
use crate::event::Event;
use crate::task::Task;
use core::mem::MaybeUninit;

//全局变量数组，用于给mq分配id
static mut MQ_LIST: [Option<_Mq>; MAX_MQS] = [None; MAX_MQS];
#[derive(Copy, Clone)]
struct _Mq {
    id: usize,
}


//仿照mutex，实现任务间的阻塞机制
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
    //创建一个mq,这个结构体必定是static分配的，可以直接初始化
    pub fn new() -> Self {
        //
        let mut id: Option<usize> = None;
        unsafe {
            for i in 0..MAX_MQS {
                if MQ_LIST[i].is_none() {
                    MQ_LIST[i] = Some(_Mq { id: i });
                    id = Some(MQ_LIST[i].unwrap().id);
                    break;
                }
            }
        }
        //如果id为None，则panic
        if id.is_none() {
            panic!("MQ_LIST is full");
        }

        Mq {
            buffer: [MaybeUninit::uninit(); N],
            head: 0,
            tail: 0,
            count: 0,
            locked: false,
            owner: None,
            id: id.unwrap(),
        }
    }

    pub fn push(&mut self, data: T) -> bool {
        if self.locked {
            self.owner.unwrap().block(Event::Mq(self.id));
            return false;
        }

        if self.count == N {
            return false;
        }

        unsafe {
            // 直接写入tail位置
            *self.buffer.get_unchecked_mut(self.tail) = MaybeUninit::new(data);

            self.count += 1;
            self.tail = (self.tail + 1) % N;
        }
        //设置owner为空
        self.owner = None;
        self.locked = false;
        //唤醒被阻塞的task
        Event::wake_task(Event::Mq(self.id));
        true
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.locked {
            self.owner.unwrap().block(Event::Mq(self.id));
            return None;
        }

        if self.count == 0 {
            return None;
        }

        let mut ret: Option<T> = None;
        unsafe {
            // 直接从head位置读取
            ret = Some(self.buffer.get_unchecked(self.head).assume_init());

            self.count -= 1;
            self.head = (self.head + 1) % N;
        }
        self.owner = None;
        self.locked = false;
        //唤醒被阻塞的task
        Event::wake_task(Event::Mq(self.id));
        ret
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::Scheduler;
    use crate::task::Task;
    use crate::utils::kernel_init;

    #[test]
    fn test_mq() {
        kernel_init();

        // 使用非可变静态数组
        let mut mq: Mq<u32, 10> = Mq::<u32, 10>::new();

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

    //测试两个任务同时push和pop
    #[test]
    fn test_mq_multi_task() {
        kernel_init();

        let mut mq: Mq<u32, 10> = Mq::<u32, 10>::new();

        //任务要空着，不能有参数
        Task::new("task1", |_| {});
        Task::new("task2", |_| {});
        Scheduler::start();

        mq.push(1);
        mq.push(2);

        Scheduler::task_switch();

        assert_eq!(mq.pop(), Some(1));
        assert_eq!(mq.pop(), Some(2));
    }

    //测试两个任务同时push和pop，但是一个任务先push，一个任务后push
    #[test]
    fn test_mq_multi_task_push_pop() {
        kernel_init();

        let mut mq: Mq<u32, 10> = Mq::<u32, 10>::new();
        Task::new("task1", |_| {});
        Task::new("task2", |_| {});
        Scheduler::start();
        //测试可能冲突的情况，第一个任务先获得锁，然后第二个任务获得锁，然后第一个任务push，然后第二个任务pop
        mq.push(1);
        mq.push(2);
        Scheduler::task_switch();
        assert_eq!(mq.pop(), Some(1));
        assert_eq!(mq.pop(), Some(2));
    }
}
