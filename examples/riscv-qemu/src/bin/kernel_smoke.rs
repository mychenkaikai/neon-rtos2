#![no_std]
#![no_main]
#[path = "../../../shared/task_lifecycle.rs"]
mod task_lifecycle;

use neon_rtos2::hal::riscv::{configure_systick, read_mtime};
use neon_rtos2::prelude::*;
use riscv_rt::entry;

static UART: UartOutput<0x1000_0000> = unsafe { UartOutput::new() };

fn exit(value: u32) -> ! {
    // QEMU virt's SiFive test finisher.
    unsafe { (0x10_0000 as *mut u32).write_volatile(value) };
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    neon_rtos2::error!("FAIL: {}", info);
    exit(0x1_3333);
}

#[entry]
fn main() -> ! {
    set_log_output(&UART);
    set_log_level(LogLevel::Info);
    kernel_init();
    const PERIOD: u32 = 20_000; // Deliberately different from the default.
    configure_systick(Duration::from_millis(2)).unwrap();
    Task::new("controller", || {
        let before = read_mtime();
        let start = Systick::now();
        Delay::delay(Duration::from_millis(40)).unwrap();
        let elapsed = read_mtime() - before;
        assert!(Systick::now() - start >= 20);
        // The first tick can be a partial period; idle must not invent 20 ticks.
        assert!(elapsed >= 19 * u64::from(PERIOD) && elapsed < 100 * u64::from(PERIOD));
        let before_mask = critical_section::with(|_| {
            let ticks = Systick::now();
            let hardware = read_mtime();
            while read_mtime() - hardware < 5 * u64::from(PERIOD) {
                core::hint::spin_loop();
            }
            ticks
        });
        while Systick::now() == before_mask {
            core::hint::spin_loop();
        }
        assert!(
            Systick::now() - before_mask >= 5,
            "missed periods were not accounted for"
        );
        task_lifecycle::exercise();
        neon_rtos2::info!(
            "PASS kernel_smoke: CLINT elapsed={}, missed ticks, task exit/reuse",
            elapsed
        );
        exit(0x5555);
    })
    .unwrap();
    Scheduler::enable_priority_scheduling();
    Scheduler::start();
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
