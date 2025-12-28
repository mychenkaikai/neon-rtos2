//! # Mock SPI 驱动
//!
//! 模拟 SPI 总线驱动，用于测试和演示。
//!
//! ## 功能特性
//!
//! - 实现 `Device`, `Spi` trait
//! - 支持全双工传输
//! - 支持配置时钟频率、模式、位顺序
//! - 提供测试辅助方法
//!
//! ## 使用示例
//!
//! ```rust
//! use neon_rtos2::drivers::examples::MockSpi;
//! use neon_rtos2::drivers::{Device, Spi, SpiConfig, SpiMode};
//!
//! let mut spi = MockSpi::new();
//! spi.init().unwrap();
//!
//! // 配置 SPI
//! let config = SpiConfig {
//!     frequency: 1_000_000,
//!     mode: SpiMode::Mode0,
//!     msb_first: true,
//! };
//! spi.configure(config).unwrap();
//!
//! // 传输数据
//! let mut rx = [0u8; 4];
//! let tx = [0x01, 0x02, 0x03, 0x04];
//! spi.transfer(&mut rx, &tx).unwrap();
//! ```

use crate::drivers::{
    Device, Spi,
    SpiConfig, SpiMode,
    DeviceError,
};

/// Mock SPI 缓冲区大小
const BUFFER_SIZE: usize = 256;

/// Mock SPI 驱动
///
/// 模拟 SPI 总线设备，支持全双工传输。
///
/// # 工作原理
///
/// - 发送的数据存储在 `tx_buffer`
/// - 接收的数据从 `rx_buffer` 读取（需要预先设置）
/// - 支持回环模式（发送的数据直接作为接收数据）
///
/// # 示例
///
/// ```rust
/// use neon_rtos2::drivers::examples::MockSpi;
/// use neon_rtos2::drivers::{Device, Spi};
///
/// let mut spi = MockSpi::new();
/// spi.init().unwrap();
///
/// // 设置回环模式
/// spi.set_loopback(true);
///
/// // 传输数据
/// let mut rx = [0u8; 3];
/// spi.transfer(&mut rx, &[1, 2, 3]).unwrap();
/// assert_eq!(rx, [1, 2, 3]); // 回环模式下收到发送的数据
/// ```
pub struct MockSpi {
    /// SPI 配置
    config: SpiConfig,
    /// 发送缓冲区
    tx_buffer: [u8; BUFFER_SIZE],
    /// 接收缓冲区（预设的响应数据）
    rx_buffer: [u8; BUFFER_SIZE],
    /// 发送缓冲区位置
    tx_pos: usize,
    /// 接收缓冲区位置
    rx_pos: usize,
    /// 是否已初始化
    initialized: bool,
    /// 回环模式
    loopback: bool,
    /// 传输次数计数
    transfer_count: usize,
    /// 传输字节计数
    byte_count: usize,
}

impl MockSpi {
    /// 创建新的 Mock SPI 实例
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockSpi;
    ///
    /// let spi = MockSpi::new();
    /// ```
    pub const fn new() -> Self {
        Self {
            config: SpiConfig {
                frequency: 1_000_000,
                mode: SpiMode::Mode0,
                msb_first: true,
            },
            tx_buffer: [0; BUFFER_SIZE],
            rx_buffer: [0; BUFFER_SIZE],
            tx_pos: 0,
            rx_pos: 0,
            initialized: false,
            loopback: false,
            transfer_count: 0,
            byte_count: 0,
        }
    }

    /// 设置回环模式
    ///
    /// 在回环模式下，发送的数据会直接作为接收数据返回。
    ///
    /// # 参数
    ///
    /// - `enable`: true 启用回环，false 禁用
    pub fn set_loopback(&mut self, enable: bool) {
        self.loopback = enable;
    }

    /// 检查是否为回环模式
    pub fn is_loopback(&self) -> bool {
        self.loopback
    }

