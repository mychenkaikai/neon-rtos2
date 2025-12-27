//! 兼容层模块
//!
//! 统一处理 no_std 和 test 环境的类型导入，避免在多个文件中重复编写条件编译代码。
//!
//! # 使用方法
//!
//! ```rust,ignore
//! use crate::compat::{Box, Vec, VecDeque};
//! ```
//!
//! # 导出类型
//!
//! - `Box` - 堆分配的智能指针
//! - `Vec` - 动态数组
//! - `VecDeque` - 双端队列
//! - `Arc` - 原子引用计数智能指针
//! - `String` - 动态字符串
//! - `vec!` - 创建 Vec 的宏

#[cfg(not(test))]
pub use alloc::{
    boxed::Box,
    collections::VecDeque,
    string::String,
    sync::Arc,
    vec,
    vec::Vec,
};

#[cfg(test)]
pub use std::{
    boxed::Box,
    collections::VecDeque,
    string::String,
    sync::Arc,
    vec,
    vec::Vec,
};

