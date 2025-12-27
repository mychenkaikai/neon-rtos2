//! Build script for RISC-V QEMU example

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // Get output directory
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    
    // Copy memory.x to output directory
    let memory_x = include_bytes!("memory.x");
    let mut f = File::create(out_dir.join("memory.x")).unwrap();
    f.write_all(memory_x).unwrap();
    
    // Tell cargo to look for linker scripts in the output directory
    println!("cargo:rustc-link-search={}", out_dir.display());
    
    // Rebuild if memory.x changes
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}

