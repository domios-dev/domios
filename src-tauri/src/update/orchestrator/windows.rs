//! Windows orchestrator — PowerShell script + schtasks one-shot dispatch.
//!
//! The script flow:
//!   1. Wait up to 30s for the running daemon to exit.
//!   2. SIGKILL any survivors named codeterm / codeterm-embed.
//!   3. Snapshot the current `codeterm.exe` to `.bak` siblings.
//!   4. Run the NSIS installer with `/S` (silent) to replace files in
//!      the install dir.
//!   5. Start the new exe detached.
//!   6. Poll /api/health (loopback, with token) up to 60s.
//!   7. If `version` doesn't match `new_version`, restore from `.bak`
//!      and start that. Update the JSON result with the outcome.
//!   8. ALWAYS write `upgrade-result.json` so the new daemon picks it up.

use std::path::PathBuf;
use std::process::Command;

use super::{Orchestrator, OrchestratorInputs, script_path};

pub struct WindowsOrchestrator;

impl Orchestrator for WindowsOrchestrator {
    fn render_script(&self, inputs: &OrchestratorInputs) -> String {
        render_ps1(inputs)
    }

    fn dispatch(&self, inputs: &OrchestratorInputs) -> Result<PathBuf, String> {
        let script = self.render_script(inputs);
        let script_path = script_path(&inputs.staging_dir);
        std::fs::write(&script_path, script)
            .map_err(|e| format!("write {}: {}", script_path.display(), e))?;

        let context_path = inputs.staging_dir.join(crate::update::result::UPGRADE_CONTEXT_FILENAME);
        std::fs::write(
            &context_path,
            serde_json::to_string_pretty(&inputs.context).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("write {}: {}", context_path.display(), e))?;

        schedule_powershell(&script_path)
    }
}

