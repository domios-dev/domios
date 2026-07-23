[CmdletBinding()]
param(
    [string]$RepoUrl = "https://github.com/rollacode/codeterm",
    [string]$RunnerName = "ROG-windows-build",
    [string]$Labels = "self-hosted,Windows,X64,rog,windows-build,codeterm,tauri,msvc",
    [string]$RunnerRoot = "$env:USERPROFILE\actions-runner",
    [string]$ToolsRoot = "$env:USERPROFILE\tools",
    [string]$WorkFolder = "_work",
    [string]$RustPinnedToolchain = "1.95.0-x86_64-pc-windows-msvc",
    [string]$PnpmVersion = "10.32.1",
    [string]$RegistrationTokenCommand,
    [switch]$ConfigureRunner,
    [switch]$Replace,
    [switch]$RunAsService,
    [switch]$InstallVisualStudioBuildTools,
    [switch]$StartListener,
    [switch]$VerifyOnly
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
    Write-Host "==> $Message"
}

function Test-Elevated {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-LatestGitHubAsset([string]$Repo, [string]$NamePattern) {
    $release = Invoke-RestMethod -Headers @{ "User-Agent" = "codeterm-runner-bootstrap" } -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $asset = $release.assets | Where-Object { $_.name -like $NamePattern } | Select-Object -First 1
    if (-not $asset) { throw "No asset matching $NamePattern in $Repo $($release.tag_name)" }
    [PSCustomObject]@{ Tag = $release.tag_name; Name = $asset.name; Url = $asset.browser_download_url }
}

function Install-ZipTool([string]$Url, [string]$ZipName, [string]$Destination, [string]$InnerDirectory) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Destination) | Out-Null
    $download = Join-Path $env:TEMP $ZipName
    $extract = Join-Path $env:TEMP ([IO.Path]::GetFileNameWithoutExtension($ZipName))
    Invoke-WebRequest -Uri $Url -OutFile $download
    if (Test-Path -LiteralPath $extract) {
        $resolved = (Resolve-Path -LiteralPath $extract).Path
        if (-not $resolved.StartsWith($env:TEMP, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove non-temp path $resolved"
        }
        Remove-Item -LiteralPath $extract -Recurse -Force
    }
    Expand-Archive -Path $download -DestinationPath $extract -Force
    $source = if ($InnerDirectory) { Join-Path $extract $InnerDirectory } else { $extract }
    if (Test-Path -LiteralPath $Destination) {
        $resolved = (Resolve-Path -LiteralPath $Destination).Path
        if (-not $resolved.StartsWith($ToolsRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to replace non-tools path $resolved"
        }
        Remove-Item -LiteralPath $Destination -Recurse -Force
    }
    Move-Item -LiteralPath $source -Destination $Destination
}

function Install-PowerShell {
    $asset = Get-LatestGitHubAsset -Repo "PowerShell/PowerShell" -NamePattern "PowerShell-*-win-x64.zip"
    $version = $asset.Tag.TrimStart("v")
    $dest = Join-Path $ToolsRoot "PowerShell\$version"
    if (-not (Test-Path -LiteralPath (Join-Path $dest "pwsh.exe"))) {
        Write-Step "Installing PowerShell $($asset.Tag)"
        Install-ZipTool -Url $asset.Url -ZipName $asset.Name -Destination $dest -InnerDirectory $null
    }
    return $dest
}

function Install-NodeLts {
    $index = Invoke-RestMethod -Uri "https://nodejs.org/dist/index.json"
    $lts = $index | Where-Object { $_.lts -ne $false -and $_.files -contains "win-x64-zip" } | Select-Object -First 1
    if (-not $lts) { throw "No supported Node LTS win-x64 zip found." }
    $version = $lts.version
    $dest = Join-Path $ToolsRoot ("Node\" + $version.TrimStart("v"))
    if (-not (Test-Path -LiteralPath (Join-Path $dest "node.exe"))) {
        Write-Step "Installing Node $version LTS $($lts.lts)"
        $zip = "node-$version-win-x64.zip"
        Install-ZipTool -Url "https://nodejs.org/dist/$version/$zip" -ZipName $zip -Destination $dest -InnerDirectory "node-$version-win-x64"
    }
    return $dest
}

function Install-Rust {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if (-not (Test-Path -LiteralPath (Join-Path $cargoBin "rustup.exe"))) {
        Write-Step "Installing rustup stable MSVC"
        $rustupInit = Join-Path $env:TEMP "rustup-init-x86_64-pc-windows-msvc.exe"
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit
        & $rustupInit -y --default-host x86_64-pc-windows-msvc --default-toolchain stable --profile default
    }
    $env:PATH = "$cargoBin;$env:PATH"
    rustup toolchain install stable-x86_64-pc-windows-msvc
    rustup toolchain install $RustPinnedToolchain --component clippy --component rustfmt --target x86_64-pc-windows-msvc --target aarch64-pc-windows-msvc
    rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc --toolchain stable-x86_64-pc-windows-msvc
    return $cargoBin
}

function Install-RustProxies([string]$CargoBin) {
    $proxyDir = Join-Path $ToolsRoot "rustup-proxies"
    $rustup = Join-Path $CargoBin "rustup.exe"
    if (-not (Test-Path -LiteralPath $rustup)) { throw "rustup.exe was not installed in $CargoBin" }
    New-Item -ItemType Directory -Force -Path $proxyDir | Out-Null
    Get-ChildItem -LiteralPath $CargoBin -Filter "*.exe" | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $proxyDir $_.Name) -Force
    }
    if (-not (Test-Path -LiteralPath (Join-Path $proxyDir "rustup.exe"))) {
        throw "Failed to create durable rustup proxies in $proxyDir"
    }
    return $proxyDir
}

