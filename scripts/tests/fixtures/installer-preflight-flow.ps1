$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../../../src-tauri/windows/installer-preflight-flow.ps1"
$root = 'C:\Domios'
$server = "$root\domios-server\domios-server.exe"
$console = "$root\conpty\x64\OpenConsole.exe"
$desktop = "$root\domios-app.exe"
$records = @(
    @{ Pid = 42; Created = 100L; ParentPid = 1; Image = $server },
    @{ Pid = 43; Created = 200L; ParentPid = 42; Image = $server },
    @{ Pid = 44; Created = 300L; ParentPid = 43; Image = $console },
    @{ Pid = 45; Created = 120L; ParentPid = 1; Image = $desktop },
    @{ Pid = 99; Created = 400L; ParentPid = 43; Image = 'C:\Dev\target\debug\domios-server.exe' },
    @{ Pid = 88; Created = 400L; ParentPid = 1; Image = 'C:\Foreign\editor.exe' }
)
$table = @{}
foreach ($record in $records) { $table[$record.Pid] = $record }
$snapshot = @{
    ready = @{ process = @{ pid = 42; created = '100'; executable = $server } }
    client = @{ process = @{ pid = 45; created = '120'; executable = $desktop } }
}
$script:fixtureRoot = $root
$script:fixtureTable = $table
$script:fixtureSnapshot = $snapshot
$script:fixtureForeign = @{ 99 = $records[4]; 88 = $records[5] }
$cases = [Collections.Generic.List[string]]::new()

function Run-Case {
    param([string]$Name, [hashtable]$Options, [bool]$ExpectedSuccess, [string[]]$ExpectedCalls)
    $script:scenario = @{
        Calls = [Collections.Generic.List[string]]::new()
        Captures = 0; Options = $Options
    }
    $ports = @{
        Capture = {
            param($installRoot)
            if ($installRoot -cne $script:fixtureRoot) { throw 'Wrong install root' }
            $script:scenario.Calls.Add('capture')
            $script:scenario.Captures++
            $options = $script:scenario.Options
            if ($script:scenario.Captures -eq 1) {
                $holders = @($options.Holders)
                $captureTable = if ($options.ContainsKey('InitialTable')) { $options.InitialTable } else { $script:fixtureTable }
            } else {
                $holders = @($options.Remaining)
                $captureTable = @{}
                foreach ($record in $script:fixtureForeign.Values) { $captureTable[$record.Pid] = $record }
                if ($options.ContainsKey('RemainingTable')) {
                    foreach ($record in $options.RemainingTable.Values) { $captureTable[$record.Pid] = $record }
                }
                foreach ($holder in $holders) { $captureTable[$holder.Pid] = $holder }
            }
            return @{ Table = $captureTable; Holders = $holders }
        }
        Dispose = { param($capture) $script:scenario.Calls.Add('dispose') }
        Token = { param($installRoot) $script:scenario.Calls.Add('token'); return 'opaque-fixture-credential' }
        Snapshot = {
            param($token)
            $script:scenario.Calls.Add('snapshot')
            if ($script:scenario.Options.SnapshotError) { throw $script:scenario.Options.SnapshotError }
            if ($script:scenario.Options.NoClient) { return @{ ready = $script:fixtureSnapshot.ready; client = $null } }
            return $script:fixtureSnapshot
        }
        Save = {
            param($token)
            $script:scenario.Calls.Add('save')
            return @{ ok = -not $script:scenario.Options.SaveError }
        }
        Quit = {
            param($token, $expected)
            $script:scenario.Calls.Add('quit')
            if ($script:scenario.Options.QuitError) { throw 'Quit unavailable' }
            return @{ outcome = $(if ($script:scenario.Options.Refused) { 'refused' } else { 'completed' }) }
        }
        Terminate = { param($record) $script:scenario.Calls.Add("kill:$($record.Pid)") }
        Wait = {
            param($record, $remaining)
            $script:scenario.Calls.Add("wait:$($record.Pid)")
            return -not ($record.Pid -eq 45 -and $script:scenario.Options.DesktopWaitError)
        }
    }
    $succeeded = $false
    try {
        $succeeded = Invoke-InstallerPreflight -InstallRoot $root -Ports $ports
        if ($succeeded) { $script:scenario.Calls.Add('copy') }
    } catch {
        $succeeded = $false
    }
    if ($succeeded -ne $ExpectedSuccess) { throw "Wrong outcome: $Name" }
    $calls = $script:scenario.Calls.ToArray()
    if (($calls -join ',') -cne ($ExpectedCalls -join ',')) {
        throw "Wrong side effects for ${Name}: $($calls -join ',')"
    }
    if ($calls -contains 'kill:45' -or $calls -contains 'kill:99' -or $calls -contains 'kill:88') {
        throw 'Desktop, DEV or foreign process was terminated'
    }
    $cases.Add($Name)
}

