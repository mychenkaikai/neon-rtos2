//! # Mock Timer 驱动
//!
//! 模拟定时器驱动，用于测试和演示。
//!
//! ## 功能特性
//!
//! - 实现 `Device`, `TimerDevice` trait
//! - 支持启动/停止/清除
//! - 支持设置周期
//! - 模拟计数器递增
//! - 提供测试辅助方法
//!
//! ## 使用示例
//!
//! ```rust
//! use neon_rtos2::drivers::examples::MockTimer;
//! use neon_rtos2::drivers::{Device, TimerDevice};
//!
//! let mut timer = MockTimer::new();
//! timer.init().unwrap();
//!
//! // 设置周期为 1000 微秒
//! timer.set_period_us(1000).unwrap();
//!
//! // 启动定时器
//! timer.start().unwrap();
//!
//! // 模拟时间流逝
//! timer.mock_tick(500);
//! assert_eq!(timer.count(), 500);
//! ```

use crate::drivers::{Device, TimerDevice, DeviceError};

/// Mock Timer 驱动
///
/// 模拟硬件定时器，支持基本的定时器操作。
///
/// # 工作原理
///
/// - 使用内部计数器模拟定时器计数
/// - 通过 `mock_tick()` 方法模拟时间流逝
/// - 支持周期设置和溢出检测
///
/// # 示例
///
/// ```rust
/// use neon_rtos2::drivers::examples::MockTimer;
/// use neon_rtos2::drivers::{Device, TimerDevice};
///
/// let mut timer = MockTimer::new();
/// timer.init().unwrap();
/// timer.set_period_us(1000).unwrap();
/// timer.start().unwrap();
///
/// // 模拟 1500 微秒后
/// timer.mock_tick(1500);
/// assert!(timer.has_overflowed());
/// ```
pub struct MockTimer {
    /// 周期（微秒）
    period_us: u32,
    /// 当前计数值
    counter: u32,
    /// 是否正在运行
    running: bool,
    /// 是否已初始化
    initialized: bool,
    /// 溢出次数
    overflow_count: usize,
    /// 是否发生溢出
    overflowed: bool,
    /// 回调函数（可选）
    callback: Option<fn()>,
}

impl MockTimer {
    /// 创建新的 Mock Timer 实例
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockTimer;
    ///
    /// let timer = MockTimer::new();
    /// ```
    pub const fn new() -> Self {
        Self {
            period_us: 1000,
            counter: 0,
            running: false,
            initialized: false,
            overflow_count: 0,
            overflowed: false,
            callback: None,
        }
    }

    /// 模拟时间流逝（测试用）
    ///
    /// 增加计数器值，模拟定时器计数。
    ///
    /// # 参数
    ///
    /// - `ticks`: 要增加的计数值
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockTimer;
    /// use neon_rtos2::drivers::{Device, TimerDevice};
    ///
    /// let mut timer = MockTimer::new();
    /// timer.init().unwrap();
    /// timer.set_period_us(1000).unwrap();
    /// timer.start().unwrap();
    ///
    /// timer.mock_tick(500);
    /// assert_eq!(timer.count(), 500);
    ///
    /// timer.mock_tick(600); // 总共 1100，超过周期 1000
    /// assert!(timer.has_overflowed());
    /// ```
    pub fn mock_tick(&mut self, ticks: u32) {
        if !self.running {
            return;
        }

        self.counter = self.counter.wrapping_add(ticks);

        // 检查溢出
        while self.counter >= self.period_us {
            self.counter -= self.period_us;
            self.overflow_count += 1;
            self.overflowed = true;

            // 调用回调
            if let Some(cb) = self.callback {
                cb();
            }
        }
    }

    /// 检查是否发生溢出
    ///
    /// 返回自上次清除以来是否发生过溢出。
    pub fn has_overflowed(&self) -> bool {
        self.overflowed
    }

    /// 清除溢出标志
    pub fn clear_overflow(&mut self) {
        self.overflowed = false;
    }

    /// 获取溢出次数
    pub fn overflow_count(&self) -> usize {
        self.overflow_count
    }

    /// 设置回调函数
    ///
    /// 当定时器溢出时调用此回调。
    ///
    /// # 参数
    ///
    /// - `callback`: 回调函数
    pub fn set_callback(&mut self, callback: fn()) {
        self.callback = Some(callback);
    }

