//! 日志模块 - 可配置的日志输出系统
//!
//! ## 设计理念
//!
//! 日志输出方式由**用户配置**，而非库硬编码。这样：
//! - 换芯片不需要修改库代码
//! - 用户可以选择 UART、半主机、RTT 等任意输出方式
//! - 用户控制硬件地址等配置
//!
//! ## 使用方法
//!
//! ### 1. 实现 LogOutput trait
//!
//! ```rust,ignore
//! use neon_rtos2::log::{LogOutput, set_log_output};
//!
//! // 定义你的 UART 输出
//! struct MyUart;
//!
//! impl LogOutput for MyUart {
//!     fn write_str(&self, s: &str) {
//!         const UART_BASE: usize = 0x4000_0000; // 你的 UART 地址
//!         for byte in s.bytes() {
//!             unsafe { core::ptr::write_volatile(UART_BASE as *mut u8, byte); }
//!         }
//!     }
//! }
//!
//! // 在 main 开始时注册
//! fn main() {
//!     set_log_output(&MyUart);
//!     // ...
//! }
//! ```
//!
//! ### 2. 使用预定义的输出实现
//!
//! ```rust,ignore
//! use neon_rtos2::log::{set_log_output, UartOutput, SemihostOutput};
//!
//! // 使用 UART 输出（指定地址）
//! static UART: UartOutput<0x1000_0000> = UartOutput::new();
//! set_log_output(&UART);
//!
//! // 或使用半主机输出（仅调试用）
//! static SEMIHOST: SemihostOutput = SemihostOutput;
//! set_log_output(&SEMIHOST);
//! ```
//!
//! ### 3. 使用日志宏
//!
//! ```rust,ignore
//! use neon_rtos2::{info, debug, error, warn, trace};
//! use neon_rtos2::log::{set_log_level, LogLevel};
//!
//! set_log_level(LogLevel::Debug);
//!
//! info!("Hello, RTOS!");
//! debug!("Debug value: {}", 42);
//! ```

use core::fmt::{self, Write};

// ============================================================================
// 日志级别
// ============================================================================

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(usize)]
pub enum LogLevel {
    /// 错误级别
    Error = 0,
    /// 警告级别
    Warn = 1,
    /// 信息级别
    Info = 2,
    /// 调试级别
    Debug = 3,
    /// 跟踪级别
    Trace = 4,
}

/// 全局日志级别，默认为Info
static mut GLOBAL_LOG_LEVEL: LogLevel = LogLevel::Info;

/// 设置全局日志级别
pub fn set_log_level(level: LogLevel) {
    unsafe {
        GLOBAL_LOG_LEVEL = level;
    }
}

/// 获取全局日志级别
pub fn get_log_level() -> LogLevel {
    unsafe { GLOBAL_LOG_LEVEL }
}

// ============================================================================
// LogOutput Trait - 用户实现此 trait 来定义日志输出方式
// ============================================================================

/// 日志输出 trait
/// 
/// 用户需要实现此 trait 来定义日志的输出方式。
/// 
/// # 示例
/// 
/// ```rust,ignore
/// struct MyUart;
/// 
/// impl LogOutput for MyUart {
///     fn write_str(&self, s: &str) {
///         // 写入到你的 UART
///         for byte in s.bytes() {
///             unsafe { 
///                 core::ptr::write_volatile(0x4000_0000 as *mut u8, byte); 
///             }
///         }
///     }
/// }
/// ```
pub trait LogOutput: Sync {
    /// 写入字符串到输出设备
    fn write_str(&self, s: &str);
    
    /// 刷新输出（可选实现）
    fn flush(&self) {}
}

// ============================================================================
// 全局日志输出注册
// ============================================================================

/// 空输出实现（默认）
struct NullOutputInner;

impl LogOutput for NullOutputInner {
    #[inline(always)]
    fn write_str(&self, _s: &str) {
        // 什么都不做
    }
}

/// 默认的空输出
static NULL_OUTPUT: NullOutputInner = NullOutputInner;

/// 全局日志输出器
static mut GLOBAL_LOG_OUTPUT: &dyn LogOutput = &NULL_OUTPUT;

/// 设置全局日志输出
/// 
/// **必须在使用任何日志宏之前调用此函数！**
/// 
/// # 参数
/// 
/// - `output`: 实现了 `LogOutput` trait 的静态引用
/// 
/// # 示例
/// 
/// ```rust,ignore
/// static MY_UART: MyUart = MyUart;
/// set_log_output(&MY_UART);
/// ```
/// 
/// # 安全性
/// 
/// 此函数应该在单线程环境下调用（通常在 main 函数开始时）。
pub fn set_log_output(output: &'static dyn LogOutput) {
    unsafe {
        GLOBAL_LOG_OUTPUT = output;
    }
}

/// 获取当前日志输出器
#[inline(always)]
fn get_log_output() -> &'static dyn LogOutput {
    unsafe { GLOBAL_LOG_OUTPUT }
}

// ============================================================================
// 预定义的日志输出实现
// ============================================================================

/// 空输出 - 丢弃所有日志
/// 
/// 用于禁用日志输出或作为占位符。
pub struct NullOutput;

