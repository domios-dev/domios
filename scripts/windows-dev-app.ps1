# Caller: scripts/dev-app-process.mjs; private Explorer-launched DEV helper mode.
param(
    [string] $App,
    [switch] $InteractiveDesktop,
    [string] $DesktopPipe,
    [int] $ControllerPid
)
$ErrorActionPreference = 'Stop'
try {
    Add-Type -Path @((Join-Path $PSScriptRoot 'windows-dev-app.cs'), (Join-Path $PSScriptRoot 'windows-dev-desktop.cs'))
    $powershell = Join-Path $PSHOME 'powershell.exe'
    if ($DesktopPipe) {
        if ($App -or $InteractiveDesktop -or $ControllerPid -le 0 -or $DesktopPipe -notmatch '^domios-dev-desktop-[a-f0-9]{32}$') {
            throw 'Invalid private DEV desktop operation'
        }
        exit [DevDesktop]::RunHelper($DesktopPipe, $ControllerPid, $powershell, $PSCommandPath)
    }
    if (-not $App -or $ControllerPid) { throw 'Missing finite DEV client operation' }
    if ($InteractiveDesktop) {
        $shell = New-Object -ComObject Shell.Application
        $window = 0
        $desktop = $shell.Windows().FindWindowSW(0, 0, 8, [ref] $window, 1)
        if ($null -eq $desktop) { throw 'DEV requires an interactive Explorer desktop' }
        [DevDesktop]::ValidateDesktopWindow($window)
        $application = $desktop.Document.Application
        $launch = [Action[string,string,string]] {
            param($executable, $arguments, $directory)
            $application.ShellExecute($executable, $arguments, $directory, 'open', 0)
        }
        exit [DevDesktop]::RunController($PSCommandPath, $App, $powershell, $launch)
    }
    [string[]] $appArguments = (ConvertFrom-Json -InputObject $env:CODETERM_DEV_APP_ARGS)
    if ($null -eq $appArguments) { $appArguments = [string[]] @() }
    exit [DevAppProcess]::Run($App, $appArguments)
} catch {
    [Console]::Error.WriteLine('Windows DEV desktop launch refused: ' + $_.Exception.GetBaseException().Message)
    exit 1
}
