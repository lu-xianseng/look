# 探测 Windows 自带 OCR（Windows.Media.Ocr）在本机是否可用，以及识别速度。
# 用法：powershell.exe -NoProfile -ExecutionPolicy Bypass -File tools\ocr-probe.ps1 -Path <图片路径>
param([string]$Path = "D:\ProjectCode\recordkit\docs\ui-toolbar.png")

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

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
if ($null -eq $engine) {
  "NO_ENGINE"
  exit 1
}
"LANG=" + $engine.RecognizerLanguage.LanguageTag
foreach ($l in [Windows.Media.Ocr.OcrEngine]::AvailableRecognizerLanguages) { "AVAILABLE=" + $l.LanguageTag }

if (-not (Test-Path -LiteralPath $Path)) { "NO_IMAGE"; exit 2 }
$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($Path)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bmp = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
$res = Await ($engine.RecognizeAsync($bmp)) ([Windows.Media.Ocr.OcrResult])
$sw.Stop()
"ELAPSED_MS=" + $sw.ElapsedMilliseconds
"LINES=" + $res.Lines.Count
"TEXT_BEGIN"
$res.Text
"TEXT_END"
