$ErrorActionPreference = 'Stop'
$windows = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../src-tauri/windows'))
. "$windows/installer-preflight-flow.ps1"
. "$windows/installer-process-capture.ps1"
Add-Type -Path "$windows/installer-process-native.cs"
$fixture = Join-Path ([IO.Path]::GetTempPath()) ("domios-preflight-" + [Guid]::NewGuid())
$root = Join-Path $fixture 'install'
$other = Join-Path $fixture 'other-dev'
New-Item -ItemType Directory -Force "$root/domios-server", "$root/conpty/x64", $other | Out-Null
$helpers = [Collections.Generic.List[Diagnostics.Process]]::new()
$source = @'
using System;
using System.IO;
class Holder {
    static void Main(string[] args) {
        using (var file = args.Length == 0 ? null : new FileStream(args[0], FileMode.Open, FileAccess.Read, FileShare.Read)) {
            Console.WriteLine("READY"); Console.Out.Flush(); Console.ReadLine();
        }
    }
}
'@
$binary = Join-Path $fixture 'fixture.exe'
Add-Type -TypeDefinition $source -OutputAssembly $binary -OutputType ConsoleApplication
Copy-Item -LiteralPath $binary -Destination "$root/domios-server/domios-server.exe"
Copy-Item -LiteralPath $binary -Destination "$root/conpty/x64/OpenConsole.exe"
Copy-Item -LiteralPath $binary -Destination "$other/domios-server.exe"
$passed = 0
function Assert($condition, [string]$message) { if (-not $condition) { throw $message } }
function Start-Helper([string]$image, [string]$file = '') {
    $start = [Diagnostics.ProcessStartInfo]::new($image)
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true; $start.RedirectStandardOutput = $true
    if ($file) { $start.Arguments = '"' + $file + '"' }
    $process = [Diagnostics.Process]::Start($start)
    $helpers.Add($process)
    $ready = $process.StandardOutput.ReadLineAsync()
    Assert ($ready.Wait(10000) -and $ready.Result -eq 'READY') 'helper did not acknowledge readiness'
    return $process
}
function Stop-Helper($process) {
    $process.StandardInput.WriteLine('exit'); $process.StandardInput.Flush()
    Assert ($process.WaitForExit(10000)) 'helper failed to exit'
}
function Entry($process) { Get-CimInstance Win32_Process -Filter "ProcessId=$($process.Id)" }
function Ports([hashtable]$discovery) {
    return @{
        Capture = { param($directory) Get-DomiosInstallCapture $directory $discovery }.GetNewClosure()
        Dispose = { param($capture) foreach ($record in $capture.Table.Values) { $record.Process.Dispose() } }
        Wait = { param($record, $remaining) $record.Process.WaitForExit($remaining) }
        Terminate = {
            param($record)
            if (-not $record.Process.HasExited) {
                Assert (Test-DomiosInstalledImage $root $record.Image) 'termination escaped the fixture install root'
                Assert ($record.Created -eq $record.Process.StartTime.ToUniversalTime().ToFileTimeUtc()) 'identity changed'
                Assert ([DomiosInstallerProcess]::TerminateProcess($record.Process.Handle, 1) -or $record.Process.HasExited) 'termination failed'
            }
        }.GetNewClosure()
    }
}
function Pass([string]$name) { $script:passed++; Write-Output "ok $script:passed - $name" }
try {
    $process = Start-Helper "$root/domios-server/domios-server.exe"
    $entry = Entry $process
    $discovery = @{
        Entries = { Stop-Helper $process; @($entry) }.GetNewClosure()
        Registered = { @() }
    }
    $capture = Get-DomiosInstallCapture $root $discovery
    Assert ($capture.Table.Count -eq 0 -and $capture.Holders.Count -eq 0) 'exited snapshot entry retained'
    Pass 'installed process exiting after CIM snapshot does not refuse'

    $process = Start-Helper "$root/conpty/x64/OpenConsole.exe"
    $entry = Entry $process
    $registered = @([DomiosInstallerProcess]::Holders(@($process.MainModule.FileName)))
    Assert (@($registered | Where-Object Pid -eq $process.Id).Count -eq 1) 'Restart Manager did not register fixture'
    $discovery = @{
        Entries = { Stop-Helper $process; @($entry) }.GetNewClosure()
        Registered = { $registered }.GetNewClosure()
    }
    $capture = Get-DomiosInstallCapture $root $discovery
    Assert ($capture.Holders.Count -eq 0) 'exited Restart Manager holder retained'
    Pass 'Restart Manager registered holder exiting after discovery is skipped'

    $entry = @{ ProcessId = 123; ParentProcessId = 1; ExecutablePath = "$root/domios-server/domios-server.exe" }
    $discovery = @{ Entries = { @($entry) }.GetNewClosure(); Registered = { @() }; Pin = { throw 'access denied' }; State = { 'Live' } }
    $refusal = $null
    try { Get-DomiosInstallCapture $root $discovery | Out-Null } catch { $refusal = $_.Exception.Message }
    Assert ($refusal -like '*domios-server.exe (PID 123)*') 'live inaccessible holder did not refuse with identity'
    Pass 'live inaccessible installed process still refuses with image and pid'

    $discovery.State = { 'Replaced' }
    $capture = Get-DomiosInstallCapture $root $discovery
    Assert ($capture.Table.Count -eq 0) 'reused identity retained'
    Pass 'reused pid is skipped without termination'

    $process = Start-Helper "$root/domios-server/domios-server.exe"
    $entry = Entry $process
    $discovery = @{ Entries = { @($entry) }.GetNewClosure(); Registered = { @() } }
    $ports = Ports $discovery
    $originalCapture = $ports.Capture
    $ports.Capture = {
        param($directory)
        if ($process.HasExited) { return @{ Table = @{}; Holders = @() } }
        & $originalCapture $directory
    }.GetNewClosure()
    Assert (Invoke-InstallerPreflight $root $ports -GracefulMilliseconds 0 -ExitMilliseconds 10000) 'lingering own process blocked install'
    Assert ($process.HasExited) 'own process still alive after preflight'
    Pass 'lingering own helper is terminated and its pinned exit confirmed'

    $file = Join-Path $root 'fixture.dll'
    [IO.File]::WriteAllText($file, 'fixture')
    $process = Start-Helper "$other/domios-server.exe" $file
    $entry = Entry $process
    $registered = @([DomiosInstallerProcess]::Holders(@($file)))
    Assert (@($registered | Where-Object Pid -eq $process.Id).Count -eq 1) 'foreign file holder not registered'
    $discovery = @{ Entries = { @($entry) }.GetNewClosure(); Registered = { $registered }.GetNewClosure() }
    $ports = Ports $discovery
    $refusal = $null
    try { Invoke-InstallerPreflight $root $ports -GracefulMilliseconds 0 | Out-Null } catch { $refusal = $_.Exception.Message }
    Assert ($refusal -like "*Retry*$($process.Id)*" -and $refusal.Contains($other)) "foreign refusal missing Retry or holder identity: $refusal"
    Assert (-not $process.HasExited) 'foreign other-DEV helper was killed'
    $output = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$windows/installer-preflight.ps1" -InstallRoot $root -TargetVersion 'fixture-version'
    Assert ($LASTEXITCODE -eq 1) "entrypoint did not refuse foreign holder: $output"
    $refusal = Get-Content -LiteralPath (Join-Path $root '.domios-installer-refusal.json') -Raw | ConvertFrom-Json
    Assert ($refusal.target_version -eq 'fixture-version' -and $refusal.message.Contains("PID $($process.Id)")) 'refusal not retained for the update panel'
    Pass 'real preflight entrypoint retains foreign holder details for relaunch recovery'
    Stop-Helper $process
    $ports = Ports @{ Entries = { @() }; Registered = { @() } }
    Assert (Invoke-InstallerPreflight $root $ports -GracefulMilliseconds 0) 'retry did not re-discover after foreign holder exit'
    Pass 'foreign holder is named and survives refusal; retry succeeds after its exit'

    $output = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$windows/installer-preflight.ps1" -InstallRoot $root -TargetVersion 'fixture-version'
    Assert ($LASTEXITCODE -eq 0) "entrypoint failed on empty fixture root: $output"
    Assert (-not (Test-Path -LiteralPath (Join-Path $root '.domios-installer-refusal.json'))) 'successful retry retained stale refusal'
    Pass 'real installer-preflight entrypoint succeeds on isolated empty install root'
    Write-Output "Passed: $passed; failed: 0"
} finally {
    foreach ($process in $helpers) {
        try { if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() } } finally { $process.Dispose() }
    }
    $resolved = [IO.Path]::GetFullPath($fixture)
    if ($resolved.StartsWith([IO.Path]::GetTempPath(), [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf).StartsWith('domios-preflight-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
