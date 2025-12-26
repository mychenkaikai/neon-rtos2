#[cfg(all(feature = "cortex_m3", not(test), target_arch = "arm"))]
pub mod cortex_m3;
#[cfg(any(test, not(target_arch = "arm")))]
pub mod test;

#[cfg(all(feature = "cortex_m3", not(test), target_arch = "arm"))]
pub(crate) use cortex_m3::{ init_task_stack, start_first_task, trigger_schedule, init_idle_task};

#[cfg(any(test, not(target_arch = "arm")))]
pub(crate) use test::{ init_task_stack, start_first_task, trigger_schedule,init_idle_task};  
