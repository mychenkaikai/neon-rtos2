use crate::kernel::task::Task;
use crate::kernel::scheduler::Scheduler;
use crate::kernel::time::timer::Timer;
use crate::kernel::time::systick::Systick;
use crate::sync::mutex::Mutex;
use crate::sync::signal::Signal;
use crate::ipc::queue::Mq;

/// 内核初始化
/// 
/// 初始化所有内核子系统，包括：
/// - 内存分配器
/// - 任务管理
/// - 调度器
/// - 定时器
/// - 系统时钟
/// - 互斥锁
/// - 信号量
/// - 消息队列
/// 
/// # 注意
/// 
/// 此函数会完全重置所有全局状态，适合在测试开始时调用。
pub fn kernel_init() {
    crate::mem::allocator::init_heap();
    Task::init();
    Scheduler::init();
    Timer::init();
    Systick::init();
    Mutex::init();
    Signal::init();
    Mq::<u8, 1>::init();  // 初始化消息队列槽位
}


pub(crate) fn task_exit_error() -> ! {
    loop {}
}