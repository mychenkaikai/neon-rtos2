use crate::kernel::task::Task;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::timer::Timer;
use crate::kernel::time::systick::Systick;
use crate::ipc::queue::Mq;

/// 内核初始化
/// 
/// 初始化所有内核子系统，包括：
/// - 内存分配器
/// - 任务管理
/// - 调度器
/// - 定时器
/// - 系统时钟
/// - 消息队列
/// 
/// # 注意
/// 
/// 同步原语（Mutex, Signal, Semaphore, CondVar）现在基于 Arc 实现，
/// 无需全局初始化，可以在任务中局部创建并通过闭包传递。
/// 
/// 此函数会完全重置所有全局状态，适合在测试开始时调用。
pub fn kernel_init() {
    crate::mem::allocator::init_heap();
    Task::init();
    Scheduler::init();
    Timer::init();
    Systick::init();
    Mq::<u8, 1>::init();  // 初始化消息队列槽位
}


pub(crate) fn task_exit_error() -> ! {
    loop {}
}