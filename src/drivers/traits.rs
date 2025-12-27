//! # 设备驱动 Trait 定义
//!
//! 提供统一的设备驱动抽象接口，支持多种外设类型。
//!
//! ## 设计理念
//!
//! 使用 Rust 的 trait 系统定义设备驱动接口，实现：
//! - **类型安全**: 编译期检查设备操作的正确性
//! - **零成本抽象**: trait 方法可以被内联优化
//! - **可组合性**: 通过 trait 组合实现复杂设备
//!
//! ## Trait 层次结构
//!
//! ```text
//! Device (基础设备)
//!    ├── Read (可读设备)
//!    ├── Write (可写设备)
//!    │      └── ReadWrite = Read + Write
//!    ├── GpioPin (GPIO 引脚)
//!    │      ├── InputPin
//!    │      └── OutputPin
//!    ├── Serial (串行通信)
//!    │      └── Uart
//!    ├── Spi (SPI 总线)
//!    └── I2c (I2C 总线)
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! # use neon_rtos2::drivers::{Device, Read, Write, GpioPin};
//!
//! #[derive(Debug)]
//! struct UartError;
//!
//! // 实现自定义 UART 驱动
//! struct MyUart {
//!     base_addr: usize,
//! }
//!
//! impl Device for MyUart {
//!     type Error = UartError;
//!     
//!     fn init(&mut self) -> Result<(), Self::Error> {
//!         // 初始化 UART
//!         Ok(())
//!     }
//!     
//!     fn name(&self) -> &'static str {
//!         "UART0"
//!     }
//! }
//!
//! impl Read for MyUart {
//!     fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
//!         // 读取数据
//!         Ok(buf.len())
//!     }
//! }
//! ```

use crate::error::RtosError;

// ============================================================================
// 基础设备 Trait
// ============================================================================

/// 基础设备 trait
///
/// 所有设备驱动都必须实现此 trait，提供基本的设备管理功能。
///
/// # 关联类型
///
/// - `Error`: 设备特定的错误类型
///
/// # 示例
///
/// ```rust,no_run
/// # use neon_rtos2::drivers::Device;
/// # struct MyDevice;
/// # #[derive(Debug)]
/// # struct MyDeviceError;
/// impl Device for MyDevice {
///     type Error = MyDeviceError;
///     
///     fn init(&mut self) -> Result<(), Self::Error> {
///         // 初始化设备
///         Ok(())
///     }
///     
///     fn name(&self) -> &'static str {
///         "MyDevice"
///     }
/// }
/// ```
pub trait Device {
    /// 设备错误类型
    type Error;

    /// 初始化设备
    ///
    /// 在使用设备之前必须调用此方法进行初始化。
    ///
    /// # 返回值
    ///
    /// 成功返回 `Ok(())`，失败返回设备特定的错误
    fn init(&mut self) -> Result<(), Self::Error>;

    /// 获取设备名称
    ///
    /// 返回设备的静态名称字符串，用于调试和日志。
    fn name(&self) -> &'static str;

    /// 检查设备是否就绪
    ///
    /// 默认实现返回 `true`，子类可以覆盖此方法。
    fn is_ready(&self) -> bool {
        true
    }

    /// 重置设备
    ///
    /// 将设备恢复到初始状态。默认实现调用 `init()`。
    fn reset(&mut self) -> Result<(), Self::Error> {
        self.init()
    }
}

// ============================================================================
// 读写 Trait
// ============================================================================

/// 可读设备 trait
///
/// 实现此 trait 的设备支持读取数据。
pub trait Read: Device {
    /// 读取数据到缓冲区
    ///
    /// # 参数
    ///
    /// - `buf`: 目标缓冲区
    ///
    /// # 返回值
    ///
    /// 成功返回实际读取的字节数，失败返回错误
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;

    /// 读取单个字节
    ///
    /// 默认实现使用 `read()` 方法
    fn read_byte(&mut self) -> Result<u8, Self::Error> {
        let mut buf = [0u8; 1];
        self.read(&mut buf)?;
        Ok(buf[0])
    }

    /// 读取直到缓冲区满
    ///
    /// 阻塞直到读取了 `buf.len()` 个字节
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        let mut offset = 0;
        while offset < buf.len() {
            let n = self.read(&mut buf[offset..])?;
            offset += n;
        }
        Ok(())
    }
}

