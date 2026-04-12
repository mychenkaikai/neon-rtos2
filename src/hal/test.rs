// 测试环境使用 critical-section 的 std feature 提供的实现
// 不需要自定义 critical_section 实现

pub(crate) fn init_task_stack(top_of_stack: &mut usize, _func: fn(usize), _p_args: usize) {

    *top_of_stack &= !7;
    
}


pub(crate) fn start_first_task() {

}

pub(crate) fn trigger_schedule() {

}

pub(crate) fn init_idle_task() {
    fn idle_task(_arg: usize) {
        loop {
            // 获取距离下一个定时器超时的预计时间
            let next_timeout = crate::kernel::time::timer::Timer::get_next_timeout();
            crate::kernel::power::enter_idle(next_timeout);
        }
    }
    let _task = crate::kernel::task::Task::new("idle", idle_task).unwrap();
}
