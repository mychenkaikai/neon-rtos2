


pub(crate) fn init_task_stack(top_of_stack: &mut usize, _func: fn(usize), _p_args: usize) {

    *top_of_stack &= !7;
    
}


pub(crate) fn start_first_task() {

}

pub(crate) fn trigger_schedule() {

}

pub(crate) fn init_idle_task() {

}
