//! # 电源管理模块
//!
//! 提供系统电源状态管理功能，支持多种低功耗模式。
//!
//! ## 电源状态
//!
//! | 状态 | 说明 | 功耗 | 唤醒时间 |
//! |------|------|------|----------|
//! | Active | 全速运行 | 最高 | - |
//! | Idle | CPU 暂停，外设运行 | 中等 | 快速 |
//! | Sleep | 低功耗，部分外设关闭 | 低 | 中等 |
//! | DeepSleep | 最低功耗，仅保留唤醒源 | 最低 | 慢速 |
//!
//! ## 状态转换图
//!
//! ```text
//!                    ┌──────────┐
//!          ┌────────►│  Active  │◄────────┐
//!          │         └────┬─────┘         │
//!          │              │               │
//!          ���    enter_idle()    enter_sleep()
//!          │              │               │
//!          │              ▼               │
//!          │         ┌──────────┐         │
//!     (interrupt)    │   Idle   │    (interrupt)
//!          │         └────┬─────┘         │
//!          │              │               │
//!          │              ▼               │
//!          │         ┌──────────┐         │
//!          └─────────│  Sleep   │─────────┘
//!                    └────┬─────┘
//!                         │
//!               enter_deep_sleep()
//!                         │
//!                         ▼
//!                    ┌──────────┐
//!                    │DeepSleep │
//!                    └──────────┘
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! # use neon_rtos2::kernel::power::{PowerState};
//! # struct PowerManager;
//! # impl PowerManager {
//! #     fn global() -> Self { Self }
//! #     fn enable_wakeup(&self, _: WakeupSource) {}
//! #     fn enter_idle(&self) {}
//! #     fn enter_sleep(&self) {}
//! # }
//! # enum WakeupSource { Timer, GpioPin(u8) }
//!
//! // 获取电源管理器
//! let pm = PowerManager::global();
//!
//! // 配置唤醒源
//! pm.enable_wakeup(WakeupSource::Timer);
//! pm.enable_wakeup(WakeupSource::GpioPin(0));
//!
//! // 进入低功耗模式
//! pm.enter_idle();
//!
//! // 或者进入更深的睡眠
//! pm.enter_sleep();
//! ```

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

// ============================================================================
// 电源状态
// ============================================================================

/// 电源状态
///
/// 表示系统当前的电源模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerState {
    /// 活动状态 - 全速运行
    ///
    /// CPU 和所有外设正常运行，功耗最高
    Active = 0,
    
    /// 空闲状态 - CPU 暂停，外设运行
    ///
    /// CPU 进入 WFI（Wait For Interrupt）状态，
    /// 外设继续运行，任何中断都可以唤醒
    Idle = 1,
    
    /// 睡眠状态 - 低功耗，快速唤醒
    ///
    /// CPU 和部分外设关闭，保留 RAM 内容，
    /// 只有配置的唤醒源可以唤醒
    Sleep = 2,
    
    /// 深度睡眠 - 最低功耗，慢速唤醒
    ///
    /// 大部分系统关闭，仅保留最小唤醒电路，
    /// 唤醒后可能需要重新初始化
    DeepSleep = 3,
}

impl From<u8> for PowerState {
    fn from(value: u8) -> Self {
        match value {
            0 => PowerState::Active,
            1 => PowerState::Idle,
            2 => PowerState::Sleep,
            3 => PowerState::DeepSleep,
            _ => PowerState::Active,
        }
    }
}

// ============================================================================
// 唤醒源
// ============================================================================

/// 唤醒源
///
/// 定义可以将系统从低功耗模式唤醒的事件源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeupSource {
    /// 外部中断
    ///
    /// 参数为中断编号（0-15）
    ExternalInterrupt(u8),
    
    /// 定时器唤醒
    Timer,
    
    /// UART 接收唤醒
    UartRx,
    
    /// GPIO 引脚唤醒
    ///
    /// 参数为引脚编号（0-7）
    GpioPin(u8),
    
    /// RTC 闹钟唤醒
    RtcAlarm,
    
    /// 看门狗唤醒
    Watchdog,
}

