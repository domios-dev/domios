param([Parameter(Mandatory=$true)][long]$WindowHandle,[Parameter(Mandatory=$true)][string]$OutputFile)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;using System.Runtime.InteropServices;
public static class NativeCapture {
 [StructLayout(LayoutKind.Sequential)]public struct RECT {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("dwmapi.dll")]public static extern int DwmFlush();
 [DllImport("user32.dll")]public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
 [DllImport("user32.dll")]public static extern int GetSystemMetrics(int index);
}
"@
$taskHandle=[IntPtr]$WindowHandle
[NativeCapture]::ShowWindow($taskHandle,9) | Out-Null
[NativeCapture]::SetWindowPos($taskHandle,[IntPtr]::Zero,20,20,([NativeCapture]::GetSystemMetrics(0)-40),([NativeCapture]::GetSystemMetrics(1)-80),4) | Out-Null
[NativeCapture]::SetForegroundWindow($taskHandle) | Out-Null
[NativeCapture]::DwmFlush() | Out-Null
if([NativeCapture]::GetForegroundWindow() -ne $taskHandle){throw 'Expected baseline window is not foreground'}
$taskRect=New-Object NativeCapture+RECT
if(-not [NativeCapture]::GetWindowRect($taskHandle,[ref]$taskRect)){throw 'Window bounds unavailable'}
$taskBitmap=New-Object Drawing.Bitmap ($taskRect.Right-$taskRect.Left),($taskRect.Bottom-$taskRect.Top)
$taskGraphics=[Drawing.Graphics]::FromImage($taskBitmap)
try{
 $taskGraphics.CopyFromScreen($taskRect.Left,$taskRect.Top,0,0,$taskBitmap.Size)
 $taskBitmap.Save($OutputFile)
}finally{$taskGraphics.Dispose();$taskBitmap.Dispose()}
