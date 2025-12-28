//! # 示例驱动实现
//!
//! 提供基于 drivers traits 的 Mock 驱动实现，用于：
//! - 演示如何实现设备驱动
//! - 单元测试和集成测试
//! - 在没有真实硬件时进行开发
//!
//! ## 可用驱动
//!
//! | 驱动 | 说明 |
//! |------|------|
//! | [`MockUart`] | Mock UART 串口驱动 |
//! | [`MockGpio`] | Mock GPIO 驱动 |
//! | [`MockSpi`] | Mock SPI 总线驱动 |
//! | [`MockTimer`] | Mock 定时器驱动 |
//!
//! ## 使用示例
//!
//! ```rust
//! use neon_rtos2::drivers::examples::{MockUart, MockGpio};
//! use neon_rtos2::drivers::{Device, Read, Write, GpioPin, OutputPin};
//!
//! // 创建并使用 Mock UART
//! let mut uart = MockUart::new();
//! uart.init().unwrap();
//! uart.write(b"Hello").unwrap();
//!
//! // 创建并使用 Mock GPIO
//! let mut gpio = MockGpio::new(0);
//! gpio.init().unwrap();
//! gpio.set_high().unwrap();
//! ```

mod mock_uart;
mod mock_gpio;
mod mock_spi;
mod mock_timer;

pub use mock_uart::MockUart;
pub use mock_gpio::MockGpio;
pub use mock_spi::MockSpi;
pub use mock_timer::MockTimer;

