//! # Mock GPIO 驱动
//!
//! 模拟 GPIO 驱动，用于测试和演示。
//!
//! ## 功能特性
//!
//! - 实现 `Device`, `GpioPin`, `InputPin`, `OutputPin` trait
//! - 支持输入/输出模式切换
//! - 支持上拉/下拉配置
//! - 支持推挽/开漏输出
//! - 提供测试辅助方法
//!
//! ## 使用示例
//!
//! ```rust
//! use neon_rtos2::drivers::examples::MockGpio;
//! use neon_rtos2::drivers::{Device, GpioPin, InputPin, OutputPin, PinMode};
//!
//! // 创建 GPIO 引脚
//! let mut gpio = MockGpio::new(5);
//! gpio.init().unwrap();
//!
//! // 配置为输出模式
//! gpio.set_mode(PinMode::Output).unwrap();
//!
//! // 设置高电平
//! gpio.set_high().unwrap();
//!
//! // 切换电平
//! gpio.toggle().unwrap();
//! ```

use crate::drivers::{
    Device, GpioPin, InputPin, OutputPin, OutputTypePin,
    PinMode, PullMode, OutputType,
    DeviceError,
};

/// Mock GPIO 驱动
///
/// 模拟 GPIO 引脚，支持输入输出操作。
///
/// # 示例
///
/// ```rust
/// use neon_rtos2::drivers::examples::MockGpio;
/// use neon_rtos2::drivers::{Device, OutputPin, PinMode};
///
/// let mut led = MockGpio::new(13);
/// led.init().unwrap();
/// led.set_mode(PinMode::Output).unwrap();
/// led.set_high().unwrap();
/// ```
pub struct MockGpio {
    /// 引脚编号
    pin: u8,
    /// 引脚模式
    mode: PinMode,
    /// 上拉/下拉配置
    pull: PullMode,
    /// 输出类型
    output_type: OutputType,
    /// 当前电平状态（true = 高电平）
    state: bool,
    /// 输入电平（用于模拟外部输入）
    input_state: bool,
    /// 是否已初始化
    initialized: bool,
    /// 状态变化计数
    toggle_count: usize,
}

impl MockGpio {
    /// 创建新的 Mock GPIO 实例
    ///
    /// # 参数
    ///
    /// - `pin`: 引脚编号
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockGpio;
    ///
    /// let gpio = MockGpio::new(0);
    /// ```
    pub const fn new(pin: u8) -> Self {
        Self {
            pin,
            mode: PinMode::Input,
            pull: PullMode::None,
            output_type: OutputType::PushPull,
            state: false,
            input_state: false,
            initialized: false,
            toggle_count: 0,
        }
    }

    /// 模拟设置输入电平（测试用）
    ///
    /// 设置外部输入的电平状态，用于测试输入功能。
    ///
    /// # 参数
    ///
    /// - `high`: true 表示高电平，false 表示低电平
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockGpio;
    /// use neon_rtos2::drivers::{Device, InputPin, PinMode};
    ///
    /// let mut gpio = MockGpio::new(0);
    /// gpio.init().unwrap();
    /// gpio.set_mode(PinMode::Input).unwrap();
    ///
    /// // 模拟外部输入高电平
    /// gpio.mock_set_input(true);
    /// assert!(gpio.is_high().unwrap());
    /// ```
    pub fn mock_set_input(&mut self, high: bool) {
        self.input_state = high;
    }

    /// 获取当前输出状态（测试用）
    ///
    /// 返回当前输出的电平状态。
    pub fn mock_get_output(&self) -> bool {
        self.state
    }

    /// 获取状态切换次数
    ///
    /// 返回 `toggle()` 被调用的次数。
    pub fn toggle_count(&self) -> usize {
        self.toggle_count
    }

    /// 获取上拉/下拉配置
    pub fn pull(&self) -> PullMode {
        self.pull
    }

    /// 获取输出类型
    pub fn output_type(&self) -> OutputType {
        self.output_type
    }

    /// 检查是否为输入模式
    pub fn is_input(&self) -> bool {
        matches!(self.mode, PinMode::Input)
    }

    /// 检查是否为输出模式
    pub fn is_output(&self) -> bool {
        matches!(self.mode, PinMode::Output)
    }
}

impl Default for MockGpio {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Device for MockGpio {
    type Error = DeviceError;

    fn init(&mut self) -> Result<(), Self::Error> {
        self.mode = PinMode::Input;
        self.pull = PullMode::None;
        self.state = false;
        self.input_state = false;
        self.toggle_count = 0;
        self.initialized = true;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockGPIO"
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn reset(&mut self) -> Result<(), Self::Error> {
        self.initialized = false;
        self.init()
    }
}

impl GpioPin for MockGpio {
    type Error = DeviceError;