function Install-Pnpm([string]$NodeDir) {
    $prefix = Join-Path $ToolsRoot "pnpm-$PnpmVersion"
    New-Item -ItemType Directory -Force -Path $prefix | Out-Null
    & (Join-Path $NodeDir "npm.cmd") install --prefix $prefix "pnpm@$PnpmVersion"
    return $prefix
}

function Write-RunnerShims([string]$NodeDir, [string]$PnpmPrefix) {
    $shim = Join-Path $ToolsRoot "runner-shims"
    New-Item -ItemType Directory -Force -Path $shim | Out-Null
    Set-Content -Encoding ascii -Path (Join-Path $shim "npm.cmd") -Value "@echo off`r`n`"$NodeDir\node.exe`" `"$NodeDir\node_modules\npm\bin\npm-cli.js`" %*"
    Set-Content -Encoding ascii -Path (Join-Path $shim "npx.cmd") -Value "@echo off`r`n`"$NodeDir\node.exe`" `"$NodeDir\node_modules\npm\bin\npx-cli.js`" %*"
    Set-Content -Encoding ascii -Path (Join-Path $shim "corepack.cmd") -Value "@echo off`r`n`"$NodeDir\node.exe`" `"$NodeDir\node_modules\corepack\dist\corepack.js`" %*"
    Set-Content -Encoding ascii -Path (Join-Path $shim "pnpm.cmd") -Value "@echo off`r`n`"$NodeDir\node.exe`" `"$PnpmPrefix\node_modules\pnpm\bin\pnpm.cjs`" %*"
    return $shim
}

function Install-Runner {
    New-Item -ItemType Directory -Force -Path $RunnerRoot | Out-Null
    if (Test-Path -LiteralPath (Join-Path $RunnerRoot "config.cmd")) { return }
    $asset = Get-LatestGitHubAsset -Repo "actions/runner" -NamePattern "actions-runner-win-x64-*.zip"
    Write-Step "Installing actions runner $($asset.Tag)"
    $zip = Join-Path $env:TEMP $asset.Name
    Invoke-WebRequest -Uri $asset.Url -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath $RunnerRoot -Force
}