/// 可写设备 trait
///
/// 实现此 trait 的设备支持写入数据。
pub trait Write: Device {
    /// 写入数据
    ///
    /// # 参数
    ///
    /// - `buf`: 要写入的数据
    ///
    /// # 返回值
    ///
    /// 成功返回实际写入的字节数，失败返回错误
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error>;

    /// 刷新缓冲区
    ///
    /// 确保所有缓冲的数据都已发送到设备
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// 写入单个字节
    ///
    /// 默认实现使用 `write()` 方法
    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.write(&[byte])?;
        Ok(())
    }

    /// 写入所有数据
    ///
    /// 阻塞直到所有数据都已写入
    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        let mut offset = 0;
        while offset < buf.len() {
            let n = self.write(&buf[offset..])?;
            offset += n;
        }
        self.flush()
    }
}

/// 可读写设备 trait
///
/// 组合了 `Read` 和 `Write` trait
pub trait ReadWrite: Read + Write {}

// 自动为同时实现 Read 和 Write 的类型实现 ReadWrite
impl<T: Read + Write> ReadWrite for T {}

// ============================================================================
// GPIO Trait
// ============================================================================

/// GPIO 引脚模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinMode {
    /// 输入模式
    Input,
    /// 输出模式
    Output,
    /// 复用功能模式
    Alternate(u8),
    /// 模拟模式
    Analog,
}

/// GPIO 上拉/下拉配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    /// 无上拉/下拉
    None,
    /// 上拉
    PullUp,
    /// 下拉
    PullDown,
}

/// GPIO 输出类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputType {
    /// 推挽输出
    PushPull,
    /// 开漏输出
    OpenDrain,
}

/// GPIO 引脚 trait
///
/// 提供 GPIO 引脚的基本操作
pub trait GpioPin {
    /// 错误类型
    type Error;

    /// 获取引脚编号
    fn pin_number(&self) -> u8;

    /// 设置引脚模式
    fn set_mode(&mut self, mode: PinMode) -> Result<(), Self::Error>;

    /// 获取当前模式
    fn mode(&self) -> PinMode;

    /// 设置上拉/下拉
    fn set_pull(&mut self, pull: PullMode) -> Result<(), Self::Error>;
}

/// 输入引脚 trait
pub trait InputPin: GpioPin {
    /// 读取引脚电平
    ///
    /// 返回 `true` 表示高电平，`false` 表示低电平
    fn is_high(&self) -> Result<bool, Self::Error>;

    /// 读取引脚电平
    ///
    /// 返回 `true` 表示低电平，`false` 表示高电平
    fn is_low(&self) -> Result<bool, Self::Error> {
        Ok(!self.is_high()?)
    }
}

/// 输出引脚 trait
pub trait OutputPin: GpioPin {
    /// 设置高电平
    fn set_high(&mut self) -> Result<(), Self::Error>;

    /// 设置低电平
    fn set_low(&mut self) -> Result<(), Self::Error>;

    /// 切换电平
    fn toggle(&mut self) -> Result<(), Self::Error>;

    /// 设置电平
    ///
    /// `high` 为 `true` 时设置高电平，否则设置低电平
    fn set_state(&mut self, high: bool) -> Result<(), Self::Error> {
        if high {
            self.set_high()
        } else {
            self.set_low()
        }
    }
}

/// 可配置输出类型的引脚
pub trait OutputTypePin: OutputPin {
    /// 设置输出类型
    fn set_output_type(&mut self, output_type: OutputType) -> Result<(), Self::Error>;
}

// ============================================================================
// 串行通信 Trait
// ============================================================================

/// 串行通信配置
#[derive(Debug, Clone, Copy)]
pub struct SerialConfig {
    /// 波特率
    pub baudrate: u32,
    /// 数据位
    pub data_bits: DataBits,
    /// 停止位
    pub stop_bits: StopBits,
    /// 校验位
    pub parity: Parity,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baudrate: 115200,
            data_bits: DataBits::Eight,
            stop_bits: StopBits::One,
            parity: Parity::None,
        }
    }
}

/// 数据位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
}

/// 停止位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    OnePointFive,
    Two,
}

/// 校验位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Even,
    Odd,
}

/// UART 设备 trait
pub trait Uart: Read + Write {
    /// 配置 UART
    fn configure(&mut self, config: SerialConfig) -> Result<(), Self::Error>;