fn render_ps1(inputs: &OrchestratorInputs) -> String {
    let ctx = &inputs.context;
    format!(
        r#"# CodeTerm self-update orchestrator
# AUTO-GENERATED — do not hand-edit. Regenerated on every `codeterm self-update --apply`.
$ErrorActionPreference = 'Continue'
$LogPath       = '{log_path}'
$ResultPath    = '{result_path}'
$InstallerPath = '{installer_path}'
$CurrentExe    = '{current_exe}'
$BackupExe     = '{backup_exe}'
$HealthUrl     = '{health_url}'
$ApiToken      = '{api_token}'
$OldVersion    = '{old_version}'
$NewVersion    = '{new_version}'
$Channel       = '{channel}'
$NotifyChatId  = {chat_id_literal}

function Log {{ param([string]$msg)
    $line = ('[{{0}}] {{1}}' -f (Get-Date -Format 'yyyy-MM-ddTHH:mm:ssZ'), $msg)
    Add-Content -Path $LogPath -Value $line -Encoding UTF8
}}

function Write-Result {{
    param([string]$Status, [string]$Observed, [string]$ErrorMsg, [string]$FinishedAt)
    $obj = [ordered]@{{
        status           = $Status
        old_version      = $OldVersion
        new_version      = $NewVersion
        observed_version = $Observed
        channel          = $Channel
        log_path         = $LogPath
        started_at       = $StartedAt
        finished_at      = $FinishedAt
        error            = $ErrorMsg
        notify_chat_id   = $NotifyChatId
    }}
    $json = $obj | ConvertTo-Json -Depth 4
    Set-Content -Path $ResultPath -Value $json -Encoding UTF8
}}

$StartedAt = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
Log "self-update started: $OldVersion -> $NewVersion (channel=$Channel)"
Log "current exe: $CurrentExe"
Log "installer:   $InstallerPath"

# 1. Wait for the running daemon to exit on its own (up to 30s).
$gracefulDeadline = (Get-Date).AddSeconds(30)
while ((Get-Date) -lt $gracefulDeadline) {{
    $alive = Get-Process -Name 'codeterm' -ErrorAction SilentlyContinue
    if (-not $alive) {{ Log 'daemon exited gracefully'; break }}
    Start-Sleep -Milliseconds 500
}}

# 2. Kill survivors.
foreach ($name in @('codeterm', 'codeterm-embed')) {{
    Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object {{
        try {{ $_.Kill() ; Log "killed $name pid=$($_.Id)" }}
        catch {{ Log "kill $name failed: $_" }}
    }}
}}
Start-Sleep -Seconds 1

# 3. Backup current binary so we can roll back.
try {{
    Copy-Item -Path $CurrentExe -Destination $BackupExe -Force -ErrorAction Stop
    Log "backup written to $BackupExe"
}} catch {{
    Log "backup failed: $_"
    Write-Result 'rollback_failed' '' "backup failed before swap: $_" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 1
}}

# 4. Run the installer silently. NSIS convention: /S = silent.
try {{
    $proc = Start-Process -FilePath $InstallerPath -ArgumentList '/S' -Wait -PassThru -ErrorAction Stop
    Log "installer exit code: $($proc.ExitCode)"
    if ($proc.ExitCode -ne 0) {{ throw "installer exit code $($proc.ExitCode)" }}
}} catch {{
    Log "installer failed: $_"
    # Restore .bak immediately so SOMETHING runs.
    try {{ Copy-Item -Path $BackupExe -Destination $CurrentExe -Force -ErrorAction Stop }} catch {{ }}
    Start-Process -FilePath $CurrentExe -WindowStyle Hidden -ErrorAction SilentlyContinue
    Write-Result 'rolled_back' '' "installer failed: $_" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 1
}}

# 5. Launch the new binary (detached).
try {{
    Start-Process -FilePath $CurrentExe -WindowStyle Hidden -ErrorAction Stop
    Log 'new daemon launched'
}} catch {{
    Log "launch failed: $_"
    # Try the backup.
    try {{ Copy-Item -Path $BackupExe -Destination $CurrentExe -Force -ErrorAction Stop }} catch {{ }}
    try {{ Start-Process -FilePath $CurrentExe -WindowStyle Hidden -ErrorAction Stop }} catch {{
        Write-Result 'rollback_failed' '' "new launch failed AND rollback launch failed: $_" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
        exit 1
    }}
    Write-Result 'rolled_back' '' "new daemon failed to launch: $_" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 1
}}

# 6. Health check loop — up to 60s for the new daemon to bind and answer.
$healthDeadline = (Get-Date).AddSeconds(60)
$observed = ''
$healthErr = ''
while ((Get-Date) -lt $healthDeadline) {{
    Start-Sleep -Seconds 2
    try {{
        $headers = @{{ Authorization = "Bearer $ApiToken" }}
        $r = Invoke-RestMethod -Uri $HealthUrl -Headers $headers -TimeoutSec 5 -ErrorAction Stop
        if ($r.version) {{ $observed = $r.version ; break }}
    }} catch {{
        $healthErr = $_.ToString()
    }}
}}

if ($observed -eq $NewVersion) {{
    Log "health OK: observed=$observed"
    Write-Result 'success' $observed '' ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 0
}}

Log "health mismatch — observed='$observed' expected='$NewVersion' err='$healthErr'"

# 7. Roll back: kill the new instance, restore .bak, start it.
foreach ($name in @('codeterm', 'codeterm-embed')) {{
    Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object {{
        try {{ $_.Kill() }} catch {{ }}
    }}
}}
Start-Sleep -Seconds 1
try {{
    Copy-Item -Path $BackupExe -Destination $CurrentExe -Force -ErrorAction Stop
    Log 'restored from .bak'
    Start-Process -FilePath $CurrentExe -WindowStyle Hidden -ErrorAction Stop
    Log 'rollback launched'
}} catch {{
    Log "rollback FAILED: $_"
    Write-Result 'rollback_failed' $observed "rollback failed: $_" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 2
}}

# Verify the rollback came up with the OLD version on /api/health.
$rollbackDeadline = (Get-Date).AddSeconds(30)
$rollbackObserved = ''
while ((Get-Date) -lt $rollbackDeadline) {{
    Start-Sleep -Seconds 2
    try {{
        $headers = @{{ Authorization = "Bearer $ApiToken" }}
        $r = Invoke-RestMethod -Uri $HealthUrl -Headers $headers -TimeoutSec 5 -ErrorAction Stop
        if ($r.version) {{ $rollbackObserved = $r.version ; break }}
    }} catch {{ }}
}}
if ($rollbackObserved -eq $OldVersion) {{
    Write-Result 'rolled_back' $observed "new daemon reported '$observed' instead of '$NewVersion'" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 0
}} else {{
    Write-Result 'rollback_failed' $rollbackObserved "rollback came up as '$rollbackObserved', expected '$OldVersion'" ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))
    exit 3
}}
"#,
        log_path = inputs.log_path.display(),
        result_path = inputs.result_path.display(),
        installer_path = inputs.installer_path.display(),
        current_exe = inputs.current_exe.display(),
        backup_exe = backup_path(&inputs.current_exe).display(),
        health_url = inputs.health_url,
        api_token = inputs.api_token.replace('\'', "''"),
        old_version = ctx.old_version,
        new_version = ctx.new_version,
        channel = ctx.channel,
        chat_id_literal = chat_id_literal(ctx.notify_chat_id),
    )
}

