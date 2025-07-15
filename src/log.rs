//! 日志模块，支持在不同环境下的日志打印
//! - QEMU环境：使用cortex-m-semihosting的hprintln
//! - 真实设备：使用串口打印
//! - 测试环境：使用标准库的println

use core::fmt::{self, Write};

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

/// 日志记录器特征
pub trait Logger {
    /// 写入字符串到日志
    fn write_str(&self, s: &str) -> fmt::Result;
    
    /// 刷新日志
    fn flush(&self) -> fmt::Result;
}

/// QEMU环境下打印日志
#[cfg(all(feature = "cortex_m3", not(test)))]
#[inline(always)]
pub fn log_write(s: &str) -> fmt::Result {
    cortex_m_semihosting::hprint!("{}", s);
    Ok(())
}

/// 测试环境下打印日志
#[cfg(test)]
#[inline(always)]
pub fn log_write(s: &str) -> fmt::Result {
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

/// 初始化日志系统
pub fn init() {
    // 无需初始化静态记录器，直接使用函数调用
}