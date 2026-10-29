$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../../../src-tauri/windows/installer-process-model.ps1"
$server = 'C:\Domios\domios-server\domios-server.exe'
$console = 'C:\Domios\conpty\x64\OpenConsole.exe'
$images = @($server, $console)
$table = @{
    42 = @{ Created = 100L; ParentPid = 1; Image = $server }
    43 = @{ Created = 200L; ParentPid = 42; Image = $server }
    44 = @{ Created = 300L; ParentPid = 43; Image = $console }
}
function Assert-Owned([bool]$Expected, [int]$SubjectPid, [long]$Created) {
    $actual = Test-InstalledDescendant -Table $table -Images $images -RootPid 42 -RootCreated 100 -SubjectPid $SubjectPid -SubjectCreated $Created
    if ($actual -ne $Expected) { throw "Ownership mismatch: $SubjectPid" }
}
Assert-Owned $true 42 100
Assert-Owned $true 43 200
Assert-Owned $true 44 300
Assert-Owned $false 44 301
$table[42].Created = 101L
Assert-Owned $false 44 300
$table[42].Created = 100L
$table[43].Created = 400L
Assert-Owned $false 44 300
$table[43].Created = 200L
$table[43].Image = 'C:\Developer\Domios\target\debug\domios-server.exe'
Assert-Owned $false 43 200
Assert-Owned $false 44 300
$table[43].Image = $server
$table[43].ParentPid = 44
Assert-Owned $false 44 300
$table.Remove(43)
Assert-Owned $false 44 300
if ((Get-InstalledImagePath '\\?\C:\Domios\domios-server\domios-server.exe') -cne $server) { throw 'Extended image path mismatch' }
foreach ($outcome in @('completed', 'uncertain')) {
    if (-not (Test-InstallerCanStopTree $true $outcome)) { throw 'Saved restore state should permit owned cleanup' }
    if (Test-InstallerCanStopTree $false $outcome) { throw 'Missing restore state must prevent owned cleanup' }
}
if (Test-InstallerCanStopTree $true 'refused') { throw 'A graceful quit refusal must be honored' }
if (Test-InstallerCanStopTree $true 'invalid') { throw 'Unknown quit outcomes must be refused' }
if (Test-InstallerCanStopTree $true 'transport_error') { throw 'Missing quit outcomes must be refused' }
Write-Output '{"assertions":18}'