    fn pin_number(&self) -> u8 {
        self.pin
    }

    fn set_mode(&mut self, mode: PinMode) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.mode = mode;
        Ok(())
    }

    fn mode(&self) -> PinMode {
        self.mode
    }

    fn set_pull(&mut self, pull: PullMode) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.pull = pull;
        
        // 模拟上拉/下拉对输入状态的影响
        if matches!(self.mode, PinMode::Input) {
            match pull {
                PullMode::PullUp => self.input_state = true,
                PullMode::PullDown => self.input_state = false,
                PullMode::None => {}
            }
        }
        Ok(())
    }
}

impl InputPin for MockGpio {
    fn is_high(&self) -> Result<bool, Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        
        // 输入模式返回外部输入状态
        // 输出模式返回当前输出状态（用于读回）
        match self.mode {
            PinMode::Input => Ok(self.input_state),
            PinMode::Output => Ok(self.state),
            _ => Ok(self.input_state),
        }
    }
}

impl OutputPin for MockGpio {
    fn set_high(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        if !matches!(self.mode, PinMode::Output) {
            return Err(DeviceError::InvalidParameter);
        }
        self.state = true;
        Ok(())
    }

    fn set_low(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        if !matches!(self.mode, PinMode::Output) {
            return Err(DeviceError::InvalidParameter);
        }
        self.state = false;
        Ok(())
    }

    fn toggle(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        if !matches!(self.mode, PinMode::Output) {
            return Err(DeviceError::InvalidParameter);
        }
        self.state = !self.state;
        self.toggle_count += 1;
        Ok(())
    }
}

impl OutputTypePin for MockGpio {
    fn set_output_type(&mut self, output_type: OutputType) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.output_type = output_type;
        Ok(())
    }
}

// ============================================================================
// GPIO 组（多个引脚）
// ============================================================================

/// Mock GPIO 端口（8个引脚）
///
/// 模拟一个 8 位 GPIO 端口。
///
/// # 示例
///
/// ```rust
/// use neon_rtos2::drivers::examples::MockGpioPort;
/// use neon_rtos2::drivers::Device;
///
/// let mut port = MockGpioPort::new(0); // Port A
/// port.init().unwrap();
///
/// // 设置输出值
/// port.write(0x55);
///
/// // 读取输入值
/// let value = port.read();
/// ```
pub struct MockGpioPort {
    /// 端口编号
    port: u8,
    /// 8 个引脚
    pins: [MockGpio; 8],
    /// 是否已初始化
    initialized: bool,
}

impl MockGpioPort {
    /// 创建新的 GPIO 端口
    ///
    /// # 参数
    ///
    /// - `port`: 端口编号（0=A, 1=B, ...）
    pub fn new(port: u8) -> Self {
        let base_pin = port * 8;
        Self {
            port,
            pins: [
                MockGpio::new(base_pin),
                MockGpio::new(base_pin + 1),
                MockGpio::new(base_pin + 2),
                MockGpio::new(base_pin + 3),
                MockGpio::new(base_pin + 4),
                MockGpio::new(base_pin + 5),
                MockGpio::new(base_pin + 6),
                MockGpio::new(base_pin + 7),
            ],
            initialized: false,
        }
    }

    /// 获取端口编号
    pub fn port_number(&self) -> u8 {
        self.port
    }

    /// 获取指定引脚的可变引用
    pub fn pin_mut(&mut self, index: usize) -> Option<&mut MockGpio> {
        self.pins.get_mut(index)
    }

    /// 获取指定引脚的引用
    pub fn pin(&self, index: usize) -> Option<&MockGpio> {
        self.pins.get(index)
    }

    /// 写入端口值（所有引脚必须为输出模式）
    pub fn write(&mut self, value: u8) -> Result<(), DeviceError> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        for (i, pin) in self.pins.iter_mut().enumerate() {
            let bit = (value >> i) & 1;
            if bit == 1 {
                pin.set_high()?;
            } else {
                pin.set_low()?;
            }
        }
        Ok(())
    }

    /// 读取端口值
    pub fn read(&self) -> Result<u8, DeviceError> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        let mut value = 0u8;
        for (i, pin) in self.pins.iter().enumerate() {
            if pin.is_high().unwrap_or(false) {
                value |= 1 << i;
            }
        }
        Ok(value)
    }

    /// 设置所有引脚为输出模式
    pub fn set_all_output(&mut self) -> Result<(), DeviceError> {
        for pin in &mut self.pins {
            pin.set_mode(PinMode::Output)?;
        }
        Ok(())
    }

    /// 设置所有引脚为输入模式
    pub fn set_all_input(&mut self) -> Result<(), DeviceError> {
        for pin in &mut self.pins {
            pin.set_mode(PinMode::Input)?;
        }
        Ok(())
    }

    /// 模拟设置输入值（测试用）
    pub fn mock_set_input(&mut self, value: u8) {
        for (i, pin) in self.pins.iter_mut().enumerate() {
            let bit = (value >> i) & 1;
            pin.mock_set_input(bit == 1);
        }
    }
}

