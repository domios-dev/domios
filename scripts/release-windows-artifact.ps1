# Caller: scripts release-bundle-smoke.mjs
param([string]$App, [string]$Cli, [string]$Embed, [string]$Version)
$ErrorActionPreference = 'Stop'
$productVersion = (Get-Item -LiteralPath $App).VersionInfo.ProductVersion
if (($productVersion -split '\+')[0] -ne $Version) { throw "Bundle version $productVersion != $Version" }
$statuses = @($App, $Cli, $Embed) | ForEach-Object {
    Get-AuthenticodeSignature -LiteralPath $_ | Select-Object Path, Status
} | ConvertTo-Json -Compress
Write-Output $statuses
