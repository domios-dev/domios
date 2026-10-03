function Test-ArchiveEntryMatchesStream($Entry, [IO.Stream]$File) {
    if ($Entry.Length -ne $File.Length) { return $false }
    $archive = $Entry.Open()
    try {
        $left = [byte[]]::new(65536); $right = [byte[]]::new(65536)
        while ($true) {
            $a = 0; $b = 0
            while ($a -lt $left.Length) { $n = $archive.Read($left, $a, $left.Length - $a); if ($n -eq 0) { break }; $a += $n }
            while ($b -lt $right.Length) { $n = $File.Read($right, $b, $right.Length - $b); if ($n -eq 0) { break }; $b += $n }
            if ($a -ne $b) { return $false }
            if ($a -eq 0) { return $true }
            for ($i = 0; $i -lt $a; $i++) { if ($left[$i] -ne $right[$i]) { return $false } }
        }
    } finally { $archive.Dispose() }
}
function Test-ArchiveEntryMatchesFile($Entry, [string]$Path) {
    $file = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try { return Test-ArchiveEntryMatchesStream $Entry $file }
    finally { $file.Dispose() }
}
function Get-ArchiveStreamContractCases {
    return @(
        @{ Name = 'compressed_archive_stream_equal_positive'; Facts = @{ DecisionKind = 'archive_stream'; Mutation = 'none' }; Expected = $true },
        @{ Name = 'compressed_archive_stream_changed_byte_refused'; Facts = @{ DecisionKind = 'archive_stream'; Mutation = 'byte' }; Expected = $false },
        @{ Name = 'compressed_archive_stream_changed_length_refused'; Facts = @{ DecisionKind = 'archive_stream'; Mutation = 'length' }; Expected = $false }
    )
}
function Test-ArchiveStreamContract([string]$Mutation) {
    $bytes = [byte[]]::new(131079)
    for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = [byte]($i % 251) }
    $container = [IO.MemoryStream]::new()
    try {
        $writer = [IO.Compression.ZipArchive]::new($container, [IO.Compression.ZipArchiveMode]::Create, $true)
        try {
            $output = $writer.CreateEntry('fixture.bin', [IO.Compression.CompressionLevel]::Optimal).Open()
            try { $output.Write($bytes, 0, $bytes.Length) } finally { $output.Dispose() }
        } finally { $writer.Dispose() }
        $container.Position = 0
        $reader = [IO.Compression.ZipArchive]::new($container, [IO.Compression.ZipArchiveMode]::Read, $true)
        try {
            if ($Mutation -eq 'byte') { $bytes[65537] = [byte]($bytes[65537] -bxor 1) }
            if ($Mutation -eq 'length') { $bytes = $bytes[0..($bytes.Length - 2)] }
            $file = [IO.MemoryStream]::new($bytes, $false)
            try { return [pscustomobject]@{ Valid = (Test-ArchiveEntryMatchesStream ($reader.GetEntry('fixture.bin')) $file); Reasons = @() } }
            finally { $file.Dispose() }
        } finally { $reader.Dispose() }
    } finally { $container.Dispose() }
}