impl WakeupSource {
    /// 获取唤醒源对应的位掩码
    fn to_bit_mask(&self) -> u32 {
        match self {
            WakeupSource::ExternalInterrupt(n) => 1 << (*n as u32).min(15),
            WakeupSource::Timer => 1 << 16,
            WakeupSource::UartRx => 1 << 17,
            WakeupSource::GpioPin(n) => 1 << (24 + (*n as u32).min(7)),
            WakeupSource::RtcAlarm => 1 << 18,
            WakeupSource::Watchdog => 1 << 19,
        }
    }
}

// ============================================================================
// 电源管理器
// ============================================================================

/// 电源管理器
///
/// 管理系统电源状态和低功耗模式
///
/// # 示例
///
/// ```rust,ignore
/// let pm = PowerManager::global();
///
/// // 启用定时器唤醒
/// pm.enable_wakeup(WakeupSource::Timer);
///
/// // 进入空闲模式
/// pm.enter_idle();
/// ```
pub struct PowerManager {
    /// 当前电源状态
    current_state: AtomicU8,
    /// 启用的唤醒源（位掩码）
    wakeup_sources: AtomicU32,
    /// 睡眠前的回调计数
    sleep_callbacks: AtomicU32,
}

impl PowerManager {
    /// 创建新的电源管理器
    pub const fn new() -> Self {
        Self {
            current_state: AtomicU8::new(PowerState::Active as u8),
            wakeup_sources: AtomicU32::new(0),
            sleep_callbacks: AtomicU32::new(0),
        }
    }

