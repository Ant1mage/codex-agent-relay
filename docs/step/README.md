# Relay 开发过程记录（按 plan 分轮）

每一轮 plan 记一份（v1、v2……），固定五段：目标 / 遇到的问题（现象+根因）/ 怎么解决 / 验证 / 遗留。

| 轮次 | 主题 | 结果 |
|---|---|---|
| [v1](v1-core-chain-and-menu-bar.md) | 核心链路验证 + macOS 菜单栏落地 | 链路跑通，菜单栏可用 |
| [v2](v2-branches-and-merge.md) | 分支整理、合并与推送 | main 合并，electron-app 归档 |
| [v3](v3-real-flow-from-zero.md) | 从零跑真实链路（不造假数据） | 真实 Codex 会话 + 真实 worker |
| [v4](v4-review-of-main.md) | 审查 main 分支 | 4 Critical / 6 High / 6 Medium / 3 Low |
| [v5](v5-fix-review-findings.md) | 修复 v4 的全部问题 | 配置控制面恢复，菜单栏成为控制面 |
| [v6](v6-update-mechanism-audit.md) | 更新机制审查 | 打包/更新链路 0 分，列出 12 条 |
| [v7](v7-panel-redesign-packaging-updater.md) | 面板重做 + 打包 + electron-updater | 可运行的 .app，更新链路接通 |

跨轮反复踩到的环境坑：ELECTRON_RUN_AS_NODE=1、better-sqlite3 ABI、ESM/CJS、file:// 的 CORS、macOS 代理 —— 见 v1 末尾。
