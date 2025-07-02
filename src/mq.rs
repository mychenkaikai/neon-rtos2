use crate::config::MAX_MQS;

static mut MQ_LIST: [_Mq<T>; MAX_MQS] = [_Mq<T> {
    id: 0,
    used: false,
    name: "",
    max_msg: 0,
    msg_count: 0,
    msg_list: [T; N],
}];

//支持泛型类型，但是长度不能超过1024
pub struct _Mq<T, const N: usize> {
    id: usize,
    used: bool,
    name: &'static str,
    max_msg: usize,
    msg_count: usize,
    msg_list: [T; N],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::kernel_init;
    use crate::task::Task;
    use crate::schedule::Scheduler;
    use crate::event::Event;
    use crate::mutex::Mutex;
    use crate::timer::Timer;

    #[test]
    fn test_mq() {
        kernel_init();
        let mq = Mq::new();
    }
}