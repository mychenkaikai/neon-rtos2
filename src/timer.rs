use crate::config::MAX_TIMERS;
use crate::event::EventType;
use crate::systick::Systick;
use crate::schedule::Scheduler;

static mut TIMER_LIST: [Timer; MAX_TIMERS] = [Timer {
    used: false,
    running: false,
    id: 0,
    timeout: 0,
}; MAX_TIMERS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Timer {
    used: bool,
    running: bool,
    id: usize,
    timeout: usize,
}

impl Timer {
    pub fn init() {
        unsafe {
            for i in 0..MAX_TIMERS {
                TIMER_LIST[i] = Timer {
                    used: false,
                    running: false,
                    id: i,
                    timeout: 0,
                };
            }
        }
    }

    pub fn new(timeout: usize) -> Self {
        unsafe {
            for i in 0..MAX_TIMERS {
                if !TIMER_LIST[i].used {
                    TIMER_LIST[i].used = true;
                    TIMER_LIST[i].running = false;
                    TIMER_LIST[i].timeout = timeout + Systick::get_current_time();
                    return TIMER_LIST[i];
                }
            }
        }
        panic!("Timer list is full");
    }

    pub fn delete(&mut self) {
        self.used = false;
    }

    pub fn start(&mut self) {
        if !self.used {
            return;
        }
        self.running = true;

    }

    pub fn stop(&mut self) {
        if !self.used {
            return;
        }
        self.running = false;
    }

    pub fn is_running(&self) -> bool {
        if !self.used {
            return false;
        }
        return self.running;
    }

    pub fn is_timeout(&self) -> bool {
        if !self.used {
            return false;
        }
        if !self.running {
            return false;
        }
        return Systick::get_current_time() >= self.timeout;
    }

    pub fn for_each<F>(mut f: F)
    where
        F: FnMut(&mut Timer, usize) -> (),
    {
        unsafe {
            for i in 0..MAX_TIMERS {
                if TIMER_LIST[i].used {
                    f(&mut TIMER_LIST[i], i);
                }
            }
        }
    }

    //阻塞当前任务并且开启定时器
    pub fn timer_wait_and_start(timeout: usize) {
        let mut timer = Timer::new(timeout);
        timer.start();
        Scheduler::get_current_task().block(EventType::Timer(timer.id));
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systick::Systick;
    use crate::utils::kernel_init;
    use crate::task::Task;
    use crate::task::TaskState;
    use crate::event::EventType;

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
        Timer::for_each(|timer, id| {
            if timer.is_timeout() {
                assert_eq!(timer.id, 0);
                timer.delete();
            }
        });

        Systick::add_current_time(1000);
        Timer::for_each(|timer, id| {
            if timer.is_timeout() {
                assert_eq!(timer.id, 1);
                timer.delete();
            }
        });
    }

    #[test]
    fn test_timer_wait_and_start() {
        //新建一个任务，并运行，然后阻塞，然后开启定时器，然后检查任务是否被唤醒
        kernel_init();
        let mut task = Task::new("test_timer_wait_and_start", |_| {});
        Scheduler::start();
        Timer::timer_wait_and_start(1000);
        Timer::for_each(|timer, id| {
            println!("timer{}: {:?}", id, timer.running);
        });
        assert_eq!(task.get_state(), TaskState::Blocked(EventType::Timer(0)));
        Systick::add_current_time(1000);
        Timer::timer_check_and_send_event();
        Timer::for_each(|timer, id| {
            println!("timer{}: {:?}", id, timer.running);
        });
        assert_eq!(task.get_state(), TaskState::Ready);
        Scheduler::schedule();
        assert_eq!(task.get_state(), TaskState::Running);
    }
}
