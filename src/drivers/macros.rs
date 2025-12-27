//! # 设备驱动宏
//!
//! 提供便捷的宏来定义设备驱动。
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! use neon_rtos2::device_driver;
//!
//! device_driver! {
//!     name: Uart0,
//!     base_addr: 0x4000_0000,
//!     registers: {
//!         data: u32, 0x00;
//!         status: u32, 0x04;
//!         control: u32, 0x08;
//!     }
//! }
//! ```

/// 定义设备驱动的宏
///
/// 自动生成设备结构体和寄存器访问方法。
///
/// # 语法
///
/// ```rust,ignore
/// device_driver! {
///     name: DeviceName,
///     base_addr: 0x4000_0000,
///     registers: {
///         reg_name: reg_type, offset;
///         ...
///     }
/// }
/// ```
///
/// # 生成内容
///
/// - 设备结构体 `DeviceName`
/// - 每个寄存器的读取方法 `reg_name()`
/// - 每个寄存器的写入方法 `reg_name_write()`
///
/// # 示例
///
/// ```rust,ignore
/// device_driver! {
///     name: Uart0,
///     base_addr: 0x4000_0000,
///     registers: {
///         data: u32, 0x00;
///         status: u32, 0x04;
///         control: u32, 0x08;
///     }
/// }
///
/// let uart = Uart0::new();
/// let status = uart.status();
/// uart.data_write(0x55);
/// ```
#[macro_export]
macro_rules! device_driver {
    (
        name: $name:ident,
        base_addr: $base:expr,
        registers: {
            $($reg_name:ident : $reg_type:ty , $offset:expr);* $(;)?
        }
    ) => {
        /// 自动生成的设备驱动结构体
        pub struct $name {
            base: usize,
        }

        impl $name {
            /// 创建新的设备实例
            pub const fn new() -> Self {
                Self { base: $base }
            }

            /// 获取基地址
            pub const fn base_addr(&self) -> usize {
                self.base
            }

            $(
                $crate::paste::paste! {
                    /// 读取寄存器值
                    #[inline]
                    pub fn $reg_name(&self) -> $reg_type {
                        unsafe { core::ptr::read_volatile((self.base + $offset) as *const $reg_type) }
                    }

                    /// 写入寄存器值
                    #[inline]
                    pub fn [<$reg_name _write>](&self, value: $reg_type) {
                        unsafe { core::ptr::write_volatile((self.base + $offset) as *mut $reg_type, value) }
                    }

                    /// 获取寄存器地址
                    #[inline]
                    pub const fn [<$reg_name _addr>](&self) -> usize {
                        self.base + $offset
                    }
                }
            )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

/// 定义位域的宏
///
/// 用于定义寄存器中的位域。
///
/// # 示例
///
/// ```rust,ignore
/// bitfield! {
///     /// UART 状态寄存器
///     pub struct UartStatus(u32) {
///         /// 发送缓冲区空
///         tx_empty: 0,
///         /// 接收缓冲区满
///         rx_full: 1,
///         /// 发送忙
///         tx_busy: 2..3,
///     }
/// }
/// ```
#[macro_export]
macro_rules! bitfield {
    (
        $(#[$meta:meta])*
        pub struct $name:ident($type:ty) {
            $(
                $(#[$field_meta:meta])*
                $field:ident: $bit:tt $(.. $end_bit:tt)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy)]
        pub struct $name($type);

        impl $name {
            /// 从原始值创建
            pub const fn from_raw(value: $type) -> Self {
                Self(value)
            }

            /// 获取原始值
            pub const fn raw(&self) -> $type {
                self.0
            }

            $(
                $crate::paste::paste! {
                    $(#[$field_meta])*
                    #[inline]
                    pub const fn $field(&self) -> bool {
                        (self.0 >> $bit) & 1 != 0
                    }

                    /// 设置位域
                    #[inline]
                    pub fn [<set_ $field>](&mut self, value: bool) {
                        if value {
                            self.0 |= 1 << $bit;
                        } else {
                            self.0 &= !(1 << $bit);
                        }
                    }
                }
            )*
        }

        impl From<$type> for $name {
            fn from(value: $type) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $type {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("raw", &self.0)
                    $(
                        .field(stringify!($field), &self.$field())
                    )*
                    .finish()
            }
        }
    };
}

/// 定义中断处理器的宏
///
/// # 示例
///
/// ```rust,ignore
/// interrupt_handler! {
///     name: uart0_irq,
///     handler: || {
///         // 处理中断
///     }
/// }
/// ```
#[macro_export]
macro_rules! interrupt_handler {
    (
        name: $name:ident,
        handler: $handler:expr
    ) => {
        #[no_mangle]
        pub extern "C" fn $name() {
            ($handler)()
        }
    };
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    // 测试 bitfield 宏
    bitfield! {
        /// 测试状态寄存器
        pub struct TestStatus(u32) {
            /// 位 0
            bit0: 0,
            /// 位 1
            bit1: 1,
            /// 位 7
            bit7: 7,
        }
    }

    #[test]
    fn test_bitfield() {
        let status = TestStatus::from_raw(0b1000_0011);
        
        assert!(status.bit0());
        assert!(status.bit1());
        assert!(status.bit7());
        
        let mut status = TestStatus::from_raw(0);
        status.set_bit0(true);
        assert!(status.bit0());
        assert_eq!(status.raw(), 1);
    }

    #[test]
    fn test_bitfield_conversion() {
        let status: TestStatus = 0x55u32.into();
        let raw: u32 = status.into();
        assert_eq!(raw, 0x55);
    }
}

