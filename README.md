# Look（录刻）

轻量级 Windows 截图 / GIF 录屏工具。Tauri 2 + React 19 + 原生 Win32，无外部依赖（不需要 ffmpeg）。

## 能做什么

- **录屏默认 GIF**：15 fps、流式编码、内存恒定；录制画面内自带鼠标指针、点击涟漪、键盘键帽 HUD
- **录完自动进剪贴板**：一次写入 `GIF`（动画）+ `CF_DIB`（首帧位图）+ `CF_HDROP` 文件，微信 / 飞书 / QQ / 画图按“图片粘贴”也能贴
- **截图带标注**：矩形、椭圆、箭头、画笔、荧光笔、文字、序号、马赛克，支持撤销 / 重做 / 改色 / 改线宽
- **统一框选入口**：一个浮层同时管截图与录屏，`Tab` 就地切换、选区保留
- **浮层内快捷键**：`Enter` 复制 / 录制，`C` 复制、`S` 复制并保存、`P` 贴图、`A` 标注、`Tab` 切截图 / 录屏、`Esc` 退出、`Ctrl+Z` 撤销、`Ctrl+Shift+Z` 重做；标注态下单键切工具 `R` 矩形 `E` 椭圆 `X` 箭头 `B` 画笔 `H` 荧光 `T` 文字 `N` 序号 `M` 马赛克
- **文字标注**：点画布落下输入框，输入后按 `Enter` 或点「确定」提交；点画布别处（内容非空）也会自动提交
- **窗口自动吸附**：鼠标悬停即虚线高亮应用窗口，单击直接采纳该窗口矩形
- **截图即钉图**：钉在屏幕上，可拖动、滚轮缩放、右键关闭
- **全局快捷键**：`Ctrl+Alt+A` 截图、`Ctrl+Alt+R` 录屏（录制中按同一键结束并保存）
- **托盘兜底**：托盘左键单击直接截图，右键菜单为「截图 / 录屏 / 设置… / 打开输出目录 / 退出」
- **开机自启**：默认开启，写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 的 `Look` 项（debug 版不写入）；可在设置页关闭

## 目录

```
src/                      前端（React）
  overlay/CaptureOverlay.tsx   框选浮层：选区 / 吸附 / 标注 / 录制参数
  overlay/draw.ts              绘图与合成导出
  control/ControlBar.tsx       录制控制条（时间 / 帧数 / 暂停 / 停止 / 丢弃）
  pin/PinWindow.tsx            贴图窗口
  settings/Settings.tsx        设置页
  lib/ipc.ts                   Tauri 命令封装
src-tauri/src/            后端（Rust）
  win32.rs    DPI 感知、显示器枚举、BitBlt 抓屏、窗口命中、剪贴板多格式写入、排除捕获
  input.rs    WH_KEYBOARD_LL / WH_MOUSE_LL 全局钩子，事件带毫秒时间戳
  composite.rs                  光标自绘、点击涟漪、键帽 HUD、SDF 圆角矩形与文字
  encode.rs    GIF 流式编码：中位切分调色板 + 1 帧前瞻定延迟 + 帧差合并
  record.rs    录制会话状态机（抓屏 → 合成 → 编码单线程串行）
  app.rs       托盘、热键、浮层窗口、控制条、贴图窗口
  commands.rs  暴露给前端的命令
docs/设计方案.md          已评审定稿的设计方案
```

## 开发

```powershell
# 前置：Rust 1.9x（MSVC 工具链）、Node 22、WebView2
npm install
npm run build                                    # 前端产物进 dist/
cargo build --bin look                      # 需 CARGO_HOME=D:\DevTools\cargo-home
```

**注意**：`tauri.conf.json` 已**移除 `devUrl` / `beforeDevCommand`**，debug 版与 release 版都从 exe 内置的 `dist/` 加载前端，**不再需要 Vite（也不再需要 `npm run dev`）**。改完前端要重新 `npm run build` 再编译 Rust 才生效，`npm run tauri dev` 的热更新也由此失效。debug 版 exe 现在可以直接双击运行。

出正式版：

```powershell
npm run build
cargo build --release --bin look
# 产物：src-tauri\target\release\look.exe（约 4MB，单文件、免安装）
```

仓库根目录的 `Look.exe` 就是这份 release 产物的副本，双击即用；启动后在**右下角托盘**，`Ctrl+Alt+A` 截图、`Ctrl+Alt+R` 录屏。

## 排查日志

启动、exe 路径、热键触发、浮层创建、前端页面加载、指针按下、录制帧数等关键事件都会写进：

