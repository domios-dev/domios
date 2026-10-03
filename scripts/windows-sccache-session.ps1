[CmdletBinding(DefaultParameterSetName = 'Library')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Contract')][switch]$ContractOnly,
    [Parameter(Mandatory, ParameterSetName = 'Library')][switch]$Library,
    [Parameter(Mandatory, ParameterSetName = 'Maintenance')]
    [ValidateSet('provision-verify-cleanup')][string]$Operation
)

$script:Pin = [ordered]@{
    Version = '0.18.0'
    SourceCommit = '01c35e69b97f516c2ebfbdb418b5cd6277bde280'
    ArchiveSize = 9694400L
    ArchiveSha256 = '8965c74d5e8a225244f741e18ad2f3f504f48228dc1bac948fc22761a348363d'
    ArchiveUrl = 'https://github.com/mozilla/sccache/releases/download/v0.18.0/sccache-v0.18.0-x86_64-pc-windows-msvc.zip'
    Root = 'sccache-v0.18.0-x86_64-pc-windows-msvc'
    Executable = 'sccache.exe'
    Destination = 'tools\sccache\0.18.0-x64'
    ServerPort = 4226
    Endpoint = '127.0.0.1:4226'
    Runner = 'ROG-windows-build'
    Account = 'amigo'
    TailBytes = 32768
    OperationMs = 300000
    CleanupReserveMs = 30000
}