    /// 设置波特率
    fn set_baudrate(&mut self, baudrate: u32) -> Result<(), Self::Error>;

    /// 获取当前波特率
    fn baudrate(&self) -> u32;

    /// 检查是否有数据可读
    fn is_rx_ready(&self) -> bool;

    /// 检查是否可以发送数据
    fn is_tx_ready(&self) -> bool;
}

// ============================================================================
// SPI Trait
// ============================================================================

/// SPI 模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiMode {
    /// CPOL=0, CPHA=0
    Mode0,
    /// CPOL=0, CPHA=1
    Mode1,
    /// CPOL=1, CPHA=0
    Mode2,
    /// CPOL=1, CPHA=1
    Mode3,
}

/// SPI 配置
#[derive(Debug, Clone, Copy)]
pub struct SpiConfig {
    /// 时钟频率 (Hz)
    pub frequency: u32,
    /// SPI 模式
    pub mode: SpiMode,
    /// 位顺序（true = MSB first）
    pub msb_first: bool,
}

impl Default for SpiConfig {
    fn default() -> Self {
        Self {
            frequency: 1_000_000,
            mode: SpiMode::Mode0,
            msb_first: true,
        }
    }
}

/// SPI 设备 trait
pub trait Spi: Device {
    /// 配置 SPI
    fn configure(&mut self, config: SpiConfig) -> Result<(), Self::Error>;

    /// 传输数据（同时读写）
    ///
    /// # 参数
    ///
    /// - `read`: 读取缓冲区
    /// - `write`: 写入数据
    ///
    /// 两个缓冲区长度必须相同
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error>;

    /// 只写数据
    fn write(&mut self, data: &[u8]) -> Result<(), Self::Error>;

    /// 只读数据
    fn read(&mut self, data: &mut [u8]) -> Result<(), Self::Error>;

    /// 传输单个字节
    fn transfer_byte(&mut self, byte: u8) -> Result<u8, Self::Error> {
        let mut read = [0u8; 1];
        self.transfer(&mut read, &[byte])?;
        Ok(read[0])
    }
}

// ============================================================================
// I2C Trait
// ============================================================================

/// I2C 配置
#[derive(Debug, Clone, Copy)]
pub struct I2cConfig {
    /// 时钟频率 (Hz)
    pub frequency: u32,
}

impl Default for I2cConfig {
    fn default() -> Self {
        Self {
            frequency: 100_000, // 标准模式 100kHz
        }
    }
}

/// I2C 设备 trait
pub trait I2c: Device {
    /// 配置 I2C
    fn configure(&mut self, config: I2cConfig) -> Result<(), Self::Error>;

    /// 写入数据到指定地址
    ///
    /// # 参数
    ///
    /// - `addr`: 7位设备地址
    /// - `data`: 要写入的数据
    fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), Self::Error>;

    /// 从指定地址读取数据
    ///
    /// # 参数
    ///
    /// - `addr`: 7位设备地址
    /// - `data`: 读取缓冲区
    fn read(&mut self, addr: u8, data: &mut [u8]) -> Result<(), Self::Error>;

    /// 写入后读取
    ///
    /// 先写入数据，然后读取响应（不发送停止位）
    ///
    /// # 参数
    ///
    /// - `addr`: 7位设备地址
    /// - `write`: 要写入的数据
    /// - `read`: 读取缓冲区
    fn write_read(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error>;
}

// ============================================================================
// 定时器 Trait
// ============================================================================

/// 定时器设备 trait
pub trait TimerDevice: Device {
    /// 启动定时器
    fn start(&mut self) -> Result<(), Self::Error>;

    /// 停止定时器
    fn stop(&mut self) -> Result<(), Self::Error>;

    /// 设置周期（微秒）
    fn set_period_us(&mut self, period: u32) -> Result<(), Self::Error>;

    /// 设置周期（毫秒）
    fn set_period_ms(&mut self, period: u32) -> Result<(), Self::Error> {
        self.set_period_us(period * 1000)
    }

    /// 获取当前计数值
    fn count(&self) -> u32;

    /// 检查定时器是否运行中
    fn is_running(&self) -> bool;

    /// 清除计数器
    fn clear(&mut self) -> Result<(), Self::Error>;
}

// ============================================================================
// PWM Trait
// ============================================================================

/// PWM 通道 trait
pub trait PwmChannel {
    /// 错误类型
    type Error;