impl Device for MockGpioPort {
    type Error = DeviceError;

    fn init(&mut self) -> Result<(), Self::Error> {
        for pin in &mut self.pins {
            pin.init()?;
        }
        self.initialized = true;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockGPIOPort"
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_gpio_new() {
        let gpio = MockGpio::new(5);
        assert_eq!(gpio.pin_number(), 5);
        assert!(!gpio.is_ready());
    }

    #[test]
    fn test_mock_gpio_init() {
        let mut gpio = MockGpio::new(0);
        assert!(gpio.init().is_ok());
        assert!(gpio.is_ready());
        assert_eq!(gpio.name(), "MockGPIO");
        assert!(gpio.is_input());
    }

    #[test]
    fn test_mock_gpio_output() {
        let mut gpio = MockGpio::new(0);
        gpio.init().unwrap();
        gpio.set_mode(PinMode::Output).unwrap();

        assert!(gpio.is_output());

        gpio.set_high().unwrap();
        assert!(gpio.mock_get_output());

        gpio.set_low().unwrap();
        assert!(!gpio.mock_get_output());

        gpio.toggle().unwrap();
        assert!(gpio.mock_get_output());
        assert_eq!(gpio.toggle_count(), 1);
    }

    #[test]
    fn test_mock_gpio_input() {
        let mut gpio = MockGpio::new(0);
        gpio.init().unwrap();
        gpio.set_mode(PinMode::Input).unwrap();

        // 模拟外部输入
        gpio.mock_set_input(true);
        assert!(gpio.is_high().unwrap());

        gpio.mock_set_input(false);
        assert!(gpio.is_low().unwrap());
    }

    #[test]
    fn test_mock_gpio_pull() {
        let mut gpio = MockGpio::new(0);
        gpio.init().unwrap();
        gpio.set_mode(PinMode::Input).unwrap();

        gpio.set_pull(PullMode::PullUp).unwrap();
        assert_eq!(gpio.pull(), PullMode::PullUp);
        assert!(gpio.is_high().unwrap()); // 上拉默认高电平

        gpio.set_pull(PullMode::PullDown).unwrap();
        assert_eq!(gpio.pull(), PullMode::PullDown);
        assert!(gpio.is_low().unwrap()); // 下拉默认低电平
    }

    #[test]
    fn test_mock_gpio_output_type() {
        let mut gpio = MockGpio::new(0);
        gpio.init().unwrap();

        gpio.set_output_type(OutputType::OpenDrain).unwrap();
        assert_eq!(gpio.output_type(), OutputType::OpenDrain);
    }

    #[test]
    fn test_mock_gpio_not_initialized() {
        let mut gpio = MockGpio::new(0);

        assert!(matches!(
            gpio.set_mode(PinMode::Output),
            Err(DeviceError::NotInitialized)
        ));
    }

    #[test]
    fn test_mock_gpio_invalid_operation() {
        let mut gpio = MockGpio::new(0);
        gpio.init().unwrap();
        gpio.set_mode(PinMode::Input).unwrap();

        // 输入模式下不能设置输出
        assert!(matches!(gpio.set_high(), Err(DeviceError::InvalidParameter)));
    }

    #[test]
    fn test_mock_gpio_port() {
        let mut port = MockGpioPort::new(0);
        port.init().unwrap();
        port.set_all_output().unwrap();

        port.write(0xAA).unwrap();
        
        // 验证各个引脚状态
        assert!(!port.pin(0).unwrap().mock_get_output()); // bit 0 = 0
        assert!(port.pin(1).unwrap().mock_get_output());  // bit 1 = 1
        assert!(!port.pin(2).unwrap().mock_get_output()); // bit 2 = 0
        assert!(port.pin(3).unwrap().mock_get_output());  // bit 3 = 1
    }

    #[test]
    fn test_mock_gpio_port_read() {
        let mut port = MockGpioPort::new(0);
        port.init().unwrap();
        port.set_all_input().unwrap();

        port.mock_set_input(0x55);
        let value = port.read().unwrap();
        assert_eq!(value, 0x55);
    }
}

