# Neon-RTOS2 测试套件

本示例提供了 Neon-RTOS2 的完整功能测试。

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
  [PASS] task_state_to_running
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