fn chat_id_literal(id: Option<i64>) -> String {
    match id {
        Some(v) => v.to_string(),
        None => "$null".to_string(),
    }
}

pub(crate) fn backup_path(exe: &std::path::Path) -> PathBuf {
    let mut s = exe.as_os_str().to_os_string();
    s.push(".bak");
    PathBuf::from(s)
}

/// Schedule the script to run via schtasks, fire-and-forget, ~15s out.
/// We use one-shot scheduling (no /SC ONCE recurrence; /SC ONCE with a
/// near-future time and /Z to delete on completion). The task runs as
/// the current user, no admin needed.
fn schedule_powershell(script_path: &std::path::Path) -> Result<PathBuf, String> {
    let task_name = "CodeTermSelfUpdate";

    // schtasks expects HH:mm and a date in /D — pick "now + 15s", but
    // schtasks rounds to the nearest minute, so "now + 1min" is the safe
    // floor that always lands in the future. We then call /Run to fire
    // immediately so we don't wait that minute.
    let fire = chrono::Local::now() + chrono::Duration::seconds(120);
    let st = fire.format("%H:%M").to_string();
    let sd = fire.format("%m/%d/%Y").to_string();

    // /F overwrites if it already exists. /Z deletes after run. /RL LIMITED
    // = standard user, no elevation. /IT = interactive (lets the script
    // outlive its parent without inheriting the dying daemon's session
    // descriptors).
    let ps_args = format!(
        "powershell -ExecutionPolicy Bypass -WindowStyle Hidden -NoProfile -File \"{}\"",
        script_path.display()
    );

    // Delete existing first (ignore failure — likely "task not found").
    let mut del = Command::new("schtasks");
    del.args([
        "/Delete",
        "/TN",
        task_name,
        "/F",
    ]);
    crate::process_util::no_window_std(&mut del);
    let _ = del.output();

    let mut create_cmd = Command::new("schtasks");
    create_cmd.args([
        "/Create",
        "/TN",
        task_name,
        "/TR",
        &ps_args,
        "/SC",
        "ONCE",
        "/ST",
        &st,
        "/SD",
        &sd,
        "/Z",
        "/F",
        "/RL",
        "LIMITED",
    ]);
    crate::process_util::no_window_std(&mut create_cmd);
    let create = create_cmd
        .output()
        .map_err(|e| format!("schtasks /Create: {}", e))?;
    if !create.status.success() {
        return Err(format!(
            "schtasks /Create failed: {}",
            String::from_utf8_lossy(&create.stderr)
        ));
    }

    let mut run_cmd = Command::new("schtasks");
    run_cmd.args(["/Run", "/TN", task_name]);
    crate::process_util::no_window_std(&mut run_cmd);
    let run = run_cmd
        .output()
        .map_err(|e| format!("schtasks /Run: {}", e))?;
    if !run.status.success() {
        return Err(format!(
            "schtasks /Run failed: {}",
            String::from_utf8_lossy(&run.stderr)
        ));
    }

    Ok(script_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::result::UpgradeContext;
    use std::path::PathBuf;

    fn sample_inputs() -> OrchestratorInputs {
        OrchestratorInputs {
            staging_dir: PathBuf::from(r"C:\Users\u\.codeterm-dev\upgrade-staging"),
            installer_path: PathBuf::from(r"C:\Users\u\.codeterm-dev\upgrade-staging\CodeTerm_1.1.3_x64-setup.exe"),
            current_exe: PathBuf::from(r"C:\Program Files\CodeTerm\codeterm.exe"),
            health_url: "http://127.0.0.1:7686/api/health".into(),
            api_token: "secret-token".into(),
            result_path: PathBuf::from(r"C:\Users\u\.codeterm-dev\upgrade-result.json"),
            log_path: PathBuf::from(r"C:\Users\u\.codeterm-dev\upgrade-1.1.2-to-1.1.3.log"),
            context: UpgradeContext {
                old_version: "1.1.2".into(),
                new_version: "1.1.3".into(),
                channel: "stable".into(),
                notify_chat_id: Some(98765),
                log_path: r"C:\Users\u\.codeterm-dev\upgrade-1.1.2-to-1.1.3.log".into(),
                started_at: None,
            },
        }
    }

    #[test]
    fn script_contains_all_phases() {
        let s = render_ps1(&sample_inputs());
        // Headers
        assert!(s.contains("self-update orchestrator"));
        // Versions threaded into the script
        assert!(s.contains("$OldVersion    = '1.1.2'"), "old version literal missing:\n{}", s);
        assert!(s.contains("$NewVersion    = '1.1.3'"));
        assert!(s.contains("$Channel       = 'stable'"));
        // Chat id
        assert!(s.contains("$NotifyChatId  = 98765"), "chat id literal missing");
        // Phases
        assert!(s.contains("Wait for the running daemon to exit"), "missing wait phase");
        assert!(s.contains("Get-Process -Name 'codeterm'"), "missing kill phase");
        assert!(s.contains("Get-Process -Name 'codeterm-embed'") || s.contains("'codeterm-embed'"),
            "missing embed kill mention");
        assert!(s.contains("Copy-Item -Path $CurrentExe -Destination $BackupExe"), "missing backup phase");
        assert!(s.contains("Start-Process -FilePath $InstallerPath -ArgumentList '/S'"), "missing install phase");
        assert!(s.contains("Start-Process -FilePath $CurrentExe"), "missing launch phase");
        assert!(s.contains("Invoke-RestMethod -Uri $HealthUrl"), "missing health check");
        assert!(s.contains("Bearer $ApiToken"), "health check must send bearer token");
        assert!(s.contains("Copy-Item -Path $BackupExe -Destination $CurrentExe"), "missing rollback restore");
        // Result-writing function present
        assert!(s.contains("function Write-Result"), "missing result writer");
        // All three statuses are emitted somewhere in the script
        assert!(s.contains("'success'"));
        assert!(s.contains("'rolled_back'"));
        assert!(s.contains("'rollback_failed'"));
    }

    #[test]
    fn script_threads_paths_into_variables() {
        let s = render_ps1(&sample_inputs());
        assert!(s.contains(r"C:\Program Files\CodeTerm\codeterm.exe"));
        assert!(s.contains("CodeTerm_1.1.3_x64-setup.exe"));
        assert!(s.contains("upgrade-result.json"));
        assert!(s.contains("upgrade-1.1.2-to-1.1.3.log"));
    }

    #[test]
    fn null_chat_id_renders_as_powershell_null() {
        let mut i = sample_inputs();
        i.context.notify_chat_id = None;
        let s = render_ps1(&i);
        assert!(s.contains("$NotifyChatId  = $null"), "should render `$null` when no chat configured");
    }

    #[test]
    fn backup_path_appends_bak() {
        let p = PathBuf::from(r"C:\foo\codeterm.exe");
        let b = backup_path(&p);
        assert_eq!(b, PathBuf::from(r"C:\foo\codeterm.exe.bak"));
    }

    #[test]
    fn quote_escaped_in_token() {
        let mut i = sample_inputs();
        i.api_token = "tok'with'quote".into();
        let s = render_ps1(&i);
        // Single quotes inside a single-quoted PowerShell literal must be doubled.
        assert!(s.contains("$ApiToken      = 'tok''with''quote'"),
            "token must be escaped for single-quoted literal");
    }
}
