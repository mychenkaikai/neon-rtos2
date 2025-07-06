#![no_std]
#![no_main]

use core::panic::PanicInfo;

use cortex_m::Peripherals;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_semihosting::hprintln;
use neon_rtos2::utils::kernel_init;
use neon_rtos2::task::Task;
use neon_rtos2::schedule::Scheduler;

const SYST_FREQ: u32 = 100;
const SYS_CLOCK: u32 = 12_000_000;
// 定义 SysTick 的重新加载值
const SYST_RELOAD: u32 = SYS_CLOCK / SYST_FREQ;

// #[panic_handler]
// fn panic_halt(p: &PanicInfo) -> ! {
//     hprintln!("{}", p);
//     loop {}
// }

fn test1(_arg: usize) {
    hprintln!("task1");
    loop {}
}
fn test2(_arg: usize) {
    hprintln!("task2");
    loop {}
}

fn main() -> ! {
    kernel_init();
    let p = Peripherals::take().unwrap();
    let mut syst = p.SYST;

    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(SYST_RELOAD); // period = 10ms
    syst.enable_counter();
    syst.enable_interrupt();

    let task1 = Task::new("task1", test1);
    let task2 = Task::new("task2", test2);

    Scheduler::start();
    loop {}
}
