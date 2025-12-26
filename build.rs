
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    
    // 只在ARM目标上编译汇编，跳过macOS
    if target == "thumbv7m-none-eabi" {
        println!("cargo:warning=在ARM目标上编译汇编");
        println!("cargo:rerun-if-changed=src/hal/cortex_m3/asm/context.s");
        cc::Build::new()
            .file("src/hal/cortex_m3/asm/context.s")
            .compile("cortex_m3_asm");
    } else {
        println!("cargo:warning=非ARM目标，跳过汇编编译: {}", target);
    }
}