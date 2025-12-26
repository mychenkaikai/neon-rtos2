#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[cfg(test)]
extern crate std;
extern crate alloc;

pub mod error;
pub mod kernel;
pub mod sync;
pub mod ipc;
pub mod hal;
pub mod mem;
pub mod config;
pub mod log;
pub mod utils;

pub use paste;
