# 启动 ScreenKit（调试版）并把诊断日志写到 logs\screenkit.log
# 用法：在项目根目录执行  .\tools\run-debug.ps1
# 需要停止时：关闭托盘图标（右键退出）或用任务管理器结束 screenkit.exe

$root = Split-Path -Parent $PSScriptRoot
# 优先用 release 版（前端已打进 exe，不依赖 Vite）；没有才退回 debug 版（debug 必须先 npm run dev）
$exe  = Join-Path $root "ScreenKit.exe"
if (-not (Test-Path -LiteralPath $exe)) {
    $exe = Join-Path $root "src-tauri\target\release\screenkit.exe"
}
if (-not (Test-Path -LiteralPath $exe)) {
    $exe = Join-Path $root "src-tauri\target\debug\screenkit.exe"
    Write-Host "警告：使用 debug 版，必须先另开一个窗口跑 npm run dev，否则界面是白的。" -ForegroundColor Yellow
}
$dir  = Join-Path $root "logs"
$log  = Join-Path $dir  "screenkit.log"

if (-not (Test-Path -LiteralPath $exe)) {
    Write-Host "找不到 $exe，请先编译：" -ForegroundColor Red
    Write-Host '  $tc="C:\Users\Administrator\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin"; $env:CARGO_HOME="D:\DevTools\cargo-home"; Set-Location "src-tauri"; & "$tc\cargo.exe" build --bin screenkit'
    exit 1
}

New-Item -ItemType Directory -Force -Path $dir | Out-Null
Get-Process screenkit -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

# 浮层/控制条默认不做 WDA_EXCLUDEFROMCAPTURE（深信服 VDI 等远程画面里被排除的窗口会整块看不见）。
# 只有确实要让它们从「屏幕捕获结果」里消失时才需要 SCREENKIT_EXCLUDE=1。
# $env:SCREENKIT_REC_SECS = "15"   # 录制自动到点停止，调试时可开

Remove-Item -LiteralPath $log -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $exe -RedirectStandardError $log -RedirectStandardOutput "$log.out" -WindowStyle Normal -PassThru
Start-Sleep -Seconds 2

Write-Host "已启动 pid=$($p.Id)，日志：$log" -ForegroundColor Green
Write-Host ""
Write-Host "验证步骤：" -ForegroundColor Cyan
Write-Host "  1) Ctrl+Alt+R → 拖拽框选 → Enter 开始录制"
Write-Host "  2) 在区域里随便敲几个键、点几下鼠标（验证键帽/涟漪是否进画面）"
Write-Host "  3) 再按 Ctrl+Alt+R 停止（若无效，右键托盘 → 停止录制（保存））"
Write-Host "  4) Ctrl+V 粘出 GIF"
Write-Host ""
Write-Host "日志里要关注的三行：" -ForegroundColor Cyan
Write-Host "  全局钩子已安装 kb=... ms=...     ← 钩子装上了"
Write-Host "  frame=30 ... input_events=N     ← N 是否随你敲键盘增长"
Write-Host "  钩子命中停止热键 vk=82 ...        ← 录制中按 Ctrl+Alt+R 应该出现"
Write-Host ""
Get-Content -LiteralPath $log -Tail 6