    /// 清除回调函数
    pub fn clear_callback(&mut self) {
        self.callback = None;
    }

    /// 获取当前周期（微秒）
    pub fn period_us(&self) -> u32 {
        self.period_us
    }

    /// 获取剩余时间（微秒）
    pub fn remaining_us(&self) -> u32 {
        if self.counter >= self.period_us {
            0
        } else {
            self.period_us - self.counter
        }
    }
}

impl Default for MockTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for MockTimer {
    type Error = DeviceError;

    fn init(&mut self) -> Result<(), Self::Error> {
        self.counter = 0;
        self.running = false;
        self.overflow_count = 0;
        self.overflowed = false;
        self.initialized = true;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockTimer"
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn reset(&mut self) -> Result<(), Self::Error> {
        self.callback = None;
        self.period_us = 1000;
        self.initialized = false;
        self.init()
    }
}

impl TimerDevice for MockTimer {
    fn start(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.running = false;
        Ok(())
    }

    fn set_period_us(&mut self, period: u32) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        if period == 0 {
            return Err(DeviceError::InvalidParameter);
        }
        self.period_us = period;
        Ok(())
    }

    fn count(&self) -> u32 {
        self.counter
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.counter = 0;
        self.overflowed = false;
        Ok(())
    }
}

// ============================================================================
// 周期性定时器包装器
// ============================================================================

/// 周期性定时器
///
/// 基于 MockTimer 的周期性定时器包装器，提供更高级的功能。
///
/// # 示例
///
/// ```rust
/// use neon_rtos2::drivers::examples::PeriodicTimer;
/// use neon_rtos2::drivers::Device;
///
/// let mut timer = PeriodicTimer::new(1000); // 1ms 周期
/// timer.init().unwrap();
/// timer.start().unwrap();
///
/// // 模拟时间流逝
/// for _ in 0..5 {
///     timer.mock_tick(1000);
/// }
/// assert_eq!(timer.elapsed_periods(), 5);
/// ```
pub struct PeriodicTimer {
    /// 内部定时器
    inner: MockTimer,
    /// 已经过的周期数
    elapsed_periods: usize,
    /// 是否自动重载
    auto_reload: bool,
}

impl PeriodicTimer {
    /// 创建新的周期性定时器
    ///
    /// # 参数
    ///
    /// - `period_us`: 周期（微秒）
    pub fn new(period_us: u32) -> Self {
        let mut inner = MockTimer::new();
        inner.period_us = period_us;
        Self {
            inner,
            elapsed_periods: 0,
            auto_reload: true,
        }
    }

    /// 设置是否自动重载
    pub fn set_auto_reload(&mut self, enable: bool) {
        self.auto_reload = enable;
    }

    /// 获取已经过的周期数
    pub fn elapsed_periods(&self) -> usize {
        self.elapsed_periods
    }

    /// 模拟时间流逝
    pub fn mock_tick(&mut self, ticks: u32) {
        let before = self.inner.overflow_count();
        self.inner.mock_tick(ticks);
        let after = self.inner.overflow_count();
        self.elapsed_periods += after - before;

        if !self.auto_reload && self.inner.has_overflowed() {
            self.inner.stop().ok();
        }
    }

    /// 启动定时器
    pub fn start(&mut self) -> Result<(), DeviceError> {
        self.inner.start()
    }

    /// 停止定时器
    pub fn stop(&mut self) -> Result<(), DeviceError> {
        self.inner.stop()
    }

    /// 重置定时器
    pub fn reset_count(&mut self) {
        self.elapsed_periods = 0;
        self.inner.clear().ok();
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        self.inner.is_running()
    }
}

impl Device for PeriodicTimer {
    type Error = DeviceError;

    fn init(&mut self) -> Result<(), Self::Error> {
        self.elapsed_periods = 0;
        self.inner.init()
    }

