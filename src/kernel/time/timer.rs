use crate::hal::trigger_schedule;
use crate::config::MAX_TIMERS;
use crate::sync::event::Event;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::systick::Systick;
use crate::error::{Result, RtosError};
use crate::compat::BinaryHeap;
use core::cmp::Ordering;
use core::cell::RefCell;
use critical_section::Mutex;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct TimerInner {
    running: bool,
    timeout: usize,
    seq: usize,
    waiter: Option<usize>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct TimerEvent {
    timeout: usize,
    id: usize,
    seq: usize,
}

impl Ord for TimerEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap: smaller timeout is higher priority
        other.timeout.cmp(&self.timeout)
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for TimerEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct TimerManager {
    timers: [Option<TimerInner>; MAX_TIMERS],
    heap: BinaryHeap<TimerEvent>,
}

impl TimerManager {
    const fn new() -> Self {
        Self {
            timers: [None; MAX_TIMERS],
            heap: BinaryHeap::new(),
        }
    }
}

static TIMER_MANAGER: Mutex<RefCell<TimerManager>> = Mutex::new(RefCell::new(TimerManager::new()));

pub struct Timer(usize);

impl Timer {
    /// 新建一个定时器的句柄
    /// 
    /// 通过 O(1) 的方式找到一个未使用的定时器槽位
    pub fn new(timeout: usize) -> Result<Timer> {
        let expire_time = timeout + Systick::get_current_time();
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            for i in 0..MAX_TIMERS {
                if manager.timers[i].is_none() {
                    manager.timers[i] = Some(TimerInner {
                        running: false,
                        timeout: expire_time,
                        seq: 0,
                        waiter: None,
                    });
                    return Ok(Timer(i));
                }
            }
            Err(RtosError::TimerSlotsFull)
        })
    }

    pub fn init() {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            for i in 0..MAX_TIMERS {
                manager.timers[i] = None;
            }
            manager.heap.clear();
        })
    }

    pub fn get_id(&self) -> usize {
        self.0
    }
    
    /// 删除定时器
    pub fn delete(&mut self) {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            manager.timers[self.0] = None;
        })
    }

    /// 启动定时器
    pub fn start(&mut self) -> Result<()> {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            let (timeout, seq) = if let Some(ref mut timer) = manager.timers[self.0] {
                timer.running = true;
                timer.seq = timer.seq.wrapping_add(1);
                timer.waiter = None;
                (timer.timeout, timer.seq)
            } else {
                return Err(RtosError::TimerNotFound);
            };
            
            manager.heap.push(TimerEvent {
                timeout,
                id: self.0,
                seq,
            });
            Ok(())
        })
    }

    /// 停止定时器
    pub fn stop(&mut self) -> Result<()> {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            if let Some(ref mut timer) = manager.timers[self.0] {
                timer.running = false;
                timer.seq = timer.seq.wrapping_add(1); // 使事件无效
                timer.waiter = None;
                Ok(())
            } else {
                Err(RtosError::TimerNotFound)
            }
        })
    }

    /// 检查定时器是否正在运行
    pub fn is_running(&self) -> bool {
        critical_section::with(|cs| {
            let manager = TIMER_MANAGER.borrow_ref(cs);
            if let Some(ref timer) = manager.timers[self.0] {
                timer.running
            } else {
                false
            }
        })
    }

    /// 检查定时器是否超时
    pub fn is_timeout(&self) -> bool {
        let current_time = Systick::get_current_time();
        critical_section::with(|cs| {
            let manager = TIMER_MANAGER.borrow_ref(cs);
            if let Some(ref timer) = manager.timers[self.0] {
                current_time >= timer.timeout
            } else {
                false
            }
        })
    }

    /// 遍历检查是否有满足条件的定时器，如果有的话就发送信号
    /// 使用 O(1)/O(log N) 的 Min-Heap 实现
    pub fn timer_check_and_send_event() {
        let current_time = Systick::get_current_time();
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            while let Some(event) = manager.heap.peek() {
                if event.timeout > current_time {
                    break;
                }
                
                let event = manager.heap.pop().unwrap();
                
                if let Some(ref mut timer) = manager.timers[event.id] {
                    if timer.running && timer.seq == event.seq {
                        let timer_event = Event::Timer(event.id);
                        let waiter = timer.waiter.take();
                        if let Some(task_id) = waiter {
                            if !Event::wake_task_by_id_if(task_id, timer_event) {
                                Event::wake_task(timer_event);
                            }
                        } else {
                            Event::wake_task(timer_event);
                        }
                    }
                }
            }
        });
    }

    pub(crate) fn set_waiter(&mut self, task_id: usize) -> Result<()> {
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            if let Some(ref mut timer) = manager.timers[self.0] {
                timer.waiter = Some(task_id);
                Ok(())
            } else {
                Err(RtosError::TimerNotFound)
            }
        })
    }

    /// 获取距离下一个定时器超时的剩余时间（毫秒/ticks）
    pub fn get_next_timeout() -> Option<usize> {
        let current_time = Systick::get_current_time();
        critical_section::with(|cs| {
            let mut manager = TIMER_MANAGER.borrow_ref_mut(cs);
            // 惰性删除已取消或过期的事件
            while let Some(event) = manager.heap.peek() {
                if let Some(ref timer) = manager.timers[event.id] {
                    if timer.running && timer.seq == event.seq {
                        if event.timeout <= current_time {
                            return Some(0);
                        } else {
                            return Some(event.timeout - current_time);
                        }
                    }
                }
                manager.heap.pop(); // 清理无效事件
            }
            None
        })
    }

    pub fn for_each_used<F>(mut f: F)
    where
        F: FnMut(&mut Timer, usize) -> bool,
    {
        let mut ids = [0; MAX_TIMERS];
        let mut count = 0;
        critical_section::with(|cs| {
            let manager = TIMER_MANAGER.borrow_ref(cs);
            for i in 0..MAX_TIMERS {
                if manager.timers[i].is_some() {
                    ids[count] = i;
                    count += 1;
                }
            }
        });
        
        for i in 0..count {
            let id = ids[i];
            let ret = f(&mut Timer(id), id);
            if ret {
                break;
            }
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.delete();
    }
}

