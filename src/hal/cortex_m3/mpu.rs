use cortex_m::peripheral::MPU;

/// Cortex-M3 MPU 配置
pub struct MpuConfig;

impl MpuConfig {
    /// 初始化 MPU，提供基本的内存保护
    ///
    /// 配置如下：
    /// 1. 背景区（Background Region）：特权模式可访问，用户模式不可访问
    /// 2. Flash 区（代码）：只读，可执行
    /// 3. RAM 区（数据）：读写，不可执行（防止栈溢出攻击）
    /// 4. 外设区：读写，不可执行
    pub fn init(mpu: &mut MPU) {
        unsafe {
            // 确保 MPU 被禁用
            mpu.ctrl.modify(|r| r & !1);

            // 区域 0: 整个 4GB 地址空间，特权读写，非特权不可访问 (背景区域)
            // 禁用，使用 CTRL.PRIVDEFENA 作为背景
            
            // 区域 1: Flash (0x08000000) 
            // 假设 512KB Flash
            mpu.rnr.write(1);
            mpu.rbar.write(0x0800_0000); // 地址
            // XN=0 (可执行), AP=0b110 (读), TEX=0b000, S=0, C=1, B=0, SIZE=18 (512KB), ENABLE=1
            mpu.rasr.write((0 << 28) | (0b011 << 24) | (0b000 << 19) | (0 << 18) | (1 << 17) | (0 << 16) | (18 << 1) | 1);

            // 区域 2: SRAM (0x20000000)
            // 假设 128KB SRAM
            mpu.rnr.write(2);
            mpu.rbar.write(0x2000_0000); // 地址
            // XN=1 (不可执行), AP=0b011 (读写), TEX=0b000, S=1, C=1, B=0, SIZE=16 (128KB), ENABLE=1
            mpu.rasr.write((1 << 28) | (0b011 << 24) | (0b000 << 19) | (1 << 18) | (1 << 17) | (0 << 16) | (16 << 1) | 1);

            // 区域 3: 外设 (0x40000000)
            // 512MB
            mpu.rnr.write(3);
            mpu.rbar.write(0x4000_0000);
            // XN=1 (不可执行), AP=0b011 (读写), TEX=0b000, S=1, C=0, B=1, SIZE=28 (512MB), ENABLE=1
            mpu.rasr.write((1 << 28) | (0b011 << 24) | (0b000 << 19) | (1 << 18) | (0 << 17) | (1 << 16) | (28 << 1) | 1);

            // 启用 MPU，启用默认内存映射作为特权访问的背景区域，并且开启 MPU 在硬故障/NMI 等处理程序期间也生效 (HFNMIENA=1)
            // ENABLE=1, HFNMIENA=1, PRIVDEFENA=1
            mpu.ctrl.modify(|r| r | (1 << 2) | (1 << 1) | 1);
            
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
        }
    }

    /// 配置任务的栈保护区域 (Stack Guard)
    /// 
    /// 这个函数在上下文切换时调用。它将 MPU 的区域 4 设定在任务栈的最低端（32 字节）。
    /// 一旦任务的栈指针越界访问该区域，立即触发 MemManage Fault，防止栈溢出破坏其他任务的数据。
    #[inline]
    pub fn configure_task_guard(stack_bottom: u32) {
        unsafe {
            let mpu_rnr = 0xE000_ED98 as *mut u32;
            let mpu_rbar = 0xE000_ED9C as *mut u32;
            let mpu_rasr = 0xE000_EDA0 as *mut u32;
            
            // 选择区域 4
            core::ptr::write_volatile(mpu_rnr, 4);
            
            // 地址必须 32 字节对齐
            core::ptr::write_volatile(mpu_rbar, stack_bottom & !0x1F);
            
            // XN=1 (不可执行), AP=0b000 (不可访问), TEX=0b000, S=1, C=1, B=0, SIZE=4 (32 Bytes), ENABLE=1
            // 这样特权和非特权代码都不能访问这个 32 字节的保护区
            let rasr_val = (1 << 28) | (0b000 << 24) | (0b000 << 19) | (1 << 18) | (1 << 17) | (0 << 16) | (4 << 1) | 1;
            core::ptr::write_volatile(mpu_rasr, rasr_val);
            
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
        }
    }
}