```
%APPDATA%\Look\look.log
```

release 版双击运行时没有控制台，出问题直接看这个文件（超过 512KB 自动重建）。

打包：

```powershell
npm run tauri build
```

产物在 `src-tauri/target/release/bundle/`（msi / nsis）。

## 输出位置

默认 `%USERPROFILE%\Pictures\Look`，可在设置页改。缓存的冻结帧与临时 GIF 在 `%TEMP%\Look`。

## 与设计方案的两处已知偏差

1. **抓屏用 GDI `BitBlt + CAPTUREBLT`，不是 WGC**。实现更短、兼容性够；代价是抓不到被遮挡窗口，高帧率下开销略高。WGC 留作 v2 优化项。
2. **Rust 模块为扁平单文件结构**，未采用方案 §7 的 `core/ shell/ windows/` 分层；模块职责与文档一致，只是层级更浅。

## 录制线程模型（与方案的一处实现差异）

录制线程 **detach**，不保存 `JoinHandle`：`Session` 只保留 `stop / pause / cancel` 三个原子标志，`record::request_stop()` 仅置位并立即返回，避免在 UI 线程 `join` 造成互等死锁。线程自己收尾：`SESSION.take()` → 发 `rec://saved` / `rec://state` → 系统通知 → 主线程关控制条。GUI 调试时可用 `SCREENKIT_REC_SECS` 自动到点停止。

## 调试开关（环境变量，仅开发期）

变量名沿用历史前缀 `SCREENKIT_`（改名 Look 时没动，避免破坏既有脚本，功能不受影响）。

| 变量 | 作用 |
| --- | --- |
| `SCREENKIT_MULTI=1` | 跳过单实例限制，允许并存多个实例 |
| `SCREENKIT_EXCLUDE=1` | 强制把浮层 / 控制条标记为“不可捕获”。**默认不开**：深信服 VDI 等远程画面里，被排除的窗口会整块看不见，只剩鼠标光标 |
| `SCREENKIT_HOTKEY_SHOT` / `SCREENKIT_HOTKEY_REC` | 覆盖默认快捷键 |
| `SCREENKIT_AUTOQUIT_SEC=120` | 到时自动退出，防遗留进程 |
| `SCREENKIT_SELFTEST=1` | 启动即跑一次录制自检并打印结果 |
| `SCREENKIT_REC_SECS=5` | 录到 N 秒自动停止（绕开快捷键，自检用） |

## 已知问题

- **多屏（跨屏框选）**：浮层是**一个**覆盖整个虚拟桌面的窗口，所有显示器的冻结帧拼成一张大图，所以跨屏拖选是连续的。但如果两块屏的系统缩放比不同（例如 100% + 150%），Windows 的单窗口只能取一个 DPI，另一块屏的画面会被拉伸错位；缩放比一致时正常。
- **录制期间的全局快捷键与低层输入钩子，只验证到“代码链路正确”，真人按键待复核**：自动化测试用 `SendKeys` / `keybd_event` 从外部进程注入时，录制期间热键无响应、`input_events` 恒为 0；而空闲态注入能正常触发热键。用 C# 独立程序在**同进程**内注入可正常收到钩子回调（10/10），所以更可能是本机安全软件拦截了“跨进程模拟按键”，而不是钩子写错。真人按键若也收不到，再考虑换 Raw Input。
- 光标位置由 `GetCursorPos` 实时读取，**不依赖钩子**，所以光标一定进画面；受影响的只是键帽 HUD 与点击涟漪。
- 剪贴板中的 GIF 动画在微信 / 飞书等真实应用里的粘贴效果只验证到剪贴板格式层面（`GIF` + `DeviceIndependentBitmap` + `FileDrop` 均在），未逐一实机粘贴。
- 钉图（F10）、截图“保存”分支、马赛克/文字等标注导出未逐一实跑。


## 命令一览（前端可调用）

`config_get` `config_set` `overlay_bootstrap` `frame_b64`（冻结帧 base64，避免 asset 协议污染画布）`window_at` `capture_close` `open_capture_ui` `shot_export` `pin_png` `record_begin` `record_stop`（无参，置标志）`record_cancel`（无参）`record_set_pause` `record_status` `output_dir_open` `settings_show` `app_exit`

## 调试脚本

- `tools\run-debug.ps1` — 起调试版并实时写 `logs\look.log`，脚本里也写了按键验证的三步和要看的三行日志。
- `tools\hook-probe.ps1` — 独立验证本机是否允许 WH_KEYBOARD_LL 全局钩子（同进程注入，正常应输出 `count=10`）。

