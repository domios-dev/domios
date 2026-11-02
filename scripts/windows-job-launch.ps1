param(
    [string]$Resources,
    [string]$Client,
    [string]$Build,
    [string]$Provenance,
    [string]$Out,
    [switch]$InjectUnsafeContainment,
    [switch]$CompileOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -Path "$PSScriptRoot/windows-job-launch.cs"
if ($CompileOnly) {
    & "$PSScriptRoot/windows-job-launch-ui.ps1" -CompileOnly
    Write-Output 'Native x64 process-event fixture compiled; no product launch or acceptance checks executed.'
    exit 0
}
if ($Build -notmatch '^[0-9a-f]{40}$' -or !$Resources -or !$Client -or !$Out -or !$Provenance) {
    throw 'Supply -Resources EXACT_STAGED_DEV_ROOT -Client EXACT_DEV_CLIENT -Build FULL_SHA -Provenance SCHEMA3_DEV_RECORD -Out NEW_EVIDENCE_DIRECTORY'
}

function Assert-Proof($Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
}
function Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Normal-Path([string]$Path) {
    if ($Path.StartsWith('\\?\')) { $Path = $Path.Substring(4) }
    return [IO.Path]::GetFullPath($Path).TrimEnd('\')
}
function Same-Path([string]$Left, [string]$Right) {
    return (Normal-Path $Left).Equals((Normal-Path $Right), [StringComparison]::OrdinalIgnoreCase)
}
function Reserve-Data([string]$Instance) {
    Assert-Proof ($Instance -match '^job-(proof|ui)-[0-9a-f]{10}$') 'Unexpected owned instance slug'
    $data = Join-Path ([Environment]::GetFolderPath('UserProfile')) ".domios-dev-$Instance"
    Assert-Proof (!(Test-Path -LiteralPath $data)) 'Refuse a pre-existing DEV directory'
    [IO.Directory]::CreateDirectory($data) | Out-Null
    $ownedData.Add($data)
    Write-Evidence 'owned-data.json' $ownedData.ToArray()
    return $data
}
function Json($Value) { ConvertTo-Json -InputObject $Value -Depth 24 -Compress }
function Write-Evidence([string]$Name, $Value) {
    [IO.File]::WriteAllText((Join-Path $Out $Name), (Json $Value), [Text.UTF8Encoding]::new($false))
}
function Remove-OwnedDirectory([string]$Directory) {
    $entries = @((Get-Item -LiteralPath $Directory)) + @(Get-ChildItem -LiteralPath $Directory -Recurse -Force)
    Assert-Proof (!($entries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint })) 'Refuse recursive cleanup containing a reparse point'
    Remove-Item -LiteralPath $Directory -Recurse -Force
}
function Remove-OwnedLocks([string]$Data) {
    $registry = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codeterm-instance-lifetimes/path-owners'
    foreach ($path in @($Data, (Join-Path $Data 'config.toml'))) {
        $hash = [Security.Cryptography.SHA256]::Create()
        try {
            $bytes = [Text.Encoding]::UTF8.GetBytes("domios-path-owner-v1`0" + (Normal-Path $path).ToLowerInvariant())
            $key = [BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-', '').ToLowerInvariant()
        } finally { $hash.Dispose() }
        $lockPath = Join-Path $registry "$key.lock"
        if (Test-Path -LiteralPath $lockPath) {
            Assert-Proof (!((Get-Item -LiteralPath $lockPath).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Refuse redirected fixture ownership lock'
            $guard = [IO.File]::Open($lockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            $guard.Dispose()
            Remove-Item -LiteralPath $lockPath
            $removedLocks.Add($lockPath)
        }
    }
}
function New-Port {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    try { $listener.Start(); $port = $listener.LocalEndpoint.Port } finally { $listener.Stop() }
    Assert-Proof ($port -notin @(7685, 7686)) 'Ephemeral port overlapped a protected port'
    return $port
}
function New-Environment([string]$HomePath, [string]$Instance, [int]$Port) {
    $values = [Collections.Generic.SortedDictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in [Environment]::GetEnvironmentVariables().GetEnumerator()) {
        if ($entry.Key -notmatch '^(CODETERM_|DOMIOS_|CT_|TAURI_|WEBVIEW2_)') { $values[$entry.Key] = $entry.Value }
    }
    foreach ($directory in @('tmp', 'AppData/Local', 'AppData/Roaming')) {
        [IO.Directory]::CreateDirectory((Join-Path $HomePath $directory)) | Out-Null
    }
    $values['HOME'] = $HomePath
    $values['USERPROFILE'] = $HomePath
    $values['APPDATA'] = Join-Path $HomePath 'AppData/Roaming'
    $values['LOCALAPPDATA'] = Join-Path $HomePath 'AppData/Local'
    $values['TMP'] = Join-Path $HomePath 'tmp'
    $values['TEMP'] = $values['TMP']
    $values['CODETERM_MACHINE_OWNER'] = 'false'
    $values['CODETERM_DEV_INSTANCE'] = $Instance
    $values['CODETERM_DEV_API_PORT'] = "$Port"
    $values['CODETERM_VITE_PORT'] = "$(New-Port)"
    $values['CODETERM_CDP_PORT'] = "$(New-Port)"
    $values['CODETERM_DEV_IDENTIFIER'] = "com.domios.dev.$Instance"
    return $values
}
function Environment-Block($Values) {
    $entries = foreach ($entry in $Values.GetEnumerator()) { "$($entry.Key)=$($entry.Value)" }
    return ($entries -join "`0") + "`0`0"
}
function Start-Owned([string]$Executable, [string[]]$Arguments, $Environment, [long]$Flags) {
    Assert-Proof ($globalClock.ElapsedMilliseconds -lt 300000) 'Fixture global five-minute deadline'
    $remaining = [Math]::Min(90000, 300000 - $globalClock.ElapsedMilliseconds)
    try { return [JobLaunchProof.NativeTree]::new($Executable, $Arguments, (Environment-Block $Environment), $Resources, $Flags, $remaining) }
    catch { $cleanupState.clean = $false; throw }
}
function Stop-Owned($Tree) {
    try { $Tree.Dispose() } catch { $cleanupState.clean = $false; throw }
    $processCleanup.Add(@{ processes = $Tree.ProcessEvidence; exits = $Tree.ExitEvidence; all_pinned_processes_exited = $true })
    Write-Evidence 'process-cleanup.json' $processCleanup.ToArray()
}
function Request($Ready, [string]$Operation) {
    return @{ protocol = $Ready.protocol; api_version = $Ready.api_version; build = $Ready.build;
        identity = $Ready.identity; challenge = [Guid]::NewGuid().ToString(); operation = $Operation;
        expected = @{ process = $Ready.process; epoch = $Ready.epoch } }
}
function Attach($Ready, [string]$Operation, $Environment) {
    $helper = Start-Owned $server @('attach') $Environment -1
    try {
        $helper.Send((Json (Request $Ready $Operation)))
        $helper.CloseInput()
        $reply = $helper.Line(35000) | ConvertFrom-Json
        Assert-Proof ($helper.Exit($helper.Pid, 5000) -eq 0) 'Exact bootstrap operation failed'
        Assert-Proof ($reply.ready.epoch -eq $Ready.epoch) 'Bootstrap epoch changed'
        return $reply
    } finally { Stop-Owned $helper }
}

$Resources = (Resolve-Path -LiteralPath $Resources).Path
$Client = (Resolve-Path -LiteralPath $Client).Path
$manifest = Get-Content -LiteralPath (Join-Path $Resources 'server-component.json') -Raw | ConvertFrom-Json
$server = Join-Path $Resources $manifest.server_executable
$server = Normal-Path $server
$cli = Join-Path $Resources 'cli/codeterm.exe'
$provenanceRecord = Get-Content -LiteralPath $Provenance -Raw | ConvertFrom-Json
Assert-Proof ($provenanceRecord.schema -eq 3 -and $provenanceRecord.buildSha -eq $Build) 'Require the actual schema3 DEV candidate record'
Assert-Proof ((Split-Path -Leaf $Client) -eq 'domios-dev-app.exe' -and $provenanceRecord.instance.identifier.StartsWith('com.domios.dev.')) 'Refuse an artifact without named DEV provenance'
Assert-Proof ((Same-Path $provenanceRecord.artifacts.app.path $Client) -and (Hash $Client) -eq $provenanceRecord.artifacts.app.sha256) 'DEV client provenance differs'
Assert-Proof ((Same-Path $provenanceRecord.artifacts.server.path $server) -and (Hash $server) -eq $provenanceRecord.artifacts.server.sha256) 'DEV server provenance differs'
Assert-Proof ((Hash $cli) -eq $provenanceRecord.artifacts.cli.sha256) 'DEV bundled CLI provenance differs'
Assert-Proof ($manifest.schema -eq 2 -and $manifest.server_build -eq $Build) 'Candidate manifest schema/build mismatch'
Assert-Proof ($manifest.client_identity.kind -eq 'sha256') 'Require a paired Windows DEV resource inventory'
Assert-Proof ((Hash $Client) -eq $manifest.client_identity.sha256) 'Exact candidate client bytes differ'
Assert-Proof ((Hash $server) -eq $manifest.server_sha256) 'Exact candidate server bytes differ'
Assert-Proof (Same-Path (Split-Path -Parent $Client) $Resources) 'Use the staged Windows client beside its actual resources'
Assert-Proof (!(Test-Path -LiteralPath $Out)) 'Evidence directory must be new'
[IO.Directory]::CreateDirectory($Out) | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('domios-job-proof-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
$globalClock = [Diagnostics.Stopwatch]::StartNew()
$cause = 'JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE'
$rows = [Collections.Generic.List[object]]::new()
$ownedData = [Collections.Generic.List[string]]::new()
$ownedNativeData = [Collections.Generic.List[string]]::new()
$removedLocks = [Collections.Generic.List[string]]::new()
$processCleanup = [Collections.Generic.List[object]]::new()
Write-Evidence 'candidate.json' @{ build = $Build; os = [Environment]::OSVersion.VersionString;
    server = $server; server_sha256 = Hash $server; client = $Client; client_sha256 = Hash $Client;
    cli = $cli; cli_sha256 = Hash $cli; resources = $Resources; scratch = $scratch;
    provenance = (Resolve-Path -LiteralPath $Provenance).Path; provenance_sha256 = Hash $Provenance;
    observation = 'DEBUG_PROCESS process events; suspended assignment; synchronous private pipes';
    unverified_platforms = @('macOS', 'Linux') }
$cleanupState = [PSCustomObject]@{ clean = $true }
try {
    foreach ($case in @(@{ name = 'outside'; flags = -1; inherited = $false },
        @{ name = 'inherited-refused-breakaway'; flags = $(if ($InjectUnsafeContainment) { 8192 } else { 0 }); inherited = $true },
        @{ name = 'unsafe'; flags = 8192; inherited = $true })) {
        $instance = 'job-proof-' + [Guid]::NewGuid().ToString('N').Substring(0, 10)
        $homePath = Join-Path $scratch $case.name
        $data = Reserve-Data $instance
        $port = New-Port
        $environment = New-Environment $homePath $instance $port
        $launch = @{ instance = @{ kind = 'development'; instance = $instance; port = $port }; resources = $Resources }
        $tree = Start-Owned $server @('launch', '--hold') $environment $case.flags
        $ready = $null
        try {
            Assert-Proof ($tree.InJob($tree.Pid) -eq $case.inherited) 'Launcher actual membership differs'
            $tree.Send((Json $launch))
            if ($case.name -eq 'unsafe') {
                $tree.CloseInput()
                $exit = $tree.Exit($tree.Pid, 35000)
                $tree.Drain(5000)
                $diagnostic = $tree.ErrorText()
                Assert-Proof ($exit -ne 0 -and $diagnostic.Contains($cause)) 'Unsafe job did not refuse with the OS containment cause'
                $rows.Add(@{ case = $case.name; flags = $tree.JobFlags; exit = $exit; refusal = $diagnostic })
            } else {
                $reply = $tree.Line(35000) | ConvertFrom-Json
                $ready = $reply.ready
                Assert-Proof ($ready.build -eq $Build) 'Bootstrap candidate SHA mismatch'
                Assert-Proof (Same-Path $ready.process.executable $server) 'Bootstrap returned a foreign executable'
                Assert-Proof (($tree.Identity($ready.process.pid) -split "`t")[1] -eq $ready.process.created) 'Bootstrap process creation identity differs from its OS event handle'
                Assert-Proof ($tree.InJob($ready.process.pid) -eq $case.inherited) 'Real server inherited unexpected containment'
                Assert-Proof (Same-Path $ready.identity.data $data) 'Server data differs from the reserved named DEV root'
                Assert-Proof (Same-Path $ready.identity.config (Join-Path $data 'config.toml')) 'Server config differs from the reserved DEV file'
                $tree.CloseInput()
                Assert-Proof ($tree.Exit($tree.Pid, 5000) -eq 0) 'Production launcher failed to close'
                Assert-Proof ($tree.Alive($ready.process.pid)) 'Server died with its launcher'
                $inspection = Attach $ready 'inspect' $environment
                $environment['CODETERM_DATA_DIR'] = $ready.identity.data
                $environment['CODETERM_API_URL'] = "http://127.0.0.1:$port"
                $environment['CODETERM_API_PORT'] = "$port"
                $environment['CODETERM_SOCKET_PATH'] = $ready.socket_address
                $environment['CODETERM_INSTANCE'] = $instance
                $environment['CODETERM_API_TOKEN'] = $inspection.api_token
                $seconds = [Math]::Min(30, [Math]::Floor((300000 - $globalClock.ElapsedMilliseconds) / 1000))
                Assert-Proof ($seconds -gt 0) 'Fixture global deadline before startup events'
                $waiter = Start-Owned (Get-Command node.exe).Source @((Join-Path $PSScriptRoot 'server-readiness.mjs'), 'wait', "http://127.0.0.1:$port", "$seconds") $environment -1
                try { Assert-Proof ($waiter.Exit($waiter.Pid, 35000) -eq 0) 'Existing startup-event readiness oracle failed' }
                finally { Stop-Owned $waiter }
                $health = Invoke-RestMethod -Uri "http://127.0.0.1:$port/api/health" -TimeoutSec 5
                Assert-Proof ($health.status -eq 'ok' -and $health.startup.state -eq 'ready') 'Post-launcher health is not ready'
                Assert-Proof ($health.build_sha -eq $Build -and $health.instance.name -eq $instance) 'Post-launcher health SHA/instance mismatch'
                $cliTree = Start-Owned $cli @('identify', '--json') $environment -1
                try {
                    $identified = $cliTree.Line(10000) | ConvertFrom-Json
                    Assert-Proof ($cliTree.Exit($cliTree.Pid, 5000) -eq 0 -and $identified.ok) 'Candidate CLI could not identify the isolated server'
                    Assert-Proof ($identified.result.cli.build_sha -eq $Build -and $identified.result.connection.instance -eq $instance) 'CLI SHA/instance mismatch'
                } finally { Stop-Owned $cliTree }
                $rows.Add(@{ case = $case.name; flags = $tree.JobFlags; launcher_exit = 0;
                    server = $tree.Identity($ready.process.pid); server_in_job = $tree.InJob($ready.process.pid);
                    build = $health.build_sha; epoch = $ready.epoch; cli_exit = 0;
                    fallback = ($case.flags -eq 0); ready = $ready })
                Attach $ready 'shutdown' $environment | Out-Null
                Assert-Proof ($tree.Exit($ready.process.pid, 10000) -eq 0) 'Server did not exit cleanly'
                $ready = $null
            }
        } finally {
            $processes = $tree.ProcessEvidence
            Stop-Owned $tree
            Write-Evidence ($case.name + '-processes.json') @{ processes = $processes; exits = $tree.ExitEvidence; all_pinned_processes_exited = $true }
            Write-Evidence 'rows.json' @($rows.ToArray())
        }
    }
    Assert-Proof (!$InjectUnsafeContainment) 'Controlled unsafe containment unexpectedly reached the success oracle'
    & "$PSScriptRoot/windows-job-launch-ui.ps1" -Client $Client -Resources $Resources -Scratch $scratch -Out $Out -Cause $cause
} catch {
    Write-Evidence 'failure.json' @{ status = 'FAIL'; error = $_.Exception.Message; build = $Build; scratch = $scratch; rows = $rows.ToArray() }
    throw
} finally {
    if ($cleanupState.clean) {
        foreach ($directory in $ownedData) {
            $parent = [Environment]::GetFolderPath('UserProfile')
            Assert-Proof (Same-Path (Split-Path -Parent $directory) $parent) 'Named DEV cleanup escaped the known user root'
            Assert-Proof ((Split-Path -Leaf $directory) -match '^\.domios-dev-job-(proof|ui)-[0-9a-f]{10}$') 'Refuse unowned DEV directory cleanup'
            Remove-OwnedDirectory $directory
            Remove-OwnedLocks $directory
        }
        foreach ($directory in $ownedNativeData) {
            $parent = Split-Path -Parent $directory
            Assert-Proof ((Same-Path $parent ([Environment]::GetFolderPath('LocalApplicationData'))) -or (Same-Path $parent ([Environment]::GetFolderPath('ApplicationData')))) 'Native profile cleanup escaped the known app-data roots'
            Assert-Proof ((Split-Path -Leaf $directory) -match '^com\.domios\.dev\.job-ui-[0-9a-f]{10}$') 'Refuse unowned native profile cleanup'
            Remove-OwnedDirectory $directory
        }
        $resolved = [IO.Path]::GetFullPath($scratch)
        $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
        Assert-Proof ($resolved.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolved).StartsWith('domios-job-proof-')) 'Cleanup escaped the owned temp root'
        Remove-OwnedDirectory $resolved
        Write-Evidence 'data-cleanup.json' @{ owned_directories_removed = $ownedData.ToArray(); native_profiles_removed = $ownedNativeData.ToArray(); ownership_locks_removed = $removedLocks.ToArray(); scratch_removed = $scratch }
    }
}
Write-Evidence 'result.json' @{ status = 'PASS'; build = $Build; rows = $rows.ToArray(); native_ui = 'native-ui.json'; cleanup = 'all pinned processes exited and owned data removed' }
