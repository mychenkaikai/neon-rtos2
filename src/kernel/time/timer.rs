use crate::hal::trigger_schedule;
use crate::config::MAX_TIMERS;
use crate::sync::event::Event;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::systick::Systick;
use crate::error::{Result, RtosError};

static mut TIMER_LIST: [Option<TimerInner>; MAX_TIMERS] = [None; MAX_TIMERS];

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct TimerInner {
    running: bool,
    id: usize,
    timeout: usize,
}

pub struct Timer(usize);

impl Timer {
    /// 新建一个定时器的句柄
    /// 
    /// 通过遍历 TIMER_LIST，找到一个未使用的定时器槽位
    /// 
    /// # 返回值
    /// - `Ok(Timer)` - 成功创建定时器
    /// - `Err(RtosError::TimerSlotsFull)` - 没有可用的定时器槽位
    pub fn new(timeout: usize) -> Result<Timer> {
        let mut id = 0;
        let mut found = false;
        Timer::for_each(|timer, _| {
            if timer.is_none() {
                *timer = Some(TimerInner {
                    running: false,
                    id: id,
                    timeout: timeout + Systick::get_current_time(),
                });
                found = true;
                return true;
            }
            id += 1;
            return false;
        });
        
        if !found {
            return Err(RtosError::TimerSlotsFull);
        }
        Ok(Timer(id))
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
    
    /// 通过 id 来获取定时器结构体并执行操作
    /// 
    /// # 返回值
    /// - `Ok(())` - 成功执行操作
    /// - `Err(RtosError::TimerNotFound)` - 定时器不存在
    fn get_timer_by_id<F>(id: usize, mut f: F) -> Result<()>
    where
        F: FnMut(&mut TimerInner),
    {
        unsafe {
            if let Some(ref mut timer) = TIMER_LIST[id] {
                f(timer);
                Ok(())
            } else {
                Err(RtosError::TimerNotFound)
            }
        }
    }
    /// 删除定时器
    pub fn delete(&mut self) {
        unsafe {
            TIMER_LIST[self.0] = None;
        }
    }

    /// 启动定时器
    pub fn start(&mut self) -> Result<()> {
        Self::get_timer_by_id(self.0, |timer| {
            timer.running = true;
        })
    }

    /// 停止定时器
    pub fn stop(&mut self) -> Result<()> {
        Self::get_timer_by_id(self.0, |timer| {
            timer.running = false;
        })
    }

    /// 检查定时器是否正在运行
    pub fn is_running(&self) -> bool {
        let mut running = false;
        let _ = Self::get_timer_by_id(self.0, |timer| {
            running = timer.running;
        });
        running
    }

    /// 检查定时器是否超时
    pub fn is_timeout(&self) -> bool {
        let mut timeout = 0;
        let _ = Self::get_timer_by_id(self.0, |timer| {
            timeout = timer.timeout;
        });
        Systick::get_current_time() >= timeout
    }

    /// 遍历检查是否有满足条件的定时器，如果有的话就发送信号，并删除定时器
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

impl Drop for Timer {
    /// 当 Timer 被 drop 时，自动删除定时器
    ///
    /// 这允许槽位被后续的 Timer::new() 重用
    fn drop(&mut self) {
        unsafe {
            TIMER_LIST[self.0] = None;
        }
    }
}

pub struct Delay;

impl Delay {
    /// 阻塞当前任务并且开启定时器
    /// 
    /// # 返回值
    /// - `Ok(())` - 延时成功完成
    /// - `Err(RtosError::TimerSlotsFull)` - 没有可用的定时器槽位
    pub fn delay(timeout: usize) -> Result<()> {
        let mut timer = Timer::new(timeout)?;
        timer.start()?;
        Scheduler::get_current_task().block(Event::Timer(timer.0));
        trigger_schedule();
        timer.delete();
        Ok(())
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
        let mut timer1 = Timer::new(1000).unwrap();
        let mut timer2 = Timer::new(2000).unwrap();
        timer1.start().unwrap();
        timer2.start().unwrap();
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

    #[test]
    fn test_timer_slots_full() {
        kernel_init();
        // 创建最大数量的定时器
        let mut timers = Vec::new();
        for i in 0..MAX_TIMERS {
            let timer = Timer::new(1000 * i);
            assert!(timer.is_ok(), "Timer {} should be created successfully", i);
            timers.push(timer.unwrap());
        }
        
        // 再创建一个应该失败
        let result = Timer::new(1000);
        assert_eq!(result.err(), Some(RtosError::TimerSlotsFull));
    }

    #[test]
    fn test_timer_start_stop() {
        kernel_init();
        let mut timer = Timer::new(1000).unwrap();
        
        assert_eq!(timer.is_running(), false);
        
        timer.start().unwrap();
        assert_eq!(timer.is_running(), true);
        
        timer.stop().unwrap();
        assert_eq!(timer.is_running(), false);
    }

    #[test]
    fn test_timer_timeout() {
        kernel_init();
        let mut timer = Timer::new(500).unwrap();
        timer.start().unwrap();
        
        assert_eq!(timer.is_timeout(), false);
        
        Systick::add_current_time(500);
        assert_eq!(timer.is_timeout(), true);
    }
}