impl LogOutput for NullOutput {
    #[inline(always)]
    fn write_str(&self, _s: &str) {}
}

/// UART 输出 - 通过内存映射 UART 输出日志
/// 
/// 使用 const generic 指定 UART 基地址，零运行时开销。
/// 
/// # 类型参数
/// 
/// - `BASE_ADDR`: UART 数据寄存器的内存地址
/// 
/// # 示例
/// 
/// ```rust,ignore
/// // QEMU RISC-V virt 平台
/// static UART: UartOutput<0x1000_0000> = UartOutput::new();
/// 
/// // STM32 USART1
/// static UART: UartOutput<0x4001_1004> = UartOutput::new();
/// 
/// set_log_output(&UART);
/// ```
pub struct UartOutput<const BASE_ADDR: usize>;

impl<const BASE_ADDR: usize> UartOutput<BASE_ADDR> {
    /// 创建新的 UART 输出实例
    pub const fn new() -> Self {
        Self
    }
}

impl<const BASE_ADDR: usize> LogOutput for UartOutput<BASE_ADDR> {
    #[inline(always)]
    fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            unsafe {
                core::ptr::write_volatile(BASE_ADDR as *mut u8, byte);
            }
        }
    }
}

/// 半主机输出 - 通过调试器输出日志（仅用于调试）
/// 
/// **注意**: 半主机输出需要连接调试器，生产环境不应使用！
/// 
/// # 示例
/// 
/// ```rust,ignore
/// #[cfg(feature = "cortex_m3")]
/// {
///     static SEMIHOST: SemihostOutput = SemihostOutput;
///     set_log_output(&SEMIHOST);
/// }
/// ```
#[cfg(feature = "cortex_m3")]
pub struct SemihostOutput;

#[cfg(feature = "cortex_m3")]
impl LogOutput for SemihostOutput {
    fn write_str(&self, s: &str) {
        cortex_m_semihosting::hprint!("{}", s);
    }
}

// ============================================================================
// 内部日志写入函数
// ============================================================================

/// 写入日志字符串
#[inline(always)]
pub fn log_write(s: &str) -> fmt::Result {
    get_log_output().write_str(s);
    Ok(())
}

/// 测试环境下的日志写入（覆盖上面的实现）
#[cfg(test)]
#[inline(always)]
pub fn log_write_test(s: &str) -> fmt::Result {
    print!("{}", s);
    Ok(())
}

/// 打印日志的宏，根据日志级别打印
#[macro_export]
macro_rules! log {
    ($level:expr, $($arg:tt)*) => {
        {
            if $level as usize <= $crate::log::get_log_level() as usize {
                use core::fmt::Write;
                let mut writer = $crate::log::LogWriter;
                let _ = write!(writer, $($arg)*);
            }
        }
    };
}

/// 日志写入器
pub struct LogWriter;

impl Write for LogWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        log_write(s)
    }
}

/// 错误级别日志
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log!($crate::log::LogLevel::Error, "[ERROR] ");
        $crate::log!($crate::log::LogLevel::Error, $($arg)*);
        $crate::log!($crate::log::LogLevel::Error, "\n");
    };
}

/// 警告级别日志
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log!($crate::log::LogLevel::Warn, "[WARN] ");
        $crate::log!($crate::log::LogLevel::Warn, $($arg)*);
        $crate::log!($crate::log::LogLevel::Warn, "\n");
    };
}

/// 信息级别日志
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log!($crate::log::LogLevel::Info, "[INFO] ");
        $crate::log!($crate::log::LogLevel::Info, $($arg)*);
        $crate::log!($crate::log::LogLevel::Info, "\n");
    };
}

/// 调试级别日志
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log!($crate::log::LogLevel::Debug, "[DEBUG] ");
        $crate::log!($crate::log::LogLevel::Debug, $($arg)*);
        $crate::log!($crate::log::LogLevel::Debug, "\n");
    };
}

/// 跟踪级别日志
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::log!($crate::log::LogLevel::Trace, "[TRACE] ");
        $crate::log!($crate::log::LogLevel::Trace, $($arg)*);
        $crate::log!($crate::log::LogLevel::Trace, "\n");
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_log_level_setting() {
        // 测试默认日志级别
        assert_eq!(get_log_level(), LogLevel::Info);
        
        // 测试设置日志级别
        set_log_level(LogLevel::Debug);
        assert_eq!(get_log_level(), LogLevel::Debug);
        
        set_log_level(LogLevel::Error);
        assert_eq!(get_log_level(), LogLevel::Error);
    }
    
    #[test]
    fn test_log_writer() {
        // 测试LogWriter的write_str功能
        let mut writer = LogWriter;
        let result = writer.write_str("测试日志");
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_log_level_comparison() {
        // 测试日志级别的比较
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Trace);
    }
    
    #[test]
    fn test_log_macros() {
        // 测试各种日志宏
        // 由于宏输出内容难以直接验证，这里主要测试不会崩溃
        error!("这是一个错误");
        warn!("这是一个警告");
        info!("这是一条信息");
        debug!("这是一条调试信息");
        trace!("这是一条跟踪信息");
    }
}