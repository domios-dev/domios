param(
    [Parameter(Mandatory)][string]$RunDirectory,
    [Parameter(Mandatory)][string]$OutputPath,
    [switch]$Verify
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class SpikeProcessTree {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct Entry {
        public uint size, usage, pid;
        public IntPtr heap;
        public uint module, threads, parent;
        public int priority;
        public uint flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string name;
    }
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr handle, ref Entry entry);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr handle, ref Entry entry);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static Entry[] Snapshot() {
        var handle = CreateToolhelp32Snapshot(2, 0);
        if (handle == new IntPtr(-1)) throw new System.ComponentModel.Win32Exception();
        try {
            var list = new List<Entry>();
            var entry = new Entry { size = (uint)Marshal.SizeOf<Entry>() };
            if (!Process32FirstW(handle, ref entry)) throw new System.ComponentModel.Win32Exception();
            do { list.Add(entry); } while (Process32NextW(handle, ref entry));
            return list.ToArray();
        } finally { CloseHandle(handle); }
    }
}
"@
$runPath = [IO.Path]::GetFullPath($RunDirectory)
$rootPid = [int](Get-Content -LiteralPath (Join-Path $runPath 'launcher-pid'))
$previous = if (Test-Path -LiteralPath $OutputPath) { Get-Content -LiteralPath $OutputPath -Raw | ConvertFrom-Json } else { $null }
$known = [Collections.Generic.HashSet[int]]::new()
[void]$known.Add($rootPid)
foreach ($entry in $previous.observed) { [void]$known.Add([int]$entry.pid) }
$snapshot = [SpikeProcessTree]::Snapshot()
$reused = @()
foreach ($entry in @($previous.observed | Group-Object pid)) {
    $original = $entry.Group | Sort-Object { ([DateTime]$_.started).ToUniversalTime() } | Select-Object -First 1
    $current = Get-Process -Id ([int]$original.pid) -ErrorAction SilentlyContinue
    if ($current -and $current.StartTime.ToUniversalTime().Ticks -ne ([DateTime]$original.started).ToUniversalTime().Ticks) {
        [void]$known.Remove([int]$original.pid)
        $reused += [pscustomobject]@{ pid = $original.pid; originalStarted = $original.started; currentStarted = $current.StartTime.ToUniversalTime().ToString('o') }
    }
}
$changed = $true
while ($changed) {
    $changed = $false
    foreach ($entry in $snapshot) {
        if ($known.Contains([int]$entry.parent) -and $known.Add([int]$entry.pid)) { $changed = $true }
    }
}
$live = @($snapshot | Where-Object { $known.Contains([int]$_.pid) } | ForEach-Object {
    $processValue = Get-Process -Id $_.pid -ErrorAction SilentlyContinue
    if (!$processValue) { return }
    [pscustomobject]@{ pid = $_.pid; parent = $_.parent; name = $_.name; started = $processValue.StartTime.ToUniversalTime().ToString('o') }
})
$history = @($previous.observed) + $live | Where-Object { $_ } | Sort-Object pid, started -Unique
$result = [pscustomobject]@{
    rootPid = $rootPid
    at = [DateTime]::UtcNow.ToString('o')
    method = 'Native Toolhelp parent closure at explicit test phases and after teardown'
    observed = @($history)
    reusedPidsExcluded = $reused
    live = $live
    verified = [bool]$Verify
    pass = ($Verify -and $live.Count -eq 0)
}
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $OutputPath -Encoding utf8
$result | Select-Object rootPid, at, verified, pass, @{Name='liveCount';Expression={$_.live.Count}}
if ($Verify -and !$result.pass) { exit 1 }
