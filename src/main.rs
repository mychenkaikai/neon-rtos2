#![no_std]
#![no_main]
mod task;
mod config;
mod arch;
mod utils;

use crate::task::Task;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

fn task1(args: usize) {
    loop {
    }
}

fn task2(args: usize) {
    loop {
    }
}

fn main() {
    let task1 = Task::new("task1", task1);
    let task2 = Task::new("task2", task2);
    task1.run();
    task2.run();
}
