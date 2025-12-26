use crate::kernel::task::Task;

static mut CURRENT_TIME: usize = 0;

pub struct Systick;

impl Systick {
    pub(crate) fn init() {
        unsafe {
            CURRENT_TIME = 0;
        }
    }

    pub(crate) fn systick_inc() {
        unsafe {
            CURRENT_TIME += 1;
        }
    }

    pub(crate) fn get_current_time() -> usize {
        unsafe {
            return CURRENT_TIME;
        }
    }

    #[cfg(test)]
    pub fn add_current_time(ms_time: usize) -> usize {
        unsafe {
            CURRENT_TIME += ms_time;
            return CURRENT_TIME;
        }
    }
}