    /// 获取全局电源管理器实例
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let pm = PowerManager::global();
    /// pm.enter_idle();
    /// ```
    pub fn global() -> &'static Self {
        static POWER_MANAGER: PowerManager = PowerManager::new();
        &POWER_MANAGER
    }

    /// 获取当前电源状态
    ///
    /// # 返回值
    ///
    /// 当前的电源状态
    pub fn state(&self) -> PowerState {
        PowerState::from(self.current_state.load(Ordering::Acquire))
    }

    /// 进入空闲状态
    ///
    /// CPU 进入 WFI 状态，等待中断唤醒。
    /// 这是最轻量的低功耗模式，任何中断都可以唤醒。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// // 在空闲任务中使用
    /// loop {
    ///     PowerManager::global().enter_idle();
    /// }
    /// ```
    pub fn enter_idle(&self) {
        self.current_state.store(PowerState::Idle as u8, Ordering::Release);
        
        // 架构相关的空闲实现
        Self::arch_enter_idle();
        
        // 唤醒后恢复 Active 状态
        self.current_state.store(PowerState::Active as u8, Ordering::Release);
    }

    /// 进入睡眠状态
    ///
    /// 进入低功耗睡眠模式，只有配置的唤醒源可以唤醒。
    ///
    /// # 注意
    ///
    /// 进入睡眠前应确保已配置适当的唤醒源，
    /// 否则系统可能无法唤醒。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let pm = PowerManager::global();
    /// pm.enable_wakeup(WakeupSource::Timer);
    /// pm.enter_sleep();
    /// ```
    pub fn enter_sleep(&self) {
        // 检查是否有唤醒源
        if self.wakeup_sources.load(Ordering::Acquire) == 0 {
            // 没有唤醒源，退化为 idle
            self.enter_idle();
            return;
        }

        self.current_state.store(PowerState::Sleep as u8, Ordering::Release);
        
        // 配置睡眠模式
        Self::arch_configure_sleep(self.wakeup_sources.load(Ordering::Acquire));
        
        // 进入���眠
        Self::arch_enter_sleep();
        
        // 唤醒后恢复 Active 状态
        self.current_state.store(PowerState::Active as u8, Ordering::Release);
    }

    /// 进入深度睡眠状态
    ///
    /// 进入最低功耗模式，大部分系统关闭。
    /// 唤醒后可能需要重新初始化部分外设。
    ///
    /// # 警告
    ///
    /// 深度睡眠会关闭大部分系统，唤醒时间较长，
    /// 且可能丢失部分状态。请谨慎使用。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let pm = PowerManager::global();
    /// pm.enable_wakeup(WakeupSource::RtcAlarm);
    /// pm.enter_deep_sleep();
    /// // 唤醒后可能需要重新初始化
    /// ```
    pub fn enter_deep_sleep(&self) {
        // 检查是否有唤醒源
        if self.wakeup_sources.load(Ordering::Acquire) == 0 {
            // 没有唤醒源，退化为 sleep
            self.enter_sleep();
            return;
        }

        self.current_state.store(PowerState::DeepSleep as u8, Ordering::Release);
        
        // 配置深度睡眠模式
        Self::arch_configure_deep_sleep(self.wakeup_sources.load(Ordering::Acquire));
        
        // 进入深度睡眠
        Self::arch_enter_deep_sleep();
        
        // 唤醒后恢复 Active 状态
        self.current_state.store(PowerState::Active as u8, Ordering::Release);
    }

    /// 启用唤醒源
    ///
    /// # 参数
    ///
    /// - `source`: 要启用的唤醒源
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let pm = PowerManager::global();
    /// pm.enable_wakeup(WakeupSource::Timer);
    /// pm.enable_wakeup(WakeupSource::GpioPin(0));
    /// ```
    pub fn enable_wakeup(&self, source: WakeupSource) {
        let bit = source.to_bit_mask();
        self.wakeup_sources.fetch_or(bit, Ordering::AcqRel);
    }

    /// 禁用唤醒源
    ///
    /// # 参数
    ///
    /// - `source`: 要禁用的唤醒源
    pub fn disable_wakeup(&self, source: WakeupSource) {
        let bit = source.to_bit_mask();
        self.wakeup_sources.fetch_and(!bit, Ordering::AcqRel);
    }

    /// 检查唤醒源是否启用
    ///
    /// # 参数
    ///
    /// - `source`: 要检查的唤醒源
    ///
    /// # 返回值
    ///
    /// 如果唤醒源已启用返回 `true`
    pub fn is_wakeup_enabled(&self, source: WakeupSource) -> bool {
        let bit = source.to_bit_mask();
        (self.wakeup_sources.load(Ordering::Acquire) & bit) != 0
    }

    /// 清除所有唤醒源
    pub fn clear_wakeup_sources(&self) {
        self.wakeup_sources.store(0, Ordering::Release);
    }

    /// 获取启用的唤醒源位掩码
    pub fn wakeup_sources_mask(&self) -> u32 {
        self.wakeup_sources.load(Ordering::Acquire)
    }

    // ========================================================================
    // 架构相关实现
    // ========================================================================

    /// 架构相关：进入空闲模式
    #[inline]
    fn arch_enter_idle() {
        #[cfg(all(target_arch = "arm", feature = "cortex_m3"))]
        unsafe {
            // Cortex-M: WFI 指令
            cortex_m::asm::wfi();
        }
        
        #[cfg(not(all(target_arch = "arm", feature = "cortex_m3")))]
        {
            // 测试/其他平台：空操作
            core::hint::spin_loop();
        }
    }

    /// 架构相关：配置睡眠模式
    #[inline]
    fn arch_configure_sleep(_wakeup_mask: u32) {
        #[cfg(all(target_arch = "arm", feature = "cortex_m3"))]
        {
            // Cortex-M: 配置 SCB->SCR 寄存器
            // 清除 SLEEPDEEP 位（普通睡眠）
            unsafe {
                let scr = 0xE000_ED10 as *mut u32;
                let val = core::ptr::read_volatile(scr);
                core::ptr::write_volatile(scr, val & !(1 << 2));
            }
        }
    }

    /// 架构相关：进入睡眠模式
    #[inline]
    fn arch_enter_sleep() {
        #[cfg(all(target_arch = "arm", feature = "cortex_m3"))]
        unsafe {
            cortex_m::asm::wfi();
        }
        
        #[cfg(not(all(target_arch = "arm", feature = "cortex_m3")))]
        {
            core::hint::spin_loop();
        }
    }

    /// 架构相关：配置深度睡眠模式
    #[inline]
    fn arch_configure_deep_sleep(_wakeup_mask: u32) {
        #[cfg(all(target_arch = "arm", feature = "cortex_m3"))]
        {
            // Cortex-M: 设置 SCB->SCR 的 SLEEPDEEP 位
            unsafe {
                let scr = 0xE000_ED10 as *mut u32;
                let val = core::ptr::read_volatile(scr);
                core::ptr::write_volatile(scr, val | (1 << 2));
            }
        }
    }

    /// 架构相关：进入深度睡眠模式
    #[inline]
    fn arch_enter_deep_sleep() {
        #[cfg(all(target_arch = "arm", feature = "cortex_m3"))]
        unsafe {
            cortex_m::asm::wfi();
        }
        
        #[cfg(not(all(target_arch = "arm", feature = "cortex_m3")))]
        {
            core::hint::spin_loop();
        }
    }
}

