//! # Mock UART 驱动
//!
//! 模拟 UART 串口驱动，用于测试和演示。
//!
//! ## 功能特性
//!
//! - 实现 `Device`, `Read`, `Write`, `Uart` trait
//! - 内部环形缓冲区模拟收发
//! - 支持配置波特率、数据位、停止位、校验位
//! - 提供测试辅助方法
//!
//! ## 使用示例
//!
//! ```rust
//! use neon_rtos2::drivers::examples::MockUart;
//! use neon_rtos2::drivers::{Device, Read, Write, Uart, SerialConfig};
//!
//! let mut uart = MockUart::new();
//!
//! // 初始化
//! uart.init().unwrap();
//!
//! // 配置
//! uart.set_baudrate(9600).unwrap();
//!
//! // 写入数据
//! uart.write(b"Hello, World!").unwrap();
//!
//! // 模拟接收数据（测试用）
//! uart.mock_receive(b"Response");
//!
//! // 读取数据
//! let mut buf = [0u8; 8];
//! let n = uart.read(&mut buf).unwrap();
//! ```

use crate::drivers::{
    Device, Read, Write, Uart,
    SerialConfig, DataBits, StopBits, Parity,
    DeviceError,
};

/// Mock UART 缓冲区大小
const BUFFER_SIZE: usize = 256;

/// Mock UART 驱动
///
/// 模拟 UART 串口设备，使用环形缓冲区存储收发数据。
///
/// # 示例
///
/// ```rust
/// use neon_rtos2::drivers::examples::MockUart;
/// use neon_rtos2::drivers::{Device, Write};
///
/// let mut uart = MockUart::new();
/// uart.init().unwrap();
/// uart.write(b"Test").unwrap();
///
/// // 获取发送的数据
/// let sent = uart.mock_get_transmitted();
/// assert_eq!(&sent, b"Test");
/// ```
pub struct MockUart {
    /// 串口配置
    config: SerialConfig,
    /// 发送缓冲区
    tx_buffer: [u8; BUFFER_SIZE],
    /// 接收缓冲区
    rx_buffer: [u8; BUFFER_SIZE],
    /// 发送缓冲区头指针
    tx_head: usize,
    /// 发送缓冲区尾指针
    tx_tail: usize,
    /// 接收缓冲区头指针
    rx_head: usize,
    /// 接收缓冲区尾指针
    rx_tail: usize,
    /// 是否已初始化
    initialized: bool,
    /// 发送字节计数
    tx_count: usize,
    /// 接收字节计数
    rx_count: usize,
}

impl MockUart {
    /// 创建新的 Mock UART 实例
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockUart;
    ///
    /// let uart = MockUart::new();
    /// ```
    pub const fn new() -> Self {
        Self {
            config: SerialConfig {
                baudrate: 115200,
                data_bits: DataBits::Eight,
                stop_bits: StopBits::One,
                parity: Parity::None,
            },
            tx_buffer: [0; BUFFER_SIZE],
            rx_buffer: [0; BUFFER_SIZE],
            tx_head: 0,
            tx_tail: 0,
            rx_head: 0,
            rx_tail: 0,
            initialized: false,
            tx_count: 0,
            rx_count: 0,
        }
    }

    /// 模拟接收数据（测试用）
    ///
    /// 将数据放入接收缓冲区，模拟从外部接收到数据。
    ///
    /// # 参数
    ///
    /// - `data`: 要放入接收缓冲区的数据
    ///
    /// # 返回值
    ///
    /// 返回实际放入缓冲区的字节数
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockUart;
    /// use neon_rtos2::drivers::{Device, Read};
    ///
    /// let mut uart = MockUart::new();
    /// uart.init().unwrap();
    ///
    /// // 模拟接收数据
    /// uart.mock_receive(b"Hello");
    ///
    /// // 读取接收到的数据
    /// let mut buf = [0u8; 5];
    /// uart.read(&mut buf).unwrap();
    /// assert_eq!(&buf, b"Hello");
    /// ```
    pub fn mock_receive(&mut self, data: &[u8]) -> usize {
        let mut count = 0;
        for &byte in data {
            let next = (self.rx_head + 1) % BUFFER_SIZE;
            if next != self.rx_tail {
                self.rx_buffer[self.rx_head] = byte;
                self.rx_head = next;
                count += 1;
            } else {
                break; // 缓冲区满
            }
        }
        count
    }

