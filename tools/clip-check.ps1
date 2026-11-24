Add-Type -AssemblyName System.Windows.Forms
$o = [System.Windows.Forms.Clipboard]::GetDataObject()
if (-not $o) { 'empty'; exit }
'formats: ' + ($o.GetFormats() -join ', ')
$i = [System.Windows.Forms.Clipboard]::GetImage()
if ($i) {
    'image: ' + $i.Width + 'x' + $i.Height
    $i.Save('D:\ProjectCode\recordkit\clip-out.png')
    'saved: D:\ProjectCode\recordkit\clip-out.png'
} else {
    'no image'
}
