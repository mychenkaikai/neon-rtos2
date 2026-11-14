#![allow(dead_code)]

use crate::drivers::traits::{
    AsyncRead, Configure, SerialPort, AsyncWrite, Device, DeviceError, SerialConfig,
};
/// STM32F1 UART DMA 驱动
/// 
/// 这个驱动展示了如何结合 HAL 2.0 的异步 trait（AsyncRead, AsyncWrite, AsyncUart）
/// 和设备配置接口。数据传输需要板级 DMA 后端，示例会显式报告 Unsupported。
pub struct Stm32UartDma {
    config: SerialConfig,
    initialized: bool,
    // 在真实的实现中，这里会持有外设的 PAC 引用
    // usart: stm32f1::stm32f103::USART1,
}

impl Stm32UartDma {
    pub fn new() -> Self {
        Self {
            config: SerialConfig::default(),
            initialized: false,
        }
    }

    fn check_transfer(&self, len: usize) -> Result<usize, DeviceError> {
        if !self.initialized { return Err(DeviceError::NotInitialized); }
        if len == 0 { Ok(0) } else { Err(DeviceError::Unsupported) }
    }

}

impl Device for Stm32UartDma {
    type Error = DeviceError;

    fn init(&mut self) -> Result<(), Self::Error> {
        // 1. 开启 USART1 和 DMA1 的时钟
        // 2. 配置 GPIO 引脚 (TX, RX)
        // 3. 配置 USART1 (波特率, 数据位等)
        // 4. 配置 DMA 中断
        
        self.initialized = true;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "STM32F1_USART1_DMA"
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }
}

impl AsyncRead for Stm32UartDma {
    async fn read_async(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.check_transfer(buf.len())
    }
}

impl AsyncWrite for Stm32UartDma {
    async fn write_async(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.check_transfer(buf.len())
    }

    async fn flush_async(&mut self) -> Result<(), Self::Error> {
        self.check_transfer(0).map(|_| ())
    }
}

impl Configure for Stm32UartDma {
    type Config = SerialConfig;
    fn configure(&mut self, config: SerialConfig) -> Result<(), Self::Error> {
        self.set_baudrate(config.baudrate)?;
        self.config = config;
        Ok(())
    }

}

impl SerialPort for Stm32UartDma {
    fn set_baudrate(&mut self, baudrate: u32) -> Result<(), Self::Error> {
        if !self.initialized { return Err(DeviceError::NotInitialized); }
        if baudrate == 0 { return Err(DeviceError::InvalidParameter); }
        self.config.baudrate = baudrate;
        // 重新计算 USART_BRR...
        Ok(())
    }

    fn baudrate(&self) -> u32 {
        self.config.baudrate
    }
}
