# Look 的文字识别：调用 Windows 自带 OCR（Windows.Media.Ocr），本机无需联网、无需额外安装。
# 识别结果写入 -Out 指定的 UTF-8 文本文件；失败时写入 "ERR: 原因" 并返回退出码 1。
# 调用方（Look.exe）会把本脚本释放到缓存目录后用隐藏窗口跑，所以这里不打印任何东西到 stdout。
param(
  [Parameter(Mandatory = $true)][string]$Path,
  [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding($false)

try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime
  # PowerShell 5 不会自动解析 WinRT 类型，必须显式带上 ContentType=WindowsRuntime 预加载
  [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
  [Windows.Media.Ocr.OcrResult, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
  [Windows.Storage.StorageFile, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
  [Windows.Storage.FileAccessMode, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
  [Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null
  [Windows.Graphics.Imaging.SoftwareBitmap, Windows.Foundation, ContentType = WindowsRuntime] | Out-Null

  $asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
      $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
    })[0]

  function Await($op, $type) {
    $asTask = $asTaskGeneric.MakeGenericMethod($type)
    $task = $asTask.Invoke($null, @($op))
    $task.Wait(-1) | Out-Null
    $task.Result
  }

  $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
  if ($null -eq $engine) {
    # 用户配置语言里没有可用 OCR 语言包，退回到已安装的第一个识别语言
    $langs = [Windows.Media.Ocr.OcrEngine]::AvailableRecognizerLanguages
    if ($langs.Count -eq 0) {
      [IO.File]::WriteAllText($Out, 'ERR:系统未安装 OCR 语言包', $utf8)
      exit 1
    }
    $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($langs[0])
  }
  if ($null -eq $engine) {
    [IO.File]::WriteAllText($Out, 'ERR:OCR 引擎创建失败', $utf8)
    exit 1
  }

  $file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($Path)) ([Windows.Storage.StorageFile])
  $stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
  $decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
  $bmp = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
  $res = Await ($engine.RecognizeAsync($bmp)) ([Windows.Media.Ocr.OcrResult])

  $lines = @()
  foreach ($l in $res.Lines) { $lines += $l.Text }
  [IO.File]::WriteAllText($Out, ($lines -join "`r`n"), $utf8)
  $stream.Dispose()
  exit 0
}
catch {
  [IO.File]::WriteAllText($Out, ('ERR:' + $_.Exception.Message), $utf8)
  exit 1
}