    /// 预设接收数据（测试用）
    ///
    /// 设置 SPI 从设备的响应数据。
    ///
    /// # 参数
    ///
    /// - `data`: 预设的响应数据
    ///
    /// # 示例
    ///
    /// ```rust
    /// use neon_rtos2::drivers::examples::MockSpi;
    /// use neon_rtos2::drivers::{Device, Spi};
    ///
    /// let mut spi = MockSpi::new();
    /// spi.init().unwrap();
    ///
    /// // 预设从设备响应
    /// spi.mock_set_response(&[0xAA, 0xBB, 0xCC]);
    ///
    /// // 传输时会收到预设的响应
    /// let mut rx = [0u8; 3];
    /// spi.transfer(&mut rx, &[0, 0, 0]).unwrap();
    /// assert_eq!(rx, [0xAA, 0xBB, 0xCC]);
    /// ```
    pub fn mock_set_response(&mut self, data: &[u8]) {
        let len = data.len().min(BUFFER_SIZE);
        self.rx_buffer[..len].copy_from_slice(&data[..len]);
        self.rx_pos = 0;
    }

    /// 获取已发送的数据（测试用）
    ///
    /// # 参数
    ///
    /// - `buf`: 目标缓冲区
    ///
    /// # 返回值
    ///
    /// 返回实际复制的字节数
    pub fn mock_get_transmitted(&self, buf: &mut [u8]) -> usize {
        let len = buf.len().min(self.tx_pos);
        buf[..len].copy_from_slice(&self.tx_buffer[..len]);
        len
    }

    /// 清空缓冲区
    pub fn clear_buffers(&mut self) {
        self.tx_buffer = [0; BUFFER_SIZE];
        self.rx_buffer = [0; BUFFER_SIZE];
        self.tx_pos = 0;
        self.rx_pos = 0;
    }

    /// 获取传输次数
    pub fn transfer_count(&self) -> usize {
        self.transfer_count
    }

    /// 获取传输字节数
    pub fn byte_count(&self) -> usize {
        self.byte_count
    }

    /// 获取当前配置
    pub fn config(&self) -> &SpiConfig {
        &self.config
    }

    /// 获取当前频率
    pub fn frequency(&self) -> u32 {
        self.config.frequency
    }

    /// 获取当前模式
    pub fn mode(&self) -> SpiMode {
        self.config.mode
    }
}

impl Default for MockSpi {
    fn default() -> Self {
        Self::new()
    }
}

impl Device for MockSpi {
    type Error = DeviceError;

    fn init(&mut self) -> Result<(), Self::Error> {
        self.clear_buffers();
        self.transfer_count = 0;
        self.byte_count = 0;
        self.initialized = true;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockSPI"
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }

    fn reset(&mut self) -> Result<(), Self::Error> {
        self.initialized = false;
        self.loopback = false;
        self.init()
    }
}

impl Spi for MockSpi {
    fn configure(&mut self, config: SpiConfig) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        if config.frequency == 0 {
            return Err(DeviceError::InvalidParameter);
        }
        self.config = config;
        Ok(())
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        if read.len() != write.len() {
            return Err(DeviceError::InvalidParameter);
        }

        let len = write.len();
        
        // 存储发送数据
        if self.tx_pos + len <= BUFFER_SIZE {
            self.tx_buffer[self.tx_pos..self.tx_pos + len].copy_from_slice(write);
            self.tx_pos += len;
        }

        // 生成接收数据
        if self.loopback {
            // 回环模式：发送数据直接作为接收数据
            read.copy_from_slice(write);
        } else {
            // 正常模式：从预设的响应缓冲区读取
            for (i, byte) in read.iter_mut().enumerate() {
                if self.rx_pos + i < BUFFER_SIZE {
                    *byte = self.rx_buffer[self.rx_pos + i];
                } else {
                    *byte = 0xFF; // 默认值
                }
            }
            self.rx_pos += len;
        }