    fn name(&self) -> &'static str {
        "PeriodicTimer"
    }

    fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_timer_new() {
        let timer = MockTimer::new();
        assert!(!timer.is_ready());
        assert!(!timer.is_running());
        assert_eq!(timer.period_us(), 1000);
    }

    #[test]
    fn test_mock_timer_init() {
        let mut timer = MockTimer::new();
        assert!(timer.init().is_ok());
        assert!(timer.is_ready());
        assert_eq!(timer.name(), "MockTimer");
    }

    #[test]
    fn test_mock_timer_start_stop() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();

        assert!(!timer.is_running());

        timer.start().unwrap();
        assert!(timer.is_running());

        timer.stop().unwrap();
        assert!(!timer.is_running());
    }

    #[test]
    fn test_mock_timer_count() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();
        timer.set_period_us(1000).unwrap();
        timer.start().unwrap();

        assert_eq!(timer.count(), 0);

        timer.mock_tick(100);
        assert_eq!(timer.count(), 100);

        timer.mock_tick(200);
        assert_eq!(timer.count(), 300);
    }

    #[test]
    fn test_mock_timer_overflow() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();
        timer.set_period_us(1000).unwrap();
        timer.start().unwrap();

        timer.mock_tick(1500);
        assert!(timer.has_overflowed());
        assert_eq!(timer.overflow_count(), 1);
        assert_eq!(timer.count(), 500); // 1500 - 1000 = 500
    }

    #[test]
    fn test_mock_timer_multiple_overflow() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();
        timer.set_period_us(100).unwrap();
        timer.start().unwrap();

        timer.mock_tick(350);
        assert_eq!(timer.overflow_count(), 3);
        assert_eq!(timer.count(), 50); // 350 - 300 = 50
    }

    #[test]
    fn test_mock_timer_clear() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();
        timer.set_period_us(1000).unwrap();
        timer.start().unwrap();

        timer.mock_tick(500);
        assert_eq!(timer.count(), 500);

        timer.clear().unwrap();
        assert_eq!(timer.count(), 0);
    }

    #[test]
    fn test_mock_timer_remaining() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();
        timer.set_period_us(1000).unwrap();
        timer.start().unwrap();

        assert_eq!(timer.remaining_us(), 1000);

        timer.mock_tick(300);
        assert_eq!(timer.remaining_us(), 700);
    }

    #[test]
    fn test_mock_timer_not_running() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();
        timer.set_period_us(1000).unwrap();
        // 不启动定时器

        timer.mock_tick(500);
        assert_eq!(timer.count(), 0); // 未运行时不计数
    }

    #[test]
    fn test_mock_timer_not_initialized() {
        let mut timer = MockTimer::new();

        assert!(matches!(timer.start(), Err(DeviceError::NotInitialized)));
        assert!(matches!(timer.stop(), Err(DeviceError::NotInitialized)));
    }

    #[test]
    fn test_mock_timer_invalid_period() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();

        assert!(matches!(
            timer.set_period_us(0),
            Err(DeviceError::InvalidParameter)
        ));
    }

    #[test]
    fn test_mock_timer_set_period_ms() {
        let mut timer = MockTimer::new();
        timer.init().unwrap();

        timer.set_period_ms(5).unwrap();
        assert_eq!(timer.period_us(), 5000);
    }

    #[test]
    fn test_periodic_timer() {
        let mut timer = PeriodicTimer::new(1000);
        timer.init().unwrap();
        timer.start().unwrap();

        for _ in 0..5 {
            timer.mock_tick(1000);
        }

        assert_eq!(timer.elapsed_periods(), 5);
    }

    #[test]
    fn test_periodic_timer_no_auto_reload() {
        let mut timer = PeriodicTimer::new(1000);
        timer.init().unwrap();
        timer.set_auto_reload(false);
        timer.start().unwrap();

        timer.mock_tick(1500);
        assert!(!timer.is_running()); // 溢出后自动停止
        assert_eq!(timer.elapsed_periods(), 1);
    }

    static mut CALLBACK_COUNT: usize = 0;

    fn test_callback() {
        unsafe {
            CALLBACK_COUNT += 1;
        }
    }

    #[test]
    fn test_mock_timer_callback() {
        unsafe {
            CALLBACK_COUNT = 0;
        }

        let mut timer = MockTimer::new();
        timer.init().unwrap();
        timer.set_period_us(100).unwrap();
        timer.set_callback(test_callback);
        timer.start().unwrap();

        timer.mock_tick(350);

        unsafe {
            assert_eq!(*core::ptr::addr_of!(CALLBACK_COUNT), 3);
        }
    }
}

