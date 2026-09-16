# CodeTerm Resilience

CodeTerm has three defense layers against the daemon hanging or crashing under
heavy memory work (dreaming, extraction, vector indexing, search evals):

1. **Backpressure inside the daemon** — bounded semaphore + per-batch lock
   release in dreaming/extraction. API requests always win contention.
   See `src-tauri/src/mem/dreaming.rs`, `src-tauri/src/mem/mod.rs::heavy_ops_permits`.
2. **Process isolation** — dreaming and extraction can run as child processes
   of the daemon. Worker death does not take the API down.
   See `src-tauri/src/mem/worker_pool.rs`.
3. **External watchdog** — OS-level supervisor restarts the daemon if it
   panics, exits non-zero, or stops responding to health probes. **This file
   covers layer 3.**

Layers 1 and 2 cover the common failure modes (lock contention, runaway
worker). Layer 3 covers the long tail (panic in core, OOM, deadlock that
slipped past static analysis). Set up at least one of the watchdogs below in
production environments.

## Health probe

The daemon exposes `GET /api/health` on its bind port (prod 7685, dev 7686).
A healthy response returns 200 with JSON `{ "status": "ok", "build", "version", "name", "platform" }`.
Watchdogs use this endpoint as the liveness signal.

A 5-second timeout is generous — handlers should respond in under 100ms even
under load (after the Phase 1 timeout fix, mem-busy returns 503 in <500ms).

## Windows Task Scheduler

Two scheduled tasks: one polls `/api/health` every 30s and kills the daemon if
it stops responding; one starts the daemon on boot and restarts it whenever
the process exits.

Save as `codeterm-watchdog.xml` and import with
`schtasks /create /tn CodeTermWatchdog /xml codeterm-watchdog.xml /ru SYSTEM`.

```xml
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Restart CodeTerm if /api/health fails 3 times in a row.</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2026-01-01T00:00:00</StartBoundary>
      <Repetition>
        <Interval>PT30S</Interval>
      </Repetition>
      <ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>S-1-5-18</UserId>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <ExecutionTimeLimit>PT1M</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>powershell.exe</Command>
      <Arguments>-NoProfile -ExecutionPolicy Bypass -File "%LOCALAPPDATA%\CodeTerm\watchdog.ps1"</Arguments>
    </Exec>
  </Actions>
</Task>
```

`watchdog.ps1` (place at `%LOCALAPPDATA%\CodeTerm\watchdog.ps1`):

```powershell
$ErrorActionPreference = 'Stop'
$stateFile = Join-Path $env:LOCALAPPDATA 'CodeTerm\watchdog-state.json'
$state = if (Test-Path $stateFile) { Get-Content $stateFile | ConvertFrom-Json } else { @{ failures = 0 } }

try {
    $r = Invoke-WebRequest -Uri 'http://127.0.0.1:7685/api/health' -TimeoutSec 5 -UseBasicParsing
    if ($r.StatusCode -eq 200) { $state.failures = 0 }
    else { $state.failures++ }
} catch {
    $state.failures++
}

if ($state.failures -ge 3) {
    Write-EventLog -LogName Application -Source 'CodeTerm' -EventId 5001 -EntryType Error `
        -Message "Health probe failed $($state.failures) times. Restarting daemon." -ErrorAction SilentlyContinue
    Get-Process codeterm -ErrorAction SilentlyContinue | Where-Object {
        ($_.MainModule.FileName -like '*\CodeTerm\*') -and ($_.Id -ne $PID)
    } | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    Start-Process -FilePath "$env:LOCALAPPDATA\Programs\CodeTerm\codeterm.exe" -WindowStyle Hidden
    $state.failures = 0
}

$state | ConvertTo-Json | Set-Content $stateFile
```

Setup steps:

```powershell
# 1. Register the event source (admin shell, one-time):
New-EventLog -LogName Application -Source 'CodeTerm'

# 2. Place watchdog.ps1 under %LOCALAPPDATA%\CodeTerm\
New-Item -Path "$env:LOCALAPPDATA\CodeTerm" -ItemType Directory -Force

# 3. Import the scheduled task:
schtasks /create /tn CodeTermWatchdog /xml codeterm-watchdog.xml /ru SYSTEM

# 4. Verify:
schtasks /query /tn CodeTermWatchdog /v /fo LIST
Get-EventLog -LogName Application -Source CodeTerm -Newest 5
```

The probe runs every 30 seconds; the daemon is restarted only after 3
consecutive failures (90s total) to avoid flapping. State file
`watchdog-state.json` carries the failure counter between probes.

Adjust `Stop-Process` filter if you keep prod and dev side-by-side: prod binds
7685, dev 7686 — match by port via `Get-NetTCPConnection -LocalPort 7685` if
the install path is shared.

## macOS launchd

`~/Library/LaunchAgents/com.codeterm.watchdog.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.codeterm.watchdog</string>
  <key>ProgramArguments</key>
  <array>
    <string>/bin/bash</string>
    <string>-c</string>
    <string>curl -fsS --max-time 5 http://127.0.0.1:7685/api/health > /dev/null || open -a CodeTerm</string>
  </array>
  <key>StartInterval</key><integer>30</integer>
  <key>RunAtLoad</key><true/>
  <key>StandardErrorPath</key><string>/tmp/codeterm-watchdog.log</string>
</dict>
</plist>
```

Load: `launchctl load ~/Library/LaunchAgents/com.codeterm.watchdog.plist`.

This is simpler than the Windows version — no failure-count debouncing — and
relies on `open -a CodeTerm` being idempotent (won't launch a second instance
if one is already running and registered with LaunchServices). For
crash-restart of the daemon binary itself, prefer launchd's `KeepAlive` on the
daemon's own plist rather than a separate watchdog.

## Linux systemd

`~/.config/systemd/user/codeterm.service`:

```ini
[Unit]
Description=CodeTerm daemon
After=network.target

[Service]
ExecStart=%h/.local/bin/codeterm daemon
Restart=always
RestartSec=2
StartLimitIntervalSec=60
StartLimitBurst=5

[Install]
WantedBy=default.target
```

Enable: `systemctl --user enable --now codeterm.service`. systemd handles
crash-restart natively — no separate health probe needed unless you want to
restart on hung-but-alive processes (add a `WatchdogSec=60` and have the
daemon emit `sd_notify(WATCHDOG=1)` periodically, future work).

## When the watchdog fires repeatedly

If `watchdog-state.json` shows the daemon restarting more than once an hour,
something is wrong at layer 1 or 2. Check:

1. `~/.codeterm/logs/codeterm.log` (prod) — look for panics or repeated
   "[mem] busy" 503s near the restart timestamps.
2. Whether a misbehaving extension or skill is hammering the API.
3. Whether the heavy_ops semaphore is exhausted (would show as queued
   dreaming/extraction work in `codeterm mem worker list`; inspect one with
   `codeterm mem worker get <id>`).

The watchdog is a last-resort safety net. Persistent restarts mean the
daemon-side backpressure (Phase 1) or process isolation (Phase 2) needs
tuning — not more aggressive restarts.