$owned = @($records[0], $records[1], $records[2], $records[3])
$beforeSave = @('capture', 'token', 'snapshot', 'dispose')
$throughSave = @('capture', 'token', 'snapshot', 'save', 'dispose')
$throughQuit = @('capture', 'token', 'snapshot', 'save', 'quit', 'dispose')
$cleanup = @('capture', 'token', 'snapshot', 'save', 'quit', 'kill:42', 'kill:43', 'kill:44', 'wait:42', 'wait:43', 'wait:44', 'wait:45')
Run-Case 'empty' @{ InitialTable = $script:fixtureForeign; Holders = @(); Remaining = @() } $true @('capture', 'capture', 'dispose', 'dispose', 'copy')
Run-Case 'desktop_only' @{ InitialTable = @{ 45 = $records[3] }; Holders = @(); Remaining = @() } $false @('capture', 'dispose')
Run-Case 'installed_cli' @{ InitialTable = @{ 70 = @{ Pid = 70; Created = 100L; Image = "$root\cli\codeterm.exe" } }; Holders = @() } $false @('capture', 'dispose')
Run-Case 'unknown_installed_image' @{ InitialTable = @{ 71 = @{ Pid = 71; Created = 100L; Image = "$root\helper.exe" } }; Holders = @() } $false @('capture', 'dispose')
Run-Case 'external_registered_holder' @{ Holders = @($records[5]) } $false @('capture', 'dispose')
Run-Case 'unproven_registered_holder' @{ Holders = @(@{ Pid = 77; Created = 100L; Image = $null }) } $false @('capture', 'dispose')
Run-Case 'unsupported_snapshot' @{ Holders = $owned; SnapshotError = 'Endpoint not supported' } $false $beforeSave
Run-Case 'unavailable_snapshot' @{ Holders = $owned; SnapshotError = 'Snapshot unavailable' } $false $beforeSave
Run-Case 'unattached_desktop' @{ Holders = $owned; NoClient = $true } $false $beforeSave
Run-Case 'unavailable_quit' @{ Holders = $owned; QuitError = $true } $false $throughQuit
Run-Case 'save_refused' @{ Holders = $owned; SaveError = $true } $false $throughSave
Run-Case 'quit_refused' @{ Holders = $owned; Refused = $true } $false $throughQuit
Run-Case 'remaining_desktop' @{ Holders = $owned; Remaining = @(); RemainingTable = @{ 45 = $records[3] } } $false ($cleanup + @('capture', 'dispose', 'dispose'))
Run-Case 'desktop_wait_deadline' @{ Holders = $owned; DesktopWaitError = $true } $false ($cleanup + @('dispose'))
Run-Case 'exact_owned_cleanup' @{ Holders = $owned; Remaining = @() } $true ($cleanup + @('capture', 'dispose', 'dispose', 'copy'))
$foreignConsole = @{ Pid = 100; Created = 500L; ParentPid = 99; Image = $console }
$table[100] = $foreignConsole
Run-Case 'foreign_ancestor' @{ Holders = @($records[0], $foreignConsole) } $false $beforeSave
Run-Case 'empty_then_desktop' @{ InitialTable = $script:fixtureForeign; Holders = @(); Remaining = @(); RemainingTable = @{ 45 = $records[3] } } $false @('capture', 'capture', 'dispose', 'dispose')

foreach ($image in @($desktop, "$root\cli\codeterm.exe", "$root\helper.exe")) {
    if (-not (Test-InstalledBinaryImage $root $image)) { throw 'Installed image was omitted from absence detection' }
}
foreach ($image in @('C:\Dev\target\debug\domios-server.exe', 'C:\Domios-other\domios-app.exe', "$root\target\debug\domios-server.exe")) {
    if (Test-InstalledBinaryImage $root $image) { throw 'Foreign image entered the bounded install-image policy' }
}
@{ cases = $cases.ToArray() } | ConvertTo-Json -Compress
