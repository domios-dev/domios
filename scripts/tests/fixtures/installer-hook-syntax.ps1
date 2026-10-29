$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -ne 'Desktop' -or $PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1) {
    throw 'Installer hook regression requires Windows PowerShell 5.1.'
}
$directory = Join-Path $PSScriptRoot '../../../src-tauri/windows'
$scripts = @(Get-ChildItem -LiteralPath $directory -Filter '*.ps1' -File | Sort-Object Name)
foreach ($script in $scripts) {
    $tokens = $null
    $parseErrors = $null
    $null = [System.Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count -gt 0) { throw ($script.Name + ': ' + ($parseErrors.Message -join '; ')) }
}
@{ edition = $PSVersionTable.PSEdition; version = '5.1'; scripts = @($scripts.Name) } | ConvertTo-Json -Compress
