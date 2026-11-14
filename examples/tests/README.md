# Neon-RTOS2 测试套件

本示例提供了 Neon-RTOS2 的完整功能测试。

## 满载解锁时的中断响应

在仓库根目录运行 `scripts/run_wait_latency.sh`。脚本编译 release 固件并在 LM3S6965 的 Cortex-M3 QEMU 模型上运行。每轮由 8 个高优先级任务阻塞在同一把启用优先级继承的 Mutex 上，另有 16 个异步 Future 登记等待。以当前配置计算，任务槽还要容纳 idle 和持锁任务，异步队列则达到配置上限。

固件在空队列解锁和满载解锁的第一个 CPSID 之后读 SysTick 当前值，软件挂起 SysTick 异常，在处理函数入口再次读数。周期中断关闭，SysTick 计数器使用内核时钟和 24 位重载值。每个场景做 16 轮，输出两组最大 tick 差值，同时检查全部等待者可继续执行、继承优先级已恢复。单次间隔须短于计数器的一整圈。测试二进制单独链接探针入口，其他固件沿用原 HAL。

本次使用 Rust 1.90.0、QEMU 10.2.1 和 `-icount shift=5,align=off,sleep=off`。同一探针下，批量唤醒改动前空队列和满载的模拟最大值分别为 145 和 3012 tick，批量唤醒改动后为 26 和 649 tick。引入任务句柄代次后的结果列在下方的架构验收记录。计数包含探针、异常分派和处理函数读数的开销，适用于这一配置的前后比较。

真板复测使用匹配板卡的链接脚本和已启用 semihosting 的调试器。本例的 `memory.x` 针对 LM3S6965，默认从 0 地址启动，RAM 为 64 KiB。在匹配目标上可运行以下命令生成 ELF，再通过调试器从复位运行并记录 `PASS wait_latency` 一行。实测微秒数等于 `ticks × 1,000,000 / 实际内核时钟 Hz`。发布数据时一并记录板型、主频、Flash 等待周期和构建配置。这个负载覆盖 Mutex 解锁时挂起的可屏蔽中断，其他中断源和系统路径需要分别采样。

```bash
cargo build --locked --release -p neon-rtos2-example-tests \
    --bin wait_latency --target thumbv7m-none-eabi
```

## 架构验收记录（2026-11-08）

- `cargo test --locked` 通过 298 个单元和集成测试、116 个文档测试。另有 16 个原已标记 ignore 的文档片段。
- `scripts/run_scheduler_smoke.sh` 和 `scripts/run_riscv_smoke.sh` 均通过。两端各运行 30 个自然返回的短任务，验证切出后复用栈、旧句柄失效，以及普通定时器表满时任务延时仍可工作。
- RISC-V 固件将 tick 周期设为 20,000 个 CLINT 计数。20 tick 延时实测为 400,029 个计数，关中断超过五个周期后也通过补时断言。
- 本次最终固件的满载解锁探针为 688 个模拟 tick，空队列为 26。上一轮满载值为 649，最初基线为 3012。数值是指定模拟配置的 16 轮观测最大值。

## 同时超时与优先级继承链（2026-11-09）

同一脚本覆盖三项负载，每项运行 16 轮。8 个高优先级任务分别调用 `Delay::delay(Ticks(1))`，由一次软件挂起的真实 SysTick 使它们同时到期，检查每轮 8 个任务全部恢复。另一项用两把 Mutex 让高优先级任务经中优先级任务向低优先级持锁者传递优先级，测量低优先级任务解锁时挂起中断的等待时间，并检查优先级恢复及两名等待者退出。

| Cortex-M3 QEMU 观测最大值（模拟 tick） | 整批处理 | 每处理一个到期项后释放中断掩码 |
|---|---:|---:|
| 8 个任务到期，SysTick 内最长连续关中断区间 | 819 | 114 |
| 同一 SysTick 处理函数总耗时 | 909 | 1249 |
| 两级优先级继承链解锁中断等待 | 298 | 298 |
| 空队列 / 满载解锁中断等待 | 31 / 692 | 31 / 692 |

两列使用相同的测试专用 CPSID、CPSIE 和 SysTick 探针。关中断读数取最外层关中断后到重新开中断前的两个时间戳，处理函数耗时含嵌套探针开销、截止于处理函数返回，后续 PendSV 切换另计。新增探针改变了固定开销，旧版探针的 26 / 688 保留为历史记录。

## 测试覆盖

### 任务管理测试
- ✅ 基本任务创建 (`Task::new`)
- ✅ Builder 模式创建 (`Task::builder`)
- ✅ 任务状态转换 (Ready → Running → Blocked → Ready)
- ✅ 任务优先级设置和修改
- ✅ 任务名称和 ID
- ✅ 栈顶对齐检查

### 任务迭代器测试
- ✅ `Task::iter()` 遍历所有任务
- ✅ `Task::ready_tasks()` 就绪任务迭代器
- ✅ `for_each` 方法

