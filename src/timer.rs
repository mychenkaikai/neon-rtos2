use crate::config::MAX_TIMERS;
use crate::event::EventType;
use crate::schedule::Scheduler;
use crate::systick::Systick;

static mut TIMER_LIST: [_Timer; MAX_TIMERS] = [_Timer {
    used: false,
    running: false,
    id: 0,
    timeout: 0,
}; MAX_TIMERS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct _Timer {
    used: bool,
    running: bool,
    id: usize,
    timeout: usize,
}

pub struct Timer(usize);

impl Timer {
    //新建一个定时器的句柄，通过遍历TIMER_LIST，找到一个未使用的定时器，并返回其id
    pub fn new(timeout: usize) -> Timer {
        unsafe {
            for i in 0..MAX_TIMERS {
                if !TIMER_LIST[i].used {
                    TIMER_LIST[i].used = true;
                    TIMER_LIST[i].running = false;
                    TIMER_LIST[i].timeout = timeout + Systick::get_current_time();
                    return Timer(i);
                }
            }
        }
        panic!("Timer list is full");
    }

    pub fn init() {
        unsafe {
            for i in 0..MAX_TIMERS {
                TIMER_LIST[i] = _Timer {
                    used: false,
                    running: false,
                    id: i,
                    timeout: 0,
                };
            }
        }
    }

    pub fn get_id(&self) -> usize {
        self.0
    }
    //通过id来获取定时器结构体
    fn get_timer_by_id(id: usize) -> &'static mut _Timer {
        unsafe {
            return &mut TIMER_LIST[id];
        }
    }

    pub fn delete(&mut self) {
        Self::get_timer_by_id(self.0).used = false;
    }

    pub fn start(&mut self) {
        Self::get_timer_by_id(self.0).running = true;
    }

    pub fn stop(&mut self) {
        Self::get_timer_by_id(self.0).running = false;
    }

    pub fn is_running(&self) -> bool {
        Self::get_timer_by_id(self.0).running
    }

    pub fn is_timeout(&self) -> bool {
        Systick::get_current_time() >= Self::get_timer_by_id(self.0).timeout
    }

    //阻塞当前任务并且开启定时器
    pub fn delay(timeout: usize) {
        let mut timer = Timer::new(timeout);
        timer.start();
        Scheduler::get_current_task().block(EventType::Timer(timer.0));
    }

    //遍历检查是否有满足条件的定时器，如果有的话就发送信号，并删除定时器
    pub fn timer_check_and_send_event() {
        Timer::for_each(|timer, id| {
            if timer.is_timeout() {
                timer.delete();
                EventType::wake_task(EventType::Timer(id));
            }
        });
    }

    pub fn for_each<F>(mut f: F)
    where
        F: FnMut(&mut Timer, usize) -> (),
    {
        unsafe {
            for i in 0..MAX_TIMERS {
                if TIMER_LIST[i].used {
                    f(&mut Timer(i), i);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventType;
    use crate::systick::Systick;
    use crate::task::Task;
    use crate::task::TaskState;
    use crate::utils::kernel_init;

    #[test]
    fn test_timer() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn test_timer_for_each() {
        kernel_init();
        let mut timer1 = Timer::new(1000);
        let mut timer2 = Timer::new(2000);
        timer1.start();
        timer2.start();
        Systick::add_current_time(1000);
        Timer::for_each(|timer, _| {
            if timer.is_timeout() {
                assert_eq!(timer.get_id(), 0);
                timer.delete();
            }
        });

        Systick::add_current_time(1000);
        Timer::for_each(|timer, _| {
            if timer.is_timeout() {
                assert_eq!(timer.get_id(), 1);
                timer.delete();
            }
        });
    }

    #[test]
    fn test_timer_wait_and_start() {
        //新建一个任务，并运行，然后阻塞，然后开启定时器，然后检查任务是否被唤醒
        kernel_init();
        let task = Task::new("test_timer_wait_and_start", |_| {});
        Scheduler::start();
        Timer::delay(1000);
        assert_eq!(task.get_state(), TaskState::Blocked(EventType::Timer(0)));
        Systick::add_current_time(1000);
        Timer::timer_check_and_send_event();
        assert_eq!(task.get_state(), TaskState::Ready);
        Scheduler::schedule();
        assert_eq!(task.get_state(), TaskState::Running);
    }
}
