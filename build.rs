
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    
    // 根据目标平台编译对应的汇编文件
    if target == "thumbv7m-none-eabi" {
        // ARM Cortex-M3 汇编
        println!("cargo:warning=在ARM目标上编译汇编");
        println!("cargo:rerun-if-changed=src/hal/cortex_m3/asm/context.s");
        cc::Build::new()
            .file("src/hal/cortex_m3/asm/context.s")
            .compile("cortex_m3_asm");
    } else if target.starts_with("riscv32") {
        // RISC-V 32位 - 使用 Rust 内联汇编，不需要外部汇编器
        println!("cargo:warning=RISC-V目标使用内联汇编");
    } else {
        println!("cargo:warning=非嵌入式目标，跳过汇编编译: {}", target);
    }
}