    /// 获取已发送的数据（测试用）
    ///
    /// 从发送缓冲区取出所有数据，用于验证发送内容。
    ///
    /// # 返回值
    ///
    /// 返回发送缓冲区中的所有数据
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockUart;
    /// use neon_rtos2::drivers::{Device, Write};
    ///
    /// let mut uart = MockUart::new();
    /// uart.init().unwrap();
    ///
    /// uart.write(b"Test").unwrap();
    ///
    /// let sent = uart.mock_get_transmitted();
    /// assert_eq!(&sent, b"Test");
    /// ```
    #[cfg(feature = "alloc")]
    pub fn mock_get_transmitted(&mut self) -> alloc::vec::Vec<u8> {
        let mut data = alloc::vec::Vec::new();
        while self.tx_tail != self.tx_head {
            data.push(self.tx_buffer[self.tx_tail]);
            self.tx_tail = (self.tx_tail + 1) % BUFFER_SIZE;
        }
        data
    }

    /// 获取已发送的数据到固定缓冲区（测试用，no_std 兼容）
    ///
    /// # 参数
    ///
    /// - `buf`: 目标缓冲区
    ///
    /// # 返回值
    ///
    /// 返回实际复制的字节数
    pub fn mock_get_transmitted_to(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        while self.tx_tail != self.tx_head && count < buf.len() {
            buf[count] = self.tx_buffer[self.tx_tail];
            self.tx_tail = (self.tx_tail + 1) % BUFFER_SIZE;
            count += 1;
        }
        count
    }

    /// 清空所有缓冲区
    pub fn clear_buffers(&mut self) {
        self.tx_head = 0;
        self.tx_tail = 0;
        self.rx_head = 0;
        self.rx_tail = 0;
    }

    /// 获取发送字节总数
    pub fn tx_count(&self) -> usize {
        self.tx_count
    }

    /// 获取接收字节总数
    pub fn rx_count(&self) -> usize {
        self.rx_count
    }

    /// 获取发送缓冲区中的数据量
    pub fn tx_pending(&self) -> usize {
        if self.tx_head >= self.tx_tail {
            self.tx_head - self.tx_tail
        } else {
            BUFFER_SIZE - self.tx_tail + self.tx_head
        }
    }

    /// 获取接收缓冲区中的数据量
    pub fn rx_available(&self) -> usize {
        if self.rx_head >= self.rx_tail {
            self.rx_head - self.rx_tail
        } else {
            BUFFER_SIZE - self.rx_tail + self.rx_head
        }
    }

    /// 获取当前配置
    pub fn config(&self) -> &SerialConfig {
        &self.config
    }
}

impl Default for MockUart {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for MockUart {
    type Error = DeviceError;

    fn init(&mut self) -> Result<(), Self::Error> {
        self.clear_buffers();
        self.tx_count = 0;
        self.rx_count = 0;
        self.initialized = true;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockUART"
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn reset(&mut self) -> Result<(), Self::Error> {
        self.initialized = false;
        self.init()
    }
}

impl Read for MockUart {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }

        let mut count = 0;
        while count < buf.len() && self.rx_tail != self.rx_head {
            buf[count] = self.rx_buffer[self.rx_tail];
            self.rx_tail = (self.rx_tail + 1) % BUFFER_SIZE;
            count += 1;
        }
        self.rx_count += count;
        Ok(count)
    }
}

impl Write for MockUart {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }

        let mut count = 0;
        for &byte in buf {
            let next = (self.tx_head + 1) % BUFFER_SIZE;
            if next == self.tx_tail {
                break; // 缓冲区满
            }
            self.tx_buffer[self.tx_head] = byte;
            self.tx_head = next;
            count += 1;
        }
        self.tx_count += count;
        Ok(count)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        // Mock 实现：立即完成
        Ok(())
    }
}