        self.transfer_count += 1;
        self.byte_count += len;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }

        // 存储发送数据
        let len = data.len();
        if self.tx_pos + len <= BUFFER_SIZE {
            self.tx_buffer[self.tx_pos..self.tx_pos + len].copy_from_slice(data);
            self.tx_pos += len;
        }

        self.transfer_count += 1;
        self.byte_count += len;
        Ok(())
    }

    fn read(&mut self, data: &mut [u8]) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }

        let len = data.len();
        
        // 从预设的响应缓冲区读取
        for (i, byte) in data.iter_mut().enumerate() {
            if self.rx_pos + i < BUFFER_SIZE {
                *byte = self.rx_buffer[self.rx_pos + i];
            } else {
                *byte = 0xFF;
            }
        }
        self.rx_pos += len;

        self.transfer_count += 1;
        self.byte_count += len;
        Ok(())
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_spi_new() {
        let spi = MockSpi::new();
        assert!(!spi.is_ready());
        assert_eq!(spi.frequency(), 1_000_000);
        assert_eq!(spi.mode(), SpiMode::Mode0);
    }

    #[test]
    fn test_mock_spi_init() {
        let mut spi = MockSpi::new();
        assert!(spi.init().is_ok());
        assert!(spi.is_ready());
        assert_eq!(spi.name(), "MockSPI");
    }

    #[test]
    fn test_mock_spi_configure() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        let config = SpiConfig {
            frequency: 4_000_000,
            mode: SpiMode::Mode3,
            msb_first: false,
        };
        spi.configure(config).unwrap();

        assert_eq!(spi.frequency(), 4_000_000);
        assert_eq!(spi.mode(), SpiMode::Mode3);
        assert!(!spi.config().msb_first);
    }

    #[test]
    fn test_mock_spi_loopback() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();
        spi.set_loopback(true);

        let tx = [1, 2, 3, 4];
        let mut rx = [0u8; 4];
        spi.transfer(&mut rx, &tx).unwrap();

        assert_eq!(rx, tx);
    }

    #[test]
    fn test_mock_spi_response() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        // 预设响应
        spi.mock_set_response(&[0xAA, 0xBB, 0xCC, 0xDD]);

        let mut rx = [0u8; 4];
        spi.transfer(&mut rx, &[0, 0, 0, 0]).unwrap();

        assert_eq!(rx, [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_mock_spi_get_transmitted() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        let tx = [0x12, 0x34, 0x56];
        let mut rx = [0u8; 3];
        spi.transfer(&mut rx, &tx).unwrap();

        let mut buf = [0u8; 3];
        let n = spi.mock_get_transmitted(&mut buf);
        assert_eq!(n, 3);
        assert_eq!(buf, tx);
    }

    #[test]
    fn test_mock_spi_counters() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        let mut rx = [0u8; 4];
        spi.transfer(&mut rx, &[1, 2, 3, 4]).unwrap();
        spi.transfer(&mut rx, &[5, 6, 7, 8]).unwrap();

        assert_eq!(spi.transfer_count(), 2);
        assert_eq!(spi.byte_count(), 8);
    }

    #[test]
    fn test_mock_spi_write_only() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        spi.write(&[0x01, 0x02]).unwrap();

        let mut buf = [0u8; 2];
        let n = spi.mock_get_transmitted(&mut buf);
        assert_eq!(n, 2);
        assert_eq!(buf, [0x01, 0x02]);
    }

    #[test]
    fn test_mock_spi_read_only() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        spi.mock_set_response(&[0xDE, 0xAD]);

        let mut buf = [0u8; 2];
        spi.read(&mut buf).unwrap();
        assert_eq!(buf, [0xDE, 0xAD]);
    }

    #[test]
    fn test_mock_spi_not_initialized() {
        let mut spi = MockSpi::new();

        let mut rx = [0u8; 1];
        assert!(matches!(
            spi.transfer(&mut rx, &[0]),
            Err(DeviceError::NotInitialized)
        ));
    }

    #[test]
    fn test_mock_spi_invalid_config() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        let config = SpiConfig {
            frequency: 0, // 无效频率
            mode: SpiMode::Mode0,
            msb_first: true,
        };
        assert!(matches!(
            spi.configure(config),
            Err(DeviceError::InvalidParameter)
        ));
    }

    #[test]
    fn test_mock_spi_transfer_length_mismatch() {
        let mut spi = MockSpi::new();
        spi.init().unwrap();

        let mut rx = [0u8; 2];
        let tx = [0u8; 3];
        assert!(matches!(
            spi.transfer(&mut rx, &tx),
            Err(DeviceError::InvalidParameter)
        ));
    }
}

