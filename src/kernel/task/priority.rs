//! 任务优先级定义
//!
//! 提供任务优先级枚举，用于优先级调度。

/// 任务优先级
///
/// 数值越大，优先级越高。
///
/// # 示例
/// ```rust
/// use neon_rtos2::kernel::task::Priority;
///
/// let priority = Priority::High;
/// assert!(priority > Priority::Normal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Priority {
    /// 空闲优先级（最低）
    /// 
    /// 仅用于系统空闲任务
    Idle = 0,
    
    /// 低优先级
    /// 
    /// 用于后台任务、日志记录等
    Low = 1,
    
    /// 普通优先级（默认）
    /// 
    /// 大多数任务使用此优先级
    Normal = 2,
    
    /// 高优先级
    /// 
    /// 用于需要快速响应的任务
    High = 3,
    
    /// 关键优先级（最高）
    /// 
    /// 用于关键系统任务，如看门狗喂狗
    Critical = 4,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

impl Priority {
    /// 获取优先级数值
    ///
    /// # 返回值
    /// 优先级对应的 u8 数值
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// 从数值创建优先级
    ///
    /// # 参数
    /// - `value`: 优先级数值 (0-4)
    ///
    /// # 返回值
    /// - `Some(Priority)`: 有效的优先级
    /// - `None`: 无效的数值
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Priority::Idle),
            1 => Some(Priority::Low),
            2 => Some(Priority::Normal),
            3 => Some(Priority::High),
            4 => Some(Priority::Critical),
            _ => None,
        }
    }

    /// 检查是否为空闲优先级
    pub fn is_idle(self) -> bool {
        self == Priority::Idle
    }

    /// 检查是否为关键优先级
    pub fn is_critical(self) -> bool {
        self == Priority::Critical
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Normal);
        assert!(Priority::Normal > Priority::Low);
        assert!(Priority::Low > Priority::Idle);
    }

    #[test]
    fn test_priority_default() {
        assert_eq!(Priority::default(), Priority::Normal);
    }

    #[test]
    fn test_priority_from_u8() {
        assert_eq!(Priority::from_u8(0), Some(Priority::Idle));
        assert_eq!(Priority::from_u8(1), Some(Priority::Low));
        assert_eq!(Priority::from_u8(2), Some(Priority::Normal));
        assert_eq!(Priority::from_u8(3), Some(Priority::High));
        assert_eq!(Priority::from_u8(4), Some(Priority::Critical));
        assert_eq!(Priority::from_u8(5), None);
    }

    #[test]
    fn test_priority_as_u8() {
        assert_eq!(Priority::Idle.as_u8(), 0);
        assert_eq!(Priority::Low.as_u8(), 1);
        assert_eq!(Priority::Normal.as_u8(), 2);
        assert_eq!(Priority::High.as_u8(), 3);
        assert_eq!(Priority::Critical.as_u8(), 4);
    }

    #[test]
    fn test_priority_is_methods() {
        assert!(Priority::Idle.is_idle());
        assert!(!Priority::Normal.is_idle());
        assert!(Priority::Critical.is_critical());
        assert!(!Priority::High.is_critical());
    }
}

