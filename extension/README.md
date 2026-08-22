# TabFlow Companion 浏览器扩展

向 TabFlow 桌面应用实时推送浏览器标签页数据（URL / 标题 / 标签 ID），
并执行来自桌面端的「关闭标签页 / 跳转到标签页」指令。

无需以调试模式启动浏览器，开箱即用于正常运行中的 Chrome / Edge /
Opera / Brave / Vivaldi。数据仅在本机 `127.0.0.1:19876` 传输。

## 安装（开发者模式加载）

1. 打开 `chrome://extensions`（Edge 为 `edge://extensions`）
2. 右上角打开 **开发者模式**
3. 点击 **加载已解压的扩展程序**，选择本目录（`tabflow/extension`）
4. 点击工具栏中的 TabFlow 图标，打开配对弹窗

## 配对

1. 启动 TabFlow 桌面应用，在「概览」页找到扩展状态区域
2. 点击 **复制 Token**
3. 粘贴到扩展弹窗的 Token 输入框，点击 **保存**
4. 状态显示「已连接 TabFlow ✓」即完成；之后无需再次操作

Token 每次桌面应用重启后会重新生成，需要重新粘贴一次。
（后续版本可改为持久化 Token。）

## Firefox 说明

本扩展为 Chromium MV3 编写。Firefox 不支持 `background.service_worker`，
需把 manifest 的 background 改为 `{ "scripts": ["background.js"] }`（事件页），
其余代码无需改动。

## 故障排查

- **一直「未连接」**：确认 TabFlow 桌面应用正在运行（它监听
  `127.0.0.1:19876`）；确认 Token 与概览页显示的一致。
- **重启桌面应用后失效**：Token 已轮换，重新复制粘贴一次。
- **标签列表不更新**：扩展每 3 秒自动重连、每分钟自检一次；也可在
  弹窗里点「重新连接」。
