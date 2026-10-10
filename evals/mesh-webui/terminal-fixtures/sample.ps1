param([ValidateSet('output','alternate')][string]$Mode = 'output')
$ErrorActionPreference = 'Stop'
$esc = [char]27
if ($Mode -eq 'output') {
    1..180 | ForEach-Object { [Console]::WriteLine(('B3:{0:000} | λ 界 é | {1}' -f $_, ('=' * 120))) }
    [Console]::WriteLine("${esc}[31mRED${esc}[0m ${esc}[32mGREEN${esc}[0m ${esc}[7mINVERSE${esc}[0m")
    [Console]::WriteLine(('B3:{0}' -f 'COMPLETE'))
} else {
    try {
        [Console]::Write("${esc}[?1049h${esc}[2J${esc}[H${esc}[?25lB3:ALTERNATE`n")
        [Console]::Write("${esc}[4;8H${esc}[36mB3:CURSOR-4-8${esc}[0m${esc}[?25h")
        $reply = Read-Host 'Type an ASCII/Unicode token, or Ctrl+C'
        [Console]::WriteLine(('B3:ECHO:{0}' -f $reply))
    } finally {
        [Console]::Write("${esc}[?25h${esc}[?1049l")
    }
}
