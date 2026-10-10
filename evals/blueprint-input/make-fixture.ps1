$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech

$destination = Join-Path $PSScriptRoot 'quick-brown-fox.wav'
$synthesizer = [System.Speech.Synthesis.SpeechSynthesizer]::new()
$format = [System.Speech.AudioFormat.SpeechAudioFormatInfo]::new(
    16000,
    [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen,
    [System.Speech.AudioFormat.AudioChannel]::Mono
)
try {
    $synthesizer.SetOutputToWaveFile($destination, $format)
    $synthesizer.Speak('The quick brown fox jumps over the lazy dog.')
}
finally {
    $synthesizer.Dispose()
}

$length = (Get-Item -LiteralPath $destination).Length
if ($length -gt 409600) {
    Remove-Item -LiteralPath $destination
    throw "Generated WAV exceeds 400 KB: $length bytes"
}
Write-Output "Generated $destination ($length bytes)"
