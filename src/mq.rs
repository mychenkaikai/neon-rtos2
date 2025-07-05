use crate::config::MAX_MQS;
use core::marker::PhantomData;

static mut MQ_LIST: [_Mq; MAX_MQS] = [_Mq {
    buffer: None,
    head: 0,
    tail: 0,
    count: 0,
    id: 0,
    used: false,
}; MAX_MQS];

#[derive(Copy, Clone)]
pub struct _Mq {
    //buffer 应该是option
    buffer: Option<*mut u8>,
    head: usize,
    tail: usize,
    count: usize,
    id: usize,
    used: bool,
}

pub struct Mq<T, const N: usize>(usize, PhantomData<T>);

impl<T, const N: usize> Mq<T, N> {
    //遍历mq_list，找到一个未使用的mq
    //入参是用户准备好的类型为T的数组
    pub fn new(data: &'static mut [T; N]) -> Self {
        unsafe {
            for i in 0..MAX_MQS {
                if !MQ_LIST[i].used {
                    MQ_LIST[i].used = true;
                    MQ_LIST[i].buffer = Some(data.as_ptr() as *mut T as *mut u8);
                    MQ_LIST[i].head = 0;  // 确保 head 初始为 0
                    MQ_LIST[i].tail = 0;  // 确保 tail 初始为 0
                    MQ_LIST[i].count = 0;  // 确保 count 初始为 0
                    return Mq(i, PhantomData);
                }
            }
        }
        panic!("No free mq");
    }

    fn get_mq(&self) -> &mut _Mq {
        unsafe { &mut MQ_LIST[self.0] }
    }

    pub fn delete(&self) {
        let mq = self.get_mq();
        mq.count = 0;
        mq.head = 0;
        mq.tail = 0;
        mq.used = false;
        mq.buffer = None;
    }

    pub fn send(&self, data: T) {
        unsafe {
            let mq = self.get_mq();
            if mq.count == N {
                panic!("MQ is full");
            }
            let buffer = mq.buffer.unwrap() as *mut T;
            if mq.tail < N {
                buffer.add(mq.tail).write(data);
                mq.count += 1;
                mq.tail += 1;
                if mq.tail == N {
                    mq.tail = 0;
                }
            } else {
                panic!("MQ tail index out of bounds");
            }
        }
    }

    pub fn recv(&self) -> T {
        unsafe {
            let mq = self.get_mq();
            if mq.count == 0 {
                panic!("MQ is empty");
            }
            let buffer = mq.buffer.unwrap() as *mut T;
            if mq.head < N {
                let ret = buffer.add(mq.head).read();
                mq.count -= 1;
                mq.head += 1;
                if mq.head == N {
                    mq.head = 0;
                }
                ret
            } else {
                panic!("MQ head index out of bounds");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::mutex::Mutex;
    use crate::schedule::Scheduler;
    use crate::task::Task;
    use crate::timer::Timer;
    use crate::utils::kernel_init;

    // 使用非可变静态数组
    static mut U32_MQ_DATA: [u32; 10] = [0; 10];

    #[test]
    fn test_mq() {
        kernel_init();
        
        // 使用非可变静态数组
        let mq = Mq::<u32, 10>::new(&mut U32_MQ_DATA);
        
        mq.send(1);
        mq.send(2);
        mq.send(3);
        mq.send(4);
        mq.send(5);
        assert_eq!(mq.recv(), 1);
        assert_eq!(mq.recv(), 2);
        assert_eq!(mq.recv(), 3);
        assert_eq!(mq.recv(), 4);
        assert_eq!(mq.recv(), 5);
    }
}
