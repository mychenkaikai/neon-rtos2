# Tasks
- [x] Task 1: 明确定时等待链路边界
  - [x] SubTask 1.1: 梳理 `Sleep`、`Waker` 与计时来源之间的现有调用关系
  - [x] SubTask 1.2: 确定最小可用的注册/唤醒接口，避免引入超出本轮范围的大改动

- [x] Task 2: 优化异步休眠实现
  - [x] SubTask 2.1: 替换 `Sleep` 中仅克隆 `waker` 的占位逻辑
  - [x] SubTask 2.2: 确保等待登记只发生一次，并在超时后可恢复执行

- [x] Task 3: 收敛执行器重复逻辑
  - [x] SubTask 3.1: 提炼任务出队与单次轮询公共路径
  - [x] SubTask 3.2: 保持 `run` 与 `poll_once` 的行为一致，同时保留空闲阻塞能力

- [x] Task 4: 补充验证与说明
  - [x] SubTask 4.1: 为异步休眠完成、任务唤醒与执行器行为补充测试或等价验证
  - [x] SubTask 4.2: 更新运行时相关说明，标注本轮支持范围和限制

# Task Dependencies
- Task 2 depends on Task 1
- Task 3 depends on Task 1
- Task 4 depends on Task 2
- Task 4 depends on Task 3
