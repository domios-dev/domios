param([string]$Out = "C:\tmp\ct.png")
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class ScreenCap {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$p = Get-Process codeterm -EA SilentlyContinue | Select -First 1
if (-not $p) { Write-Error "no codeterm"; exit 1 }

[ScreenCap]::ShowWindow($p.MainWindowHandle, 9)
[ScreenCap]::SetForegroundWindow($p.MainWindowHandle)
Start-Sleep -Milliseconds 500

# Use DWM extended frame bounds for accurate window rect (no shadows/borders)
$r = New-Object ScreenCap+RECT
$hr = [ScreenCap]::DwmGetWindowAttribute($p.MainWindowHandle, 9, [ref]$r, [Runtime.InteropServices.Marshal]::SizeOf($r))
if ($hr -ne 0) {
    # Fallback to GetWindowRect
    [ScreenCap]::GetWindowRect($p.MainWindowHandle, [ref]$r)
}

$w = $r.Right - $r.Left
$h = $r.Bottom - $r.Top
Write-Output "Window: ${w}x${h} at ($($r.Left),$($r.Top))"

$bmp = New-Object Drawing.Bitmap($w, $h)
$g = [Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object Drawing.Size($w, $h)))
$bmp.Save($Out)
$g.Dispose()
$bmp.Dispose()
Write-Output "Saved to $Out"