### 互斥锁测试
- ✅ 基本创建和加锁/解锁
- ✅ RAII 守卫 (`lock_guard()`)
- ✅ 闭包风格 (`with_lock()`)

### 定时器测试
- ✅ 定时器创建
- ✅ 启动/停止
- ✅ 延时功能 (`Delay::delay`)

### 消息队列测试
- ✅ 队列创建
- ✅ 发送/接收
- ✅ 空队列处理
- ✅ 满队列检测
- ✅ 队列长度

### 错误处理测试
- ✅ 任务槽满错误 (`TaskSlotsFull`)

## 环境要求

### 安装 ARM 目标

```bash
rustup target add thumbv7m-none-eabi
```

### 安装 QEMU

**macOS:**
```bash
brew install qemu
```

**Ubuntu/Debian:**
```bash
sudo apt install qemu-system-arm
```

## 运行测试

```bash
# 编译
cargo build --release

# 使用 QEMU 运行测试
qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic \
    -semihosting-config enable=on,target=native \
    -kernel target/thumbv7m-none-eabi/release/mutex_test
```

## 预期输出

```
Neon-RTOS2 Test Framework Initialized

Starting scheduler...

Test runner task started

================================================
       Neon-RTOS2 Test Suite
================================================

--- Task Management Tests ---
Testing: Task Creation
  [PASS] task_creation_basic
Testing: Task Builder Pattern
  [PASS] task_builder_creation
  [PASS] task_builder_priority
Testing: Task State Transitions
  [PASS] task_state_creation
  [PASS] task_state_initial_ready
  [PASS] task_state_to_blocked
  [PASS] task_state_back_to_ready
Testing: Task Priority
  [PASS] task_priority_creation
  [PASS] task_priority_initial
  [PASS] task_priority_modified
Testing: Task Name and ID
  [PASS] task_name_creation
  [PASS] task_name_correct
  [PASS] task_id_valid
Testing: Stack Alignment
  [PASS] stack_align_creation
  [PASS] stack_alignment_8byte

--- Task Iterator Tests ---
Testing: Task Iterator
  [PASS] task_iterator_count
  [PASS] task_iterator_ready
  [PASS] task_iterator_foreach

--- Mutex Tests ---
Testing: Mutex Basic
  [PASS] mutex_creation
  [PASS] mutex_unlock
Testing: Mutex RAII Guard
  [PASS] mutex_raii_lock
  [PASS] mutex_raii_relock
Testing: Mutex Closure Style
  [PASS] mutex_closure_executed

--- Timer Tests ---
Testing: Timer Basic
  [PASS] timer_initial_stopped
  [PASS] timer_started
  [PASS] timer_stopped
Testing: Delay
  [PASS] delay_basic

--- Message Queue Tests ---
Testing: Message Queue
  [PASS] mq_creation
  [PASS] mq_send
  [PASS] mq_receive
  [PASS] mq_receive_empty
  [PASS] mq_full
  [PASS] mq_length

--- Error Handling Tests ---
Testing: Error Handling - Task Slots Full
  [PASS] error_task_slots_full

================================================
               Test Results
================================================
Total:  35
Passed: 35
Failed: 0

All tests PASSED!
================================================

Tests completed, exiting...
```

## 项目结构

```
tests/
├── Cargo.toml          # 项目配置
├── build.rs            # 构建脚本
├── memory.x            # 链接脚本
├── src/
│   └── bin/
│       └── main.rs     # 测试主程序
└── README.md           # 本文件
```

## 添加新测试

1. 在 `main.rs` 中添加测试函数：

```rust
fn test_my_feature() -> bool {
    info!("Testing: My Feature");
    
    // 测试逻辑
    let result = some_operation();
    
    // 使用断言宏
    test_assert!(result.is_ok(), "my_feature_test", "Should succeed")
}
```

2. 在 `run_all_tests()` 中调用：

```rust
fn run_all_tests() {
    // ...
    info!("--- My Feature Tests ---");
    test_my_feature();
    // ...
}
```

## 测试宏说明

### `test_assert!`

```rust
test_assert!(condition, "test_name", "failure_message")
```

- `condition`: 布尔表达式
- `test_name`: 测试名称（用于报告）
- `failure_message`: 失败时的消息

### `test_assert_eq!`

```rust
test_assert_eq!(left, right, "test_name")
```

- `left`: 左值
- `right`: 右值（期望值）
- `test_name`: 测试名称

## 注意事项

1. **测试顺序** - 某些测试（如 `test_error_task_slots_full`）会影响系统状态，应放在最后运行

2. **资源清理** - 当前测试框架不支持自动清理，创建的任务会一直存在

3. **QEMU 退出** - 测试完成后会自动调用 `debug::exit()` 退出 QEMU

## 常见问题

### Q: 测试卡住不动？

A: 检查是否有任务进入死循环或死锁。

### Q: 某些测试失败？

A: 查看失败消息，可能是：
- 资源不足（任务槽满、互斥锁槽满等）
- 状态不正确
- 时序问题

### Q: 如何只运行部分测试？

A: 修改 `run_all_tests()` 函数，注释掉不需要的测试。

