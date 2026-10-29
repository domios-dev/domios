param([Parameter(Mandatory = $true)][string] $Path)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class DevLaunchFileLock
{
    [StructLayout(LayoutKind.Sequential)]
    struct Overlapped
    {
        public UIntPtr Internal, InternalHigh;
        public uint Offset, OffsetHigh;
        public IntPtr Event;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool LockFileEx(SafeFileHandle file, uint flags, uint reserved,
        uint low, uint high, ref Overlapped overlapped);

    public static void Hold(string path)
    {
        using (var file = new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.ReadWrite))
        {
            var overlapped = new Overlapped();
            if (!LockFileEx(file.SafeFileHandle, 3, 0, 1, 0, ref overlapped))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "DEV launch owner unavailable: another launcher owns this instance");
            Console.WriteLine("locked");
            Console.Out.Flush();
            Console.In.ReadToEnd();
        }
    }
}
'@
[DevLaunchFileLock]::Hold($Path)
