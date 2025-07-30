#![no_std]
#![no_main]


use cortex_m_rt::entry;
use neon_rtos2::schedule::Scheduler;
use neon_rtos2::task::Task;
use neon_rtos2::utils::kernel_init;
use neon_rtos2::define_signal;
use neon_rtos2::timer::{Timer, Delay};
use neon_rtos2::log::{log_write, LogLevel, set_log_level, get_log_level};
use neon_rtos2::{info, error, warn, debug, trace};





define_signal!(MY_SIGNAL);
fn test1(_arg: usize) {
    info!("task1");
    loop {
        debug!("task1");
        // MY_SIGNAL().wait();
        //Delay::delay(1000);
    }
}
fn test2(_arg: usize) {
    info!("task2");
    loop {
        debug!("task2");
        // MY_SIGNAL().send();
        //Delay::delay(1000);
    }
}



#[entry]
fn main() -> ! {
    kernel_init();
    set_log_level(LogLevel::Debug);
    info!("初始化完成");


    let task1 = Task::new("task1", test1);
    let task2 = Task::new("task2", test2);
    info!("任务创建完成");

    Scheduler::start();
    loop {}
    
}
