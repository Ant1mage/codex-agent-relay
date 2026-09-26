# v2 · 分支整理、合并与推送

## 目标
把"菜单栏 + 浏览器 Inspector"的工作合并进 main 并推送远端，同时保留一个 Electron 单窗口版本的归档分支。

## 遇到的问题
1. **一条分支混着两种形态。** 早期 Electron 单窗口应用（apps/desktop，56 个文件）与后来的"菜单栏 + 浏览器 Inspector"在同一条线上，直接合并会让 main 同时带两套界面。
2. **归档分支提交了无关大文件。** electron-app 的 .gitignore 没有 output/，一次提交把约 1.7MB 截图写进历史。
3. **合并方向容易搞错。** 需求是：先推 feature 分支 → 从 main 切 electron-app 归档 → 把 feature 合进 main → 推 main。

## 怎么解决
- 按上述顺序执行；main 上删除 apps/desktop（不再有应用窗口），只保留 apps/menu-bar、apps/web、apps/relayd。
- .gitignore 补 output/，避免截图再进版本库。

## 验证
- git log 与远端一致：feature 分支 cba23e5、electron-app 20c1f99、main 合并后 af2558d。
- main 上 pnpm typecheck + pnpm test 通过。

## 遗留
- electron-app 上用户自己的提交 e9980d8 含 1.7MB 截图，是否改写历史由用户决定（未动）。
- 合并后没有任何"能不能发布"的检查 —— v4 变成 Critical（release 链路）。
