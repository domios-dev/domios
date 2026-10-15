# Caller: scripts/dev-app-process.mjs
param([Parameter(Mandatory = $true)][string] $App)
$ErrorActionPreference = 'Stop'
Add-Type -Path (Join-Path $PSScriptRoot 'windows-dev-app.cs')
[string[]] $appArguments = (ConvertFrom-Json -InputObject $env:CODETERM_DEV_APP_ARGS)
if ($null -eq $appArguments) { $appArguments = [string[]] @() }
exit [DevAppProcess]::Run($App, $appArguments)

