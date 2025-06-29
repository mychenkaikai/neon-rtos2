#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
mod arch;
mod config;
mod task;
mod schedule;
mod utils;
mod signal;

use core::panic::PanicInfo;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
    }
}
