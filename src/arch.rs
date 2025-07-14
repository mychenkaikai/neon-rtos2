#[cfg(all(feature = "cortex_m3", not(test)))]
mod cortex_m3;
#[cfg(test)]
mod test;

#[cfg(all(feature = "cortex_m3", not(test)))]
pub(crate) use cortex_m3::{init_task_stack, start_first_task};

#[cfg(test)]
pub(crate) use test::{init_task_stack, start_first_task};
