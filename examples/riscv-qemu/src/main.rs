//! RISC-V QEMU Example for Neon-RTOS2
//!
//! This example demonstrates the basic functionality of Neon-RTOS2
//! running on QEMU RISC-V virt machine.
//!
//! # Running
//!
//! ```bash
//! # Install QEMU
//! brew install qemu  # macOS
//! # or
//! sudo apt install qemu-system-misc  # Ubuntu
//!
//! # Install RISC-V target
//! rustup target add riscv32imac-unknown-none-elf
//!
//! # Run with QEMU
//! cargo run --release
//! ```

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::arch::asm;

// External symbols from linker script
extern "C" {
    static _sbss: u8;
    static _ebss: u8;
    static _sdata: u8;
    static _edata: u8;
    static _stack_top: u8;
    static _heap_start: u8;
    static _heap_end: u8;
}

/// Entry point - called from reset handler
#[no_mangle]
#[link_section = ".text.init"]
pub unsafe extern "C" fn _start() -> ! {
    // Set up stack pointer
    let stack_top = &_stack_top as *const u8 as usize;
    asm!(
        "mv sp, {0}",
        in(reg) stack_top,
        options(nostack)
    );
    
    // Clear BSS section
    let sbss = &_sbss as *const u8 as *mut u8;
    let ebss = &_ebss as *const u8 as *mut u8;
    let bss_len = ebss as usize - sbss as usize;
    core::ptr::write_bytes(sbss, 0, bss_len);
    
    // Jump to main
    main();
}

/// Main function
fn main() -> ! {
    // Print welcome message
    print_str("===========================================\n");
    print_str("  Neon-RTOS2 RISC-V QEMU Example\n");
    print_str("===========================================\n\n");
    
    // Test 1: Basic output
    print_str("[TEST 1] Basic Output: ");
    print_str("PASSED\n");
    
    // Test 2: Memory operations
    print_str("[TEST 2] Memory Operations: ");
    test_memory();
    print_str("PASSED\n");
    
    // Test 3: Stack operations
    print_str("[TEST 3] Stack Operations: ");
    test_stack();
    print_str("PASSED\n");
    
    // Test 4: Loop and counter
    print_str("[TEST 4] Loop Counter: ");
    test_loop();
    print_str("PASSED\n");
    
    // Test 5: Function calls
    print_str("[TEST 5] Function Calls: ");
    let result = add(10, 20);
    if result == 30 {
        print_str("PASSED\n");
    } else {
        print_str("FAILED\n");
    }
    
    print_str("\n===========================================\n");
    print_str("  All tests completed!\n");
    print_str("===========================================\n");
    
    // Exit QEMU
    exit_qemu(0);
}

/// Test memory read/write
fn test_memory() {
    static mut TEST_VAR: u32 = 0;
    unsafe {
        TEST_VAR = 0x12345678;
        let val = core::ptr::read_volatile(&TEST_VAR);
        assert!(val == 0x12345678);
    }
}

/// Test stack operations
fn test_stack() {
    let a: u32 = 100;
    let b: u32 = 200;
    let c = a + b;
    assert!(c == 300);
    
    // Test array on stack
    let arr = [1u32, 2, 3, 4, 5];
    let sum: u32 = arr.iter().sum();
    assert!(sum == 15);
}

/// Test loop counter
fn test_loop() {
    let mut counter = 0u32;
    for i in 0..100 {
        counter += i;
    }
    assert!(counter == 4950); // Sum of 0..99
}

/// Simple add function for testing function calls
fn add(a: u32, b: u32) -> u32 {
    a + b
}

/// Print a string using QEMU semihosting
fn print_str(s: &str) {
    for byte in s.bytes() {
        print_char(byte);
    }
}

/// Print a single character using QEMU semihosting
fn print_char(c: u8) {
    // Use RISC-V semihosting to output character
    // SYS_WRITEC = 0x03
    unsafe {
        asm!(
            ".option push",
            ".option norvc",
            "li a0, 0x03",      // SYS_WRITEC
            "mv a1, {0}",       // Character address
            ".balign 16",
            "slli x0, x0, 0x1f",
            "ebreak",
            "srai x0, x0, 0x07",
            ".option pop",
            in(reg) &c as *const u8,
            out("a0") _,
            out("a1") _,
            options(nostack)
        );
    }
}

/// Exit QEMU using semihosting
fn exit_qemu(code: u32) -> ! {
    // SYS_EXIT = 0x18
    // ADP_Stopped_ApplicationExit = 0x20026
    let args = [0x20026u32, code];
    unsafe {
        asm!(
            ".option push",
            ".option norvc",
            "li a0, 0x18",      // SYS_EXIT
            "mv a1, {0}",       // Args address
            ".balign 16",
            "slli x0, x0, 0x1f",
            "ebreak",
            "srai x0, x0, 0x07",
            ".option pop",
            in(reg) args.as_ptr(),
            out("a0") _,
            out("a1") _,
            options(nostack, noreturn)
        );
    }
}

/// Panic handler
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print_str("\n!!! PANIC !!!\n");
    if let Some(location) = info.location() {
        print_str("Location: ");
        print_str(location.file());
        print_str("\n");
    }
    exit_qemu(1);
}