impl Uart for MockUart {
    fn configure(&mut self, config: SerialConfig) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.config = config;
        Ok(())
    }

    fn set_baudrate(&mut self, baudrate: u32) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        if baudrate == 0 {
            return Err(DeviceError::InvalidParameter);
        }
        self.config.baudrate = baudrate;
        Ok(())
    }

    fn baudrate(&self) -> u32 {
        self.config.baudrate
    }

    fn is_rx_ready(&self) -> bool {
        self.rx_tail != self.rx_head
    }

    fn is_tx_ready(&self) -> bool {
        let next = (self.tx_head + 1) % BUFFER_SIZE;
        next != self.tx_tail
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_uart_new() {
        let uart = MockUart::new();
        assert!(!uart.is_ready());
        assert_eq!(uart.baudrate(), 115200);
    }

    #[test]
    fn test_mock_uart_init() {
        let mut uart = MockUart::new();
        assert!(uart.init().is_ok());
        assert!(uart.is_ready());
        assert_eq!(uart.name(), "MockUART");
    }

    #[test]
    fn test_mock_uart_write_read() {
        let mut uart = MockUart::new();
        uart.init().unwrap();

        // 模拟接收数据
        let received = uart.mock_receive(b"Hello");
        assert_eq!(received, 5);

        // 读取数据
        let mut buf = [0u8; 10];
        let n = uart.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"Hello");
    }

    #[test]
    fn test_mock_uart_write() {
        let mut uart = MockUart::new();
        uart.init().unwrap();

        // 写入数据
        let n = uart.write(b"World").unwrap();
        assert_eq!(n, 5);
        assert_eq!(uart.tx_pending(), 5);

        // 获取发送的数据
        let mut buf = [0u8; 10];
        let n = uart.mock_get_transmitted_to(&mut buf);
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"World");
    }

    #[test]
    fn test_mock_uart_not_initialized() {
        let mut uart = MockUart::new();

        let mut buf = [0u8; 10];
        assert!(matches!(uart.read(&mut buf), Err(DeviceError::NotInitialized)));
        assert!(matches!(uart.write(b"test"), Err(DeviceError::NotInitialized)));
    }

    #[test]
    fn test_mock_uart_configure() {
        let mut uart = MockUart::new();
        uart.init().unwrap();

        uart.set_baudrate(9600).unwrap();
        assert_eq!(uart.baudrate(), 9600);

        let config = SerialConfig {
            baudrate: 19200,
            data_bits: DataBits::Seven,
            stop_bits: StopBits::Two,
            parity: Parity::Even,
        };
        uart.configure(config).unwrap();
        assert_eq!(uart.baudrate(), 19200);
    }

    #[test]
    fn test_mock_uart_tx_rx_ready() {
        let mut uart = MockUart::new();
        uart.init().unwrap();

        // 初始状态
        assert!(!uart.is_rx_ready());
        assert!(uart.is_tx_ready());

        // 模拟接收数据后
        uart.mock_receive(b"X");
        assert!(uart.is_rx_ready());
    }

    #[test]
    fn test_mock_uart_counters() {
        let mut uart = MockUart::new();
        uart.init().unwrap();

        uart.write(b"12345").unwrap();
        assert_eq!(uart.tx_count(), 5);

        uart.mock_receive(b"ABC");
        let mut buf = [0u8; 3];
        uart.read(&mut buf).unwrap();
        assert_eq!(uart.rx_count(), 3);
    }

    #[test]
    fn test_mock_uart_reset() {
        let mut uart = MockUart::new();
        uart.init().unwrap();
        uart.write(b"test").unwrap();

        uart.reset().unwrap();
        assert!(uart.is_ready());
        assert_eq!(uart.tx_pending(), 0);
        assert_eq!(uart.tx_count(), 0);
    }
}

