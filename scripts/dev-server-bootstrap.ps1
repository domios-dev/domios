param([string] $Server, [ValidateSet('attach', 'observe', 'launch')] [string] $Operation)
$ErrorActionPreference = 'Stop'
$child = $null
try {
    $request = [Console]::In.ReadLine()
    if (-not $request -or $request.Length -gt 32768) { throw 'Invalid private bootstrap input' }
    $child = New-Object System.Diagnostics.Process
    $child.StartInfo = New-Object System.Diagnostics.ProcessStartInfo($Server, $Operation)
    $child.StartInfo.UseShellExecute = $false
    $child.StartInfo.CreateNoWindow = $true
    $child.StartInfo.RedirectStandardInput = $true
    $child.StartInfo.RedirectStandardOutput = $true
    $child.StartInfo.RedirectStandardError = $true
    if (-not $child.Start()) { throw 'Bootstrap helper did not start' }
    $child.StandardInput.WriteLine($request)
    $child.StandardInput.Close()
    $errors = $child.StandardError.ReadToEndAsync()
    while ($null -ne ($line = $child.StandardOutput.ReadLine())) {
        if ($line.Length -gt 65536) { throw 'Bootstrap output exceeds bound' }
        [Console]::Out.WriteLine($line)
        [Console]::Out.Flush()
    }
    $child.WaitForExit()
    exit $child.ExitCode
} catch {
    [Console]::Error.WriteLine('Private DEV bootstrap failed')
    exit 1
} finally {
    if ($null -ne $child) {
        if (-not $child.HasExited) { $child.Kill(); $child.WaitForExit() }
        $child.Dispose()
    }
}
