
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap();
    if target == "thumbv7m-none-eabi" {
        let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
        File::create(out.join("memory.x"))
            .unwrap()
            .write_all(include_bytes!("memory.x"))
            .unwrap();
        println!("cargo:rustc-link-search={}", out.display());
        println!("cargo:rerun-if-changed=memory.x");
        println!("cargo:rustc-link-arg=--nmagic");
        println!("cargo:rustc-link-arg=-Tlink.x");
        // Only the latency firmware injects a software-pended interrupt at CPSID.
        println!("cargo:rustc-link-arg-bin=wait_latency=--wrap=__cpsid");
        println!("cargo:rustc-link-arg-bin=wait_latency=--wrap=SysTick");
        println!("cargo:rustc-link-arg-bin=wait_latency=--wrap=__cpsie");
    } else {
        println!("Skipping memory.x for x86_64 target");
    }
}