function Update-RunnerEnv([string[]]$Prepend) {
    $machine = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $user = [Environment]::GetEnvironmentVariable("Path", "User")
    $parts = @($Prepend + (($machine, $user) -join ";").Split(";")) | Where-Object { $_ }
    $seen = @{}
    $deduped = foreach ($p in $parts) {
        $key = $p.TrimEnd("\").ToLowerInvariant()
        if (-not $seen.ContainsKey($key)) { $seen[$key] = $true; $p }
    }
    Set-Content -Encoding ascii -Path (Join-Path $RunnerRoot ".env") -Value ("PATH=" + ($deduped -join ";"))
}

function Get-Token {
    if (-not $RegistrationTokenCommand) { throw "ConfigureRunner requires -RegistrationTokenCommand. Keep token retrieval external." }
    $token = Invoke-Expression $RegistrationTokenCommand
    if (-not $token) { throw "Registration token command returned no token." }
    return ($token | Select-Object -First 1).ToString().Trim()
}

function Configure-Runner {
    $runnerFile = Join-Path $RunnerRoot ".runner"
    if ((Test-Path -LiteralPath $runnerFile) -and -not $Replace) {
        Write-Step "Runner already configured; use -Replace to re-register."
        return
    }
    $token = Get-Token
    Push-Location $RunnerRoot
    try {
        $args = @("--unattended", "--url", $RepoUrl, "--token", $token, "--name", $RunnerName, "--labels", $Labels, "--work", $WorkFolder)
        if ($Replace) { $args += "--replace" }
        if ($RunAsService) {
            if (-not (Test-Elevated)) { throw "-RunAsService requires an elevated shell." }
            $args += "--runasservice"
        }
        & .\config.cmd @args
    } finally {
        $token = $null
        Pop-Location
    }
}

function Test-MsvcInventory {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $vcvars = $null
    if (Test-Path -LiteralPath $vswhere) {
        $vcvars = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find "VC\Auxiliary\Build\vcvarsall.bat" | Select-Object -First 1
    }
    $sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\Include" -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match "^10\." } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    [PSCustomObject]@{
        VisualStudioBuildTools = [bool]$vcvars
        VcVarsAll = $vcvars
        WindowsSdk = [bool]$sdk
        WindowsSdkVersion = if ($sdk) { $sdk.Name } else { $null }
        Escalation = if (-not $vcvars -or -not $sdk) {
            "Install Visual Studio Build Tools elevated with Microsoft.VisualStudio.Workload.VCTools and a Windows 10/11 SDK."
        } else { $null }
    }
}

function Install-VisualStudioBuildTools {
    $inventory = Test-MsvcInventory
    if ($inventory.VisualStudioBuildTools -and $inventory.WindowsSdk) { return $inventory }
    if (-not $InstallVisualStudioBuildTools) { return $inventory }
    if (-not (Test-Elevated)) {
        throw "Visual Studio Build Tools installation requires elevation. Re-run in an elevated shell with -InstallVisualStudioBuildTools."
    }
    $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $winget) { throw "winget.exe not found. Install Visual Studio Build Tools manually with C++ build tools and Windows SDK." }
    Write-Step "Installing Visual Studio Build Tools C++ workload"
    $override = "--wait --quiet --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --add Microsoft.VisualStudio.Component.VC.Tools.ARM64 --add Microsoft.VisualStudio.Component.Windows11SDK.26100"
    & $winget.Source install --id Microsoft.VisualStudio.2022.BuildTools --exact --silent --accept-package-agreements --accept-source-agreements --override $override
    $inventory = Test-MsvcInventory
    if (-not $inventory.VisualStudioBuildTools -or -not $inventory.WindowsSdk) {
        throw "Visual Studio Build Tools or Windows SDK still missing after installer completed. Reboot if requested, then re-run verification."
    }
    return $inventory
}

function Test-WebView2 {
    $dir = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application" -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match "^\d+\." } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    [PSCustomObject]@{ Installed = [bool]$dir; Version = if ($dir) { $dir.Name } else { $null } }
}

