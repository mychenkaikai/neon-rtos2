#![no_std]
#![no_main]

use core::panic::PanicInfo;
use cortex_m::Peripherals;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::entry;
use cortex_m_semihosting::hprintln;
use neon_rtos2::schedule::Scheduler;
use neon_rtos2::task::Task;
use neon_rtos2::utils::kernel_init;
use neon_rtos2::define_signal;
use neon_rtos2::timer::{Timer, Delay};

const SYST_FREQ: u32 = 1000;
const SYS_CLOCK: u32 = 12_000_000;
const SYST_RELOAD: u32 = SYS_CLOCK / SYST_FREQ;


define_signal!(MY_SIGNAL);
fn test1(_arg: usize) {
    hprintln!("task1");
    loop {
        hprintln!("task1");
        // MY_SIGNAL().wait();
        Delay::delay(1000);
    }
}
fn test2(_arg: usize) {
    hprintln!("task2");
    loop {
        hprintln!("task2");
        // MY_SIGNAL().send();
        Delay::delay(1000);
    }
}



#[entry]
fn main() -> ! {
    kernel_init();
    let p = Peripherals::take().unwrap();
    let mut syst = p.SYST;

    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(SYST_RELOAD); 
    syst.enable_counter();
    syst.enable_interrupt();

    let task1 = Task::new("task1", test1);
    let task2 = Task::new("task2", test2);

    Scheduler::start();
    loop {}
    
}
