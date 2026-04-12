# Neon-RTOS2 文档中心

> 最后更新：2026年04月12日

---

## 📚 文档索引

| 文档 | 说明 |
|------|------|
| [PROJECT_INFO.md](../PROJECT_INFO.md) | 📖 项目概述、核心特性、架构设计及未来愿景 |
| [API_GUIDE.md](./API_GUIDE.md) | 🔧 API 使用指南和最佳实践 |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 🏗️ 详细的系统架构设计与内部模块机制 |

---

## 🚀 快速开始

```rust
use neon_rtos2::prelude::*;

fn main() {
    kernel_init();
    
    Task::builder("my_task")
        .priority(Priority::High)
        .spawn(|_| {
            loop {
                info!("Running");
                Delay::delay(1000).unwrap();
            }
        }).unwrap();
    
    Scheduler::start();
}
```

详细 API 使用请参阅 [API_GUIDE.md](./API_GUIDE.md)

