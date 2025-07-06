
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    
    // 只在ARM目标上编译汇编，跳过macOS
    if target == "thumbv7m-none-eabi" {
        println!("cargo:warning=在ARM目标上编译汇编");
        println!("cargo:rerun-if-changed=src/arch/cortex_m3/cortex_m3.s");
        cc::Build::new()
            .file("src/arch/cortex_m3/cortex_m3.s")
            .compile("cortex_m3_asm");
    } else {
        println!("cargo:warning=非ARM目标，跳过汇编编译: {}", target);
    }
}