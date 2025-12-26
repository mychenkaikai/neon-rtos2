use crate::config::MAX_MUTEXES;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::task::Task;
use crate::sync::event::Event;
use crate::sync::guard::MutexGuard;
use crate::error::{Result, RtosError};

static mut MUTEX_LIST: [MutexInner; MAX_MUTEXES] = [MutexInner {
    locked: false,
    used: false,
    owner: None,
}; MAX_MUTEXES];

#[derive(PartialEq, Clone, Copy)]
pub struct MutexInner {
    used: bool,
    locked: bool,
    owner: Option<Task>,
}

#[derive(Debug)]
pub struct Mutex(usize);

impl Mutex {
    pub fn init() {
        unsafe {
            for i in 0..MAX_MUTEXES {
                MUTEX_LIST[i] = MutexInner {
                    locked: false,
                    used: false,
                    owner: None,
                };
            }
        }
    }

    pub fn new() -> Result<Self> {
        unsafe {
            for i in 0..MAX_MUTEXES {
                if !MUTEX_LIST[i].used {
                    MUTEX_LIST[i].used = true;
                    MUTEX_LIST[i].owner = None;
                    return Ok(Mutex(i));
                }
            }
        }
        Err(RtosError::MutexSlotsFull)
    }

    pub fn lock(&self) {
        unsafe {
            if MUTEX_LIST[self.0].locked {
                Scheduler::get_current_task().block(Event::Mutex(self.0));
                return;
            }
            MUTEX_LIST[self.0].locked = true;
            MUTEX_LIST[self.0].owner = Some(Scheduler::get_current_task());
        }
    }

    pub fn unlock(&self) -> Result<()> {
        unsafe {
            if MUTEX_LIST[self.0].owner != Some(Scheduler::get_current_task()) {
                return Err(RtosError::MutexNotOwned);
            }
            MUTEX_LIST[self.0].locked = false;
            MUTEX_LIST[self.0].owner = None;
            Event::wake_task(Event::Mutex(self.0));
            Ok(())
        }
    }

    /// 获取锁，返回 RAII 守卫
    ///
    /// 当返回的 MutexGuard 离开作用域时，锁会自动释放。
    ///
    /// # 返回值
    /// - `MutexGuard` - 锁守卫，离开作用域自动释放
    ///
    /// # 示例
    /// ```rust
    /// {
    ///     let _guard = mutex.lock_guard();
    ///     // 临界区代码
    /// } // 自动释放锁
    /// ```
    pub fn lock_guard(&self) -> MutexGuard<'_> {
        self.lock();
        MutexGuard::new(self)
    }

    /// 闭包风格 API
    ///
    /// 在持有锁期间执行闭包，闭包执行完毕后自动释放锁。
    ///
    /// # 参数
    /// - `f`: 在持有锁期间执行的闭包
    ///
    /// # 返回值
    /// - `R`: 闭包的返回值
    ///
    /// # 示例
    /// ```rust
    /// let result = mutex.with_lock(|| {
    ///     // 临界区代码
    ///     42
    /// });
    /// ```
    pub fn with_lock<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = self.lock_guard();
        f()
    }
}

impl Drop for Mutex {
    /// 当 Mutex 被 drop 时，自动释放槽位
    ///
    /// 这允许槽位被后续的 Mutex::new() 重用
    fn drop(&mut self) {
        unsafe {
            MUTEX_LIST[self.0].used = false;
            MUTEX_LIST[self.0].locked = false;
            MUTEX_LIST[self.0].owner = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::scheduler::Scheduler;
    use crate::kernel::task::Task;
    use crate::kernel::task::TaskState;
    use crate::utils::kernel_init;
    use serial_test::serial;

    #[test]
    #[serial]
    #[should_panic]
    fn test_mutex() {
        kernel_init();
        let mutex = Mutex::new().unwrap();
        //现在是模拟测试所以不需要有任务内容,但是需要有任务
        Task::new("test_mutex", |_| {}).unwrap();
        Task::new("test_mutex2", |_| {}).unwrap();
        //当调度开始的时候,当前任务应该处于Running状态
        //当前任务触发mutex.lock()之后,应该是running状态
        //当前任务触发mutex.unlock()之后,应该是running状态

        Scheduler::start();

        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        mutex.lock();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        let old_task = Scheduler::get_current_task();
        //如果任务被调度走，处于ready状态
        Scheduler::task_switch();
        assert_eq!(old_task.get_state(), TaskState::Ready);

        //此时模拟运行第二个任务，第二个任务尝试获取锁，会panic

        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );

        mutex.unlock().unwrap();
    }

    #[test]
    #[serial]
    fn test_mutex_lock_unlock() {
        kernel_init();
        let mutex = Mutex::new().unwrap();
        //现在是模拟测试所以不需要有任务内容,但是需要有任务
        Task::new("test_mutex", |_| {}).unwrap();
        Task::new("test_mutex2", |_| {}).unwrap();
        //当调度开始的时候,当前任务应该处于Running状态
        //当前任务触发mutex.lock()之后,应该是running状态
        //当前任务触发mutex.unlock()之后,应该是running状态

        Scheduler::start();

        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        mutex.lock();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );

        mutex.unlock().unwrap();

        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        Scheduler::task_switch();

        mutex.lock();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );

        mutex.unlock().unwrap();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
    }

    #[test]
    #[serial]
    fn test_mutex_lock_unlock_2() {
        kernel_init();
        let mutex = Mutex::new().unwrap();
        Task::new("test_mutex", |_| {}).unwrap();
        Task::new("test_mutex2", |_| {}).unwrap();
        //测试状态是否��确
        Scheduler::start();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        mutex.lock();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        Scheduler::task_switch();
        mutex.lock();
        let old_task = Scheduler::get_current_task();
        assert_eq!(
            old_task.get_state(),
            TaskState::Blocked(Event::Mutex(mutex.0))
        );
        Scheduler::task_switch();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        assert_eq!(old_task.get_state(), TaskState::Blocked(Event::Mutex(mutex.0)));
        mutex.unlock().unwrap();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        assert_eq!(old_task.get_state(), TaskState::Ready);
        Scheduler::task_switch();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
        assert_eq!(old_task.get_state(), TaskState::Running);

    }

    #[test]
    #[serial]
    fn test_mutex_overflow() {
        kernel_init();
        
        // 分配超过最大数量的互斥锁，需要保存到 Vec 中防止被 drop
        let mut mutexes = Vec::new();
        for _ in 0..MAX_MUTEXES {
            mutexes.push(Mutex::new().unwrap());
        }
        assert_eq!(Mutex::new().err(), Some(RtosError::MutexSlotsFull));
    }
    
    #[test]
    #[serial]
    fn test_mutex_reuse() {
        kernel_init();
        Task::new("reuse_test", |_| {}).unwrap();
        Scheduler::start();
        
        // 创建一个互斥锁
        let mutex1 = Mutex::new().unwrap();
        
        // 使用后释放它
        mutex1.lock();
        mutex1.unlock().unwrap();
        
        // 释放互斥锁本身（通过drop）
        drop(mutex1);
        
        // 创建一个新的互斥锁，应该能重用之前的槽位
        let mutex2 = Mutex::new().unwrap();
        
        // 确保可以正常使用
        mutex2.lock();
        mutex2.unlock().unwrap();
    }
    
    #[test]
    #[serial]
    fn test_multiple_blocked_tasks() {
        kernel_init();
        
        let mutex = Mutex::new().unwrap();
        let task1 = Task::new("block_test1", |_| {}).unwrap();
        let task2 = Task::new("block_test2", |_| {}).unwrap();
        let task3 = Task::new("block_test3", |_| {}).unwrap();
        
        Scheduler::start();
        
        // 当前任务（task1）获取锁
        let owner_task = Scheduler::get_current_task();
        mutex.lock();
        assert_eq!(owner_task.get_state(), TaskState::Running);
        
        // 切换到第二个任务
        Scheduler::task_switch();
        let blocked_task2 = Scheduler::get_current_task();
        assert_ne!(blocked_task2.get_taskid(), owner_task.get_taskid());
        
        // 第二个任务尝试获取锁，应该被阻塞
        mutex.lock();
        assert_eq!(blocked_task2.get_state(), TaskState::Blocked(Event::Mutex(mutex.0)));
        
        // 切换到第三个任务
        Scheduler::task_switch();
        let blocked_task3 = Scheduler::get_current_task();
        
        // 如果切换到的是 owner_task，则由它释放锁
        if blocked_task3.get_taskid() == owner_task.get_taskid() {
            // owner 释放锁
            mutex.unlock().unwrap();
            // 验证被阻塞的任务被唤醒
            assert_eq!(blocked_task2.get_state(), TaskState::Ready);
        } else {
            // 第三个任务也尝试获取锁
            mutex.lock();
            assert_eq!(blocked_task3.get_state(), TaskState::Blocked(Event::Mutex(mutex.0)));
            
            // 切换回 owner 任务
            Scheduler::task_switch();
            
            // owner 释放锁
            mutex.unlock().unwrap();
            
            // 验证被阻塞的任务被唤醒
            assert_eq!(blocked_task2.get_state(), TaskState::Ready);
            assert_eq!(blocked_task3.get_state(), TaskState::Ready);
        }
    }
}