impl Default for PowerManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 便捷函数
// ============================================================================

/// 进入空闲模式
///
/// 这是 `PowerManager::global().enter_idle()` 的便捷函数
///
/// # 示例
///
/// ```rust,ignore
/// use neon_rtos2::kernel::power::enter_idle;
///
/// // 在空闲任务中
/// loop {
///     enter_idle();
/// }
/// ```
pub fn enter_idle() {
    PowerManager::global().enter_idle();
}

/// 进入睡眠模式
///
/// 这是 `PowerManager::global().enter_sleep()` 的便捷函数
pub fn enter_sleep() {
    PowerManager::global().enter_sleep();
}

/// 进入深度睡眠模式
///
/// 这是 `PowerManager::global().enter_deep_sleep()` 的便捷函数
pub fn enter_deep_sleep() {
    PowerManager::global().enter_deep_sleep();
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_state_conversion() {
        assert_eq!(PowerState::from(0), PowerState::Active);
        assert_eq!(PowerState::from(1), PowerState::Idle);
        assert_eq!(PowerState::from(2), PowerState::Sleep);
        assert_eq!(PowerState::from(3), PowerState::DeepSleep);
        assert_eq!(PowerState::from(255), PowerState::Active); // 无效值
    }

    #[test]
    fn test_wakeup_source_bit_mask() {
        assert_eq!(WakeupSource::ExternalInterrupt(0).to_bit_mask(), 1 << 0);
        assert_eq!(WakeupSource::ExternalInterrupt(5).to_bit_mask(), 1 << 5);
        assert_eq!(WakeupSource::Timer.to_bit_mask(), 1 << 16);
        assert_eq!(WakeupSource::UartRx.to_bit_mask(), 1 << 17);
        assert_eq!(WakeupSource::GpioPin(0).to_bit_mask(), 1 << 24);
        assert_eq!(WakeupSource::GpioPin(3).to_bit_mask(), 1 << 27);
        assert_eq!(WakeupSource::RtcAlarm.to_bit_mask(), 1 << 18);
        assert_eq!(WakeupSource::Watchdog.to_bit_mask(), 1 << 19);
    }

    #[test]
    fn test_power_manager_initial_state() {
        let pm = PowerManager::new();
        assert_eq!(pm.state(), PowerState::Active);
        assert_eq!(pm.wakeup_sources_mask(), 0);
    }

    #[test]
    fn test_enable_disable_wakeup() {
        let pm = PowerManager::new();
        
        // 启用唤醒源
        pm.enable_wakeup(WakeupSource::Timer);
        assert!(pm.is_wakeup_enabled(WakeupSource::Timer));
        assert!(!pm.is_wakeup_enabled(WakeupSource::UartRx));
        
        // 启用多个唤醒源
        pm.enable_wakeup(WakeupSource::UartRx);
        pm.enable_wakeup(WakeupSource::GpioPin(0));
        assert!(pm.is_wakeup_enabled(WakeupSource::Timer));
        assert!(pm.is_wakeup_enabled(WakeupSource::UartRx));
        assert!(pm.is_wakeup_enabled(WakeupSource::GpioPin(0)));
        
        // 禁用唤醒源
        pm.disable_wakeup(WakeupSource::Timer);
        assert!(!pm.is_wakeup_enabled(WakeupSource::Timer));
        assert!(pm.is_wakeup_enabled(WakeupSource::UartRx));
        
        // 清除所有唤醒源
        pm.clear_wakeup_sources();
        assert_eq!(pm.wakeup_sources_mask(), 0);
    }

    #[test]
    fn test_enter_idle() {
        let pm = PowerManager::new();
        
        // 进入空闲模式（在测试环境中会立即返回）
        pm.enter_idle();
        
        // 应该恢复到 Active 状态
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_enter_sleep_without_wakeup_source() {
        let pm = PowerManager::new();
        
        // 没有唤醒源时，sleep 应该退化为 idle
        pm.enter_sleep();
        
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_enter_sleep_with_wakeup_source() {
        let pm = PowerManager::new();
        
        pm.enable_wakeup(WakeupSource::Timer);
        pm.enter_sleep();
        
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_global_power_manager() {
        let pm1 = PowerManager::global();
        let pm2 = PowerManager::global();
        
        // 应该是同一个实例
        assert!(core::ptr::eq(pm1, pm2));
    }
}

