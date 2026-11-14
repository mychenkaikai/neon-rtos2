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
    fn idle_task() {
        loop {
            crate::kernel::power::enter_idle(None);
        }
    }
    let _task = crate::kernel::task::Task::builder("idle").priority(crate::kernel::task::Priority::Idle).spawn(idle_task).unwrap();
}
