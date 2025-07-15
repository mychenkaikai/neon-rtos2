use crate::config::MAX_MUTEXES;
use crate::schedule::Scheduler;
use crate::task::Task;
use crate::event::Event;

static mut MUTEX_LIST: [_Mutex; MAX_MUTEXES] = [_Mutex {
    locked: false,
    used: false,
    owner: None,
}; MAX_MUTEXES];

#[derive(PartialEq, Clone, Copy)]
pub struct _Mutex {
    used: bool,
    locked: bool,
    owner: Option<Task>,
}

pub struct Mutex(usize);

impl Mutex {
    pub fn init() {
        unsafe {
            for i in 0..MAX_MUTEXES {
                MUTEX_LIST[i] = _Mutex {
                    locked: false,
                    used: false,
                    owner: None,
                };
            }
        }
    }

    pub fn new() -> Self {
        unsafe {
            for i in 0..MAX_MUTEXES {
                if !MUTEX_LIST[i].used {
                    MUTEX_LIST[i].used = true;
                    MUTEX_LIST[i].owner = None;
                    return Mutex(i);
                }
            }
        }
        panic!("No free mutex slot");
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

    pub fn unlock(&self) {
        unsafe {
            if MUTEX_LIST[self.0].owner != Some(Scheduler::get_current_task()) {
                panic!("Mutex not owned by current task {:?}", MUTEX_LIST[self.0].owner.unwrap().get_name());
            }
            MUTEX_LIST[self.0].locked = false;
            MUTEX_LIST[self.0].owner = None;
            Event::wake_task(Event::Mutex(self.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::Scheduler;
    use crate::task::Task;
    use crate::task::TaskState;
    use crate::utils::kernel_init;

    #[test]
    #[should_panic]
    fn test_mutex() {
        kernel_init();
        let mutex = Mutex::new();
        //现在是模拟测试所以不需要有任务内容,但是需要有任务
        Task::new("test_mutex", |_| {});
        Task::new("test_mutex2", |_| {});
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

        mutex.unlock();
    }

    #[test]
    fn test_mutex_lock_unlock() {
        kernel_init();
        let mutex = Mutex::new();
        //现在是模拟测试所以不需要有任务内容,但是需要有任务
        Task::new("test_mutex", |_| {});
        Task::new("test_mutex2", |_| {});
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

        mutex.unlock();

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

        mutex.unlock();
        assert_eq!(
            Scheduler::get_current_task().get_state(),
            TaskState::Running
        );
    }

    #[test]
    fn test_mutex_lock_unlock_2() {
        kernel_init();
        let mutex = Mutex::new();
        Task::new("test_mutex", |_| {});
        Task::new("test_mutex2", |_| {});
        //测试状态是否正确
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
        mutex.unlock();
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
}