    /// 设置占空比（0-100%）
    fn set_duty(&mut self, duty: u8) -> Result<(), Self::Error>;

    /// 获取当前占空比
    fn duty(&self) -> u8;

    /// 设置频率 (Hz)
    fn set_frequency(&mut self, freq: u32) -> Result<(), Self::Error>;

    /// 获取当前频率
    fn frequency(&self) -> u32;

    /// 启用 PWM 输出
    fn enable(&mut self) -> Result<(), Self::Error>;

    /// 禁用 PWM 输出
    fn disable(&mut self) -> Result<(), Self::Error>;
}

// ============================================================================
// ADC Trait
// ============================================================================

/// ADC 通道 trait
pub trait AdcChannel {
    /// 错误类型
    type Error;

    /// 读取原始 ADC 值
    fn read_raw(&mut self) -> Result<u16, Self::Error>;

    /// 读取电压值（毫伏）
    fn read_mv(&mut self) -> Result<u32, Self::Error>;

    /// 获取 ADC 分辨率（位数）
    fn resolution(&self) -> u8;

    /// 获取参考电压（毫伏）
    fn reference_mv(&self) -> u32;
}

// ============================================================================
// 设备错误类型
// ============================================================================

/// 通用设备错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    /// 设备未初始化
    NotInitialized,
    /// 设备忙
    Busy,
    /// 超时
    Timeout,
    /// 无效参数
    InvalidParameter,
    /// 通信错误
    CommunicationError,
    /// 设备不存在
    NotFound,
    /// 缓冲区溢出
    BufferOverflow,
    /// 其他错误
    Other,
}

impl From<DeviceError> for RtosError {
    fn from(_: DeviceError) -> Self {
        RtosError::InvalidHandle
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // 模拟设备用于测试
    struct MockDevice {
        initialized: bool,
        data: [u8; 16],
        pos: usize,
    }

    impl MockDevice {
        fn new() -> Self {
            Self {
                initialized: false,
                data: [0; 16],
                pos: 0,
            }
        }
    }

    impl Device for MockDevice {
        type Error = DeviceError;

        fn init(&mut self) -> Result<(), Self::Error> {
            self.initialized = true;
            Ok(())
        }

        fn name(&self) -> &'static str {
            "MockDevice"
        }

        fn is_ready(&self) -> bool {
            self.initialized
        }
    }

    impl Read for MockDevice {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            if !self.initialized {
                return Err(DeviceError::NotInitialized);
            }
            let len = buf.len().min(self.data.len() - self.pos);
            buf[..len].copy_from_slice(&self.data[self.pos..self.pos + len]);
            self.pos += len;
            Ok(len)
        }
    }

    impl Write for MockDevice {
        fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            if !self.initialized {
                return Err(DeviceError::NotInitialized);
            }
            let len = buf.len().min(self.data.len() - self.pos);
            self.data[self.pos..self.pos + len].copy_from_slice(&buf[..len]);
            self.pos += len;
            Ok(len)
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn test_device_init() {
        let mut device = MockDevice::new();
        assert!(!device.is_ready());
        
        device.init().unwrap();
        assert!(device.is_ready());
        assert_eq!(device.name(), "MockDevice");
    }

    #[test]
    fn test_device_read_write() {
        let mut device = MockDevice::new();
        device.init().unwrap();

        // 写入数据
        let write_data = [1, 2, 3, 4];
        let written = device.write(&write_data).unwrap();
        assert_eq!(written, 4);

        // 重置位置
        device.pos = 0;

        // 读取数据
        let mut read_data = [0u8; 4];
        let read = device.read(&mut read_data).unwrap();
        assert_eq!(read, 4);
        assert_eq!(read_data, write_data);
    }

    #[test]
    fn test_serial_config_default() {
        let config = SerialConfig::default();
        assert_eq!(config.baudrate, 115200);
        assert_eq!(config.data_bits, DataBits::Eight);
        assert_eq!(config.stop_bits, StopBits::One);
        assert_eq!(config.parity, Parity::None);
    }

    #[test]
    fn test_spi_config_default() {
        let config = SpiConfig::default();
        assert_eq!(config.frequency, 1_000_000);
        assert_eq!(config.mode, SpiMode::Mode0);
        assert!(config.msb_first);
    }

    #[test]
    fn test_i2c_config_default() {
        let config = I2cConfig::default();
        assert_eq!(config.frequency, 100_000);
    }
}