function Get-MsvcEnvironmentPaths {
    $inventory = Test-MsvcInventory
    if (-not $inventory.VisualStudioBuildTools -or -not $inventory.WindowsSdk) { return $null }
    $vcRoot = Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $inventory.VcVarsAll))) ""
    $msvcRoot = Join-Path $vcRoot "Tools\MSVC"
    $msvcVersion = (Get-ChildItem $msvcRoot -Directory | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1).Name
    $sdkVersion = $inventory.WindowsSdkVersion
    [PSCustomObject]@{
        Bin = Join-Path $msvcRoot "$msvcVersion\bin\Hostx64\x64"
        Include = @(
            (Join-Path $msvcRoot "$msvcVersion\include"),
            "${env:ProgramFiles(x86)}\Windows Kits\10\Include\$sdkVersion\ucrt",
            "${env:ProgramFiles(x86)}\Windows Kits\10\Include\$sdkVersion\shared",
            "${env:ProgramFiles(x86)}\Windows Kits\10\Include\$sdkVersion\um",
            "${env:ProgramFiles(x86)}\Windows Kits\10\Include\$sdkVersion\winrt",
            "${env:ProgramFiles(x86)}\Windows Kits\10\Include\$sdkVersion\cppwinrt"
        ) -join ";"
        Lib = @(
            (Join-Path $msvcRoot "$msvcVersion\lib\x64"),
            "${env:ProgramFiles(x86)}\Windows Kits\10\Lib\$sdkVersion\ucrt\x64",
            "${env:ProgramFiles(x86)}\Windows Kits\10\Lib\$sdkVersion\um\x64"
        ) -join ";"
        SdkBin = "${env:ProgramFiles(x86)}\Windows Kits\10\bin\$sdkVersion\x64"
        SdkUcrtBin = "${env:ProgramFiles(x86)}\Windows Kits\10\bin\$sdkVersion\x64\ucrt"
        VCINSTALLDIR = $vcRoot
        VCToolsInstallDir = Join-Path $msvcRoot "$msvcVersion\"
        WindowsSdkDir = "${env:ProgramFiles(x86)}\Windows Kits\10\"
        WindowsSDKVersion = "$sdkVersion\"
    }
}

function Show-Verification {
    $envLine = Get-Content -LiteralPath (Join-Path $RunnerRoot ".env") | Where-Object { $_ -like "PATH=*" } | Select-Object -First 1
    $env:PATH = $envLine.Substring(5)
    foreach ($cmd in "pwsh", "rustup", "cargo", "rustc", "node", "npm", "corepack", "pnpm", "git") {
        $hit = Get-Command $cmd -ErrorAction SilentlyContinue
        if ($hit) { Write-Host "$cmd -> $($hit.Source)" } else { Write-Host "MISSING: $cmd" }
    }
    pwsh -NoProfile -Command '$PSVersionTable.PSVersion.ToString()'
    rustup --version
    cargo --version
    rustc -vV
    node --version
    npm --version
    corepack --version
    pnpm --version
    git --version
    Test-MsvcInventory | ConvertTo-Json -Compress
    Test-WebView2 | ConvertTo-Json -Compress
}

if (-not $VerifyOnly) {
    $pwshDir = Install-PowerShell
    $nodeDir = Install-NodeLts
    $cargoBin = Install-Rust
    $rustProxyDir = Install-RustProxies -CargoBin $cargoBin
    $pnpmPrefix = Install-Pnpm -NodeDir $nodeDir
    $shimDir = Write-RunnerShims -NodeDir $nodeDir -PnpmPrefix $pnpmPrefix
    $msvcInventory = Install-VisualStudioBuildTools
    $msvcEnv = Get-MsvcEnvironmentPaths
    Install-Runner
    $prepend = @($shimDir, $nodeDir, $pwshDir, $rustProxyDir, $cargoBin)
    if ($msvcEnv) { $prepend += @($msvcEnv.Bin, $msvcEnv.SdkBin, $msvcEnv.SdkUcrtBin) }
    Update-RunnerEnv -Prepend $prepend
    if ($msvcEnv) {
        Add-Content -Encoding ascii -Path (Join-Path $RunnerRoot ".env") -Value @(
            "INCLUDE=$($msvcEnv.Include)",
            "LIB=$($msvcEnv.Lib)",
            "VCINSTALLDIR=$($msvcEnv.VCINSTALLDIR)",
            "VCToolsInstallDir=$($msvcEnv.VCToolsInstallDir)",
            "WindowsSdkDir=$($msvcEnv.WindowsSdkDir)",
            "WindowsSDKVersion=$($msvcEnv.WindowsSDKVersion)",
            "UniversalCRTSdkDir=$($msvcEnv.WindowsSdkDir)",
            "VisualStudioVersion=17.0",
            "VSCMD_ARG_HOST_ARCH=x64",
            "VSCMD_ARG_TGT_ARCH=x64"
        )
    }
    if ($ConfigureRunner) { Configure-Runner }
}

Show-Verification

if ($StartListener -and -not $RunAsService) {
    Start-Process -FilePath $env:ComSpec -ArgumentList @("/d", "/c", "run.cmd") -WorkingDirectory $RunnerRoot -WindowStyle Hidden
}
