use crate::config::MAX_MQS;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr::NonNull;

static mut MQ_LIST: [_Mq; MAX_MQS] = [_Mq {
    buffer: None,
    size: 0,
    head: 0,
    tail: 0,
    count: 0,
    id: 0,
    used: false,
}; MAX_MQS];

#[derive(Copy, Clone)]
pub struct _Mq {
    buffer: Option<NonNull<u8>>,
    size: usize,  // 每个元素的大小
    head: usize,
    tail: usize,
    count: usize,
    id: usize,
    used: bool,
}

pub struct Mq<T, const N: usize>(usize, PhantomData<T>);

impl<T, const N: usize> Mq<T, N> {
    //遍历mq_list，找到一个未使用的mq
    //入参是用户准备好的类型MqStorage<T, N>
    pub fn new(data: &'static MqStorage<T, N>) -> Self {
        unsafe {
            for i in 0..MAX_MQS {
                if !MQ_LIST[i].used {
                    MQ_LIST[i].used = true;
                    // 存储数据的首地址
                    MQ_LIST[i].buffer = NonNull::new(data.data.as_ptr() as *mut u8);
                    MQ_LIST[i].size = core::mem::size_of::<T>();
                    MQ_LIST[i].head = 0;
                    MQ_LIST[i].tail = 0;
                    MQ_LIST[i].count = 0;
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
        let mq = self.get_mq();
        if mq.count == N {
            panic!("MQ is full");
        }

        // 检查buffer是否为None
        let buffer_ptr = match mq.buffer {
            Some(ptr) => ptr.as_ptr(),
            None => panic!("MQ buffer is not initialized"),
        };
        
        unsafe {
            if mq.tail < N {
                // 计算正确的内存位置
                let elem_ptr = buffer_ptr.add(mq.tail * mq.size) as *mut MaybeUninit<T>;
                elem_ptr.write(MaybeUninit::new(data));
                
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
        let mq = self.get_mq();
        if mq.count == 0 {
            panic!("MQ is empty");
        }

        let buffer_ptr = match mq.buffer {
            Some(ptr) => ptr.as_ptr(),
            None => panic!("MQ buffer is not initialized"),
        };
        
        unsafe {
            if mq.head < N {
                // 计算正确的内存位置
                let elem_ptr = buffer_ptr.add(mq.head * mq.size) as *mut MaybeUninit<T>;
                let ret = elem_ptr.read().assume_init();
                
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

//使用maybe_uninit来初始化一个数组
pub struct MqStorage<T, const N: usize> {
    // 使用数组而不是整体放在MaybeUninit中
    data: [MaybeUninit<T>; N],
}

// 允许从多个线程访问
unsafe impl<T, const N: usize> Sync for MqStorage<T, N> {}

impl<T, const N: usize> MqStorage<T, N> {
    // 创建一个未初始化但有效的内存区域
    const ELEM: MaybeUninit<T> = MaybeUninit::uninit();
    const INIT: [MaybeUninit<T>; N] = [Self::ELEM; N];

    pub const fn new() -> Self {
        Self { data: Self::INIT }
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

    #[test]
    fn test_mq() {
        kernel_init();

        // 使用非可变静态数组，使用maybe_uninit
        static U32_MQ_DATA: MqStorage<u32, 10> = MqStorage::new();
        // 使用非可变静态数组
        let mq = Mq::<u32, 10>::new(&U32_MQ_DATA);
        println!("U32_MQ_DATA address: {:p}", &U32_MQ_DATA as *const _);
        println!("U32_MQ_DATA data: {:?}", mq.get_mq().buffer);

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
