function Get-InstalledImagePath {
    param([string]$Path)
    if ($Path.StartsWith('\\?\')) { return $Path.Substring(4) }
    return $Path
}

function Test-InstalledBinaryImage {
    param([string]$Root, [string]$Image)
    if ([string]::IsNullOrWhiteSpace($Image)) { return $false }
    $path = Get-InstalledImagePath $Image
    if ([IO.Path]::GetExtension($path) -ine '.exe') { return $false }
    $directories = @($Root, "$Root\cli", "$Root\domios-server", "$Root\conpty\x64", "$Root\conpty\arm64")
    return $directories -contains [IO.Path]::GetDirectoryName($path)
}

function Test-DomiosInstalledImage {
    param([string]$Root, [string]$Image)
    if (-not (Test-InstalledBinaryImage $Root $Image)) { return $false }
    $path = Get-InstalledImagePath $Image
    $relative = $path.Substring($Root.Length).TrimStart('\').ToLowerInvariant()
    return $relative -in @(
        'domios-app.exe', 'domios-dev-app.exe', 'domios-server\domios-server.exe',
        'cli\codeterm.exe', 'cli\domios.exe',
        'cli\codeterm-x86_64-pc-windows-msvc.exe', 'cli\codeterm-aarch64-pc-windows-msvc.exe',
        'cli\codeterm-i686-pc-windows-msvc.exe', 'conpty\x64\openconsole.exe', 'conpty\arm64\openconsole.exe'
    )
}
