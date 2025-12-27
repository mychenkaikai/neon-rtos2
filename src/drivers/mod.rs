//! # 设备驱动框架
//!
//! 提供统一的设备驱动抽象，支持多种外设类型。
//!
//! ## 模块结构
//!
//! - [`traits`]: 设备驱动 trait 定义
//! - [`macros`]: 设备驱动宏
//!
//! ## 支持的设备类型
//!
//! | 设备类型 | Trait | 说明 |
//! |---------|-------|------|
//! | 基础设备 | `Device` | 所有设备的基础 trait |
//! | 可读设备 | `Read` | 支持读取数据 |
//! | 可写设��� | `Write` | 支持写入数据 |
//! | GPIO | `GpioPin`, `InputPin`, `OutputPin` | GPIO 引脚操作 |
//! | UART | `Uart` | 串行通信 |
//! | SPI | `Spi` | SPI 总线 |
//! | I2C | `I2c` | I2C 总线 |
//! | 定时器 | `TimerDevice` | 硬件定时器 |
//! | PWM | `PwmChannel` | PWM 输出 |
//! | ADC | `AdcChannel` | 模数转换 |
//!
//! ## 使用示例
//!
//! ### 实现自定义驱动
//!
//! ```rust,ignore
//! use neon_rtos2::drivers::{Device, Read, Write, DeviceError};
//!
//! struct MyUart {
//!     base_addr: usize,
//!     baudrate: u32,
//! }
//!
//! impl Device for MyUart {
//!     type Error = DeviceError;
//!
//!     fn init(&mut self) -> Result<(), Self::Error> {
//!         // 初始化 UART 硬件
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
//!         // 从 UART 读取数据
//!         Ok(buf.len())
//!     }
//! }
//!
//! impl Write for MyUart {
//!     fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
//!         // 向 UART 写入数据
//!         Ok(buf.len())
//!     }
//!
//!     fn flush(&mut self) -> Result<(), Self::Error> {
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ### 使用设备驱动宏
//!
//! ```rust,ignore
//! use neon_rtos2::device_driver;
//!
//! device_driver! {
//!     name: Uart0,
//!     base_addr: 0x4000_0000,
//!     registers: {
//!         data: u32 @ 0x00,
//!         status: u32 @ 0x04,
//!         control: u32 @ 0x08,
//!     }
//! }
//!
//! let uart = Uart0::new();
//! let status = uart.status();
//! uart.data_write(0x55);
//! ```
//!
//! ### 使用位域宏
//!
//! ```rust,ignore
//! use neon_rtos2::bitfield;
//!
//! bitfield! {
//!     /// UART 状态寄存器
//!     pub struct UartStatus(u32) {
//!         tx_empty: 0,
//!         rx_full: 1,
//!         tx_busy: 2,
//!     }
//! }
//!
//! let status = UartStatus::from_raw(uart.status());
//! if status.tx_empty() {
//!     // 可以发送数据
//! }
//! ```

pub mod traits;
pub mod macros;

// 重新导出常用类型
pub use traits::{
    // 基础 trait
    Device,
    Read,
    Write,
    ReadWrite,
    
    // GPIO
    GpioPin,
    InputPin,
    OutputPin,
    OutputTypePin,
    PinMode,
    PullMode,
    OutputType,
    
    // 串行通信
    Uart,
    SerialConfig,
    DataBits,
    StopBits,
    Parity,
    
    // SPI
    Spi,
    SpiConfig,
    SpiMode,
    
    // I2C
    I2c,
    I2cConfig,
    
    // 定时器
    TimerDevice,
    
    // PWM
    PwmChannel,
    
    // ADC
    AdcChannel,
    
    // 错误类型
    DeviceError,
};