function Get-ArchiveDecision($Facts, $Entries) {
    $reasons = [System.Collections.Generic.List[string]]::new()
    if ($Facts.Size -ne $script:Pin.ArchiveSize) { $reasons.Add('archive_size_mismatch') }
    if ([string]$Facts.Sha256 -cne $script:Pin.ArchiveSha256) { $reasons.Add('archive_digest_mismatch') }
    if ($Entries.Count -lt 1 -or $Entries.Count -gt 64) { $reasons.Add('entry_count_out_of_bounds') }
    $total = [long]0
    $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $executables = 0
    foreach ($entry in $Entries) {
        $name = [string]$entry.Name
        $normalized = $name.TrimEnd('/')
        $segments = $normalized.Split('/')
        if (-not $name -or $name.Contains('\') -or $name.StartsWith('/') -or $name -match '^[A-Za-z]:' -or $name.Contains(':')) { $reasons.Add('unsafe_entry_path'); continue }
        if ($segments | Where-Object { $_ -in @('', '.', '..') }) { $reasons.Add('unsafe_entry_segment') }
        if ($segments | Where-Object { $_.EndsWith('.') -or $_.EndsWith(' ') }) { $reasons.Add('unsafe_windows_segment') }
        if ($segments.Count -lt 1 -or $segments[0] -cne $script:Pin.Root -or ($segments.Count -eq 1 -and -not $entry.Directory)) { $reasons.Add('unexpected_archive_root') }
        if ($normalized -cnotin @($script:Pin.Root, ($script:Pin.Root + '/' + $script:Pin.Executable))) { $reasons.Add('unexpected_archive_shape') }
        if ($entry.Directory -and [long]$entry.Length -ne 0) { $reasons.Add('directory_entry_has_data') }
        if ($segments | Where-Object { $deviceBase = $_.TrimEnd([char[]]@('.', ' ')).Split('.')[0]; $deviceBase -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$' }) { $reasons.Add('device_entry_name') }
        if ($entry.Reparse -or $entry.Symlink -or $entry.Device -or $entry.UnknownType) { $reasons.Add('non_regular_entry') }
        if (-not $seen.Add($normalized)) { $reasons.Add('case_collision') }
        if ([long]$entry.Length -lt 0) { $reasons.Add('negative_entry_length') }
        elseif ([long]$entry.Length -gt (134217728L - $total)) { $reasons.Add('uncompressed_limit_exceeded'); $total = 134217729L }
        else { $total += [long]$entry.Length }
        if (-not $entry.Directory -and $normalized -ceq ($script:Pin.Root + '/' + $script:Pin.Executable)) {
            if ($entry.Reparse -or $entry.Symlink -or $entry.Device -or $entry.UnknownType) { $reasons.Add('executable_not_regular') }
            $executables++
        }
    }
    if ($executables -ne 1) { $reasons.Add('pinned_executable_shape_mismatch') }
    return [pscustomobject]@{ Valid = ($reasons.Count -eq 0); Reasons = @($reasons | Select-Object -Unique); TotalUncompressed = $total }
}

function Get-StaticIdentityDecision($Facts) {
    $reasons = [System.Collections.Generic.List[string]]::new()
    foreach ($name in @('ExpectedSha', 'WorkflowSha', 'EventSha', 'CheckoutSha')) {
        if ([string]$Facts[$name] -cnotmatch '^[0-9a-f]{40}$') { $reasons.Add('invalid_' + $name.ToLowerInvariant()) }
    }
    if ($Facts.ExpectedSha -cne $Facts.WorkflowSha -or $Facts.ExpectedSha -cne $Facts.EventSha -or $Facts.ExpectedSha -cne $Facts.CheckoutSha) { $reasons.Add('source_identity_mismatch') }
    if ($Facts.ContainsKey('Clean') -and -not $Facts.Clean) { $reasons.Add('dirty_checkout') }
    if ($Facts.Runner -cne $script:Pin.Runner -or $Facts.Arch -cne 'X64') { $reasons.Add('runner_identity_mismatch') }
    if ($Facts.Account -cne $script:Pin.Account -or $Facts.UserProfile -ine 'C:\Users\dev') { $reasons.Add('account_identity_mismatch') }
    return [pscustomobject]@{ Valid = ($reasons.Count -eq 0); Reasons = @($reasons | Select-Object -Unique) }
}

function Get-InputDecision($Facts) {
    $reasons = [System.Collections.Generic.List[string]]::new()
    $identity = Get-StaticIdentityDecision $Facts
    $reasons.AddRange([string[]]$identity.Reasons)
    if (-not $Facts.ToolResolutionPreserved) { $reasons.Add('tool_resolution_changed') }
    if ($Facts.DestinationState -notin @('absent', 'known_receipt')) { $reasons.Add('destination_ownership_unknown') }
    if ($Facts.DestinationState -eq 'known_receipt' -and -not $Facts.ReceiptValid) { $reasons.Add('destination_receipt_invalid') }
    if ($Facts.OwnerCount -ne 0 -and $Facts.OwnerCount -ne 1) { $reasons.Add('endpoint_ownership_ambiguous') }
    if ($Facts.OwnerCount -eq 1 -and -not $Facts.OwnerIsCapturedServer) { $reasons.Add('foreign_endpoint_owner') }
    if ($Facts.RemainingMs -le $Facts.CleanupReserveMs) { $reasons.Add('deadline_expired') }
    return [pscustomobject]@{ Valid = ($reasons.Count -eq 0); Reasons = @($reasons | Select-Object -Unique) }
}

function Format-CaptureReceipt($Tail, [long]$TotalBytes, [bool]$WasTruncated) {
    $bytes = [byte[]]$Tail
    if ($bytes.Length -gt $script:Pin.TailBytes) { $bytes = $bytes[($bytes.Length - $script:Pin.TailBytes)..($bytes.Length - 1)]; $WasTruncated = $true }
    $text = [Text.Encoding]::UTF8.GetString($bytes)
    $text = [regex]::Replace($text, '(?i)\b(gh[pousr]_[A-Za-z0-9_]{16,}|github_pat_[A-Za-z0-9_]{16,})\b', '[redacted-token]')
    $text = [regex]::Replace($text, '(?i)\bBearer\s+[^\s,;]+', 'Bearer [redacted-token]')
    $text = [regex]::Replace($text, '(?i)\b(password|passwd|token|secret|authorization)\s*[:=]\s*[^\s,;]+', '$1=[redacted]')
    $text = [regex]::Replace($text, '(?i)(https?://)[^/@\s:]+:[^/@\s]+@', '$1[redacted]@')
    return [pscustomobject]@{ Text = $text; TotalBytes = $TotalBytes; Truncated = ($WasTruncated -or $TotalBytes -gt $script:Pin.TailBytes) }
}

function Get-CompletionDecision($Facts) {
    $reasons = [System.Collections.Generic.List[string]]::new()
    $ready = $Facts.StartupNotification -and $Facts.CapturedProcess -and $Facts.JobMembership -and $Facts.AccountAndImage -and $Facts.EndpointOwner -and $Facts.StatsCompleted -and $Facts.StatsCaptureComplete
    if ($Facts.Cancelled) { $reasons.Add('cancelled') }
    if ($Facts.DeadlineExpired) { $reasons.Add('deadline_expired') }
    if ($Facts.CleanupError) { $reasons.Add('cleanup_error') }
    if (-not $Facts.CaptureComplete) { $reasons.Add('capture_unconfirmed') }
    if (-not $Facts.ActiveZeroEvent) { $reasons.Add('active_zero_event_unconfirmed') }
    if ($Facts.ActiveProcesses -ne 0) { $reasons.Add('active_processes_unconfirmed') }
    if (-not $Facts.ProcessExited) { $reasons.Add('process_exit_unconfirmed') }
    if (-not $Facts.HandlesClosed) { $reasons.Add('handle_closure_unconfirmed') }
    if ($Facts.TerminationKind -eq 'deliberate' -and -not $Facts.TerminateResult) { $reasons.Add('job_termination_failed') }
    if ($Facts.TerminationKind -notin @('deliberate', 'natural')) { $reasons.Add('termination_kind_unconfirmed') }
    if (-not $ready -and $Facts.RequireReadiness) { $reasons.Add('owned_readiness_unconfirmed') }
    if ($Facts.StatsExitCode -eq 0 -and -not ($Facts.StartupNotification -and $Facts.CapturedProcess -and $Facts.JobMembership -and $Facts.AccountAndImage -and $Facts.EndpointOwner)) { $reasons.Add('stats_exit_is_not_readiness') }
    if (-not $Facts.ArtifactFresh) { $reasons.Add('artifact_not_fresh') }
    $primary = if ($Facts.PrimaryFailure) { [string]$Facts.PrimaryFailure } else { $null }
    $success = ($reasons.Count -eq 0 -and -not $primary)
    return [pscustomobject]@{ Success = $success; Ready = $ready; Reasons = @($reasons | Select-Object -Unique); PrimaryFailure = $primary; Outcome = $(if ($primary) { 'primary_failure_preserved' } elseif ($success) { 'passed' } else { 'unqualified' }) }
}

function Get-TerminationPlan($Facts) {
    if (-not $Facts.CapturedJobHandle) { return [pscustomobject]@{ Valid = $false; Action = 'refuse'; ProtocolShutdown = $false; ForeignSignal = $false; Reason = 'captured_job_handle_missing' } }
    return [pscustomobject]@{ Valid = $true; Action = 'terminate_captured_job_handle'; ProtocolShutdown = $false; ForeignSignal = $false; EndpointReuseIgnored = [bool]$Facts.EndpointReused }
}

function Invoke-ContractGroup($Name, $Cases, [scriptblock]$Decision) {
    $passed = 0
    $failed = 0
    $results = [System.Collections.Generic.List[object]]::new()
    foreach ($case in $Cases) {
        $actual = & $Decision $case.Facts
        $ok = ([bool]$actual.Valid -eq [bool]$case.Expected)
        if ($case.ContainsKey('ExpectedPrimary')) { $ok = $ok -and ([string]$actual.PrimaryFailure -ceq [string]$case.ExpectedPrimary) }
        if ($ok) { $passed++ } else { $failed++ }
        $results.Add([ordered]@{ selector = $case.Name; passed = $ok; reasons = @($actual.Reasons) })
    }
    return [pscustomobject]@{ Group = $Name; Selected = $Cases.Count; Passed = $passed; Failed = $failed; Cases = @($results) }
}

function Invoke-ContractOnly {
    $goodEntry = @{ Name = 'sccache-v0.18.0-x86_64-pc-windows-msvc/sccache.exe'; Length = 9000000L; Directory = $false; Reparse = $false; Symlink = $false; Device = $false }
    $baseArchive = @{ Size = $script:Pin.ArchiveSize; Sha256 = $script:Pin.ArchiveSha256 }
    $archiveCases = @(
        @{ Name = 'pin_and_shape_positive'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry) }; Expected = $true },
        @{ Name = 'wrong_size_refused'; Facts = @{ Size = 1L; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry) }; Expected = $false },
        @{ Name = 'wrong_digest_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = ('0' * 64); Entries = @($goodEntry) }; Expected = $false },
        @{ Name = 'missing_executable_shape_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @(@{ Name = 'other/readme'; Length = 1; Directory = $false }) }; Expected = $false },
        @{ Name = 'unexpected_archive_entry_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry, @{ Name = $script:Pin.Root + '/readme.txt'; Length = 1; Directory = $false }) }; Expected = $false },
        @{ Name = 'traversal_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry, @{ Name = '../escape'; Length = 1; Directory = $false }) }; Expected = $false },
        @{ Name = 'device_name_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry, @{ Name = $script:Pin.Root + '/NUL.txt'; Length = 1; Directory = $false }) }; Expected = $false },
        @{ Name = 'ads_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry, @{ Name = $script:Pin.Root + '/file:stream'; Length = 1; Directory = $false }) }; Expected = $false },
        @{ Name = 'reparse_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry, @{ Name = $script:Pin.Root + '/link'; Length = 1; Directory = $false; Reparse = $true }) }; Expected = $false },
        @{ Name = 'symlink_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry, @{ Name = $script:Pin.Root + '/link'; Length = 1; Directory = $false; Symlink = $true }) }; Expected = $false },
        @{ Name = 'entry_count_limit_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry) + @((1..64 | ForEach-Object { @{ Name = $script:Pin.Root + '/other' + $_; Length = 1; Directory = $false } })) }; Expected = $false },
        @{ Name = 'uncompressed_limit_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @(@{ Name = $goodEntry.Name; Length = 134217729L; Directory = $false }) }; Expected = $false },
        @{ Name = 'case_collision_refused'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = @($goodEntry, @{ Name = $goodEntry.Name; Length = 1; Directory = $false }) }; Expected = $false }
    )
    $group1 = Invoke-ContractGroup 'archive_pin_and_shape' $archiveCases {
        param($facts) Get-ArchiveDecision @{ Size = $facts.Size; Sha256 = $facts.Sha256 } $facts.Entries
    }
    $goodInput = @{ ExpectedSha = 'a' * 40; WorkflowSha = 'a' * 40; EventSha = 'a' * 40; CheckoutSha = 'a' * 40; Clean = $true; Runner = 'ROG-windows-build'; Arch = 'X64'; Account = 'amigo'; UserProfile = 'C:\Users\dev'; ToolResolutionPreserved = $true; DestinationState = 'known_receipt'; ReceiptValid = $true; OwnerCount = 0; OwnerIsCapturedServer = $false; RemainingMs = 60000; CleanupReserveMs = 30000 }
    $inputCases = @(
        @{ Name = 'known_receipt_owner_positive'; Facts = $goodInput; Expected = $true },
        @{ Name = 'absent_destination_owner_positive'; Facts = ($goodInput.Clone() + @{ DestinationState = 'absent'; ReceiptValid = $false }); Expected = $true },
        @{ Name = 'wrong_runner_refused'; Facts = ($goodInput.Clone() + @{ Runner = 'other' }); Expected = $false },
        @{ Name = 'wrong_architecture_refused'; Facts = ($goodInput.Clone() + @{ Arch = 'ARM64' }); Expected = $false },
        @{ Name = 'wrong_account_refused'; Facts = ($goodInput.Clone() + @{ Account = 'other' }); Expected = $false },
        @{ Name = 'foreign_destination_refused'; Facts = ($goodInput.Clone() + @{ DestinationState = 'foreign' }); Expected = $false },
        @{ Name = 'missing_receipt_refused'; Facts = ($goodInput.Clone() + @{ DestinationState = 'missing_receipt' }); Expected = $false },
        @{ Name = 'invalid_receipt_refused'; Facts = ($goodInput.Clone() + @{ ReceiptValid = $false }); Expected = $false },
        @{ Name = 'foreign_endpoint_owner_refused'; Facts = ($goodInput.Clone() + @{ OwnerCount = 1; OwnerIsCapturedServer = $false }); Expected = $false },
        @{ Name = 'ambiguous_ownership_refused'; Facts = ($goodInput.Clone() + @{ OwnerCount = 2 }); Expected = $false },
        @{ Name = 'tool_resolution_change_refused'; Facts = ($goodInput.Clone() + @{ ToolResolutionPreserved = $false }); Expected = $false },
        @{ Name = 'source_mismatch_refused'; Facts = ($goodInput.Clone() + @{ EventSha = 'b' * 40 }); Expected = $false },
        @{ Name = 'deadline_expiry_refused'; Facts = ($goodInput.Clone() + @{ RemainingMs = 30000 }); Expected = $false }
    )
    $group2 = Invoke-ContractGroup 'inputs_ownership_and_deadline' $inputCases { param($facts) Get-InputDecision $facts }
    $readyFacts = @{ StartupNotification = $true; CapturedProcess = $true; JobMembership = $true; AccountAndImage = $true; EndpointOwner = $true; StatsCompleted = $true; StatsCaptureComplete = $true; StatsExitCode = 0; RequireReadiness = $true; CaptureComplete = $true; ActiveZeroEvent = $true; ActiveProcesses = 0; ProcessExited = $true; HandlesClosed = $true; TerminationKind = 'deliberate'; TerminateResult = $true; Cancelled = $false; DeadlineExpired = $false; CleanupError = $false; ArtifactFresh = $true; PrimaryFailure = $null }
    $completionCases = @(
        @{ Name = 'fresh_zero_stats_owned_readiness_positive'; Facts = $readyFacts; Expected = $true },
        @{ Name = 'stats_exit_alone_refused'; Facts = ($readyFacts.Clone() + @{ StartupNotification = $false; CapturedProcess = $false; JobMembership = $false; AccountAndImage = $false; EndpointOwner = $false }); Expected = $false },
        @{ Name = 'active_process_unconfirmed_refused'; Facts = ($readyFacts.Clone() + @{ ActiveProcesses = 1 }); Expected = $false },
        @{ Name = 'active_zero_event_unconfirmed_refused'; Facts = ($readyFacts.Clone() + @{ ActiveZeroEvent = $false }); Expected = $false },
        @{ Name = 'capture_unconfirmed_refused'; Facts = ($readyFacts.Clone() + @{ CaptureComplete = $false }); Expected = $false },
        @{ Name = 'cancelled_refused'; Facts = ($readyFacts.Clone() + @{ Cancelled = $true }); Expected = $false },
        @{ Name = 'deadline_refused'; Facts = ($readyFacts.Clone() + @{ DeadlineExpired = $true }); Expected = $false },
        @{ Name = 'process_exit_unconfirmed_refused'; Facts = ($readyFacts.Clone() + @{ ProcessExited = $false }); Expected = $false },
        @{ Name = 'termination_error_refused'; Facts = ($readyFacts.Clone() + @{ TerminateResult = $false }); Expected = $false },
        @{ Name = 'cleanup_error_refused'; Facts = ($readyFacts.Clone() + @{ CleanupError = $true }); Expected = $false },
        @{ Name = 'capture_error_refused'; Facts = ($readyFacts.Clone() + @{ CaptureComplete = $false; CleanupError = $true }); Expected = $false },
        @{ Name = 'handle_close_unconfirmed_refused'; Facts = ($readyFacts.Clone() + @{ HandlesClosed = $false }); Expected = $false },
        @{ Name = 'primary_failure_preserved'; Facts = ($readyFacts.Clone() + @{ PrimaryFailure = 'case_failed' }); Expected = $false; ExpectedPrimary = 'case_failed' },
        @{ Name = 'stale_artifact_refused'; Facts = ($readyFacts.Clone() + @{ ArtifactFresh = $false }); Expected = $false },
        @{ Name = 'endpoint_reuse_terminates_only_captured_job'; Facts = @{ CapturedJobHandle = $true; EndpointReused = $true }; Expected = $true },
        @{ Name = 'missing_job_handle_refused'; Facts = @{ CapturedJobHandle = $false; EndpointReused = $true }; Expected = $false }
    )
    $group3 = Invoke-ContractGroup 'completion_output_and_freshness' $completionCases {
        param($facts)
        $decision = Get-CompletionDecision $facts
        $formatted = Format-CaptureReceipt ([Text.Encoding]::UTF8.GetBytes((('x' * 33000) + 'ghp_' + ('a' * 24))) ) 33028 $false
        $plan = Get-TerminationPlan $facts
        $valid = if ($facts.ContainsKey('CapturedJobHandle')) { $plan.Valid -and $plan.Action -eq 'terminate_captured_job_handle' -and -not $plan.ProtocolShutdown -and -not $plan.ForeignSignal -and $plan.EndpointReuseIgnored -eq [bool]$facts.EndpointReused } else { $decision.Success -and $formatted.Text.Length -le $script:Pin.TailBytes -and $formatted.Truncated -and -not $formatted.Text.Contains('ghp_') -and $formatted.TotalBytes -eq 33028 }
        [pscustomobject]@{ Valid = $valid; Reasons = $decision.Reasons; PrimaryFailure = $decision.PrimaryFailure }
    }
    $groups = @($group1, $group2, $group3)
    $selected = ($groups | Measure-Object -Property Selected -Sum).Sum
    $passed = ($groups | Measure-Object -Property Passed -Sum).Sum
    $failed = ($groups | Measure-Object -Property Failed -Sum).Sum
    $summary = [ordered]@{ groups = $groups; selected = $selected; passed = $passed; failed = $failed; outcome = $(if ($selected -gt 0 -and $failed -eq 0 -and $passed -eq $selected) { 'passed' } else { 'refused' }) }
    [Console]::Out.WriteLine(($summary | ConvertTo-Json -Compress -Depth 8))
    if ($selected -eq 0 -or $failed -ne 0 -or $passed -ne $selected) { return 1 }
    return 0
}

$script:NativeSource = @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

public static class WindowsSccacheNative {
    const uint CREATE_SUSPENDED = 0x00000004;
    const uint CREATE_NEW_PROCESS_GROUP = 0x00000200;
    const uint CREATE_NO_WINDOW = 0x08000000;
    const uint CREATE_UNICODE_ENVIRONMENT = 0x00000400;
    const uint EXTENDED_STARTUPINFO_PRESENT = 0x00080000;
    const uint STARTF_USESTDHANDLES = 0x00000100;
    const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
    const int JobObjectBasicAccountingInformation = 1;
    const int JobObjectBasicProcessIdList = 3;
    const int JobObjectAssociateCompletionPortInformation = 7;
    const int JobObjectExtendedLimitInformation = 9;
    const uint JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO = 4;
    const int PROC_THREAD_ATTRIBUTE_HANDLE_LIST = 0x00020002;
    const uint STILL_ACTIVE = 259;
    const uint WAIT_OBJECT_0 = 0;
    const uint WAIT_TIMEOUT = 258;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const uint TOKEN_QUERY = 0x0008;
    const uint GENERIC_READ = 0x80000000;
    const uint FILE_SHARE_READ = 1;
    const uint FILE_SHARE_WRITE = 2;
    const uint OPEN_EXISTING = 3;
    const uint FILE_ATTRIBUTE_NORMAL = 0x80;
    const uint HANDLE_FLAG_INHERIT = 1;

    [StructLayout(LayoutKind.Sequential)] struct SECURITY_ATTRIBUTES { public int nLength; public IntPtr lpSecurityDescriptor; public int bInheritHandle; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct STARTUPINFO { public int cb; public string lpReserved; public string lpDesktop; public string lpTitle; public uint dwX; public uint dwY; public uint dwXSize; public uint dwYSize; public uint dwXCountChars; public uint dwYCountChars; public uint dwFillAttribute; public uint dwFlags; public ushort wShowWindow; public ushort cbReserved2; public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError; }
    [StructLayout(LayoutKind.Sequential)] struct STARTUPINFOEX { public STARTUPINFO StartupInfo; public IntPtr lpAttributeList; }
    [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr hProcess; public IntPtr hThread; public uint dwProcessId; public uint dwThreadId; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_BASIC_LIMIT_INFORMATION { public long PerProcessUserTimeLimit; public long PerJobUserTimeLimit; public uint LimitFlags; public UIntPtr MinimumWorkingSetSize; public UIntPtr MaximumWorkingSetSize; public uint ActiveProcessLimit; public UIntPtr Affinity; public uint PriorityClass; public uint SchedulingClass; }
    [StructLayout(LayoutKind.Sequential)] struct IO_COUNTERS { public ulong ReadOperationCount; public ulong WriteOperationCount; public ulong OtherOperationCount; public ulong ReadTransferCount; public ulong WriteTransferCount; public ulong OtherTransferCount; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION { public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation; public IO_COUNTERS IoInfo; public UIntPtr ProcessMemoryLimit; public UIntPtr JobMemoryLimit; public UIntPtr PeakProcessMemoryUsed; public UIntPtr PeakJobMemoryUsed; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_ASSOCIATE_COMPLETION_PORT { public IntPtr CompletionKey; public IntPtr CompletionPort; }
    [StructLayout(LayoutKind.Sequential)] struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION { public long TotalUserTime; public long TotalKernelTime; public long ThisPeriodTotalUserTime; public long ThisPeriodTotalKernelTime; public uint TotalPageFaults; public uint TotalProcesses; public uint ActiveProcesses; public uint TotalTerminatedProcesses; }

    [DllImport("kernel32.dll", SetLastError = true)] static extern bool CreatePipe(out IntPtr hReadPipe, out IntPtr hWritePipe, ref SECURITY_ATTRIBUTES attrs, uint size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateFileW(string name, uint access, uint share, ref SECURITY_ATTRIBUTES attrs, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateProcessW(string app, StringBuilder command, IntPtr procAttrs, IntPtr threadAttrs, bool inherit, uint flags, StringBuilder environment, string cwd, ref STARTUPINFOEX startup, out PROCESS_INFORMATION info);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateDirectoryW(string path, IntPtr securityAttributes);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetProcessTimes(IntPtr process, out long creation, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool QueryFullProcessImageNameW(IntPtr process, uint flags, StringBuilder image, ref int size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool GetTokenInformation(IntPtr token, int infoClass, IntPtr info, int length, out int needed);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateJobObjectW(IntPtr attrs, string name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length, out uint needed);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateIoCompletionPort(IntPtr file, IntPtr existing, UIntPtr key, uint threads);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetQueuedCompletionStatus(IntPtr port, out uint bytes, out UIntPtr key, out IntPtr overlapped, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("iphlpapi.dll", SetLastError = true)] static extern int GetExtendedTcpTable(byte[] table, ref int size, bool order, int family, int tableClass, uint reserved);

    static Exception Last(string op) { return new Win32Exception(Marshal.GetLastWin32Error(), op); }
    static void Need(bool ok, string op) { if (!ok) throw Last(op); }
    static int Remaining(long until) { long left = until - Environment.TickCount64; return left <= 0 ? 0 : (int)Math.Min(Int32.MaxValue, left); }

    public sealed class Capture : IDisposable {
        readonly IntPtr rawHandle;
        readonly SafeFileHandle handle;
        readonly object gate = new object();
        readonly byte[] tail = new byte[32768];
        int used;
        long total;
        bool truncated;
        string error;
        public readonly ManualResetEvent Eof = new ManualResetEvent(false);
        readonly Task reader;
        public Capture(IntPtr readHandle) {
            rawHandle = readHandle; handle = new SafeFileHandle(readHandle, false);
            reader = Task.Run(() => {
                try {
                    using (var stream = new FileStream(handle, FileAccess.Read, 4096, false)) {
                        byte[] chunk = new byte[8192];
                        int n;
                        while ((n = stream.Read(chunk, 0, chunk.Length)) != 0) Append(chunk, n);
                    }
                } catch (Exception e) { lock (gate) error = e.GetType().Name; }
                finally { Eof.Set(); }
            });
        }
        void Append(byte[] source, int count) {
            lock (gate) {
                total += count;
                if (count >= tail.Length) { Buffer.BlockCopy(source, count - tail.Length, tail, 0, tail.Length); used = tail.Length; truncated = true; return; }
                if (used + count > tail.Length) { int drop = used + count - tail.Length; Buffer.BlockCopy(tail, drop, tail, 0, used - drop); used -= drop; truncated = true; }
                Buffer.BlockCopy(source, 0, tail, used, count); used += count;
            }
        }
        public byte[] Tail { get { lock (gate) { var value = new byte[used]; Buffer.BlockCopy(tail, 0, value, 0, used); return value; } } }
        public long TotalBytes { get { lock (gate) return total; } }
        public bool Truncated { get { lock (gate) return truncated; } }
        public string Error { get { lock (gate) return error; } }
        public bool WaitEof(int milliseconds) { return Eof.WaitOne(milliseconds); }
        public bool WaitReader(int milliseconds) { return reader.Wait(Math.Max(0, milliseconds)); }
        public bool ReaderComplete { get { return reader.IsCompleted; } }
        public string Text { get { return Encoding.UTF8.GetString(Tail); } }
        public bool CloseChecked(int milliseconds) {
            bool eofBeforeClose = Eof.WaitOne(0);
            handle.Dispose(); bool closed = CloseHandle(rawHandle);
            bool settled = reader.Wait(Math.Max(0, milliseconds));
            bool eof = Eof.WaitOne(0);
            string captureError = Error;
            if (settled) Eof.Dispose();
            return closed && eofBeforeClose && settled && eof && captureError == null;
        }
        public void Dispose() { CloseChecked(0); }
    }

    public sealed class ProcessLease : IDisposable {
        public readonly IntPtr Handle;
        public readonly uint Pid;
        public readonly Capture Stdout;
        public readonly Capture Stderr;
        public ProcessLease(IntPtr process, uint pid, Capture stdout, Capture stderr) { Handle = process; Pid = pid; Stdout = stdout; Stderr = stderr; }
        public bool Wait(int milliseconds) { return WaitForSingleObject(Handle, (uint)Math.Max(0, milliseconds)) == WAIT_OBJECT_0; }
        public uint ExitCode { get { uint code; Need(GetExitCodeProcess(Handle, out code), "GetExitCodeProcess"); return code; } }
        public bool Alive { get { uint code; Need(GetExitCodeProcess(Handle, out code), "GetExitCodeProcess"); return code == STILL_ACTIVE && WaitForSingleObject(Handle, 0) == WAIT_TIMEOUT; } }
        public long CreationFileTime { get { long created, exited, kernel, user; Need(GetProcessTimes(Handle, out created, out exited, out kernel, out user), "GetProcessTimes"); return created; } }
        public string Image { get { var b = new StringBuilder(32768); int n = b.Capacity; Need(QueryFullProcessImageNameW(Handle, 0, b, ref n), "QueryFullProcessImageNameW"); return b.ToString(0, n); } }
        public string UserSid {
            get {
                IntPtr token; Need(OpenProcessToken(Handle, TOKEN_QUERY, out token), "OpenProcessToken");
                try {
                    int needed; GetTokenInformation(token, 1, IntPtr.Zero, 0, out needed);
                    if (needed <= 0 || needed > 65536) throw new InvalidOperationException("token_information_size_invalid");
                    IntPtr data = Marshal.AllocHGlobal(needed);
                    try { Need(GetTokenInformation(token, 1, data, needed, out needed), "GetTokenInformation"); return new SecurityIdentifier(Marshal.ReadIntPtr(data)).Value; }
                    finally { Marshal.FreeHGlobal(data); }
                } finally { Need(CloseHandle(token), "CloseHandle(process_token)"); }
            }
        }
        public bool CloseChecked(int milliseconds) {
            long until = Environment.TickCount64 + Math.Max(0, milliseconds);
            bool a = Stdout.CloseChecked(Remaining(until)); bool b = Stderr.CloseChecked(Remaining(until)); bool c = CloseHandle(Handle);
            return a && b && c;
        }
        public void Dispose() { CloseChecked(0); }
    }

    public sealed class JobLease : IDisposable {
        public readonly IntPtr Handle;
        readonly IntPtr port;
        public JobLease() {
            Handle = CreateJobObjectW(IntPtr.Zero, null); if (Handle == IntPtr.Zero) throw Last("CreateJobObjectW");
            port = IntPtr.Zero;
            try {
                port = CreateIoCompletionPort(new IntPtr(-1), IntPtr.Zero, UIntPtr.Zero, 0); if (port == IntPtr.Zero) throw Last("CreateIoCompletionPort");
                var limits = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION(); limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION)));
                try { Marshal.StructureToPtr(limits, p, false); Need(SetInformationJobObject(Handle, JobObjectExtendedLimitInformation, p, (uint)Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION))), "SetInformationJobObject(limits)"); }
                finally { Marshal.FreeHGlobal(p); }
                var assoc = new JOBOBJECT_ASSOCIATE_COMPLETION_PORT { CompletionKey = new IntPtr(1), CompletionPort = port };
                p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(JOBOBJECT_ASSOCIATE_COMPLETION_PORT)));
                try { Marshal.StructureToPtr(assoc, p, false); Need(SetInformationJobObject(Handle, JobObjectAssociateCompletionPortInformation, p, (uint)Marshal.SizeOf(typeof(JOBOBJECT_ASSOCIATE_COMPLETION_PORT))), "SetInformationJobObject(completion)"); }
                finally { Marshal.FreeHGlobal(p); }
            } catch (Exception original) {
                bool portClosed = port == IntPtr.Zero || CloseHandle(port);
                bool jobClosed = CloseHandle(Handle);
                if (!portClosed || !jobClosed) throw new InvalidOperationException("partial_job_handle_close_unconfirmed", original);
                throw;
            }
        }
        public uint ActiveCount {
            get {
                IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)));
                try { uint needed; Need(QueryInformationJobObject(Handle, JobObjectBasicAccountingInformation, p, (uint)Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)), out needed), "QueryInformationJobObject(accounting)"); return ((JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)Marshal.PtrToStructure(p, typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION))).ActiveProcesses; }
                finally { Marshal.FreeHGlobal(p); }
            }
        }
        public uint[] Members() {
            const int capacity = 8192;
            int length = 8 + capacity * IntPtr.Size;
            IntPtr p = Marshal.AllocHGlobal(length);
            try {
                uint needed; Need(QueryInformationJobObject(Handle, JobObjectBasicProcessIdList, p, (uint)length, out needed), "QueryInformationJobObject(process_ids)");
                uint assigned = unchecked((uint)Marshal.ReadInt32(p)); uint listed = unchecked((uint)Marshal.ReadInt32(p, 4));
                if (listed > capacity || assigned != listed) throw new InvalidOperationException("job_process_list_incomplete");
                var values = new uint[(int)listed];
                for (int i = 0; i < (int)listed; i++) values[i] = unchecked((uint)Marshal.ReadIntPtr(p, 8 + i * IntPtr.Size).ToInt64());
                return values;
            } finally { Marshal.FreeHGlobal(p); }
        }
        public bool Contains(IntPtr process) { bool result; Need(IsProcessInJob(process, Handle, out result), "IsProcessInJob"); return result; }
        public bool Terminate(uint exitCode) { return TerminateJobObject(Handle, exitCode); }
        public bool WaitForZero(int milliseconds) {
            long until = Environment.TickCount64 + Math.Max(0, milliseconds);
            while (true) {
                int left = Remaining(until); if (left == 0) return false;
                uint bytes; UIntPtr key; IntPtr overlapped;
                bool ok = GetQueuedCompletionStatus(port, out bytes, out key, out overlapped, (uint)left);
                if (!ok) { int err = Marshal.GetLastWin32Error(); if (overlapped == IntPtr.Zero && err == 258) return false; throw new Win32Exception(err, "GetQueuedCompletionStatus"); }
                if (key == new UIntPtr(1) && bytes == JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO && ActiveCount == 0) return true;
            }
        }
        public bool CloseChecked() { bool a = CloseHandle(port); bool b = CloseHandle(Handle); return a && b; }
        public void Dispose() { CloseChecked(); }
    }

    public static JobLease CreateJob() { return new JobLease(); }
    public static void CreateDirectoryNew(string path) { Need(CreateDirectoryW(path, IntPtr.Zero), "CreateDirectoryW"); }
    public static ProcessLease Launch(IntPtr job, string app, string commandLine, string cwd, IDictionary<string,string> env, int cleanupMs) {
        SECURITY_ATTRIBUTES sa = new SECURITY_ATTRIBUTES { nLength = Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)), bInheritHandle = 1 };
        IntPtr outRead = IntPtr.Zero, outWrite = IntPtr.Zero, errRead = IntPtr.Zero, errWrite = IntPtr.Zero, nullIn = IntPtr.Zero;
        IntPtr attrs = IntPtr.Zero, handleArray = IntPtr.Zero, list = IntPtr.Zero;
        PROCESS_INFORMATION pi = new PROCESS_INFORMATION();
        bool created = false;
        bool assigned = false;
        bool processTransferred = false;
        Capture stdout = null, stderr = null;
        try {
            Need(CreatePipe(out outRead, out outWrite, ref sa, 0), "CreatePipe(stdout)");
            Need(CreatePipe(out errRead, out errWrite, ref sa, 0), "CreatePipe(stderr)");
            Need(SetHandleInformation(outRead, HANDLE_FLAG_INHERIT, 0), "SetHandleInformation(stdout)");
            Need(SetHandleInformation(errRead, HANDLE_FLAG_INHERIT, 0), "SetHandleInformation(stderr)");
            nullIn = CreateFileW("NUL", GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, ref sa, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, IntPtr.Zero);
            if (nullIn == new IntPtr(-1)) throw Last("CreateFileW(NUL)");
            var handles = new[] { nullIn, outWrite, errWrite };
            IntPtr attrSize = IntPtr.Zero; InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attrSize);
            if (attrSize == IntPtr.Zero || attrSize.ToInt64() > 65536) throw new InvalidOperationException("process_attribute_size_invalid");
            attrs = Marshal.AllocHGlobal(attrSize); handleArray = Marshal.AllocHGlobal(handles.Length * IntPtr.Size);
            Need(InitializeProcThreadAttributeList(attrs, 1, 0, ref attrSize), "InitializeProcThreadAttributeList"); list = attrs;
            for (int i = 0; i < handles.Length; i++) Marshal.WriteIntPtr(handleArray, i * IntPtr.Size, handles[i]);
            Need(UpdateProcThreadAttribute(list, 0, new IntPtr(PROC_THREAD_ATTRIBUTE_HANDLE_LIST), handleArray, new IntPtr(handles.Length * IntPtr.Size), IntPtr.Zero, IntPtr.Zero), "UpdateProcThreadAttribute");
            var si = new STARTUPINFOEX(); si.lpAttributeList = list; si.StartupInfo.cb = Marshal.SizeOf(typeof(STARTUPINFOEX)); si.StartupInfo.dwFlags = STARTF_USESTDHANDLES; si.StartupInfo.hStdInput = nullIn; si.StartupInfo.hStdOutput = outWrite; si.StartupInfo.hStdError = errWrite;
            var envText = new StringBuilder(); foreach (var pair in env) envText.Append(pair.Key).Append('=').Append(pair.Value).Append('\0'); envText.Append('\0');
            var mutableCommand = new StringBuilder(commandLine);
            Need(CreateProcessW(app, mutableCommand, IntPtr.Zero, IntPtr.Zero, true, CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT, envText, cwd, ref si, out pi), "CreateProcessW"); created = true;
            Need(AssignProcessToJobObject(job, pi.hProcess), "AssignProcessToJobObject");
            assigned = true;
            Need(CloseHandle(outWrite), "CloseHandle(stdout_write)"); outWrite = IntPtr.Zero;
            Need(CloseHandle(errWrite), "CloseHandle(stderr_write)"); errWrite = IntPtr.Zero;
            Need(CloseHandle(nullIn), "CloseHandle(stdin_null)"); nullIn = IntPtr.Zero;
            stdout = new Capture(outRead); outRead = IntPtr.Zero;
            stderr = new Capture(errRead); errRead = IntPtr.Zero;
            if (ResumeThread(pi.hThread) == UInt32.MaxValue) throw Last("ResumeThread");
            Need(CloseHandle(pi.hThread), "CloseHandle(primary_thread)"); pi.hThread = IntPtr.Zero;
            var result = new ProcessLease(pi.hProcess, pi.dwProcessId, stdout, stderr);
            processTransferred = true;
            return result;
        } catch (Exception original) {
            long until = Environment.TickCount64 + Math.Max(0, cleanupMs);
            bool exited = !created;
            bool unassignedExitUnconfirmed = false;
            if (created && !assigned) { bool requested = TerminateProcess(pi.hProcess, 1); exited = WaitForSingleObject(pi.hProcess, (uint)Remaining(until)) == WAIT_OBJECT_0; unassignedExitUnconfirmed = !requested && !exited; }
            if (stdout != null) { if (exited) stdout.WaitEof(Remaining(until)); stdout.CloseChecked(assigned ? 0 : Remaining(until)); }
            if (stderr != null) { if (exited) stderr.WaitEof(Remaining(until)); stderr.CloseChecked(assigned ? 0 : Remaining(until)); }
            if (pi.hThread != IntPtr.Zero) CloseHandle(pi.hThread);
            if (pi.hProcess != IntPtr.Zero && !processTransferred) CloseHandle(pi.hProcess);
            if (unassignedExitUnconfirmed) throw new InvalidOperationException("unassigned_suspended_child_termination_unconfirmed", original);
            if (created && !assigned && !exited) throw new InvalidOperationException("unassigned_suspended_child_exit_unconfirmed", original);
            if (assigned) throw new InvalidOperationException("assigned_child_launch_failed_job_cleanup_required", original);
            throw;
        } finally {
            if (list != IntPtr.Zero) DeleteProcThreadAttributeList(list);
            if (attrs != IntPtr.Zero) Marshal.FreeHGlobal(attrs); if (handleArray != IntPtr.Zero) Marshal.FreeHGlobal(handleArray);
            foreach (var h in new[] { outRead, outWrite, errRead, errWrite, nullIn }) if (h != IntPtr.Zero && h != new IntPtr(-1)) CloseHandle(h);
        }
    }
    public static bool InJob(IntPtr process, IntPtr job) { bool result; Need(IsProcessInJob(process, job, out result), "IsProcessInJob"); return result; }
    public static IntPtr OpenProcessHandle(uint pid) { IntPtr p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid); if (p == IntPtr.Zero) throw Last("OpenProcess"); return p; }
    public static long ProcessCreation(IntPtr p) { long created, exited, kernel, user; Need(GetProcessTimes(p, out created, out exited, out kernel, out user), "GetProcessTimes"); return created; }
    public static string ProcessImage(IntPtr p) { var b = new StringBuilder(32768); int n = b.Capacity; Need(QueryFullProcessImageNameW(p, 0, b, ref n), "QueryFullProcessImageNameW"); return b.ToString(0, n); }
    public static string ProcessUserSid(IntPtr p) {
        IntPtr token; Need(OpenProcessToken(p, TOKEN_QUERY, out token), "OpenProcessToken");
        try { int needed; GetTokenInformation(token, 1, IntPtr.Zero, 0, out needed); if (needed <= 0 || needed > 65536) throw new InvalidOperationException("token_information_size_invalid"); IntPtr data = Marshal.AllocHGlobal(needed); try { Need(GetTokenInformation(token, 1, data, needed, out needed), "GetTokenInformation"); return new SecurityIdentifier(Marshal.ReadIntPtr(data)).Value; } finally { Marshal.FreeHGlobal(data); } }
        finally { Need(CloseHandle(token), "CloseHandle(process_token)"); }
    }
    public static bool HandleAlive(IntPtr p) { uint code; Need(GetExitCodeProcess(p, out code), "GetExitCodeProcess"); return code == STILL_ACTIVE && WaitForSingleObject(p, 0) == WAIT_TIMEOUT; }
    public static uint ExitCodeHandle(IntPtr p) { uint code; Need(GetExitCodeProcess(p, out code), "GetExitCodeProcess"); return code; }
    public static bool WaitHandle(IntPtr p, int ms) { return WaitForSingleObject(p, (uint)Math.Max(0, ms)) == WAIT_OBJECT_0; }
    public static bool WaitCapture(Capture c, int ms) { return c.WaitEof(ms); }
    public static object CaptureFacts(Capture c) { return new { Tail = c.Tail, TotalBytes = c.TotalBytes, Truncated = c.Truncated, Error = c.Error, Eof = c.Eof.WaitOne(0), ReaderComplete = c.ReaderComplete }; }
    static byte[] TcpTable(int family) {
        int size = 1024 * 1024; byte[] data = new byte[size]; int result = GetExtendedTcpTable(data, ref size, false, family, 3, 0);
        if (result != 0) throw new Win32Exception(result, "GetExtendedTcpTable"); return data;
    }
    public static object[] Listeners(int port) {
        var found = new List<object>();
        byte[] v4 = TcpTable(2); int n4 = BitConverter.ToInt32(v4, 0); if (n4 < 0 || n4 > (v4.Length - 4) / 24) throw new InvalidOperationException("tcp_v4_table_invalid");
        for (int i = 0; i < n4; i++) { int o = 4 + i * 24; uint p = BitConverter.ToUInt32(v4, o + 4); int hostPort = (int)(((p & 255) << 8) | ((p >> 8) & 255)); if (hostPort == port) { uint addr = BitConverter.ToUInt32(v4, o); found.Add(new { Family = 4, Address = new IPAddress(BitConverter.GetBytes(addr)).ToString(), Pid = BitConverter.ToUInt32(v4, o + 20) }); } }
        byte[] v6 = TcpTable(23); int n6 = BitConverter.ToInt32(v6, 0); if (n6 < 0 || n6 > (v6.Length - 4) / 56) throw new InvalidOperationException("tcp_v6_table_invalid");
        for (int i = 0; i < n6; i++) { int o = 4 + i * 56; uint p = BitConverter.ToUInt32(v6, o + 20); int hostPort = (int)(((p & 255) << 8) | ((p >> 8) & 255)); if (hostPort == port) { var a = new byte[16]; Buffer.BlockCopy(v6, o, a, 0, 16); found.Add(new { Family = 6, Address = new IPAddress(a, BitConverter.ToUInt32(v6, o + 16)).ToString(), Pid = BitConverter.ToUInt32(v6, o + 52) }); } }
        return found.ToArray();
    }
    public static bool WaitJobZero(JobLease job, int ms) { return job.WaitForZero(ms); }
    public static uint ActiveCount(JobLease job) { return job.ActiveCount; }
    public static uint[] Members(JobLease job) { return job.Members(); }
    public static object Snapshot(JobLease job) { uint[] members = job.Members(); return new { ActiveCount = (uint)members.Length, Members = members }; }
    public static bool Contains(JobLease job, IntPtr process) { return job.Contains(process); }
    public static bool Terminate(JobLease job, uint code) { return job.Terminate(code); }
    public static bool CloseHandlePublic(IntPtr handle) { return handle != IntPtr.Zero && CloseHandle(handle); }
}
'@

function Initialize-WindowsSccacheNative {
    if (-not ('WindowsSccacheNative' -as [type])) { Add-Type -TypeDefinition $script:NativeSource -Language CSharp -ErrorAction Stop }
}

function Get-RemainingMs([long]$DeadlineTicks, [int]$ReserveMs = 0) {
    $remaining = [long](($DeadlineTicks - [Diagnostics.Stopwatch]::GetTimestamp()) * 1000 / [Diagnostics.Stopwatch]::Frequency) - $ReserveMs
    if ($remaining -lt 0) { return 0 }
    return [int][Math]::Min([int]::MaxValue, $remaining)
}

function Get-RunIdentity {
    $root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    if (Test-Path -LiteralPath (Join-Path $root '.env')) { throw 'ambient_dotenv_refused' }
    $liveBindings = @(Get-ChildItem Env: | Where-Object { $_.Name -match '^CODETERM_' -and $_.Name -ne 'CODETERM_EMBED_CACHE_DIR' -and $_.Value })
    if ($liveBindings.Count -ne 0 -or $env:CARGO_TARGET_DIR -or $env:CARGO_TEST_CMD -or $env:CARGO_TEST_BUILD_CMD) { throw 'ambient_instance_or_target_override_refused' }
    $expected = $env:EXPECTED_SOURCE_SHA
    $workflow = $env:WORKFLOW_SOURCE_SHA
    $eventSha = $env:EVENT_SOURCE_SHA
    $git = (Get-Command git -CommandType Application -ErrorAction Stop | Select-Object -First 1).Path
    $envMap = Get-ChildEnvironment -ToolPaths @{ git = $git }
    $headProc = Launch-OwnedProcess $script:Session $git ((Quote-WindowsArgument $git) + ' rev-parse HEAD') $root $envMap
    Wait-OwnedProcess $script:Session $headProc $script:Session.WorkDeadlineTicks
    $headText = $headProc.Stdout.Text.Trim()
    if ($headProc.ExitCode -ne 0 -or $headText -cnotmatch '^[0-9a-f]{40}$') { throw 'checkout_identity_unavailable' }
    $statusProc = Launch-OwnedProcess $script:Session $git ((Quote-WindowsArgument $git) + ' status --porcelain=v1 --untracked-files=all') $root $envMap
    Wait-OwnedProcess $script:Session $statusProc $script:Session.WorkDeadlineTicks
    $status = $statusProc.Stdout.Text.Trim()
    $facts = @{ ExpectedSha = $expected; WorkflowSha = $workflow; EventSha = $eventSha; CheckoutSha = $headText; Clean = ($statusProc.ExitCode -eq 0 -and -not $status); Runner = $env:RUNNER_NAME; Arch = $env:RUNNER_ARCH; Account = [Security.Principal.WindowsIdentity]::GetCurrent().Name.Split('\')[-1]; UserProfile = $env:USERPROFILE; ToolResolutionPreserved = $true; DestinationState = 'absent'; ReceiptValid = $false; OwnerCount = 0; OwnerIsCapturedServer = $false; RemainingMs = (Get-RemainingMs $script:Session.WorkDeadlineTicks); CleanupReserveMs = $script:Pin.CleanupReserveMs }
    $decision = Get-InputDecision $facts
    if (-not $decision.Valid) { throw ('source_runner_or_checkout_refused:' + ($decision.Reasons -join ',')) }
    return [pscustomobject]@{ Root = $root; Head = $headText; Git = $git; UserSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value }
}

function Quote-WindowsArgument([string]$Value) {
    if ($Value -notmatch '[\s"]') { return $Value }
    $builder = [Text.StringBuilder]::new('"')
    $slashes = 0
    foreach ($char in $Value.ToCharArray()) {
        if ($char -eq '\') { $slashes++; continue }
        if ($char -eq '"') { [void]$builder.Append(('\' * (2 * $slashes + 1))).Append('"'); $slashes = 0; continue }
        if ($slashes) { [void]$builder.Append(('\' * $slashes)); $slashes = 0 }
        [void]$builder.Append($char)
    }
    if ($slashes) { [void]$builder.Append(('\' * (2 * $slashes)) }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function Launch-OwnedProcess($Session, [string]$Executable, [string]$CommandLine, [string]$WorkingDirectory, $Environment) {
    $cleanupMs = Get-RemainingMs $Session.CleanupDeadlineTicks
    try {
        $process = [WindowsSccacheNative]::Launch($Session.Job.Handle, $Executable, $CommandLine, $WorkingDirectory, $Environment, $cleanupMs)
        $Session.ProcessTiming[[string]$process.Pid] = [ordered]@{ launched_ticks = [Diagnostics.Stopwatch]::GetTimestamp(); elapsed_ms = $null }
        return $process
    }
    catch { $Session.LaunchFailure = $true; throw }
}

function Get-ToolResolutions {
    $result = [ordered]@{}
    foreach ($name in @('rustup', 'rustc', 'cargo', 'node')) {
        $resolved = Get-Command $name -CommandType Application -ErrorAction Stop | Select-Object -First 1
        $result[$name] = $resolved.Path
    }
    return $result
}

function Get-ChildEnvironment($ToolPaths) {
    $tools = @{}
    if ($ToolPaths) { foreach ($key in $ToolPaths.Keys) { $tools[$key] = $ToolPaths[$key] } }
    foreach ($name in @('rustup', 'rustc', 'cargo', 'node')) {
        $hit = Get-Command $name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($hit) { $tools[$name] = $hit.Path }
    }
    $dirs = [System.Collections.Generic.List[string]]::new()
    foreach ($path in $tools.Values) { $dir = Split-Path -Parent $path; if ($dir -and -not $dirs.Contains($dir)) { $dirs.Add($dir) } }
    foreach ($dir in @((Join-Path $env:SystemRoot 'System32'), (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0'), (Join-Path $env:ProgramFiles 'Git\cmd'))) { if (Test-Path -LiteralPath $dir -PathType Container -and -not $dirs.Contains($dir)) { $dirs.Add($dir) } }
    $names = @('SystemRoot','WINDIR','USERPROFILE','HOMEDRIVE','HOMEPATH','TEMP','TMP','LOCALAPPDATA','APPDATA','PROCESSOR_ARCHITECTURE','INCLUDE','LIB','LIBPATH','VCToolsInstallDir','VCINSTALLDIR','VisualStudioVersion','WindowsSdkDir','WindowsSDKVersion','UCRTVersion','VSCMD_VER','VSCMD_ARG_TGT_ARCH')
    $map = [System.Collections.Generic.SortedDictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($name in $names) { $value = [Environment]::GetEnvironmentVariable($name, 'Process'); if ($null -ne $value) { $map[$name] = $value } }
    $map['PATH'] = $env:PATH
    $map['SCCACHE_CONF'] = $script:Session.ConfigPath
    $map['SCCACHE_DIR'] = $script:Session.CachePath
    $map['SCCACHE_SERVER_PORT'] = [string]$script:Pin.ServerPort
    return $map
}

function Get-ProcessCommandLine([string]$Executable, [string[]]$Arguments) {
    return ((@((Quote-WindowsArgument $Executable)) + @($Arguments | ForEach-Object { Quote-WindowsArgument ([string]$_) })) -join ' ')
}

function Wait-OwnedProcess($Session, $Process, [long]$DeadlineTicks) {
    if (-not $Session.Processes.Contains($Process)) { $Session.Processes.Add($Process) }
    try {
        $remaining = Get-RemainingMs $DeadlineTicks
        if ($remaining -le 0 -or -not $Process.Wait($remaining)) { throw 'child_exit_deadline_unconfirmed' }
        foreach ($capture in @($Process.Stdout, $Process.Stderr)) {
            $remaining = Get-RemainingMs $DeadlineTicks
            if ($remaining -le 0 -or -not $capture.WaitEof($remaining)) { throw 'child_capture_eof_unconfirmed' }
            $remaining = Get-RemainingMs $DeadlineTicks
            if ($remaining -le 0 -or -not $capture.WaitReader($remaining)) { throw 'child_capture_completion_unconfirmed' }
            if ($capture.Error) { throw 'child_capture_error' }
        }
    } finally {
        $timing = $Session.ProcessTiming[[string]$Process.Pid]
        $now = [Diagnostics.Stopwatch]::GetTimestamp()
        $timing.completed_ticks = $now
        $timing.elapsed_ms = [long](($now - $timing.launched_ticks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
    }
}

function Get-CaptureRecord($Capture) {
    $facts = [WindowsSccacheNative]::CaptureFacts($Capture)
    $tail = Format-CaptureReceipt $facts.Tail ([long]$facts.TotalBytes) ([bool]$facts.Truncated)
    return [ordered]@{ total_bytes = $tail.TotalBytes; truncated = $tail.Truncated; output_tail = $tail.Text; eof = [bool]$facts.Eof; reader_complete = [bool]$facts.ReaderComplete; error = $facts.Error }
}

function Get-ArchiveEntries([string]$Path) {
    $entries = [System.Collections.Generic.List[object]]::new()
    $archive = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        if ($archive.Entries.Count -gt 64) { throw 'archive_entry_count_exceeded' }
        foreach ($item in $archive.Entries) {
            $unixMode = (($item.ExternalAttributes -shr 16) -band 0xF000)
            $dosAttrs = ($item.ExternalAttributes -band 0xFFFF)
            $directory = $item.FullName.EndsWith('/') -or $item.FullName.EndsWith('\')
            $entries.Add(@{ Name = $item.FullName; Length = [long]$item.Length; Directory = $directory; Symlink = ($unixMode -eq 0xA000); Device = ($unixMode -in @(0x2000,0x6000,0xC000)); UnknownType = ($unixMode -ne 0 -and $unixMode -notin @(0x8000,0x4000,0xA000,0x2000,0x6000,0xC000)); Reparse = (($dosAttrs -band 0x400) -ne 0) })
        }
    } finally { $archive.Dispose() }
    return ,$entries.ToArray()
}

function Get-PEIdentity([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        if ($stream.Length -lt 128 -or $stream.Length -gt 134217728L) { throw 'pe_file_size_out_of_bounds' }
        $reader = [IO.BinaryReader]::new($stream)
        if ($reader.ReadUInt16() -ne 0x5A4D) { throw 'pe_dos_signature_invalid' }
        $stream.Position = 0x3C; $offset = $reader.ReadInt32()
        if ($offset -lt 64 -or $offset -gt ($stream.Length - 24)) { throw 'pe_header_offset_invalid' }
        $stream.Position = $offset
        if ($reader.ReadUInt32() -ne 0x00004550) { throw 'pe_signature_invalid' }
        $machine = $reader.ReadUInt16(); $sections = $reader.ReadUInt16(); $stream.Position = $offset + 22; $characteristics = $reader.ReadUInt16(); $stream.Position = $offset + 24; $magic = $reader.ReadUInt16()
        if ($machine -ne 0x8664 -or $magic -ne 0x20B -or $sections -lt 1 -or ($characteristics -band 0x0002) -eq 0 -or ($characteristics -band 0x2000) -ne 0) { throw 'pe_x64_identity_invalid' }
        return [pscustomobject]@{ Machine = 'x64'; Size = $stream.Length; Sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
    } finally { $stream.Dispose() }
}

function Get-InstallPaths {
    if ($env:USERPROFILE -ine 'C:\Users\dev' -or [IO.Path]::GetFullPath($env:USERPROFILE) -ine 'C:\Users\dev') { throw 'runner_profile_mismatch' }
    $toolsRoot = Join-Path $env:USERPROFILE 'tools\sccache'
    $destination = Join-Path $env:USERPROFILE $script:Pin.Destination
    return [pscustomobject]@{ Root = $toolsRoot; Destination = $destination; Executable = Join-Path $destination $script:Pin.Executable; Receipt = Join-Path $destination 'receipt.json' }
}

function Test-InstalledReceipt($Paths) {
    if (-not (Test-Path -LiteralPath $Paths.Destination -PathType Container)) { return $false }
    $directory = Get-Item -LiteralPath $Paths.Destination -Force
    if ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'destination_reparse_refused' }
    $items = @(Get-ChildItem -LiteralPath $Paths.Destination -Force)
    if ($items.Count -ne 2 -or -not (Test-Path -LiteralPath $Paths.Executable -PathType Leaf) -or -not (Test-Path -LiteralPath $Paths.Receipt -PathType Leaf)) { throw 'destination_contains_unowned_files' }
    foreach ($item in $items) { if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'destination_file_reparse_refused' } }
    $receipt = Get-Content -LiteralPath $Paths.Receipt -Raw | ConvertFrom-Json
    $pe = Get-PEIdentity $Paths.Executable
    if ($receipt.schema -ne 1 -or $receipt.archive_sha256 -cne $script:Pin.ArchiveSha256 -or [long]$receipt.archive_size -ne $script:Pin.ArchiveSize -or $receipt.exe_sha256 -cne $pe.Sha256 -or [long]$receipt.exe_size -ne $pe.Size -or $receipt.source_commit -cne $script:Pin.SourceCommit -or $receipt.version -cne $script:Pin.Version -or $receipt.runner -cne $script:Pin.Runner -or $receipt.account -cne $script:Pin.Account -or $pe.Machine -ne 'x64') { throw 'destination_receipt_mismatch' }
    return $true
}

function Receive-PinnedArchive([string]$Directory, [long]$DeadlineTicks) {
    $path = Join-Path $Directory 'sccache-v0.18.0-x64.zip'
    $handler = [Net.Http.HttpClientHandler]::new(); $handler.AllowAutoRedirect = $false; $handler.UseDefaultCredentials = $false; $handler.Credentials = $null; $handler.DefaultProxyCredentials = $null
    $client = [Net.Http.HttpClient]::new($handler); $client.Timeout = [Threading.Timeout]::InfiniteTimeSpan
    try {
        $uri = [Uri]$script:Pin.ArchiveUrl
        $response = $null
        for ($redirect = 0; $redirect -le 2; $redirect++) {
            $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get, $uri)
            $remaining = Get-RemainingMs $DeadlineTicks
            if ($remaining -le 0) { throw 'download_deadline_expired' }
            $cancellation = [Threading.CancellationTokenSource]::new($remaining)
            try { $response = $client.SendAsync($request, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancellation.Token).GetAwaiter().GetResult() }
            finally { $cancellation.Dispose(); $request.Dispose() }
            if ($response.StatusCode -in @([Net.HttpStatusCode]::MovedPermanently,[Net.HttpStatusCode]::Found,[Net.HttpStatusCode]::SeeOther,[Net.HttpStatusCode]::TemporaryRedirect,[Net.HttpStatusCode]::PermanentRedirect)) {
                if ($redirect -ge 2 -or -not $response.Headers.Location) { throw 'release_redirect_limit' }
                $next = [Uri]::new($uri, $response.Headers.Location); $response.Dispose()
                if ($next.Scheme -ne 'https' -or $next.Port -ne 443 -or $next.UserInfo -or $next.Host -notin @('github.com','release-assets.githubusercontent.com','objects.githubusercontent.com')) { throw 'release_redirect_host_refused' }
                $uri = $next; continue
            }
            if (-not $response.IsSuccessStatusCode) { throw 'release_http_status_refused' }
            break
        }
        if (-not $response -or -not $response.Content.Headers.ContentLength -or $response.Content.Headers.ContentLength -ne $script:Pin.ArchiveSize) { throw 'release_content_length_mismatch' }
        $inputStream = $null; $output = $null; $hash = $null
        try {
            $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
            $output = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            $hash = [Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
            $buffer = [byte[]]::new(65536); $total = 0L
            while ($true) {
                $remaining = Get-RemainingMs $DeadlineTicks
                if ($remaining -le 0) { throw 'download_deadline_expired' }
                $cancellation = [Threading.CancellationTokenSource]::new($remaining)
                try { $read = $inputStream.ReadAsync($buffer, 0, $buffer.Length, $cancellation.Token).GetAwaiter().GetResult() }
                finally { $cancellation.Dispose() }
                if ($read -eq 0) { break }
                $total += $read
                if ($total -gt $script:Pin.ArchiveSize) { throw 'release_archive_too_large' }
                $hash.AppendData($buffer, 0, $read); $output.Write($buffer, 0, $read)
            }
            $output.Flush(); $digest = ([Convert]::ToHexString($hash.GetHashAndReset())).ToLowerInvariant()
        } finally {
            if ($hash) { $hash.Dispose() }
            if ($output) { $output.Dispose() }
            if ($inputStream) { $inputStream.Dispose() }
        }
        if ($total -ne $script:Pin.ArchiveSize -or $digest -cne $script:Pin.ArchiveSha256) { throw 'release_archive_digest_mismatch' }
        return [pscustomobject]@{ Path = $path; Size = $total; Sha256 = $digest }
    } finally { if ($response) { $response.Dispose() }; $client.Dispose(); $handler.Dispose() }
}

function Install-PinnedSccache($Session) {
    $installStartedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
    $paths = Get-InstallPaths
    if (Test-InstalledReceipt $paths) {
        $Session.Receipt.install = 'reused_known_receipt'
        $Session.Receipt.install_elapsed_ms = [long](([Diagnostics.Stopwatch]::GetTimestamp() - $installStartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
        return $paths
    }
    if (Test-Path -LiteralPath $paths.Destination) { throw 'destination_exists_without_valid_receipt' }
    $toolsDirectory = Join-Path $env:USERPROFILE 'tools'
    if (Test-Path -LiteralPath $toolsDirectory) {
        $toolsItem = Get-Item -LiteralPath $toolsDirectory -Force
        if (-not $toolsItem.PSIsContainer -or ($toolsItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'tools_directory_not_owned_regular_directory' }
    }
    if (Test-Path -LiteralPath $paths.Root) {
        $rootItem = Get-Item -LiteralPath $paths.Root -Force
        if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'tool_root_not_owned_regular_directory' }
    } else { $null = [IO.Directory]::CreateDirectory($paths.Root) }
    $rootItem = Get-Item -LiteralPath $paths.Root -Force
    if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'tool_root_not_owned_regular_directory' }
    $stage = Join-Path $paths.Root ('.stage-' + [Guid]::NewGuid().ToString('N'))
    $opDir = $Session.OperationDirectory
    $stageCreated = $false
    try {
        $Session.ArchivePath = Join-Path $opDir 'sccache-v0.18.0-x64.zip'
        $archive = Receive-PinnedArchive $opDir $Session.WorkDeadlineTicks
        $Session.ArchivePath = $archive.Path
        $entries = Get-ArchiveEntries $archive.Path
        $decision = Get-ArchiveDecision @{ Size = $archive.Size; Sha256 = $archive.Sha256 } $entries
        if (-not $decision.Valid) { throw ('archive_refused:' + ($decision.Reasons -join ',')) }
        $zip = [IO.Compression.ZipFile]::OpenRead($archive.Path)
        $input = $null; $output = $null
        try {
            $exeEntry = $zip.GetEntry($script:Pin.Root + '/' + $script:Pin.Executable)
            if (-not $exeEntry) { throw 'pinned_executable_missing' }
            $input = $exeEntry.Open()
            [WindowsSccacheNative]::CreateDirectoryNew($stage); $stageCreated = $true
            $stageItem = Get-Item -LiteralPath $stage -Force
            if (-not $stageItem.PSIsContainer -or ($stageItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or @(Get-ChildItem -LiteralPath $stage -Force).Count -ne 0) { throw 'stage_directory_not_owned_empty' }
            $stagedExe = Join-Path $stage $script:Pin.Executable
            $output = [IO.File]::Open($stagedExe, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            $input.CopyTo($output); $output.Flush()
        } finally {
            if ($output) { $output.Dispose() }
            if ($input) { $input.Dispose() }
            $zip.Dispose()
        }
        $pe = Get-PEIdentity $stagedExe
        if ($pe.Machine -ne 'x64') { throw 'pinned_pe_architecture_mismatch' }
        $receipt = [ordered]@{ schema = 1; version = $script:Pin.Version; source_commit = $script:Pin.SourceCommit; archive_url = $script:Pin.ArchiveUrl; archive_size = $archive.Size; archive_sha256 = $archive.Sha256; exe_size = $pe.Size; exe_sha256 = $pe.Sha256; runner = $env:RUNNER_NAME; account = $env:USERNAME }
        $receiptPath = Join-Path $stage 'receipt.json'
        $receipt | ConvertTo-Json -Compress | Set-Content -LiteralPath $receiptPath -Encoding utf8
        if (Test-Path -LiteralPath $paths.Destination) { throw 'destination_appeared_during_install' }
        [IO.Directory]::Move($stage, $paths.Destination); $stageCreated = $false
        $Session.InstallCreated = $true; $Session.Paths = $paths; $Session.Receipt.install = 'installed_additively'
        if (-not (Test-InstalledReceipt $paths)) { throw 'installed_receipt_readback_failed' }
        $Session.Receipt.install_elapsed_ms = [long](([Diagnostics.Stopwatch]::GetTimestamp() - $installStartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
        return $paths
    } finally {
        if ($stageCreated -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
    }
}

function Get-ListenerFacts([uint32]$ExpectedPid = 0) {
    $rows = @([WindowsSccacheNative]::Listeners($script:Pin.ServerPort))
    if ($rows.Count -eq 0) { return [pscustomobject]@{ Count = 0; Owned = $false; Rows = @() } }
    $onlyExpected = $rows.Count -eq 1 -and [uint32]$rows[0].Pid -eq $ExpectedPid -and $rows[0].Address -eq '127.0.0.1'
    return [pscustomobject]@{ Count = $rows.Count; Owned = $onlyExpected; Rows = $rows }
}

function Get-SafeToolReceipt($Paths, $ResolutionBefore) {
    $now = Get-ToolResolutions
    foreach ($name in $ResolutionBefore.Keys) { if ($now[$name] -ine $ResolutionBefore[$name]) { throw 'tool_resolution_changed:' + $name } }
    $sccache = Get-Command sccache -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    return [ordered]@{ selected = $Paths.Executable; before = $ResolutionBefore; after = $now; sccache_before = $(if ($sccache) { $sccache.Path } else { $null }); sccache_sha256 = (Get-FileHash -LiteralPath $Paths.Executable -Algorithm SHA256).Hash.ToLowerInvariant() }
}

function New-SessionConfig($Session) {
    $Session.ConfigPath = Join-Path $Session.OperationDirectory 'local-only.toml'
    $Session.CachePath = Join-Path $env:LOCALAPPDATA 'Mozilla\sccache'
    foreach ($path in @($env:LOCALAPPDATA, (Join-Path $env:LOCALAPPDATA 'Mozilla'), $Session.CachePath)) {
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'persistent_cache_path_reparse_or_non_directory' }
        }
    }
    if (Test-Path -LiteralPath $Session.OperationDirectory) { throw 'operation_directory_already_exists' }
    [WindowsSccacheNative]::CreateDirectoryNew($Session.OperationDirectory)
    [IO.File]::WriteAllText($Session.ConfigPath, "[cache.disk]`r`n", [Text.UTF8Encoding]::new($false))
}

function Start-WindowsSccacheSession {
    [CmdletBinding()]
    param([Parameter(Mandatory)][long]$DeadlineTicks, [switch]$Maintenance, [bool]$ArtifactFresh = $false, [long]$OperationStartedTicks = 0)
    $workDeadline = $DeadlineTicks - [long]($script:Pin.CleanupReserveMs * [Diagnostics.Stopwatch]::Frequency / 1000)
    $profile = Join-Path $env:RUNNER_TEMP ('rust-windows-task-local-' + $env:GITHUB_RUN_ID + '-' + $env:GITHUB_RUN_ATTEMPT)
    if ($Maintenance) { $profile = Join-Path $env:RUNNER_TEMP ('windows-sccache-maintenance-' + $env:GITHUB_RUN_ID + '-' + $env:GITHUB_RUN_ATTEMPT) }
    if ($OperationStartedTicks -le 0) { $OperationStartedTicks = [Diagnostics.Stopwatch]::GetTimestamp() }
    $session = [ordered]@{ WorkDeadlineTicks = $workDeadline; CleanupDeadlineTicks = $DeadlineTicks; StartedTicks = $OperationStartedTicks; Processes = [System.Collections.Generic.List[object]]::new(); ProcessTiming = @{}; PrimaryFailure = $null; CleanupError = $null; CleanupAttempted = $false; CleanupResult = $null; InstallCreated = $false; TerminateResult = $null; TerminationKind = 'unconfirmed'; Server = $null; ConfigPath = $null; CachePath = $null; OperationDirectory = (Join-Path $profile 'cache-session'); ReceiptPath = (Join-Path $profile 'cache-session-receipt.json'); ArtifactFresh = $ArtifactFresh; Receipt = [ordered]@{ version = $script:Pin.Version; source_commit = $script:Pin.SourceCommit; endpoint = $script:Pin.Endpoint; startup = 'unproven'; stats = 'not_run'; termination = 'unconfirmed'; environment_keys = @(); environment_paths = @{}; commands = @() } }
    $session.Receipt.operation_budget_ms = [long](($DeadlineTicks - $session.StartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
    $session.Receipt.cleanup_reserve_ms = $script:Pin.CleanupReserveMs
    $script:Session = $session
    try {
        if ($env:OS -ne 'Windows_NT' -or $env:RUNNER_OS -ne 'Windows') { throw 'windows_runner_required' }
        if (Test-Path Env:SCCACHE_BASEDIRS) { throw 'SCCACHE_BASEDIRS_must_be_absent' }
        if ($env:GITHUB_RUN_ID -notmatch '^\d+$' -or $env:GITHUB_RUN_ATTEMPT -notmatch '^\d+$') { throw 'run_identity_invalid' }
        $root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
        if (Test-Path -LiteralPath (Join-Path $root '.env')) { throw 'ambient_dotenv_refused' }
        $liveBindings = @(Get-ChildItem Env: | Where-Object { $_.Name -match '^CODETERM_' -and $_.Name -ne 'CODETERM_EMBED_CACHE_DIR' -and $_.Value })
        if ($liveBindings.Count -ne 0 -or $env:CARGO_TARGET_DIR -or $env:CARGO_TEST_CMD -or $env:CARGO_TEST_BUILD_CMD) { throw 'ambient_instance_or_target_override_refused' }
        $staticIdentity = Get-StaticIdentityDecision @{ ExpectedSha = $env:EXPECTED_SOURCE_SHA; WorkflowSha = $env:WORKFLOW_SOURCE_SHA; EventSha = $env:EVENT_SOURCE_SHA; CheckoutSha = $env:EXPECTED_SOURCE_SHA; Runner = $env:RUNNER_NAME; Arch = $env:RUNNER_ARCH; Account = [Security.Principal.WindowsIdentity]::GetCurrent().Name.Split('\')[-1]; UserProfile = $env:USERPROFILE }
        if (-not $staticIdentity.Valid) { throw ('dispatch_or_runner_identity_refused:' + ($staticIdentity.Reasons -join ',')) }
        Initialize-WindowsSccacheNative
        $session.Job = [WindowsSccacheNative]::CreateJob()
        New-SessionConfig $session
        $run = Get-RunIdentity
        $session.Root = $run.Root; $session.UserSid = $run.UserSid
        $session.Receipt.source_sha = $run.Head; $session.Receipt.runner = $env:RUNNER_NAME; $session.Receipt.runner_arch = $env:RUNNER_ARCH; $session.Receipt.account = [Security.Principal.WindowsIdentity]::GetCurrent().Name
        $cachePath = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Mozilla\sccache'))
        $rootPrefix = $run.Root.TrimEnd('\') + '\'
        if ($cachePath.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'persistent_cache_inside_worktree_refused' }
        $listener = Get-ListenerFacts
        $paths = Get-InstallPaths
        $destinationState = 'absent'; $receiptValid = $false
        if (Test-Path -LiteralPath $paths.Destination) { $receiptValid = Test-InstalledReceipt $paths; $destinationState = if ($receiptValid) { 'known_receipt' } else { 'foreign' } }
        $inputFacts = @{ ExpectedSha = $env:EXPECTED_SOURCE_SHA; WorkflowSha = $env:WORKFLOW_SOURCE_SHA; EventSha = $env:EVENT_SOURCE_SHA; CheckoutSha = $run.Head; Clean = $true; Runner = $env:RUNNER_NAME; Arch = $env:RUNNER_ARCH; Account = [Security.Principal.WindowsIdentity]::GetCurrent().Name.Split('\')[-1]; UserProfile = $env:USERPROFILE; ToolResolutionPreserved = $true; DestinationState = $destinationState; ReceiptValid = $receiptValid; OwnerCount = $listener.Count; OwnerIsCapturedServer = $false; RemainingMs = (Get-RemainingMs $workDeadline); CleanupReserveMs = 0 }
        $preflight = Get-InputDecision $inputFacts
        if (-not $preflight.Valid) { throw ('preflight_refused:' + ($preflight.Reasons -join ',')) }
        if ($Maintenance) { $paths = Install-PinnedSccache $session } elseif (-not $receiptValid) { throw 'pinned_install_receipt_required' }
        $session.Paths = $paths
        $beforeTools = Get-ToolResolutions
        $session.ToolResolution = Get-SafeToolReceipt $paths $beforeTools
        $session.PreviousEnvironment = @{}
        foreach ($key in @('PATH','RUSTC_WRAPPER','SCCACHE_DIR','SCCACHE_CONF','SCCACHE_SERVER_PORT')) { $session.PreviousEnvironment[$key] = [Environment]::GetEnvironmentVariable($key, 'Process') }
        $env:PATH = $paths.Destination + ';' + $env:PATH
        $env:RUSTC_WRAPPER = 'sccache'; $env:SCCACHE_DIR = $session.CachePath; $env:SCCACHE_CONF = $session.ConfigPath; $env:SCCACHE_SERVER_PORT = [string]$script:Pin.ServerPort
        $afterTools = Get-ToolResolutions
        $toolsPreserved = $true
        foreach ($name in $beforeTools.Keys) { if ($afterTools[$name] -ine $beforeTools[$name]) { $toolsPreserved = $false } }
        $inputFacts.ToolResolutionPreserved = $toolsPreserved
        $inputFacts.RemainingMs = Get-RemainingMs $workDeadline
        $toolDecision = Get-InputDecision $inputFacts
        if (-not $toolDecision.Valid) { throw ('tool_resolution_or_deadline_refused:' + ($toolDecision.Reasons -join ',')) }
        $pinnedResolution = Get-Command sccache -CommandType Application -ErrorAction Stop | Select-Object -First 1
        if ($pinnedResolution.Path -ine $paths.Executable) { throw 'pinned_sccache_resolution_changed' }
        $session.Receipt.environment_keys = @('SystemRoot','WINDIR','USERPROFILE','HOMEDRIVE','HOMEPATH','TEMP','TMP','LOCALAPPDATA','APPDATA','PROCESSOR_ARCHITECTURE','INCLUDE','LIB','LIBPATH','VCToolsInstallDir','VCINSTALLDIR','VisualStudioVersion','WindowsSdkDir','WindowsSDKVersion','UCRTVersion','VSCMD_VER','VSCMD_ARG_TGT_ARCH','PATH','RUSTC_WRAPPER','SCCACHE_CONF','SCCACHE_DIR','SCCACHE_SERVER_PORT')
        $session.Receipt.environment_paths = @{ PATH = $env:PATH; USERPROFILE = $env:USERPROFILE; SCCACHE_CONF = $session.ConfigPath; SCCACHE_DIR = $session.CachePath }
        $childEnv = Get-ChildEnvironment @{}
        $session.ChildEnv = $childEnv
        $session.Receipt.child_environment_keys = @($childEnv.Keys)
        $session.Receipt.child_environment_paths = @{ PATH = $childEnv['PATH']; USERPROFILE = $childEnv['USERPROFILE']; SCCACHE_CONF = $childEnv['SCCACHE_CONF']; SCCACHE_DIR = $childEnv['SCCACHE_DIR'] }
        $version = Launch-OwnedProcess $session $paths.Executable (Get-ProcessCommandLine $paths.Executable @('--version')) $paths.Destination $childEnv
        Wait-OwnedProcess $session $version $workDeadline
        $versionText = $version.Stdout.Text.Trim()
        if ($version.ExitCode -ne 0 -or $versionText -cne ('sccache ' + $script:Pin.Version)) { throw 'pinned_version_output_mismatch' }
        $session.Receipt.version_output = $versionText
        $start = Launch-OwnedProcess $session $paths.Executable (Get-ProcessCommandLine $paths.Executable @('--start-server')) $paths.Destination $childEnv
        Wait-OwnedProcess $session $start $workDeadline
        $startupText = $start.Stdout.Text
        if ($start.ExitCode -ne 0 -or $startupText -notmatch '(?m)^sccache: Listening on address 127\.0\.0\.1:4226\r?$') { throw 'startup_notification_missing' }
        $session.Receipt.startup_elapsed_ms = [long](([Diagnostics.Stopwatch]::GetTimestamp() - $session.StartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
        $jobSnapshot = [WindowsSccacheNative]::Snapshot($session.Job)
        if ($jobSnapshot.ActiveCount -ne 1) { throw 'job_active_count_not_one_server' }
        $pids = @($jobSnapshot.Members)
        if ($pids.Count -ne 1) { throw 'job_server_membership_ambiguous' }
        $serverPid = [uint32]$pids[0]
        $serverHandle = [WindowsSccacheNative]::OpenProcessHandle($serverPid)
        $session.Server = [pscustomobject]@{ Pid = $serverPid; Handle = $serverHandle; Creation = [WindowsSccacheNative]::ProcessCreation($serverHandle); Image = [WindowsSccacheNative]::ProcessImage($serverHandle); UserSid = [WindowsSccacheNative]::ProcessUserSid($serverHandle); Hash = (Get-FileHash -LiteralPath $paths.Executable -Algorithm SHA256).Hash.ToLowerInvariant() }
        if (-not [WindowsSccacheNative]::Contains($session.Job, $serverHandle) -or $session.Server.Image -ine $paths.Executable -or $session.Server.UserSid -cne $session.UserSid -or $session.Server.Hash -cne $session.ToolResolution.sccache_sha256 -or -not [WindowsSccacheNative]::HandleAlive($serverHandle)) { throw 'captured_server_identity_refused' }
        $owners = Get-ListenerFacts $serverPid
        if (-not $owners.Owned) { throw 'owned_listener_proof_failed' }
        if (-not [WindowsSccacheNative]::Contains($session.Job, $serverHandle) -or -not [WindowsSccacheNative]::HandleAlive($serverHandle)) { throw 'captured_server_changed_at_readiness_event' }
        $session.Receipt.startup = 'notification_process_job_account_image_hash_listener_proven'
        $session.Receipt.server = @{ pid = $serverPid; creation_filetime = $session.Server.Creation; image = $session.Server.Image; sha256 = $session.Server.Hash; user_sid = $session.Server.UserSid; listener = $owners.Rows }
        $stats = Launch-OwnedProcess $session $paths.Executable (Get-ProcessCommandLine $paths.Executable @('--show-stats')) $paths.Destination $childEnv
        $session.StatsProcess = $stats
        Wait-OwnedProcess $session $stats $workDeadline
        if ($stats.ExitCode -ne 0) { throw 'show_stats_protocol_failed' }
        if (-not [WindowsSccacheNative]::HandleAlive($serverHandle) -or -not [WindowsSccacheNative]::Contains($session.Job, $serverHandle)) { throw 'captured_server_not_live_after_stats' }
        $session.Receipt.stats = 'completed_after_owned_readiness;diagnostic_only'
        $session.Receipt.stats_stdout = Get-CaptureRecord $stats.Stdout
        $session.Receipt.stats_stderr = Get-CaptureRecord $stats.Stderr
        $session.Receipt.startup_stdout = Get-CaptureRecord $start.Stdout
        $session.Receipt.startup_stderr = Get-CaptureRecord $start.Stderr
        $session.Receipt.tool_resolution = $session.ToolResolution
        Write-SessionReceipt $session
        return $session
    } catch {
        $primary = $_.Exception.GetType().Name + ':' + $_.Exception.Message
        $session.PrimaryFailure = $primary
        try { $null = Stop-WindowsSccacheSession $session $primary $false } catch { $session.Receipt.cleanup_internal_error = $_.Exception.GetType().Name }
        throw
    }
}

function Write-SessionReceipt($Session) {
    if (-not $Session.ReceiptPath) { $Session.ReceiptPath = Join-Path $Session.OperationDirectory 'session-receipt.json' }
    $Session.Receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $Session.ReceiptPath -Encoding utf8
}

function Remove-OperationFiles($Session) {
    foreach ($path in @($Session.ConfigPath, $Session.ArchivePath)) {
        if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) { Remove-Item -LiteralPath $path -Force }
    }
}

function Stop-WindowsSccacheSession($Session, [string]$PrimaryFailure = $null, [bool]$Cancelled = $false) {
    if (-not $Session) { return [pscustomobject]@{ Success = $true; TerminationKind = 'natural'; Error = $null } }
    if ($Session.CleanupAttempted) {
        if ($Session.CleanupResult) { return $Session.CleanupResult }
        return [pscustomobject]@{ Success = $false; TerminationKind = $Session.TerminationKind; TerminateResult = $Session.TerminateResult; Errors = @('cleanup_result_unconfirmed'); ReceiptPath = $Session.ReceiptPath }
    }
    $Session.CleanupAttempted = $true
    $cleanupStartedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
    $cleanErrors = [System.Collections.Generic.List[string]]::new()
    $startupProven = ($Session.Receipt.startup -eq 'notification_process_job_account_image_hash_listener_proven')
    $serverWasAlive = $false
    if ($Session.Server -and $Session.Server.Handle -ne [IntPtr]::Zero) {
        try { $serverWasAlive = [WindowsSccacheNative]::HandleAlive($Session.Server.Handle) } catch { $cleanErrors.Add('server_liveness_unconfirmed') }
        if ($startupProven -and -not $serverWasAlive) { $cleanErrors.Add('server_exited_before_owned_cleanup') }
    }
    $activeBefore = $null
    $terminationPlan = Get-TerminationPlan @{ CapturedJobHandle = [bool]$Session.Job; EndpointReused = $false }
    if ($Session.Job) {
        try { $activeBefore = [WindowsSccacheNative]::ActiveCount($Session.Job) } catch { $cleanErrors.Add('job_accounting_before_failed') }
        if ($activeBefore -eq 0) { $Session.TerminationKind = 'natural'; $Session.TerminateResult = $null }
        else {
            $Session.TerminationKind = 'deliberate'
            if (-not $terminationPlan.Valid) { $cleanErrors.Add('captured_job_termination_refused') }
            else {
                try {
                    $Session.TerminateResult = [WindowsSccacheNative]::Terminate($Session.Job, 0xC0DE)
                    if (-not $Session.TerminateResult) { $cleanErrors.Add('TerminateJobObject_failed') }
                } catch { $Session.TerminateResult = $false; $cleanErrors.Add('TerminateJobObject_exception:' + $_.Exception.GetType().Name) }
            }
        }
    } else { $Session.TerminationKind = 'natural'; $Session.TerminateResult = $null }
    if ($Session.Server -and $Session.Server.Handle -ne [IntPtr]::Zero) {
        try {
            $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
            if ($remaining -le 0 -or -not [WindowsSccacheNative]::WaitHandle($Session.Server.Handle, $remaining)) { $cleanErrors.Add('server_process_exit_unconfirmed') }
            else { $Session.Server.ExitCode = [WindowsSccacheNative]::ExitCodeHandle($Session.Server.Handle) }
        } catch { $cleanErrors.Add('server_process_exit_unconfirmed') }
    }
    $zeroConfirmed = $false; $activeAfter = $null
    if ($Session.Job) {
        try {
            $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
            if ($remaining -gt 0) { $zeroConfirmed = [WindowsSccacheNative]::WaitJobZero($Session.Job, $remaining) }
        } catch { $cleanErrors.Add('job_active_zero_wait_failed') }
        if (-not $zeroConfirmed) { $cleanErrors.Add('job_active_zero_event_unconfirmed') }
        try {
            $activeAfter = [WindowsSccacheNative]::ActiveCount($Session.Job)
            if ($activeAfter -ne 0) { $cleanErrors.Add('job_active_count_nonzero') }
        } catch { $cleanErrors.Add('job_accounting_after_failed') }
    } else { $zeroConfirmed = $true; $activeAfter = 0 }
    $processExitsComplete = $true
    foreach ($process in $Session.Processes) {
        try {
            $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
            if ($remaining -le 0 -or -not $process.Wait($remaining)) { $processExitsComplete = $false; $cleanErrors.Add('owned_command_exit_unconfirmed') }
        } catch { $processExitsComplete = $false; $cleanErrors.Add('owned_command_exit_unconfirmed') }
    }
    $captureComplete = -not [bool]$Session.LaunchFailure
    if ($Session.LaunchFailure) { $cleanErrors.Add('partial_assigned_launch_process_or_capture_unconfirmed') }
    foreach ($process in $Session.Processes) {
        foreach ($capture in @($process.Stdout, $process.Stderr)) {
            try {
                $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
                if ($remaining -le 0 -or -not $capture.WaitEof($remaining)) { $captureComplete = $false; $cleanErrors.Add('capture_eof_unconfirmed') }
                elseif (-not $capture.WaitReader((Get-RemainingMs $Session.CleanupDeadlineTicks))) { $captureComplete = $false; $cleanErrors.Add('capture_reader_completion_unconfirmed') }
                elseif ($capture.Error) { $captureComplete = $false; $cleanErrors.Add('capture_error') }
            } catch { $captureComplete = $false; $cleanErrors.Add('capture_wait_failed') }
        }
    }
    $Session.Receipt.termination = $Session.TerminationKind
    $Session.Receipt.terminate_job_result = $Session.TerminateResult
    $Session.Receipt.job_active_processes = $activeAfter
    $Session.Receipt.server_alive_before_cleanup = $serverWasAlive
    $Session.Receipt.termination_target = $terminationPlan.Action
    $Session.Receipt.server_exit_code = if ($Session.Server) { $Session.Server.ExitCode } else { $null }
    $Session.Receipt.protocol_shutdown_sent = $false
    $Session.Receipt.foreign_resource_signaled = $false
    $Session.Receipt.primary_failure = $PrimaryFailure
    $Session.Receipt.cancelled = $Cancelled
    $Session.Receipt.deliberate_termination_disposition = 'owned_job_only; no_protocol_shutdown; in-flight_cache_writes_may_be_lost'
    foreach ($process in $Session.Processes) {
        try { $Session.Receipt.commands += @(@{ pid = $process.Pid; exit_code = $(if ($process.Wait(0)) { $process.ExitCode } else { $null }); timing = $Session.ProcessTiming[[string]$process.Pid]; stdout = Get-CaptureRecord $process.Stdout; stderr = Get-CaptureRecord $process.Stderr }) } catch { $cleanErrors.Add('process_receipt_error') }
    }
    $statsCaptureComplete = $false; $statsExit = $null
    if ($Session.StatsProcess) {
        try {
            $statsExit = if ($Session.StatsProcess.Wait(0)) { $Session.StatsProcess.ExitCode } else { $null }
            $statsCaptureComplete = $Session.StatsProcess.Stdout.Eof.WaitOne(0) -and $Session.StatsProcess.Stderr.Eof.WaitOne(0) -and -not $Session.StatsProcess.Stdout.Error -and -not $Session.StatsProcess.Stderr.Error
        } catch { $cleanErrors.Add('stats_completion_unconfirmed') }
    }
    $handlesClosed = $true
    if ($Session.LaunchFailure) { $handlesClosed = $false; $cleanErrors.Add('launch_failure_handle_closure_unconfirmed') }
    if ($Session.Server -and $Session.Server.Handle -ne [IntPtr]::Zero) {
        try {
            if ([WindowsSccacheNative]::CloseHandlePublic($Session.Server.Handle)) { $Session.Server.Handle = [IntPtr]::Zero }
            else { $handlesClosed = $false; $cleanErrors.Add('server_handle_close_failed') }
        } catch { $handlesClosed = $false; $cleanErrors.Add('server_handle_close_failed') }
    }
    foreach ($process in $Session.Processes) { try { if (-not $process.CloseChecked((Get-RemainingMs $Session.CleanupDeadlineTicks))) { $handlesClosed = $false; $cleanErrors.Add('owned_process_handle_close_failed') } } catch { $handlesClosed = $false; $cleanErrors.Add('owned_process_handle_close_failed') } }
    if ($Session.Job) { try { if (-not $Session.Job.CloseChecked()) { $handlesClosed = $false; $cleanErrors.Add('owned_job_handle_close_failed') } } catch { $handlesClosed = $false; $cleanErrors.Add('owned_job_handle_close_failed') } }
    if ($Session.PreviousEnvironment) {
        foreach ($key in $Session.PreviousEnvironment.Keys) {
            try { [Environment]::SetEnvironmentVariable($key, $Session.PreviousEnvironment[$key], 'Process') } catch { $handlesClosed = $false; $cleanErrors.Add('process_environment_restore_failed') }
        }
    }
    try { Remove-OperationFiles $Session } catch { $cleanErrors.Add('operation_file_cleanup_failed') }
    $nowTicks = [Diagnostics.Stopwatch]::GetTimestamp()
    $Session.Receipt.cleanup_elapsed_ms = [long](($nowTicks - $cleanupStartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
    $Session.Receipt.session_elapsed_ms = [long](($nowTicks - $Session.StartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
    $facts = @{ StartupNotification = $startupProven; CapturedProcess = [bool]$Session.Server; JobMembership = [bool]$Session.Server; AccountAndImage = [bool]$Session.Server; EndpointOwner = $startupProven; StatsCompleted = ($Session.Receipt.stats -eq 'completed_after_owned_readiness;diagnostic_only'); StatsCaptureComplete = $statsCaptureComplete; StatsExitCode = $statsExit; RequireReadiness = $true; CaptureComplete = ($captureComplete -and $cleanErrors.Count -eq 0); ActiveZeroEvent = $zeroConfirmed; ActiveProcesses = $activeAfter; ProcessExited = ($processExitsComplete -and $cleanErrors -notcontains 'server_process_exit_unconfirmed'); HandlesClosed = $handlesClosed; TerminationKind = $Session.TerminationKind; TerminateResult = $Session.TerminateResult; Cancelled = $Cancelled; DeadlineExpired = ((Get-RemainingMs $Session.CleanupDeadlineTicks) -le 0); CleanupError = ($cleanErrors.Count -gt 0); ArtifactFresh = [bool]$Session.ArtifactFresh; PrimaryFailure = $PrimaryFailure }
    $decision = Get-CompletionDecision $facts
    $Session.Receipt.outcome = $decision.Outcome
    $Session.Receipt.completion_reasons = $decision.Reasons
    $Session.Receipt.active_zero_event = $zeroConfirmed
    $Session.Receipt.handles_closed = $handlesClosed
    $ownedSettled = $zeroConfirmed -and $activeAfter -eq 0 -and $handlesClosed -and ($cleanErrors -notcontains 'server_process_exit_unconfirmed') -and ($cleanErrors -notcontains 'capture_eof_unconfirmed')
    if ($Session.InstallCreated -and -not $decision.Success -and $ownedSettled) { Remove-CreatedInstall $Session }
    $Session.Receipt.cleanup_errors = @($cleanErrors)
    try { Write-SessionReceipt $Session } catch { $cleanErrors.Add('receipt_write_failed') }
    $Session.CleanupResult = [pscustomobject]@{ Success = ($decision.Success -and $cleanErrors.Count -eq 0); Decision = $decision; TerminationKind = $Session.TerminationKind; TerminateResult = $Session.TerminateResult; Errors = @($cleanErrors); ReceiptPath = $Session.ReceiptPath }
    return $Session.CleanupResult
}

function Remove-CreatedInstall($Session) {
    try {
        $paths = $Session.Paths
        if (-not $paths -or -not (Test-Path -LiteralPath $paths.Destination -PathType Container)) { return }
        if (-not (Test-InstalledReceipt $paths)) { throw 'created_install_receipt_no_longer_matches' }
        Remove-Item -LiteralPath $paths.Executable -Force
        Remove-Item -LiteralPath $paths.Receipt -Force
        [IO.Directory]::Delete($paths.Destination, $false)
        $Session.Receipt.install_rollback = 'removed_exact_created_receipt_owned_files'
    } catch { $Session.Receipt.install_rollback = 'failed:' + $_.Exception.GetType().Name }
}

function Invoke-Maintenance([string]$Operation) {
    $script:Session = $null
    $started = [Diagnostics.Stopwatch]::GetTimestamp()
    $deadline = $started + [long]($script:Pin.OperationMs * [Diagnostics.Stopwatch]::Frequency / 1000)
    $receiptDir = Join-Path $env:RUNNER_TEMP ('windows-sccache-maintenance-' + $env:GITHUB_RUN_ID + '-' + $env:GITHUB_RUN_ATTEMPT)
    $fresh = $false; $session = $null; $primary = $null; $result = 1
    try {
        if (Test-Path -LiteralPath $receiptDir) { throw 'receipt_directory_already_exists' }
        $null = New-Item -ItemType Directory -Path $receiptDir; $fresh = $true
        'fresh_receipt_directory=true' | Out-File -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8 -Append
        if ($Operation -cne 'provision-verify-cleanup') { throw 'maintenance_operation_mismatch' }
        $session = Start-WindowsSccacheSession -DeadlineTicks $deadline -Maintenance -ArtifactFresh $fresh -OperationStartedTicks $started
        $session.ArtifactFresh = $fresh
        $session.CleanupDeadlineTicks = $deadline
        $session.Receipt.maintenance = 'provision-verify-cleanup'
        $session.Receipt.operation_budget_ms = $script:Pin.OperationMs
        $session.Receipt.cleanup_reserve_ms = $script:Pin.CleanupReserveMs
        $session.Receipt.archive = @{ size = $script:Pin.ArchiveSize; sha256 = $script:Pin.ArchiveSha256; url = $script:Pin.ArchiveUrl }
        $session.Receipt.install_receipt_path = $session.Paths.Receipt
        $session.Receipt.artifact_path = $receiptDir
        Write-SessionReceipt $session
        $result = 0
    } catch { $primary = $_.Exception.GetType().Name + ':' + $_.Exception.Message; $session = $script:Session; Write-Host ('maintenance_refused=' + $_.Exception.GetType().Name) }
    finally {
        if ($session) {
            if (-not $session.CleanupDeadlineTicks) { $session.CleanupDeadlineTicks = $deadline }
            try {
                $cleanup = Stop-WindowsSccacheSession $session $primary $false
                if (-not $cleanup.Success) { $result = 1 }
            } catch { $session.Receipt.cleanup_internal_error = $_.Exception.GetType().Name; $result = 1 }
        }
        if ($fresh -and -not $session) {
            $fallback = [ordered]@{ outcome = 'unqualified'; primary_failure = $primary; cleanup = 'no_session_created'; archive_sha256 = $script:Pin.ArchiveSha256; operation_budget_ms = $script:Pin.OperationMs; cleanup_reserve_ms = $script:Pin.CleanupReserveMs }
            $fallback | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $receiptDir 'receipt.json') -Encoding utf8
        }
    }
    if ($result -eq 0) { Write-Host 'maintenance_outcome=passed' } else { Write-Host 'maintenance_outcome=unqualified' }
    return $result
}

if ($PSCmdlet.ParameterSetName -eq 'Library') { return }
if ($PSCmdlet.ParameterSetName -eq 'Contract') { exit (Invoke-ContractOnly) }
if ($PSCmdlet.ParameterSetName -eq 'Maintenance') { exit (Invoke-Maintenance $Operation) }
