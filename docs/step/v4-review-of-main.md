# v4 · 审查 main 分支

## 目标
按 10 点提纲审查当前 main：配置丢失、菜单栏是否控制面、Web 越界、relayd 定位、daemon PID 安全、release 完整性、Codex 生命周期、配置漂移、刷新/SSE、产品边界。要求：不改代码、不谈 UI 审美、只出 Critical/High/Medium/Low。

## 遇到的问题（审查结论）
**Critical**
- **配置层整体丢失**：迁移后没有任何代码写 profiles.json / settings.json，Agent/Model/Reasoning/Capabilities/Policy/Workspace 全部只能手改 JSON。
- **菜单栏不是控制面**：只有状态和跳转，Agents 子项点了打开 Inspector 就走进死胡同。
- **release 断链**：relayd 从来没被构建过，MCP 入口写的是 corepack pnpm mcp:dev —— 装机环境不可能跑。
- **Codex 集成没有生命周期**：只有 install，没有检测状态、更新、修复、卸载；hooks 从未安装；configured 不等于可运行。

**High**
- daemon 用 PID 判断存活：既会误杀无关进程，也会被 stale 文件挡住启动。
- 配置漂移：MCP 只在启动时读一次 profiles；relayd 永久缓存环境；/api/refresh 没有调用方。
- SSE 的 revision 只覆盖 SQLite，改配置不推送。
- spawn 没有 error 处理，daemon 起不来会把托盘带崩。
- Policy 在 UI 上不可见，"当前生效什么"无从得知。
- model/reasoning 发现变成死代码。

**Medium/Low**：hooks 未安装、Web 承担安装动作（越界）、/api/refresh 与 model 发现无人调用、server.json 权限 0644、并发启动无锁、文档与现状不一致。

## 怎么解决
本轮只审查、不改代码，输出按 位置 / 实际行为 / 为什么有问题 / 影响场景 四条写清，作为 v5 的输入。

## 验证
每条结论都有可复现的命令或代码位置（例如 grep 不到 electron-builder、/api/refresh 无调用方、server.json 权限实测 0644）。

## 遗留
全部问题转 v5 修复。
