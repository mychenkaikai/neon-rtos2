# 异步运行时整洁化优化 Spec

## Why
当前异步运行时已经具备基础执行能力，但 `Sleep` future 仍保留占位式 TODO，导致定时等待链路并未真正闭环。与此同时，执行器中的取任务、唤醒和空闲阻塞逻辑存在重复与耦合，增加了阅读和后续维护成本。

## What Changes
- 为 `Sleep` future 定义最小可用的唤醒注册机制，移除仅克隆 `waker` 的占位实现
- 收敛 `Executor` 中的任务出队、轮询和空闲阻塞职责，减少重复逻辑
- 为异步休眠与执行器唤醒路径补充可验证测试或等价验证入口
- 更新运行时模块说明，明确异步定时等待的行为边界与使用方式

## Impact
- Affected specs: 异步运行时、定时等待、任务唤醒
- Affected code: `src/runtime/future.rs`、`src/runtime/executor.rs`、`src/runtime/waker.rs`、`src/runtime/mod.rs`、`tests/` 或相关示例

## ADDED Requirements
### Requirement: 异步休眠可注册唤醒
The system SHALL 为 `Sleep` future 提供一次性注册的唤醒机制，使其在截止时间到达后能够被重新调度，而不是停留在占位 TODO 状态。

#### Scenario: 成功登记等待
- **WHEN** 异步任务 `await sleep(...)` 且当前时间尚未到达截止时间
- **THEN** future 返回 `Poll::Pending` 并登记后续唤醒所需的信息

#### Scenario: 超时后继续执行
- **WHEN** 截止时间到达且等待中的异步任务被唤醒
- **THEN** 执行器能够再次轮询该任务并使 `Sleep` 完成

### Requirement: 执行器职责收敛
The system SHALL 将执行器中的“取待执行任务”“执行单次轮询”“空闲时阻塞当前 RTOS 任务”拆分为清晰、可复用的内部职责，避免 `run` 与 `poll_once` 长期复制相同流程。

#### Scenario: 单次轮询与循环运行行为一致
- **WHEN** 同一个异步任务分别经过 `poll_once` 与 `run` 驱动
- **THEN** 两条路径遵循一致的出队、轮询和完成处理规则

## MODIFIED Requirements
### Requirement: 轻量级执行器
The system SHALL 继续保持嵌入式场景下的轻量执行器设计，但需要将空闲阻塞和唤醒路径组织为更易理解、可测试的结构，而不是依赖分散在多个分支中的重复流程。

## REMOVED Requirements
- 无
