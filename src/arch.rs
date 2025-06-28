use crate::utils::task_exit_error;
use core::mem::size_of;

#[cfg(not(test))]
pub(crate) fn init_task_stack(top_of_stack: &mut usize, func: fn(usize), p_args: usize) {
    unsafe {
        *top_of_stack &= !7;
        *top_of_stack -= 1 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = 0x0100_0000;
        *top_of_stack -= 1 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = 0xffff_fffe & (func as usize);
        *top_of_stack -= 1 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = task_exit_error as usize;
        *top_of_stack -= 5 * size_of::<usize>();
        *(*top_of_stack as *mut usize) = p_args;
        *top_of_stack -= 8 * size_of::<usize>();
    }
}

#[cfg(test)]
pub(crate) fn init_task_stack(top_of_stack: &mut usize, _func: fn(usize), _p_args: usize) {

    *top_of_stack &= !7;
    
}