/// 提供延时功能
pub struct Delay;

impl Delay {
    /// 阻塞当前任务指定的时间（以 tick/毫秒为单位）
    ///
    /// 取代了之前需要手动创建 `Timer` 实例并 `start()`、`delete()` 的繁琐过程。
    pub fn delay(timeout: usize) -> Result<()> {
        let mut timer = Timer::new(timeout)?;
        timer.start()?;
        let task_id = Scheduler::get_current_task().get_taskid();
        timer.set_waiter(task_id)?;
        Scheduler::get_current_task().block(Event::Timer(timer.0));
        trigger_schedule();
        timer.delete();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::sync::event::Event;
    use crate::kernel::time::systick::Systick;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
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
            assert_eq!(id, 0);
            assert_eq!(timer.get_id(), 0);
            assert_eq!(timer.is_running(), true);
            if timer.is_timeout() {
                assert_eq!(timer.is_running(), true);
                assert_eq!(timer.get_id(), 0);
                timer.delete();
                return true;
            }
            return false;
        });

        Systick::add_current_time(1000);
        Timer::for_each_used(|timer, _| {
            assert_eq!(timer.is_running(), true);
            if timer.is_timeout() {
                assert_eq!(timer.is_running(), true);
                assert_eq!(timer.get_id(), 1);
                timer.delete();
                return true;
            }
            return false;
        });
    }

    #[test]
    #[serial]
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
    #[serial]
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
    #[serial]
    fn test_timer_timeout() {
        kernel_init();
        let mut timer = Timer::new(500).unwrap();
        timer.start().unwrap();
        
        assert_eq!(timer.is_timeout(), false);
        
        Systick::add_current_time(500);
        assert_eq!(timer.is_timeout(), true);
    }

    #[test]
    #[serial]
    fn test_timer_min_heap_ordering() {
        kernel_init();
        
        let mut t1 = Timer::new(300).unwrap();
        let mut t2 = Timer::new(100).unwrap();
        let mut t3 = Timer::new(200).unwrap();
        
        t1.start().unwrap();
        t2.start().unwrap();
        t3.start().unwrap();
        
        // 期望下一个超时时间是 100
        assert_eq!(Timer::get_next_timeout(), Some(100));
        
        Systick::add_current_time(100);
        Timer::timer_check_and_send_event(); // 处理 t2 超时
        
        // 期望下一个超时时间是 100 (t3 剩余)
        assert_eq!(Timer::get_next_timeout(), Some(100));
        
        Systick::add_current_time(100);
        Timer::timer_check_and_send_event(); // 处理 t3 超时
        
        // 期望下一个超时时间是 100 (t1 剩余)
        assert_eq!(Timer::get_next_timeout(), Some(100));
    }

    #[test]
    #[serial]
    fn test_timer_lazy_deletion() {
        kernel_init();
        
        let mut t1 = Timer::new(100).unwrap();
        let mut t2 = Timer::new(200).unwrap();
        
        t1.start().unwrap();
        t2.start().unwrap();
        
        assert_eq!(Timer::get_next_timeout(), Some(100));
        
        // 停止 t1，它的事件仍然在堆中，但是无效（seq 不匹配）
        t1.stop().unwrap();
        
        // `get_next_timeout` 会惰性删除堆顶无效事件，下一个应该是 t2 (200)
        assert_eq!(Timer::get_next_timeout(), Some(200));
    }

    #[test]
    #[serial]
    fn test_timer_directed_wakeup() {
        kernel_init();

        let mut task = Task::new("timer_waiter", |_| {}).unwrap();
        let mut timer = Timer::new(100).unwrap();
        timer.start().unwrap();

        Scheduler::start();

        let current_id = Scheduler::get_current_task().get_taskid();
        if task.get_taskid() != current_id {
            timer.set_waiter(task.get_taskid()).unwrap();
            task.block(Event::Timer(timer.get_id()));
            Systick::add_current_time(100);
            Timer::timer_check_and_send_event();
            assert_eq!(task.get_state(), TaskState::Ready);
        }
    }
}
