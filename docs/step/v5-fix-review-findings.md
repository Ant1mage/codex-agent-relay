# v5 · 修复 v4 的全部问题

## 目标
恢复配置控制面；把菜单栏做成真正的控制面；补齐 Codex 集成生命周期；修 daemon 的 PID 安全与配置漂移；补全 release 产物；让 Web 回到只读查看器。

## 遇到的问题
1. **配置无处可写。** 没有 owner，谁都不知道该由谁写 profiles.json。
2. **长驻 MCP 与配置变更冲突。** Codex 侧的 MCP 进程是长驻的，改完配置不重启 Codex 就看不到。
3. **Codex 集成是"一次性复制"。** 检测不出 stale/legacy/outdated，也没有卸载路径。
4. **daemon 身份靠 PID。** PID 会复用，重启操作可能杀掉无关进程。
5. **面板放哪。** 先做成 file:// 页面，结果 Chromium 拦截模块与样式：
```
Access to script at 'file:///…/assets/index-*.js' from origin 'null'
has been blocked by CORS policy
```
而且 file:// 页面的 Origin 也过不了 daemon 的同源校验。

## 怎么解决
- 新增 packages/config（@relay/config）：profiles.json / settings.json 的唯一写者，校验 + 原子写 + revision 戳。relayd 通过 API 读写：/api/config、/api/config/profiles/:id、/api/config/policy、/api/runtimes/:id/options。
- MCP 在 list_agents 与 run_agent 前重读配置并 ProfileRegistry.sync()（新增/更新/删除），policy 每次 run 前重新解析 —— 改配置不需要重启 Codex。
- Codex 集成走真实 plugin marketplace：生成 ~/.relay/codex-plugin（marketplace.json + plugin.json + hooks + skill），驱动 codex plugin marketplace add / plugin add / mcp add；5 项检查各带 status（ok/missing/stale/outdated/legacy）与 hint；install/repair/update/remove 四个动作，remove 完整撤销（插件、marketplace、MCP 入口、本地树、旧 skill）。
- daemon 身份改成 server.json 里的 pid + nonce 与 /api/health 回显比对；只有校验通过才允许 SIGTERM；stale 文件不再阻塞启动；加 daemon.lock 防并发；server.json 写 0600。
- 控制面板由 daemon 托管（/panel/）而不是 file://：同源、无 CORS 问题、token 走 URL 片段，Origin 校验不需要特例。
- Web 移除"安装到 Codex"，改成指路到菜单栏。
- release：pnpm build 产出 out/relayd/serve.js（含 web 与 panel）、out/mcp/stdio.js、out/menu-bar。

## 验证
- 5 项 Codex 检查全绿；codex plugin list 显示 relay@relay installed+enabled；remove 后 Codex 里零残留，再 install 又全绿。
- 配置热更新：同一个 MCP 进程内，daemon 写 profile → 立刻可见（HOT RELOAD: PASS）。
- 改配置后 SSE 收到第二个 snapshot（PUSH ON CONFIG CHANGE: PASS）。
- server.json 权限实测 -rw-------。
- 128 个测试通过，typecheck 干净。

## 遗留
- 打包仍然不存在（v6 审查的对象）。
- 面板表单在 420px 里排布不佳（v7 重做）。
