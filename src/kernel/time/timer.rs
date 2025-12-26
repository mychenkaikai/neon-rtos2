use crate::hal::trigger_schedule;
use crate::config::MAX_TIMERS;
use crate::sync::event::Event;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::systick::Systick;

static mut TIMER_LIST: [Option<TimerInner>; MAX_TIMERS] = [None; MAX_TIMERS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct TimerInner {
    running: bool,
    id: usize,
    timeout: usize,
}

pub struct Timer(usize);

impl Timer {
    //新建一个定时器的句柄，通过遍历TIMER_LIST，找到一个未使用的定时器，并返回其id
    //for_each遍历所有定时器，如果定时器未使用，则返回其id
    pub fn new(timeout: usize) -> Timer {
        let mut id = 0;
        Timer::for_each(|timer, _| {
            if timer.is_none() {
                *timer = Some(TimerInner {
                    running: false,
                    id: id,
                    timeout: timeout + Systick::get_current_time(),
                });
                return true;
            }
            id += 1;
            return false;
        });
        if id == MAX_TIMERS {
            panic!("No free timer slot");
        }
        return Timer(id);
    }

    pub fn init() {
        unsafe {
            for i in 0..MAX_TIMERS {
                TIMER_LIST[i] = None;
            }
        }
    }

    pub fn get_id(&self) -> usize {
        self.0
    }
    //通过id来获取定时器结构体,入参是函数
    fn get_timer_by_id<F>(id: usize, mut f: F)
    where
        F: FnMut(&mut TimerInner),
    {
        unsafe {
            if let Some(ref mut timer) = TIMER_LIST[id] {
                f(timer);
            }
            else{
                panic!("Timer not found");
            }
        }
    }
    //删除定时器
    pub fn delete(&mut self) {
        unsafe {
            TIMER_LIST[self.0] = None;
        }
    }

    pub fn start(&mut self) {
        Self::get_timer_by_id(self.0, |timer| {
            timer.running = true;
        });
    }

    pub fn stop(&mut self) {
        Self::get_timer_by_id(self.0, |timer| {
            timer.running = false;
        });
    }

    pub fn is_running(&self) -> bool {
        let mut running = false;
        Self::get_timer_by_id(self.0, |timer| {
            running = timer.running;
        });
        return running;
    }

    pub fn is_timeout(&self) -> bool {
        let mut timeout = 0;
        Self::get_timer_by_id(self.0, |timer| {
            timeout = timer.timeout;
        });
        return Systick::get_current_time() >= timeout;
    }

    //遍历检查是否有满足条件的定时器，如果有的话就发送信号，并删除定时器
    pub fn timer_check_and_send_event() {
        Timer::for_each_used(|timer, id| {
            if timer.is_timeout() {
                Event::wake_task(Event::Timer(id));
                return true;
            }
            return false;
        });
    }

    pub fn for_each<F>(mut f: F)
    where
        F: FnMut(&mut Option<TimerInner>, usize) -> bool,
    {
        unsafe {
            for i in 0..MAX_TIMERS {
                let ret: bool = f(&mut TIMER_LIST[i], i);
                if ret {
                    break;
                }
            }
        }
    }

    pub fn for_each_used<F>(mut f: F)
    where
        F: FnMut(&mut Timer, usize) -> bool,
    {
        for i in 0..MAX_TIMERS {
            unsafe {
                if TIMER_LIST[i].is_some() {
                    let ret: bool = f(&mut Timer(i), i);
                    if ret {
                        break;
                    }
                }
            }
        }
    }
}
pub struct Delay;
impl Delay {
    //阻塞当前任务并且开启定时器
    pub fn delay(timeout: usize) {
        let mut timer = Timer::new(timeout);
        timer.start();
        Scheduler::get_current_task().block(Event::Timer(timer.0));
        trigger_schedule();
        timer.delete();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::event::Event;
    use crate::kernel::time::systick::Systick;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::utils::kernel_init;

    #[test]
    fn test_timer_for_each() {
        kernel_init();
        let mut timer1 = Timer::new(1000);
        let mut timer2 = Timer::new(2000);
        timer1.start();
        timer2.start();
        assert_eq!(timer1.is_running(), true);
        assert_eq!(timer2.is_running(), true);
        Systick::add_current_time(1000);
        Timer::for_each_used(|timer, id| {
            unsafe {
                assert_eq!(id, 0);
                assert_eq!(timer.0, 0);
                assert_eq!(TIMER_LIST[timer.0].unwrap().running, true);
            }
            if timer.is_timeout() {
                unsafe {
                    assert_eq!(TIMER_LIST[timer.0].unwrap().running, true);
                }
                assert_eq!(timer.get_id(), 0);
                timer.delete();
                return true;
            }
            return false;
        });

        Systick::add_current_time(1000);
        Timer::for_each_used(|timer, _| {
            unsafe {
                assert_eq!(TIMER_LIST[timer.0].unwrap().running, true);
            }
            if timer.is_timeout() {
                unsafe {
                    assert_eq!(TIMER_LIST[timer.0].unwrap().running, true);
                }
                assert_eq!(timer.get_id(), 1);
                timer.delete();
                return true;
            }
            return false;
        });
    }
}
