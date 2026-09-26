# v6 · 更新机制审查

## 目标
检查 Relay 的应用更新机制是否完整：有没有正式打包、四个产物能否作为同一版本发布、macOS 签名/公证/更新 metadata 链路、升级后是否仍依赖源码环境；并区分 Relay App 更新与 Codex 集成更新两个生命周期。

## 遇到的问题（审查结论）
**Critical**
- 没有 electron-builder / electron-updater / CI：打包、签名、公证、更新 metadata 全部为 0。
- 托盘找不到产物 daemon：候选路径 apps/menu-bar/out/relayd/serve.js 等三个全不存在，真实产物在 out/relayd/serve.js，开发态靠回落 corepack 掩盖。
- 打包后 Codex 会被指向 Electron 可执行文件（process.execPath），且没设 ELECTRON_RUN_AS_NODE。
- 集成源文件没进产物：脱离仓库后找不到 integrations/codex，发布版装不了 Codex 集成。
- App 更新与 daemon 生命周期脱钩：daemon 是 detached 的，退出 App 不停它；托盘也不比对版本，升级后新版托盘会驱动旧版 daemon。

**High**
- 原生依赖只以符号链接出现在 out/node_modules（装机后必然起不来）。
- 没有单一版本来源：所有包 0.0.0，root 无 version。
- App 版本与集成之间只有 skill/hooks 内容哈希这一个间接信号。

**Medium/Low**：配置损坏静默回落；产物带 sourcemap、无 LSUIElement；菜单里没有更新入口，看不到版本；文档与现状不一致。

**两个生命周期判定**：Codex 集成侧闭环成立（最新/需要更新/部分配置损坏/未安装四态基本覆盖），App 侧完全不存在；两者结构上没混淆，但缺少"集成是哪个 App 版本装的"记录。

## 怎么解决
本轮只审查、不改代码，12 条按严重度列出，作为 v7 的输入。

## 验证
结论均可用命令复现：grep 不到 electron-builder、三个 daemon 候选路径实测 missing、模拟发布根目录搜索 integrations/codex 返回 NO、模拟打包树的 out/node_modules 是 symlink。

## 遗留
全部问题转 v7。
