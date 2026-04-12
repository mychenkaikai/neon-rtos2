#[cfg(target_arch = "riscv32")]
use riscv::register::{pmpcfg0, pmpaddr0, pmpaddr1, pmpaddr2};

/// RISC-V PMP 配置
pub struct PmpConfig;

impl PmpConfig {
    /// 初始化 PMP (Physical Memory Protection)
    ///
    /// 配置如下：
    /// 1. 区域 0: 整个内存空间可被特权模式访问（M模式默认有所有权限，PMP主要限制U/S模式，
    ///    这里我们提供基础的配置模板）
    pub fn init() {
        #[cfg(target_arch = "riscv32")]
        unsafe {
            // PMP 是一种安全机制，用于限制物理内存的访问。
            // 这里我们配置 pmp0 允许整个地址空间的读写执行（仅作为演示模板）
            // 在实际的生产系统中，应根据任务权限配置具体的 PMP 槽位。

            // 配置 PMPADDR0，覆盖整个 4GB 空间（基于 NAPOT 模式）
            core::arch::asm!("csrw pmpaddr0, {}", in(reg) 0xFFFF_FFFFusize);

            // 配置 PMPCFG0
            // R=1, W=1, X=1, A=NAPOT (3) -> 0x0F | (3 << 3) = 0x1F
            core::arch::asm!("csrw pmpcfg0, {}", in(reg) 0x1F);
            
            // 确保 PMP 设置生效
            core::arch::asm!("sfence.vma");
        }
    }

    /// 配置任务的栈保护区域 (Stack Guard)
    /// 
    /// 类似 Cortex-M 的 MPU 保护，这个函数利用 PMP 的一个槽位 (比如 pmpaddr1)
    /// 将任务栈底部 32 字节配置为不可访问 (No Access)，防止栈溢出。
    #[inline]
    pub fn configure_task_guard(stack_bottom: usize) {
        #[cfg(target_arch = "riscv32")]
        unsafe {
            // PMPADDR1 编码为 NAPOT，大小 32 字节 (2^5)
            // 编码公式： (base >> 2) | ((1 << (5-3)) - 1) = (base >> 2) | 0b011
            let pmp_addr = (stack_bottom >> 2) | 0b011;
            core::arch::asm!("csrw pmpaddr1, {}", in(reg) pmp_addr);

            // PMPCFG0 的第 1 字节对应 PMP 1
            // A=NAPOT (3), R=0, W=0, X=0, L=1 (Lock，在 U 模式和 M 模式均生效)
            // L=1(bit 7), A=3(bit 3,4) -> 0x80 | 0x18 = 0x98
            
            // 先读取当前的 pmpcfg0，仅修改第 1 字节
            let current_cfg = pmpcfg0::read().bits;
            let new_cfg = (current_cfg & 0xFFFF_00FF) | (0x98 << 8);
            // 这里我们用内联汇编写入 pmpcfg0，因为 rsicv 库的一些版本不直接支持 write(bits)
            core::arch::asm!("csrw pmpcfg0, {}", in(reg) new_cfg);
            
            // 确保 PMP 设置生效
            core::arch::asm!("sfence.vma");
        }
        
        #[cfg(not(target_arch = "riscv32"))]
        {
            let _ = stack_bottom;
        }
    }
}
