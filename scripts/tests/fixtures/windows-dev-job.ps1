param([string] $Root, [uint32] $Flags)
$ErrorActionPreference = 'Stop'
Add-Type -Path @((Join-Path $Root 'scripts/windows-dev-app.cs'), (Join-Path $Root 'scripts/windows-dev-desktop.cs'))
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DevJobFixture {
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateJobObjectW(IntPtr attributes, IntPtr name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int kind, IntPtr data, uint length);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    public static void Enter(uint flags) {
        IntPtr job = CreateJobObjectW(IntPtr.Zero, IntPtr.Zero);
        if (job == IntPtr.Zero) throw new InvalidOperationException("CreateJobObject failed");
        int size = IntPtr.Size == 8 ? 144 : 112;
        IntPtr limits = Marshal.AllocHGlobal(size);
        try {
            Marshal.Copy(new byte[size], 0, limits, size);
            Marshal.WriteInt32(limits, 16, unchecked((int)flags));
            if (!SetInformationJobObject(job, 9, limits, (uint)size) || !AssignProcessToJobObject(job, GetCurrentProcess()))
                throw new InvalidOperationException("Fixture job assignment failed: " + Marshal.GetLastWin32Error());
        } finally { Marshal.FreeHGlobal(limits); }
    }
}
'@
[DevJobFixture]::Enter($Flags)
$method = [DevDesktop].GetMethod('CurrentJobSurvivesLauncher', [Reflection.BindingFlags]'NonPublic,Static')
$method.Invoke($null, @())
