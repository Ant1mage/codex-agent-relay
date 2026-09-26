# v3 · 从零跑真实链路（不造假数据）

## 目标
清掉所有数据，从 0 个 agent 开始跑一遍真实流程看效果 —— 不是造一份好看的数据。

## 遇到的问题
1. **第一版验收用 seed 数据，被明确否掉。** seed-demo 能插出漂亮的会话与事件，但它绕过真实链路，真 bug 在 seed 下永远不出现。
2. **真实链路第一次跑就崩：daemon 抢建了表。**
```
SqliteError: table relay_events already exists
  at SqliteEventStore.#migrate
```
根因：schema 归 relay-mcp 所有（PRAGMA user_version 分级迁移），但 relayd 为读投影用 CREATE TABLE IF NOT EXISTS 抢先建了 relay_events，MCP 第一次真实委派时迁移撞车。
3. **会话名是编的。** 早期用 host session id 造显示名，看着像真实会话名，其实不是。

## 怎么解决
- daemon 不建表：RelayStore 加 isInitialised() 闸门；库没被 MCP 初始化前返回空投影，取消明确回 accepted:false 并说明原因。
- 补回归测试：daemon 在空库上启动，断言它没有创建 relay_events。
- 会话名改从 Codex app-server 的 thread/read 取真实 thread name 与 cwd（integrations/codex/src/thread-metadata.ts），Relay 不再自造名字。
- 验证脚本换成真实 MCP stdio 客户端驱动，不用 seed。

## 验证
留档证据：会话 codex:01a0d9b0-00c9-7e43-853d-32773dbf1dad（真实线程名「按阶段提交并合并到 main」、cwd 为仓库根）、run bc8d3495-…、profile deepseek-research、真实 dsh worker 进程、35 条事件（31 条 worker/reasoning 按设计隐藏）、最终答案 7352、状态 awaiting_host。

## 遗留
- 真实链路只覆盖成功路径，失败/超时未在真实环境复现。
- 会话同步依赖 codex 在 PATH 上（当时靠 shim 绕过），v7 才修成自动查找 VS Code 扩展自带的副本。
