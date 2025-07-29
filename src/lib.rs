#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[cfg(test)]
extern crate std;
extern crate alloc;
mod arch;
mod config;
pub mod task;
pub mod schedule;
pub mod utils;
pub mod event;
pub mod signal;
pub mod timer;
pub mod systick;
pub mod mutex;
mod mq;
pub mod log;
pub mod ipc;
pub mod ipc_elegant;
#[cfg(not(test))]
mod allocator;

pub use paste;