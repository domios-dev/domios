# Screenshot helper for CodeTerm testing
# Usage: powershell -File screenshot.ps1 [output_path]

param([string]$OutputPath = "C:\tmp\codeterm_screenshot.png")

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinAPI {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# Find CodeTerm window
$hwnd = [WinAPI]::FindWindow([NullString]::Value, "CodeTerm")
if ($hwnd -eq [IntPtr]::Zero) {
    # Try partial match
    $proc = Get-Process -Name "codeterm" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($proc) { $hwnd = $proc.MainWindowHandle }
}

if ($hwnd -eq [IntPtr]::Zero) {
    Write-Error "CodeTerm window not found"
    exit 1
}

# Bring to front
[WinAPI]::ShowWindow($hwnd, 9) # SW_RESTORE
[WinAPI]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 300

# Get window rect
$rect = New-Object WinAPI+RECT
[WinAPI]::GetWindowRect($hwnd, [ref]$rect)

$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top

# Capture
$bitmap = New-Object System.Drawing.Bitmap($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($width, $height)))
$bitmap.Save($OutputPath)
$graphics.Dispose()
$bitmap.Dispose()

Write-Output "Screenshot saved to $OutputPath (${width}x${height})"
