param(
    [Parameter(Mandatory=$true)]
    [ValidateSet('claude','grok','aider','opencode','codex')]
    [string]$Provider,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference='Stop'
$taskOut=[IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $taskOut | Out-Null
$taskCwd=Join-Path 'C:/Developer/codeterm-native-baseline' ([IO.Path]::GetFileName([IO.Path]::GetDirectoryName($taskOut))+'-'+$Provider)
New-Item -ItemType Directory -Force -Path $taskCwd | Out-Null
Get-ChildItem Env:CODETERM* | Remove-Item
Remove-Item Env:CLAUDECODE,Env:TERM,Env:TERM_PROGRAM,Env:COLORTERM -ErrorAction SilentlyContinue
Set-Location $taskCwd
$Host.UI.RawUI.WindowTitle='Nativebaseline'+$Provider
$taskCommand=Get-Command $Provider -ErrorAction Stop
$taskMarker='NATIVE_'+$Provider+'_'+[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$taskRecord=@{provider=$Provider; shell_pid=$PID; cwd=$taskCwd; executable=$taskCommand.Source; token=$taskMarker; utc=[DateTime]::UtcNow.ToString('o'); timestamp=[Diagnostics.Stopwatch]::GetTimestamp(); frequency=[Diagnostics.Stopwatch]::Frequency; domios_environment_count=@(Get-ChildItem Env:CODETERM*).Count; mode='independent interactive console; existing provider user configuration/authentication retained'}
$taskRecord | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOut 'launch.json') -Encoding UTF8
Write-Host ('START '+$taskRecord.utc+' '+$taskMarker)
$taskPrompt='Reply with exactly '+$taskMarker+' and nothing else. Do not run tools or change files.'
switch($Provider){
 'claude' { & $taskCommand.Source --debug-file (Join-Path $taskOut 'provider-debug.log') $taskPrompt }
 'grok' { & $taskCommand.Source --debug-file (Join-Path $taskOut 'provider-debug.log') $taskPrompt }
 'aider' { & $taskCommand.Source --no-git }
 'opencode' { & $taskCommand.Source --print-logs --log-level DEBUG }
 'codex' { & $taskCommand.Source --no-daemon $taskPrompt }
}
@{utc=[DateTime]::UtcNow.ToString('o');exit_code=$LASTEXITCODE} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOut 'exit.json') -Encoding UTF8
