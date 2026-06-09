param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]] $BuildArgs
)

$ErrorActionPreference = "Stop"

$candidates = @(
  "$env:ProgramFiles\Git\bin\bash.exe",
  "$env:ProgramFiles\Git\usr\bin\bash.exe",
  "${env:ProgramFiles(x86)}\Git\bin\bash.exe",
  "C:\msys64\usr\bin\bash.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }

if (-not $candidates -or $candidates.Count -eq 0) {
  throw "Git Bash/MSYS bash not found. Install Git for Windows or run build.sh from a real bash."
}

$bash = $candidates[0]
$repo = Split-Path -Parent $MyInvocation.MyCommand.Path
$scriptArgs = @("-lc", "./build.sh $($BuildArgs -join ' ')")

function Invoke-CdpReload {
  param([int]$TimeoutSeconds = 30)
  $target_url = "http://127.0.0.1:1420/"
  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)

  while ((Get-Date) -lt $deadline) {
    try {
      $targets = Invoke-RestMethod "http://127.0.0.1:9223/json"
      $page = @($targets | Where-Object { $_.type -eq "page" })[0]
      if (-not $page) {
        Start-Sleep -Milliseconds 250
        continue
      }

      $ws = [System.Net.WebSockets.ClientWebSocket]::new()
      $uri = [Uri]$page.webSocketDebuggerUrl
      $ws.ConnectAsync($uri, [Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null

      function Send-CdpMessage {
        param([System.Net.WebSockets.ClientWebSocket]$WebSocket, [string]$Json, [int]$Id)
        $jsonBytes = [Text.Encoding]::UTF8.GetBytes($Json)
        $wsPayload = [ArraySegment[byte]]::new($jsonBytes)
        $WebSocket.SendAsync(
          $wsPayload,
          [System.Net.WebSockets.WebSocketMessageType]::Text,
          $true,
          [Threading.CancellationToken]::None
        ).GetAwaiter().GetResult() | Out-Null
      }

      function Receive-CdpResponse {
        param([System.Net.WebSockets.ClientWebSocket]$WebSocket, [int]$RequestId)
        while ($true) {
          $buffer = New-Object byte[] 65536
          $segment = [ArraySegment[byte]]::new($buffer)
          $response = $WebSocket.ReceiveAsync($segment, [Threading.CancellationToken]::None).GetAwaiter().GetResult()

          if ($response.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
            return $null
          }

          $json = [Text.Encoding]::UTF8.GetString($buffer, 0, $response.Count)
          $payload = $null
          try {
            $payload = $json | ConvertFrom-Json -ErrorAction Stop
          } catch {
            continue
          }

          if ($payload.id -eq $RequestId) {
            return $payload
          }
        }
      }

      $stateId = 1
      Send-CdpMessage -WebSocket $ws -Id $stateId -Json ('{"id":' + $stateId + ',"method":"Runtime.evaluate","params":{"expression":"window.location.href + \"\\n\" + (document.getElementById(\"root\") ? \"1\" : \"0\");","returnByValue":true}}')
      $state = Receive-CdpResponse -WebSocket $ws -RequestId $stateId
      $stateValue = ""
      if ($state -and $state.result -and $state.result.result -and $state.result.result.value) {
        $stateValue = [string]$state.result.result.value
      }

      $parts = $stateValue -split "`n", 2
      $currentUrl = $parts[0]
      $hasRoot = $parts.Count -gt 1 -and $parts[1] -eq "1"

      if (-not $currentUrl.StartsWith($target_url) -or -not $hasRoot) {
        $navId = 2
        if (-not $currentUrl.StartsWith($target_url)) {
          Send-CdpMessage -WebSocket $ws -Id $navId -Json ('{"id":' + $navId + ',"method":"Page.navigate","params":{"url":"' + $target_url + '"}}')
          $navId++
          Start-Sleep -Milliseconds 700
        }

        if (-not $hasRoot) {
          $reloadId = $navId
          Send-CdpMessage -WebSocket $ws -Id $reloadId -Json ('{"id":' + $reloadId + ',"method":"Page.reload","params":{"ignoreCache":true}}')
        }
      } else {
        $ws.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, 'ok', [Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
        break
      }

      Start-Sleep -Milliseconds 700
      $ws.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, 'ok', [Threading.CancellationToken]::None).GetAwaiter().GetResult() | Out-Null
    } catch {
      Start-Sleep -Milliseconds 250
      continue
    }
  }
}

Push-Location $repo
try {
  & $bash @scriptArgs
  $exitCode = $LASTEXITCODE
  if ($exitCode -eq 0 -and $BuildArgs.Count -gt 0 -and $BuildArgs[0] -eq "dev") {
    Invoke-CdpReload
  }
  exit $exitCode
} finally {
  Pop-Location
}
