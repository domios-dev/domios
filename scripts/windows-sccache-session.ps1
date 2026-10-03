[CmdletBinding(DefaultParameterSetName = 'Library')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Contract')][switch]$ContractOnly,
    [Parameter(Mandatory, ParameterSetName = 'Library')][switch]$Library,
    [Parameter(Mandatory, ParameterSetName = 'Maintenance')]
    [ValidateSet('provision-verify-cleanup')][string]$Operation
)
. (Join-Path $PSScriptRoot 'windows-sccache-archive.ps1')
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
$script:RequiredStatsCounters = @('Compile requests', 'Compile requests executed', 'Cache hits', 'Cache misses', 'Cache timeouts', 'Cache read errors', 'Forced recaches', 'Cache write errors', 'Cache errors', 'Compilations', 'Compilation failures', 'Non-cacheable compilations', 'Non-cacheable calls', 'Non-compilation calls', 'Unsupported compiler calls', 'Failed distributed compilations')
$script:PinnedStatsOutputFixture = @'
Compile requests 0
Compile requests executed 0
Cache hits 0
Cache misses 0
Cache timeouts 0
Cache read errors 0
Forced recaches 0
Cache write errors 0
Cache errors 0
Compilations 0
Compilation failures 0
Non-cacheable compilations 0
Non-cacheable calls 0
Non-compilation calls 0
Unsupported compiler calls 0
Failed distributed compilations 0
'@

function Copy-FactsWithOverrides($Source, $Overrides) {
    $copy = $Source.Clone()
    foreach ($key in $Overrides.Keys) { $copy[$key] = $Overrides[$key] }
    return $copy
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
        if ($normalized -cnotin @($script:Pin.Root, ($script:Pin.Root + '/' + $script:Pin.Executable), ($script:Pin.Root + '/README.md'), ($script:Pin.Root + '/LICENSE'))) { $reasons.Add('unexpected_archive_shape') }
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
    if ($Facts.Account -cne $script:Pin.Account -or $Facts.UserProfile -ine 'C:\Users\amigo') { $reasons.Add('account_identity_mismatch') }
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
function Get-PinnedReuseDecision($Facts) {
    $reasons = [System.Collections.Generic.List[string]]::new()
    if (-not $Facts.ReceiptValid) { $reasons.Add('pinned_receipt_invalid') }
    if (-not $Facts.ArchiveValid) { $reasons.Add('pinned_archive_invalid') }
    if (-not $Facts.PinnedExecutableValid) { $reasons.Add('pinned_executable_invalid') }
    if (-not $Facts.ExecutableBytesEqual) { $reasons.Add('installed_executable_differs_from_pinned_archive') }
    return [pscustomobject]@{ Valid = ($reasons.Count -eq 0); Reasons = @($reasons | Select-Object -Unique) }
}
function Get-StatsDecision([string]$Text, [bool]$CaptureComplete, [bool]$Truncated) {
    $reasons = [System.Collections.Generic.List[string]]::new(); $counters = [ordered]@{}
    if (-not $CaptureComplete) { $reasons.Add('stats_capture_incomplete') }
    if ($Truncated) { $reasons.Add('stats_capture_truncated') }
    foreach ($name in $script:RequiredStatsCounters) {
        $pattern = '(?m)^' + [regex]::Escape($name) + '\s+(?<value>\S+)\s*$'
        $counterMatches = [regex]::Matches($Text, $pattern)
        if ($counterMatches.Count -ne 1) { $reasons.Add('stats_counter_missing_or_duplicate:' + $name); continue }
        $value = $counterMatches[0].Groups['value'].Value
        if ($value -cnotmatch '^\d+$') { $reasons.Add('stats_counter_malformed:' + $name); continue }
        $counters[$name] = [long]$value
        if ([long]$value -ne 0) { $reasons.Add('stats_counter_nonzero:' + $name) }
    }
    return [pscustomobject]@{ Valid = ($reasons.Count -eq 0); Counters = $counters; Reasons = @($reasons | Select-Object -Unique) }
}
function Get-StatsSchemaDecision($CounterNames, [string]$Fixture) {
    $fixtureNames = @($Fixture -split '\r?\n' | ForEach-Object { if ($_ -match '^(?<name>.+?)\s+\d+\s*$') { $Matches.name } })
    $same = $fixtureNames.Count -eq $CounterNames.Count
    if ($same) {
        for ($index = 0; $index -lt $CounterNames.Count; $index++) { if ([string]$CounterNames[$index] -cne [string]$fixtureNames[$index]) { $same = $false; break } }
    }
    return [pscustomobject]@{ Valid = $same; Reasons = $(if ($same) { @() } else { @('pinned_stats_schema_mismatch') }) }
}

function Get-InstallRollbackDecision($Facts) {
    $reasons = [System.Collections.Generic.List[string]]::new()
    if (-not $Facts.InstallCreated) { $reasons.Add('install_not_created') }
    if (-not $Facts.OperationFailed) { $reasons.Add('operation_not_failed') }
    if (-not $Facts.OwnershipResolved) { $reasons.Add('resource_ownership_unresolved') }
    if (-not $Facts.JobZeroConfirmed) { $reasons.Add('job_settlement_unconfirmed') }
    if ($Facts.ActiveProcesses -ne 0) { $reasons.Add('job_active_processes_remain') }
    if (-not $Facts.ProcessExitsComplete) { $reasons.Add('child_exit_unconfirmed') }
    if (-not $Facts.CaptureComplete) { $reasons.Add('capture_settlement_unconfirmed') }
    if (-not $Facts.HandlesClosed) { $reasons.Add('handle_settlement_unconfirmed') }
    if ($Facts.Cancelled) { $reasons.Add('operation_cancelled') }
    if ($Facts.DeadlineExpired) { $reasons.Add('cleanup_deadline_expired') }
    if (@($Facts.CleanupErrors).Count -ne 0) { $reasons.Add('cleanup_not_clean') }
    if ($Facts.TerminationKind -notin @('natural', 'deliberate')) { $reasons.Add('termination_kind_unconfirmed') }
    if ($Facts.TerminationKind -eq 'deliberate' -and -not $Facts.TerminateResult) { $reasons.Add('job_termination_unconfirmed') }
    return [pscustomobject]@{ Remove = ($reasons.Count -eq 0); Reasons = @($reasons | Select-Object -Unique) }
}
function Get-JobMembershipDecision($Facts) {
    $evidence = [WindowsSccacheNative]::ClassifyJobMembers($Facts.Snapshot, [object[]]$Facts.Leases)
    $reasons = [System.Collections.Generic.List[string]]::new()
    $reasons.AddRange([string[]]$evidence.Reasons)
    $proofs = @($Facts.CandidateProofs)
    $total = if ($Facts.ContainsKey('CandidateInventoryTotal')) { [int]$Facts.CandidateInventoryTotal } else { $evidence.CandidatePids.Count }
    $truncated = if ($Facts.ContainsKey('CandidateInventoryTruncated')) { [bool]$Facts.CandidateInventoryTruncated } else { $false }
    $complete = if ($Facts.ContainsKey('CandidateInventoryComplete')) { [bool]$Facts.CandidateInventoryComplete } else { $proofs.Count -eq [Math]::Min($total, 32) }
    $inventory = [WindowsSccacheNative]::ClassifyCandidateInventory($total, $truncated, $complete, [uint32[]]$evidence.CandidatePids, [object[]]$proofs)
    $reasons.AddRange([string[]]$inventory.Reasons)
    if ($evidence.CandidatePids.Count -eq 0) { $reasons.Add('job_server_candidate_missing') }
    elseif ($evidence.CandidatePids.Count -ne 1) { $reasons.Add('job_server_candidate_ambiguous') }
    else {
        $candidateProof = @($proofs | Where-Object { $_.Pid -eq $evidence.CandidatePids[0] })
        if ($candidateProof.Count -ne 1) { $reasons.Add('job_server_candidate_proof_missing_or_ambiguous') }
        else {
            $proof = $candidateProof[0]
            if (@($proof.Errors).Count -ne 0) { $reasons.Add('job_server_candidate_api_error') }
            if (-not $proof.HandleRetained -or $proof.PidFromHandle -ne $proof.Pid -or $proof.VerifiedPidFromHandle -ne $proof.Pid -or $proof.CreationFileTime -le 0 -or $proof.CreationFileTime -ne $proof.VerifiedCreationFileTime) { $reasons.Add('job_server_candidate_handle_identity_invalid') }
            if (-not $proof.IsProcessInJobBefore -or -not $proof.IsProcessInJobAfter) { $reasons.Add('job_server_candidate_job_membership_unconfirmed') }
            if (-not $proof.ImageMatches -or -not $proof.HashMatches -or -not $proof.AccountMatches) { $reasons.Add('job_server_candidate_image_hash_or_account_refused') }
            if (-not $proof.AliveBefore -or -not $proof.AliveAfter) { $reasons.Add('job_server_candidate_liveness_unconfirmed') }
            if (-not $proof.ListenerMatches) { $reasons.Add('job_server_candidate_listener_refused') }
        }
    }
    return [pscustomobject]@{ Valid = ($reasons.Count -eq 0); Reasons = @($reasons | Select-Object -Unique); Members = $evidence.Members; CandidatePids = $evidence.CandidatePids; CandidateInventoryTotal = $total; CandidateInventoryTruncated = $truncated; CandidateInventoryComplete = $complete; CapturedCandidatePids = $inventory.CapturedPids; OwnedLaunchFacts = $evidence.OwnedLaunchFacts; ExcludedOwnedExits = @($evidence.ExcludedOwnedExits | ForEach-Object { $_.Pid }); EligibleOwnedExits = $evidence.EligibleOwnedExits; UnlistedOwnedExits = $evidence.UnlistedOwnedExits; AccountingMinimum = $evidence.AccountingMinimum; AccountingMaximum = $evidence.AccountingMaximum }
}
function Set-CandidateInventoryReceipt($Session, $Inventory, $Proofs, $ReportedPids) {
    $safeProofs = [System.Collections.Generic.List[object]]::new(); $capturedPids = [System.Collections.Generic.List[uint32]]::new()
    foreach ($proof in $Proofs) { $safe = @{}; foreach ($key in $proof.Keys) { if ($key -cne 'Lease') { $safe[$key] = $proof[$key] } }; $safeProofs.Add($safe); $capturedPids.Add([uint32]$proof.Pid) }
    $Session.Receipt.job_membership_candidate_inventory = @{ total = $Inventory.Total; inspected = $Inventory.Inspected; maximum = 32; truncated = $Inventory.Truncated; complete = $Inventory.Complete; collected_pids = $capturedPids.ToArray(); reported_pids = @($ReportedPids); error = $Inventory.Error }
    $Session.Receipt.job_membership_candidate_proofs = $safeProofs.ToArray()
}
function Get-OwnedLaunchIdentityFacts($Leases) {
    $facts = [System.Collections.Generic.List[object]]::new()
    foreach ($lease in $Leases) {
        if (-not $lease.Created) { continue }
        $handlePid = [uint32]0; $currentCreation = [long]0; $handleRetained = ($lease.Handle -ne [IntPtr]::Zero); $identityError = $null; $directServer = [bool]$lease.DirectServer; $alive = $false
        try {
            if (-not $handleRetained) { throw 'owned_process_handle_missing' }
            $handlePid = [WindowsSccacheNative]::HandleProcessId($lease.Handle)
            $currentCreation = [WindowsSccacheNative]::ProcessCreation($lease.Handle)
            if ($directServer) { $alive = [WindowsSccacheNative]::HandleAlive($lease.Handle) }
        } catch {
            $win32 = if ($_.Exception -is [System.ComponentModel.Win32Exception]) { $_.Exception.NativeErrorCode } else { 'not_win32' }
            $identityError = $_.Exception.GetType().Name + ':' + $win32
        }
        $identityValid = $handleRetained -and $lease.Pid -eq $handlePid -and $lease.CreatedFileTime -gt 0 -and $lease.CreatedFileTime -eq $currentCreation -and -not $identityError
        $facts.Add(@{ Created = $true; Pid = [uint32]$lease.Pid; HandlePid = $handlePid; CapturedCreationFileTime = [long]$lease.CreatedFileTime; CurrentCreationFileTime = $currentCreation; Assigned = [bool]$lease.Assigned; HandleRetained = [bool]$handleRetained; IdentityValid = [bool]$identityValid; IdentityError = $identityError; ProcessExitConfirmed = [bool]$lease.ProcessExitConfirmed; ExitCode = $(if ($lease.ProcessExitConfirmed) { $lease.ExitCode } else { $null }); CapturesComplete = [bool]$lease.CapturesComplete; DirectServer = $directServer; Alive = $alive })
    }
    return @($facts)
}
function Format-CaptureReceipt($Tail, [long]$TotalBytes, [bool]$WasTruncated) {
    $bytes = [byte[]]$Tail
    if ($bytes.Length -gt $script:Pin.TailBytes) { $bytes = $bytes[($bytes.Length - $script:Pin.TailBytes)..($bytes.Length - 1)]; $WasTruncated = $true }
    $leadingLineOmitted = $false
    $WasTruncated = $WasTruncated -or $TotalBytes -gt $script:Pin.TailBytes
    if ($WasTruncated -and $bytes.Length -gt 0) {
        $lineEnd = [Array]::IndexOf($bytes, [byte]10)
        if ($lineEnd -lt 0 -or $lineEnd -eq ($bytes.Length - 1)) { $bytes = [byte[]]::new(0) }
        else { $bytes = $bytes[($lineEnd + 1)..($bytes.Length - 1)] }
        $leadingLineOmitted = $true
    }
    $text = [Text.Encoding]::UTF8.GetString($bytes)
    $text = [regex]::Replace($text, '(?i)\b(gh[pousr]_[A-Za-z0-9_]{16,}|github_pat_[A-Za-z0-9_]{16,})\b', '[redacted-token]')
    $text = [regex]::Replace($text, '(?i)\bBearer\s+[^\s,;]+', 'Bearer [redacted-token]')
    $text = [regex]::Replace($text, '(?i)\b(password|passwd|token|secret|authorization)\s*[:=]\s*[^\s,;]+', '$1=[redacted]')
    $text = [regex]::Replace($text, '(?i)(https?://)[^/@\s:]+:[^/@\s]+@', '$1[redacted]@')
    return [pscustomobject]@{ Text = $text; TotalBytes = $TotalBytes; Truncated = $WasTruncated; LeadingLineOmitted = $leadingLineOmitted }
}
function Get-CompletionDecision($Facts) {
    $reasons = [System.Collections.Generic.List[string]]::new()
    $stats = Get-StatsDecision ([string]$Facts.StatsOutput) ([bool]$Facts.StatsOutputComplete) ([bool]$Facts.StatsOutputTruncated)
    $ready = $Facts.StartupNotification -and $Facts.CapturedProcess -and $Facts.JobMembership -and $Facts.AccountAndImage -and $Facts.EndpointOwner -and $Facts.StatsCompleted -and $Facts.StatsCaptureComplete -and $stats.Valid
    if ($Facts.Cancelled) { $reasons.Add('cancelled') }
    if ($Facts.DeadlineExpired) { $reasons.Add('deadline_expired') }
    if ($Facts.CleanupError) { $reasons.Add('cleanup_error') }
    if (-not $Facts.CaptureComplete) { $reasons.Add('capture_unconfirmed') }
    if (-not $Facts.ActiveZeroEvent) { $reasons.Add('active_zero_event_unconfirmed') }
    if ($Facts.ActiveProcesses -ne 0) { $reasons.Add('active_processes_unconfirmed') }
    if (-not $Facts.ProcessExited) { $reasons.Add('process_exit_unconfirmed') }
    if (-not $Facts.HandlesClosed) { $reasons.Add('handle_closure_unconfirmed') }
    if (-not $stats.Valid) { $reasons.AddRange([string[]]$stats.Reasons) }
    if ($Facts.TerminationKind -eq 'deliberate' -and -not $Facts.TerminateResult) { $reasons.Add('job_termination_failed') }
    if ($Facts.TerminationKind -notin @('deliberate', 'natural')) { $reasons.Add('termination_kind_unconfirmed') }
    if (-not $ready -and $Facts.RequireReadiness) { $reasons.Add('owned_readiness_unconfirmed') }
    if ($Facts.StatsExitCode -eq 0 -and -not ($Facts.StartupNotification -and $Facts.CapturedProcess -and $Facts.JobMembership -and $Facts.AccountAndImage -and $Facts.EndpointOwner)) { $reasons.Add('stats_exit_is_not_readiness') }
    if (-not $Facts.ArtifactFresh) { $reasons.Add('artifact_not_fresh') }
    $primary = if ($Facts.PrimaryFailure) { [string]$Facts.PrimaryFailure } else { $null }
    $success = ($reasons.Count -eq 0 -and -not $primary)
    return [pscustomobject]@{ Success = $success; Ready = $ready; StatsCounters = $stats.Counters; Reasons = @($reasons | Select-Object -Unique); PrimaryFailure = $primary; Outcome = $(if ($primary) { 'primary_failure_preserved' } elseif ($success) { 'passed' } else { 'unqualified' }) }
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
        if ($case.ContainsKey('ExpectedExcludedExitPids')) { $ok = $ok -and ((@($actual.ExcludedOwnedExits | Sort-Object) -join ',') -ceq (@($case.ExpectedExcludedExitPids | Sort-Object) -join ',')) }
        if ($ok) { $passed++ } else { $failed++ }
        $results.Add([ordered]@{ selector = $case.Name; passed = $ok; reasons = @($actual.Reasons) })
    }
    return [pscustomobject]@{ Group = $Name; Selected = $Cases.Count; Passed = $passed; Failed = $failed; Cases = @($results) }
}

function Convert-NativeContractCases($Controls, [int]$ExpectedCount) {
    $cases = @($Controls | ForEach-Object { @{ Name = $_.Name; Facts = @{ NativePassed = [bool]$_.Passed; NativeError = [string]$_.Error }; Expected = $true } })
    if ($cases.Count -ne $ExpectedCount) { $cases += @{ Name = 'native_control_selection_count_refused'; Facts = @{ NativePassed = $false; NativeError = 'native_control_selection_count_mismatch' }; Expected = $true } }
    return $cases
}

function New-ContractZeroStatsOutput {
    return $script:PinnedStatsOutputFixture.TrimEnd()
}
function New-ContractJobSnapshot([uint32[]]$Pids, [uint32]$Active, [uint32]$Assigned, [uint32]$Listed, [bool]$Complete = $true, [bool]$AccountingQuerySucceeded = $true, [bool]$ProcessListQuerySucceeded = $true, [bool]$AccountingComplete = $true) {
    return @{ AccountingQuerySucceeded = $AccountingQuerySucceeded; AccountingComplete = $AccountingComplete; AccountingError = $(if ($AccountingQuerySucceeded) { 0 } else { 5 }); AccountingActiveProcesses = $Active; AccountingReturnLength = 48; AccountingRequiredBytes = 48; AccountingStartedTicks = 1L; AccountingCompletedTicks = 2L; ProcessListQuerySucceeded = $ProcessListQuerySucceeded; ProcessListError = $(if ($ProcessListQuerySucceeded) { 0 } else { 5 }); AssignedCount = $Assigned; ListedCount = $Listed; ProcessListReturnLength = [uint32](8 + 8 * $Listed); BufferCapacity = 8192; BufferBytes = 65544; RequiredBytes = [uint64](8 + 8 * $Assigned); ProcessListStartedTicks = 3L; ProcessListCompletedTicks = 4L; ProcessIds = $Pids; Complete = $Complete }
}
function New-ContractOwnedExit([uint32]$ProcessIdValue, [bool]$CapturesComplete = $true) {
    $creation = [long](100000 + $ProcessIdValue)
    return @{ Created = $true; Pid = $ProcessIdValue; HandlePid = $ProcessIdValue; CapturedCreationFileTime = $creation; CurrentCreationFileTime = $creation; Assigned = $true; HandleRetained = $true; IdentityValid = $true; IdentityError = $null; ProcessExitConfirmed = $true; ExitCode = 0; CapturesComplete = $CapturesComplete }
}
function New-ContractCandidateProof([uint32]$ProcessIdValue) {
    $creation = [long](200000 + $ProcessIdValue)
    return @{ Pid = $ProcessIdValue; HandleRetained = $true; PidFromHandle = $ProcessIdValue; VerifiedPidFromHandle = $ProcessIdValue; CreationFileTime = $creation; VerifiedCreationFileTime = $creation; PidObserved = $true; VerifiedPidObserved = $true; CreationObserved = $true; VerifiedCreationObserved = $true; ImageObserved = $true; AccountObserved = $true; JobMembershipObservedBefore = $true; JobMembershipObservedAfter = $true; AliveObservedBefore = $true; AliveObservedAfter = $true; ExitCodeObserved = $false; IsProcessInJobBefore = $true; IsProcessInJobAfter = $true; ImageMatches = $true; HashMatches = $true; AccountMatches = $true; AliveBefore = $true; AliveAfter = $true; ListenerMatches = $true; Errors = @() }
}

function Invoke-ContractOnly {
    $goodEntry = @{ Name = 'sccache-v0.18.0-x86_64-pc-windows-msvc/sccache.exe'; Length = 9000000L; Directory = $false; Reparse = $false; Symlink = $false; Device = $false }
    $publishedEntries = @(
        @{ Name = 'sccache-v0.18.0-x86_64-pc-windows-msvc/'; Length = 0L; Directory = $true },
        @{ Name = 'sccache-v0.18.0-x86_64-pc-windows-msvc/sccache.exe'; Length = 23757824L; Directory = $false },
        @{ Name = 'sccache-v0.18.0-x86_64-pc-windows-msvc/README.md'; Length = 19610L; Directory = $false },
        @{ Name = 'sccache-v0.18.0-x86_64-pc-windows-msvc/LICENSE'; Length = 11358L; Directory = $false }
    )
    $baseArchive = @{ Size = $script:Pin.ArchiveSize; Sha256 = $script:Pin.ArchiveSha256 }
    $archiveCases = @(
        @{ Name = 'digest_verified_published_archive_shape'; Facts = @{ Size = $baseArchive.Size; Sha256 = $baseArchive.Sha256; Entries = $publishedEntries }; Expected = $true },
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
    $goodInput = @{ ExpectedSha = 'a' * 40; WorkflowSha = 'a' * 40; EventSha = 'a' * 40; CheckoutSha = 'a' * 40; Clean = $true; Runner = 'ROG-windows-build'; Arch = 'X64'; Account = 'amigo'; UserProfile = 'C:\Users\amigo'; ToolResolutionPreserved = $true; DestinationState = 'known_receipt'; ReceiptValid = $true; OwnerCount = 0; OwnerIsCapturedServer = $false; RemainingMs = 60000; CleanupReserveMs = 30000 }
    $inputCases = @(
        @{ Name = 'known_receipt_owner_positive'; Facts = $goodInput; Expected = $true },
        @{ Name = 'absent_destination_owner_positive'; Facts = (Copy-FactsWithOverrides $goodInput @{ DestinationState = 'absent'; ReceiptValid = $false }); Expected = $true },
        @{ Name = 'wrong_runner_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ Runner = 'other' }); Expected = $false },
        @{ Name = 'wrong_architecture_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ Arch = 'ARM64' }); Expected = $false },
        @{ Name = 'wrong_account_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ Account = 'other' }); Expected = $false },
        @{ Name = 'foreign_destination_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ DestinationState = 'foreign' }); Expected = $false },
        @{ Name = 'missing_receipt_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ DestinationState = 'missing_receipt' }); Expected = $false },
        @{ Name = 'invalid_receipt_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ ReceiptValid = $false }); Expected = $false },
        @{ Name = 'foreign_endpoint_owner_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ OwnerCount = 1; OwnerIsCapturedServer = $false }); Expected = $false },
        @{ Name = 'ambiguous_ownership_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ OwnerCount = 2 }); Expected = $false },
        @{ Name = 'tool_resolution_change_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ ToolResolutionPreserved = $false }); Expected = $false },
        @{ Name = 'source_mismatch_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ EventSha = 'b' * 40 }); Expected = $false },
        @{ Name = 'deadline_expiry_refused'; Facts = (Copy-FactsWithOverrides $goodInput @{ RemainingMs = 30000 }); Expected = $false },
        @{ Name = 'pinned_runtime_reuse_positive'; Facts = @{ DecisionKind = 'pinned_reuse'; ReceiptValid = $true; ArchiveValid = $true; PinnedExecutableValid = $true; ExecutableBytesEqual = $true }; Expected = $true },
        @{ Name = 'recomputed_receipt_cannot_replace_archive_bytes'; Facts = @{ DecisionKind = 'pinned_reuse'; ReceiptValid = $true; ArchiveValid = $true; PinnedExecutableValid = $true; ExecutableBytesEqual = $false }; Expected = $false },
        @{ Name = 'pinned_archive_reuse_refused'; Facts = @{ DecisionKind = 'pinned_reuse'; ReceiptValid = $true; ArchiveValid = $false; PinnedExecutableValid = $true; ExecutableBytesEqual = $true }; Expected = $false },
        @{ Name = 'pinned_reuse_receipt_invalid_refused'; Facts = @{ DecisionKind = 'pinned_reuse'; ReceiptValid = $false; ArchiveValid = $true; PinnedExecutableValid = $true; ExecutableBytesEqual = $true }; Expected = $false },
        @{ Name = 'pinned_reuse_executable_invalid_refused'; Facts = @{ DecisionKind = 'pinned_reuse'; ReceiptValid = $true; ArchiveValid = $true; PinnedExecutableValid = $false; ExecutableBytesEqual = $true }; Expected = $false }
    )
    $inputCases += Convert-NativeContractCases ([WindowsSccacheNative]::TcpDecoderContractControls()) 6
    $inputCases += Convert-NativeContractCases ([WindowsSccacheNative]::LaunchLeaseContractControls()) 7
    $inputCases += Convert-NativeContractCases ([WindowsSccacheNative]::CandidateInventoryContractControls()) 3
    $inputCases += Convert-NativeContractCases ([WindowsSccacheNative]::StartupNotificationContractControls()) 21
    $inputCases += Convert-NativeContractCases ([WindowsSccacheNative]::DirectServerMemberContractControls()) 3
    $inputCases += Get-ArchiveStreamContractCases
    $group2 = Invoke-ContractGroup 'inputs_ownership_deadline_and_pinned_reuse' $inputCases {
        param($facts)
        if ($facts.ContainsKey('NativePassed')) { [pscustomobject]@{ Valid = $facts.NativePassed; Reasons = $(if ($facts.NativeError) { @($facts.NativeError) } else { @() }) } }
        elseif ($facts.DecisionKind -eq 'archive_stream') { Test-ArchiveStreamContract $facts.Mutation }
        elseif ($facts.DecisionKind -eq 'pinned_reuse') { Get-PinnedReuseDecision $facts }
        else { Get-InputDecision $facts }
    }
    $readyFacts = @{ StartupNotification = $true; CapturedProcess = $true; JobMembership = $true; AccountAndImage = $true; EndpointOwner = $true; StatsCompleted = $true; StatsCaptureComplete = $true; StatsOutput = (New-ContractZeroStatsOutput); StatsOutputComplete = $true; StatsOutputTruncated = $false; StatsExitCode = 0; RequireReadiness = $true; CaptureComplete = $true; ActiveZeroEvent = $true; ActiveProcesses = 0; ProcessExited = $true; HandlesClosed = $true; TerminationKind = 'deliberate'; TerminateResult = $true; Cancelled = $false; DeadlineExpired = $false; CleanupError = $false; ArtifactFresh = $true; PrimaryFailure = $null }
    $completionCases = @(
        @{ Name = 'fresh_zero_stats_owned_readiness_positive'; Facts = $readyFacts; Expected = $true },
        @{ Name = 'stats_exit_alone_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ StartupNotification = $false; CapturedProcess = $false; JobMembership = $false; AccountAndImage = $false; EndpointOwner = $false }); Expected = $false },
        @{ Name = 'stats_nonzero_counter_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ StatsOutput = $readyFacts.StatsOutput.Replace('Cache hits 0', 'Cache hits 1') }); Expected = $false },
        @{ Name = 'stats_missing_counter_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ StatsOutput = [regex]::Replace($readyFacts.StatsOutput, '(?m)^Cache hits\s+0\n?', '') }); Expected = $false },
        @{ Name = 'stats_malformed_counter_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ StatsOutput = $readyFacts.StatsOutput.Replace('Cache hits 0', 'Cache hits unknown') }); Expected = $false },
        @{ Name = 'stats_incomplete_capture_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ StatsOutputComplete = $false }); Expected = $false },
        @{ Name = 'stats_truncated_capture_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ StatsOutputTruncated = $true }); Expected = $false },
        @{ Name = 'stats_duplicate_counter_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ StatsOutput = $readyFacts.StatsOutput + "`nCache hits 0" }); Expected = $false },
        @{ Name = 'active_process_unconfirmed_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ ActiveProcesses = 1 }); Expected = $false },
        @{ Name = 'active_zero_event_unconfirmed_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ ActiveZeroEvent = $false }); Expected = $false },
        @{ Name = 'capture_unconfirmed_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ CaptureComplete = $false }); Expected = $false },
        @{ Name = 'cancelled_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ Cancelled = $true }); Expected = $false },
        @{ Name = 'deadline_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ DeadlineExpired = $true }); Expected = $false },
        @{ Name = 'process_exit_unconfirmed_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ ProcessExited = $false }); Expected = $false },
        @{ Name = 'termination_error_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ TerminateResult = $false }); Expected = $false },
        @{ Name = 'cleanup_error_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ CleanupError = $true }); Expected = $false },
        @{ Name = 'capture_error_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ CaptureComplete = $false; CleanupError = $true }); Expected = $false },
        @{ Name = 'handle_close_unconfirmed_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ HandlesClosed = $false }); Expected = $false },
        @{ Name = 'primary_failure_preserved'; Facts = (Copy-FactsWithOverrides $readyFacts @{ PrimaryFailure = 'case_failed' }); Expected = $false; ExpectedPrimary = 'case_failed' },
        @{ Name = 'stale_artifact_refused'; Facts = (Copy-FactsWithOverrides $readyFacts @{ ArtifactFresh = $false }); Expected = $false },
        @{ Name = 'endpoint_reuse_terminates_only_captured_job'; Facts = @{ CapturedJobHandle = $true; EndpointReused = $true }; Expected = $true },
        @{ Name = 'missing_job_handle_refused'; Facts = @{ CapturedJobHandle = $false; EndpointReused = $true }; Expected = $false }
    )
    $ownedExitA = New-ContractOwnedExit 201
    $ownedExitB = New-ContractOwnedExit 202
    $completionCases += @(
        @{ Name = 'job_membership_one_live_candidate_positive'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(301)) 1 1 1); Leases = @(); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $true },
        @{ Name = 'job_membership_retained_exits_plus_candidate_positive'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(301)) 3 1 1); Leases = @($ownedExitA, $ownedExitB); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $true; ExpectedExcludedExitPids = @(201,202) },
        @{ Name = 'job_membership_all_exited_no_server_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(201)) 1 1 1); Leases = @($ownedExitA); CandidateProofs = @() }; Expected = $false },
        @{ Name = 'job_membership_two_live_candidates_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(301,302)) 2 2 2); Leases = @(); CandidateProofs = @((New-ContractCandidateProof 301), (New-ContractCandidateProof 302)) }; Expected = $false },
        @{ Name = 'job_membership_nonmatching_retained_identity_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(201)) 1 1 1); Leases = @((Copy-FactsWithOverrides $ownedExitA @{ HandlePid = 299; IdentityValid = $false })); CandidateProofs = @() }; Expected = $false },
        @{ Name = 'job_membership_unsettled_exited_capture_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(201,301)) 2 2 2); Leases = @((New-ContractOwnedExit 201 $false)); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $false },
        @{ Name = 'job_membership_accounting_api_error_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(301)) 1 1 1 $true $false); Leases = @(); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $false },
        @{ Name = 'job_membership_accounting_return_length_incomplete_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (Copy-FactsWithOverrides (New-ContractJobSnapshot ([uint32[]]@(301)) 1 1 1) @{ AccountingComplete = $false; AccountingReturnLength = 40 }); Leases = @(); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $false },
        @{ Name = 'job_membership_process_list_api_error_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(301)) 1 1 1 $true $true $false); Leases = @(); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $false },
        @{ Name = 'job_membership_incomplete_pid_list_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(301)) 2 2 1 $false); Leases = @(); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $false },
        @{ Name = 'job_membership_accounting_outside_retained_exit_bound_refused'; Facts = @{ Kind = 'job_membership'; Snapshot = (New-ContractJobSnapshot ([uint32[]]@(301)) 4 1 1); Leases = @($ownedExitA, $ownedExitB); CandidateProofs = @(New-ContractCandidateProof 301) }; Expected = $false }
    )
    $schemaFixture = $script:PinnedStatsOutputFixture.TrimEnd()
    $completionCases += @(
        @{ Name = 'pinned_sixteen_counter_schema_positive'; Facts = @{ Kind = 'stats_schema'; Names = $script:RequiredStatsCounters; Fixture = $schemaFixture }; Expected = $true },
        @{ Name = 'pinned_counter_schema_drift_refused'; Facts = @{ Kind = 'stats_schema'; Names = @('Changed counter') + @($script:RequiredStatsCounters | Select-Object -Skip 1); Fixture = $schemaFixture }; Expected = $false }
    )
    $boundaryTail = [WindowsSccacheNative]::SyntheticCaptureTail(@(('x' * 10) + 'ghp_', (('a' * 24) + ('z' * 32746) + "`nvisible-line`n")))
    $boundaryReceipt = Format-CaptureReceipt $boundaryTail.Tail ([long]$boundaryTail.TotalBytes) ([bool]$boundaryTail.Truncated)
    $completeTokenText = 'token=ghp_' + ('b' * 24) + "`n"
    $completeToken = Format-CaptureReceipt ([Text.Encoding]::UTF8.GetBytes($completeTokenText)) ([Text.Encoding]::UTF8.GetByteCount($completeTokenText)) $false
    $completionCases += @(
        @{ Name = 'truncated_token_crossing_stream_tail_and_chunk_boundary_omitted'; Facts = @{ Kind = 'capture'; Receipt = $boundaryReceipt; Forbidden = ('a' * 16); ExpectedText = "visible-line`n"; Truncated = $true; LeadingLineOmitted = $true }; Expected = $true },
        @{ Name = 'complete_token_redacted'; Facts = @{ Kind = 'capture'; Receipt = $completeToken; Forbidden = 'ghp_'; ExpectedText = "token=[redacted]`n"; Truncated = $false; LeadingLineOmitted = $false }; Expected = $true }
    )
    $rollbackReady = @{ InstallCreated = $true; OperationFailed = $true; OwnershipResolved = $true; JobZeroConfirmed = $true; ActiveProcesses = 0; ProcessExitsComplete = $true; CaptureComplete = $true; HandlesClosed = $true; Cancelled = $false; DeadlineExpired = $false; CleanupErrors = @(); TerminationKind = 'deliberate'; TerminateResult = $true }
    $completionCases += @(
        @{ Name = 'fully_settled_owned_install_rollback_positive'; Facts = @{ Kind = 'rollback'; Rollback = $rollbackReady }; Expected = $true },
        @{ Name = 'rollback_process_exit_unconfirmed_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ ProcessExitsComplete = $false }) }; Expected = $false },
        @{ Name = 'rollback_capture_or_reader_unconfirmed_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ CaptureComplete = $false }) }; Expected = $false },
        @{ Name = 'rollback_job_settlement_unconfirmed_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ JobZeroConfirmed = $false }) }; Expected = $false },
        @{ Name = 'rollback_active_process_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ ActiveProcesses = 1 }) }; Expected = $false },
        @{ Name = 'rollback_handle_settlement_unconfirmed_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ HandlesClosed = $false }) }; Expected = $false },
        @{ Name = 'rollback_cleanup_api_error_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ CleanupErrors = @('api_error') }) }; Expected = $false },
        @{ Name = 'rollback_termination_error_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ TerminateResult = $false }) }; Expected = $false },
        @{ Name = 'rollback_ownership_unresolved_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ OwnershipResolved = $false }) }; Expected = $false },
        @{ Name = 'rollback_cancelled_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ Cancelled = $true }) }; Expected = $false },
        @{ Name = 'rollback_expired_retains_install'; Facts = @{ Kind = 'rollback'; Rollback = (Copy-FactsWithOverrides $rollbackReady @{ DeadlineExpired = $true }) }; Expected = $false }
    )
    $group3 = Invoke-ContractGroup 'completion_output_and_freshness' $completionCases {
        param($facts)
        if ($facts.Kind -eq 'stats_schema') {
            $schema = Get-StatsSchemaDecision $facts.Names $facts.Fixture
            return $schema
        }
        if ($facts.Kind -eq 'capture') {
            $receipt = $facts.Receipt
            $valid = $receipt.Truncated -eq $facts.Truncated -and $receipt.LeadingLineOmitted -eq $facts.LeadingLineOmitted -and $receipt.Text -ceq $facts.ExpectedText -and -not $receipt.Text.Contains($facts.Forbidden)
            return [pscustomobject]@{ Valid = $valid; Reasons = $(if ($valid) { @() } else { @('capture_boundary_or_redaction_mismatch') }) }
        }
        if ($facts.Kind -eq 'rollback') {
            $rollback = Get-InstallRollbackDecision $facts.Rollback
            return [pscustomobject]@{ Valid = $rollback.Remove; Reasons = $rollback.Reasons }
        }
        if ($facts.Kind -eq 'job_membership') {
            $membership = Get-JobMembershipDecision $facts
            return [pscustomobject]@{ Valid = $membership.Valid; Reasons = $membership.Reasons; ExcludedOwnedExits = $membership.ExcludedOwnedExits }
        }
        if ($facts.ContainsKey('CapturedJobHandle')) {
            $plan = Get-TerminationPlan $facts
            $valid = $plan.Valid -and $plan.Action -eq 'terminate_captured_job_handle' -and -not $plan.ProtocolShutdown -and -not $plan.ForeignSignal -and $plan.EndpointReuseIgnored -eq [bool]$facts.EndpointReused
            return [pscustomobject]@{ Valid = $valid; Reasons = $(if ($valid) { @() } else { @($plan.Reason) }) }
        }
        $decision = Get-CompletionDecision $facts
        return [pscustomobject]@{ Valid = $decision.Success; Reasons = $decision.Reasons; PrimaryFailure = $decision.PrimaryFailure }
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
function Initialize-WindowsSccacheNative {
    if (-not ('WindowsSccacheNative' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'windows-sccache-native.cs') -ErrorAction Stop }
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
    if ($slashes) { [void]$builder.Append(('\' * (2 * $slashes))) }
    [void]$builder.Append('"')
    return $builder.ToString()
}
function Launch-OwnedProcess($Session, [string]$Executable, [string]$CommandLine, [string]$WorkingDirectory, $Environment, [switch]$DirectServer) {
    $process = if ($DirectServer) { [WindowsSccacheNative]::RegisterDirectServerLaunch($Session.LaunchRegistry) } else { [WindowsSccacheNative]::RegisterLaunch($Session.LaunchRegistry) }
    $launchedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
    try {
        [WindowsSccacheNative]::Launch($Session.Job.Handle, $Executable, $CommandLine, $WorkingDirectory, $Environment, $process)
        $Session.Processes.Add($process)
        $Session.ProcessTiming[[string]$process.Pid] = [ordered]@{ launched_ticks = $launchedTicks; elapsed_ms = $null }
        return $process
    }
    catch {
        if ($process.Pid -ne 0 -and -not $Session.ProcessTiming.ContainsKey([string]$process.Pid)) { $Session.ProcessTiming[[string]$process.Pid] = [ordered]@{ launched_ticks = $launchedTicks; elapsed_ms = $null } }
        $Session.LaunchFailure = $true
        throw
    }
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
    foreach ($dir in @((Join-Path $env:SystemRoot 'System32'), (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0'), (Join-Path $env:ProgramFiles 'Git\cmd'))) { if ((Test-Path -LiteralPath $dir -PathType Container) -and -not $dirs.Contains($dir)) { $dirs.Add($dir) } }
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
    try {
        $remaining = Get-RemainingMs $DeadlineTicks
        if ($remaining -le 0 -or -not $Process.Wait($remaining)) { throw 'child_exit_deadline_unconfirmed' }
        $Process.RecordProcessExited([WindowsSccacheNative]::ExitCodeHandle($Process.Handle))
        foreach ($capture in @($Process.Stdout, $Process.Stderr)) {
            $remaining = Get-RemainingMs $DeadlineTicks
            if ($remaining -le 0 -or -not $capture.WaitEof($remaining)) { throw 'child_capture_eof_unconfirmed' }
            $remaining = Get-RemainingMs $DeadlineTicks
            if ($remaining -le 0 -or -not $capture.WaitReader($remaining)) { throw 'child_capture_completion_unconfirmed' }
            if ($capture.Error) { throw 'child_capture_error' }
        }
        $Process.RecordCapturesSettled($true)
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
    return [ordered]@{ total_bytes = $tail.TotalBytes; truncated = $tail.Truncated; leading_partial_line_omitted = $tail.LeadingLineOmitted; output_tail = $tail.Text; eof = [bool]$facts.Eof; reader_complete = [bool]$facts.ReaderComplete; error = $facts.Error }
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
    if ($env:USERPROFILE -ine 'C:\Users\amigo' -or [IO.Path]::GetFullPath($env:USERPROFILE) -ine 'C:\Users\amigo') { throw 'runner_profile_mismatch' }
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

function Confirm-PinnedRuntimeReuse($Session, $Paths) {
    $receiptValid = Test-InstalledReceipt $Paths
    $Session.ArchivePath = Join-Path $Session.OperationDirectory 'sccache-v0.18.0-x64.zip'
    $archive = Receive-PinnedArchive $Session.OperationDirectory $Session.WorkDeadlineTicks
    $Session.ArchivePath = $archive.Path
    $archiveDecision = Get-ArchiveDecision @{ Size = $archive.Size; Sha256 = $archive.Sha256 } (Get-ArchiveEntries $archive.Path)
    if (-not $archiveDecision.Valid) { throw ('pinned_reuse_archive_refused:' + ($archiveDecision.Reasons -join ',')) }
    $zip = [IO.Compression.ZipFile]::OpenRead($archive.Path)
    try { $matches = Test-ArchiveEntryMatchesFile ($zip.GetEntry($script:Pin.Root + '/' + $script:Pin.Executable)) $Paths.Executable } finally { $zip.Dispose() }
    $pe = Get-PEIdentity $Paths.Executable
    $decision = Get-PinnedReuseDecision @{ ReceiptValid = ($receiptValid -and (Test-InstalledReceipt $Paths)); ArchiveValid = $archiveDecision.Valid; PinnedExecutableValid = ($pe.Machine -eq 'x64'); ExecutableBytesEqual = $matches }
    if (-not $decision.Valid) { throw ('pinned_runtime_reuse_refused:' + ($decision.Reasons -join ',')) }
    $Session.Receipt.pinned_runtime = @{ archive_size = $archive.Size; archive_sha256 = $archive.Sha256; executable_size = $pe.Size; executable_sha256 = $pe.Sha256; archive_entry_bytes_equal = $matches }
}

function Install-PinnedSccache($Session) {
    $installStartedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
    $paths = Get-InstallPaths
    if (Test-InstalledReceipt $paths) {
        Confirm-PinnedRuntimeReuse $Session $paths
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
    $session = [ordered]@{ WorkDeadlineTicks = $workDeadline; CleanupDeadlineTicks = $DeadlineTicks; StartedTicks = $OperationStartedTicks; Processes = [System.Collections.Generic.List[object]]::new(); CandidateHandles = [System.Collections.Generic.List[object]]::new(); LaunchRegistry = $null; ProcessTiming = @{}; PrimaryFailure = $null; CleanupError = $null; CleanupAttempted = $false; CleanupResult = $null; InstallCreated = $false; TerminateResult = $null; TerminationKind = 'unconfirmed'; Server = $null; StartupNotification = $null; ConfigPath = $null; CachePath = $null; OperationDirectory = (Join-Path $profile 'cache-session'); ReceiptPath = (Join-Path $profile 'cache-session-receipt.json'); ArtifactFresh = $ArtifactFresh; Receipt = [ordered]@{ version = $script:Pin.Version; source_commit = $script:Pin.SourceCommit; endpoint = $script:Pin.Endpoint; startup = 'unproven'; stats = 'not_run'; termination = 'unconfirmed'; environment_keys = @(); environment_paths = @{}; commands = @() } }
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
        $session.LaunchRegistry = [WindowsSccacheNative]::CreateLaunchRegistry()
        $session.Job = [WindowsSccacheNative]::CreateJob()
        [WindowsSccacheNative]::InitializeJob($session.Job)
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
        if ($Maintenance) { $paths = Install-PinnedSccache $session } elseif (-not $receiptValid) { throw 'pinned_install_receipt_required' } else { Confirm-PinnedRuntimeReuse $session $paths }
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
        $session.StartupNotification = [WindowsSccacheNative]::CreateStartupNotificationLease()
        $pipePath = [WindowsSccacheNative]::OpenStartupNotificationPipe($session.StartupNotification)
        [WindowsSccacheNative]::BeginStartupNotificationRead($session.StartupNotification)
        $serverEnv = [WindowsSccacheNative]::StartupServerEnvironment($childEnv, $pipePath)
        $session.Receipt.server_environment_keys = @($serverEnv.Keys)
        $start = Launch-OwnedProcess $session $paths.Executable (Get-ProcessCommandLine $paths.Executable @()) $paths.Destination $serverEnv -DirectServer
        $notice = [WindowsSccacheNative]::WaitForStartupNotification($session.StartupNotification, $start, $session.Job, (Get-RemainingMs $workDeadline))
        $session.Receipt.startup_notification = @{ pipe_path = $notice.PipePath; connected = $notice.Connected; reader_complete = $notice.ReaderComplete; client_pid = $notice.ClientPid; server_pid = $notice.ServerPid; handle_pid_before = $notice.HandlePidBefore; handle_pid_after = $notice.HandlePidAfter; captured_creation_filetime = $notice.CapturedCreationFileTime; creation_filetime_before = $notice.CreationFileTimeBefore; creation_filetime_after = $notice.CreationFileTimeAfter; job_membership_before = $notice.JobMembershipBefore; job_membership_after = $notice.JobMembershipAfter; alive_before = $notice.AliveBefore; alive_after = $notice.AliveAfter; status = $(if ($notice.Frame) { $notice.Frame.Status } else { $null }); address = $(if ($notice.Frame) { $notice.Frame.Address } else { $null }); payload_bytes = $(if ($notice.Frame) { $notice.Frame.PayloadBytes } else { 0 }); valid = $notice.Valid; reasons = @($notice.Reasons); error = $notice.ReaderError }
        Write-SessionReceipt $session
        if (-not $notice.Valid) { throw ('direct_server_startup_refused:' + ($notice.Reasons -join ',')) }
        $session.Receipt.server_console = @{ mode = 'inherit_existing_parent'; parent_code_page = $start.ParentConsoleCodePage; membership_confirmed = [WindowsSccacheNative]::ConsoleContainsProcess($start.Pid) }
        if (-not $session.Receipt.server_console.membership_confirmed) { throw 'direct_server_parent_console_membership_unconfirmed' }
        $session.Receipt.startup_elapsed_ms = [long](([Diagnostics.Stopwatch]::GetTimestamp() - $session.StartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
        $snapshotStartedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
        $nativeSnapshot = $null; $snapshotException = $null
        try { $nativeSnapshot = [WindowsSccacheNative]::Snapshot($session.Job) } catch { $snapshotException = $_.Exception.GetType().Name + ':' + $(if ($_.Exception -is [System.ComponentModel.Win32Exception]) { $_.Exception.NativeErrorCode } else { 'not_win32' }) }
        $snapshotCompletedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
        if ($nativeSnapshot) {
            $jobSnapshot = @{ AccountingQuerySucceeded = [bool]$nativeSnapshot.AccountingQuerySucceeded; AccountingComplete = [bool]$nativeSnapshot.AccountingComplete; AccountingError = [int]$nativeSnapshot.AccountingError; AccountingActiveProcesses = [uint32]$nativeSnapshot.AccountingActiveProcesses; AccountingReturnLength = [uint32]$nativeSnapshot.AccountingReturnLength; AccountingRequiredBytes = [uint32]$nativeSnapshot.AccountingRequiredBytes; AccountingStartedTicks = [long]$nativeSnapshot.AccountingStartedTicks; AccountingCompletedTicks = [long]$nativeSnapshot.AccountingCompletedTicks; ProcessListQuerySucceeded = [bool]$nativeSnapshot.ProcessListQuerySucceeded; ProcessListError = [int]$nativeSnapshot.ProcessListError; AssignedCount = [uint32]$nativeSnapshot.AssignedCount; ListedCount = [uint32]$nativeSnapshot.ListedCount; ProcessListReturnLength = [uint32]$nativeSnapshot.ProcessListReturnLength; BufferCapacity = [uint32]$nativeSnapshot.BufferCapacity; BufferBytes = [uint32]$nativeSnapshot.BufferBytes; RequiredBytes = [uint64]$nativeSnapshot.RequiredBytes; ProcessListStartedTicks = [long]$nativeSnapshot.ProcessListStartedTicks; ProcessListCompletedTicks = [long]$nativeSnapshot.ProcessListCompletedTicks; ProcessIds = @($nativeSnapshot.ProcessIds); Complete = [bool]$nativeSnapshot.Complete; StartedTicks = $snapshotStartedTicks; CompletedTicks = $snapshotCompletedTicks; Exception = $snapshotException }
        } else { $jobSnapshot = @{ AccountingQuerySucceeded = $false; AccountingComplete = $false; AccountingError = -1; AccountingActiveProcesses = 0; AccountingReturnLength = 0; AccountingRequiredBytes = 48; AccountingStartedTicks = 0; AccountingCompletedTicks = 0; ProcessListQuerySucceeded = $false; ProcessListError = -1; AssignedCount = 0; ListedCount = 0; ProcessListReturnLength = 0; BufferCapacity = 8192; BufferBytes = 65544; RequiredBytes = 0; ProcessListStartedTicks = 0; ProcessListCompletedTicks = 0; ProcessIds = @(); Complete = $false; StartedTicks = $snapshotStartedTicks; CompletedTicks = $snapshotCompletedTicks; Exception = $snapshotException } }
        $session.Receipt.job_membership_snapshot = $jobSnapshot
        Write-SessionReceipt $session
        $launches = @([WindowsSccacheNative]::SnapshotLaunches($session.LaunchRegistry))
        $launchFacts = Get-OwnedLaunchIdentityFacts $launches
        $memberEvidence = [WindowsSccacheNative]::ClassifyJobMembers($jobSnapshot, [object[]]$launchFacts)
        $session.Receipt.job_membership_launches = $launchFacts
        $session.Receipt.job_membership_classification = @{ Reasons = @($memberEvidence.Reasons); Members = @($memberEvidence.Members); CandidatePids = @($memberEvidence.CandidatePids); OwnedLaunchFacts = @($memberEvidence.OwnedLaunchFacts); ExcludedOwnedExits = @($memberEvidence.ExcludedOwnedExits); EligibleOwnedExits = $memberEvidence.EligibleOwnedExits; UnlistedOwnedExits = $memberEvidence.UnlistedOwnedExits; AccountingMinimum = $memberEvidence.AccountingMinimum; AccountingMaximum = $memberEvidence.AccountingMaximum }
        Write-SessionReceipt $session
        $candidateProofs = @(); $candidateInventory = [pscustomobject]@{ Total = $memberEvidence.CandidatePids.Count; Inspected = 0; Truncated = ($memberEvidence.CandidatePids.Count -gt 32); Complete = $false; Error = $null; Proofs = @() }
        if ($memberEvidence.CandidatePids.Count -gt 0) {
            try { $candidateInventory = [WindowsSccacheNative]::CaptureCandidateInventory($session.Job, $session.CandidateHandles, [uint32[]]$memberEvidence.CandidatePids, $workDeadline); $candidateProofs = @($candidateInventory.Proofs) }
            catch { $candidateInventory.Complete = $false; $candidateInventory.Error = $_.Exception.GetType().Name }
        } else { $candidateInventory.Truncated = $false; $candidateInventory.Complete = $true }
        Set-CandidateInventoryReceipt $session $candidateInventory $candidateProofs $memberEvidence.CandidatePids
        Write-SessionReceipt $session
        $proofOwned = $false
        if ($candidateProofs.Count -eq 1 -and $candidateProofs[0].HandleRetained) {
            $proof = $candidateProofs[0]; $candidate = $proof.Lease; $serverPid = [uint32]$proof.Pid
            try {
                if ((Get-RemainingMs $workDeadline) -le 0) { throw 'candidate_identity_deadline_expired' }
                $candidate.Hash = (Get-FileHash -LiteralPath $paths.Executable -Algorithm SHA256).Hash.ToLowerInvariant()
                $proof.ImageMatches = $candidate.Image -ieq $paths.Executable; $proof.HashMatches = $candidate.Hash -ceq $session.ToolResolution.sccache_sha256; $proof.AccountMatches = $candidate.UserSid -ceq $session.UserSid
                if ((Get-RemainingMs $workDeadline) -le 0) { throw 'candidate_identity_deadline_expired' }
                $owners = Get-ListenerFacts $serverPid; $proof.ListenerMatches = [bool]$owners.Owned; $proof.ListenerCount = $owners.Count
                $proof.ListenerRows = @($owners.Rows | Select-Object -First 32 | ForEach-Object { @{ Family = $_.Family; Address = $_.Address; Port = $_.Port; Pid = $_.Pid } }); $proof.ListenerRowsTruncated = $owners.Count -gt $proof.ListenerRows.Count
                [WindowsSccacheNative]::VerifyCandidateIdentity($session.Job, $proof, $workDeadline)
            } catch {
                $win32 = if ($_.Exception -is [System.ComponentModel.Win32Exception]) { $_.Exception.NativeErrorCode } else { 'not_win32' }
                $proof.Errors = @($proof.Errors) + @($_.Exception.GetType().Name + ':' + $win32)
            }
            if (@($proof.Errors).Count -gt 0) { $candidateInventory.Complete = $false }
            $proofOwned = $proof.HandleRetained -and $proof.PidFromHandle -eq $serverPid -and $proof.VerifiedPidFromHandle -eq $serverPid -and $proof.CreationFileTime -gt 0 -and $proof.CreationFileTime -eq $proof.VerifiedCreationFileTime -and $proof.IsProcessInJobBefore -and $proof.IsProcessInJobAfter -and $proof.ImageMatches -and $proof.HashMatches -and $proof.AccountMatches -and $proof.AliveBefore -and $proof.AliveAfter -and $proof.ListenerMatches -and @($proof.Errors).Count -eq 0
            if ($proofOwned) { $session.Server = $candidate; [void]$session.CandidateHandles.Remove($candidate) }
        }
        $membership = Get-JobMembershipDecision @{ Snapshot = $jobSnapshot; Leases = $launchFacts; CandidateProofs = $candidateProofs; CandidateInventoryTotal = $candidateInventory.Total; CandidateInventoryTruncated = $candidateInventory.Truncated; CandidateInventoryComplete = $candidateInventory.Complete }
        Set-CandidateInventoryReceipt $session $candidateInventory $candidateProofs $memberEvidence.CandidatePids
        $session.Receipt.job_membership_decision = $membership
        $directDecision = [WindowsSccacheNative]::ClassifyDirectServerReadiness($notice, [uint32[]]$memberEvidence.CandidatePids, [bool]$membership.Valid, [bool]$candidateInventory.Complete, [bool]$proofOwned)
        $session.Receipt.direct_server_readiness = $directDecision
        Write-SessionReceipt $session
        if (-not $membership.Valid) { throw ('job_membership_refused:' + ($membership.Reasons -join ',')) }
        if (-not $directDecision.Valid) { throw ('direct_server_readiness_refused:' + ($directDecision.Reasons -join ',')) }
        $serverPid = [uint32]$session.Server.Pid
        $serverHandle = $session.Server.Handle
        $session.Receipt.startup = 'notification_process_job_account_image_hash_listener_proven'
        $session.Receipt.server = @{ pid = $serverPid; creation_filetime = $session.Server.Creation; image = $session.Server.Image; sha256 = $session.Server.Hash; user_sid = $session.Server.UserSid; listener = $candidateProofs[0].ListenerRows }
        $stats = Launch-OwnedProcess $session $paths.Executable (Get-ProcessCommandLine $paths.Executable @('--show-stats')) $paths.Destination $childEnv
        $session.StatsProcess = $stats
        Wait-OwnedProcess $session $stats $workDeadline
        if ($stats.ExitCode -ne 0) { throw 'show_stats_protocol_failed' }
        $statsOut = [WindowsSccacheNative]::CaptureFacts($stats.Stdout); $statsErr = [WindowsSccacheNative]::CaptureFacts($stats.Stderr)
        $statsComplete = $statsOut.Eof -and $statsOut.ReaderComplete -and -not $statsOut.Error -and $statsErr.Eof -and $statsErr.ReaderComplete -and -not $statsErr.Error
        $statsTruncated = $statsOut.Truncated -or $statsErr.Truncated
        $statsDecision = Get-StatsDecision $stats.Stdout.Text $statsComplete $statsTruncated
        if (-not $statsDecision.Valid) { throw ('show_stats_counters_refused:' + ($statsDecision.Reasons -join ',')) }
        if (-not [WindowsSccacheNative]::HandleAlive($serverHandle) -or -not [WindowsSccacheNative]::Contains($session.Job, $serverHandle)) { throw 'captured_server_not_live_after_stats' }
        $session.Receipt.stats = 'completed_after_owned_readiness;diagnostic_only'
        $session.Receipt.stats_counters = $statsDecision.Counters
        $session.Receipt.stats_stdout = Get-CaptureRecord $stats.Stdout
        $session.Receipt.stats_stderr = Get-CaptureRecord $stats.Stderr
        $session.Receipt.server_stdout_at_readiness = Get-CaptureRecord $start.Stdout
        $session.Receipt.server_stderr_at_readiness = Get-CaptureRecord $start.Stderr
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
    $leases = @()
    if ('WindowsSccacheNative' -as [type]) { $leases = @([WindowsSccacheNative]::SnapshotLaunches($Session.LaunchRegistry)) }
    elseif ($Session.LaunchRegistry) { $cleanErrors.Add('native_launch_registry_unavailable') }
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

    if ($Session.StartupNotification) { try { $Session.Receipt.startup_notification_cancelled = [WindowsSccacheNative]::CancelStartupNotification($Session.StartupNotification); if (-not $Session.Receipt.startup_notification_cancelled) { $cleanErrors.Add('startup_notification_cancel_failed') } } catch { $Session.Receipt.startup_notification_cancelled = $false; $cleanErrors.Add('startup_notification_cancel_error:' + $_.Exception.GetType().Name) } }; foreach ($lease in $leases) {
        try { if (-not [WindowsSccacheNative]::CloseLaunchParentHandles($lease)) { $cleanErrors.Add('launch_parent_handle_close_failed') } }
        catch { $cleanErrors.Add('launch_parent_handle_close_error:' + $_.Exception.GetType().Name) }
        if ($lease.Created -and -not $lease.Assigned -and -not $lease.ProcessExitConfirmed) {
            try {
                if (-not $lease.UnassignedTerminationAttempted -and -not [WindowsSccacheNative]::TerminateUnassigned($lease)) { $cleanErrors.Add('partial_unassigned_child_termination_failed') }
            } catch { $cleanErrors.Add('partial_unassigned_child_termination_error:' + $_.Exception.GetType().Name) }
        }
    }

    $serverExitConfirmed = -not [bool]$Session.Server
    if ($Session.Server -and $Session.Server.Handle -ne [IntPtr]::Zero) {
        try {
            $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
            if ($remaining -le 0) { $cleanErrors.Add('server_process_exit_deadline_expired') } elseif (-not [WindowsSccacheNative]::WaitHandle($Session.Server.Handle, $remaining)) { $cleanErrors.Add('server_process_wait_timeout') } else { $Session.Server.ExitCode = [WindowsSccacheNative]::ExitCodeHandle($Session.Server.Handle); $serverExitConfirmed = $true }
        } catch { $win32 = if ($_.Exception -is [System.ComponentModel.Win32Exception]) { $_.Exception.NativeErrorCode } else { 'not_win32' }; $cleanErrors.Add('server_process_wait_api_error:' + $_.Exception.GetType().Name + ':' + $win32 + ':' + $_.Exception.Message) }
    } elseif ($Session.Server) { $cleanErrors.Add('server_process_handle_missing') }
    $zeroConfirmed = $false; $activeAfter = $null
    if ($Session.Job) {
        $assignedLaunches = @($leases | Where-Object { $_.Assigned })
        $jobEventRequired = ($assignedLaunches.Count -gt 0)
        if ($activeBefore -gt 0) { $jobEventRequired = $true }
        if ($jobEventRequired) {
            try {
                $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
                if ($remaining -gt 0) { $zeroConfirmed = [WindowsSccacheNative]::WaitJobZero($Session.Job, $remaining) }
            } catch { $win32 = if ($_.Exception -is [System.ComponentModel.Win32Exception]) { $_.Exception.NativeErrorCode } else { 'not_win32' }; $cleanErrors.Add('job_active_zero_wait_api_error:' + $_.Exception.GetType().Name + ':' + $win32 + ':' + $_.Exception.Message) }
        } else { $zeroConfirmed = ($activeBefore -eq 0) }
        if (-not $zeroConfirmed) { $cleanErrors.Add('job_active_zero_event_unconfirmed') }
        try {
            $activeAfter = [WindowsSccacheNative]::ActiveCount($Session.Job)
            if ($activeAfter -ne 0) { $cleanErrors.Add('job_active_count_nonzero') }
        } catch { $cleanErrors.Add('job_accounting_after_failed') }
    } else { $zeroConfirmed = $true; $activeAfter = 0 }
    $processExitsComplete = $serverExitConfirmed
    foreach ($lease in $leases) {
        if (-not $lease.Created) { continue }
        try {
            if ($lease.Handle -eq [IntPtr]::Zero) { throw 'owned_process_handle_missing' }
            if (-not $lease.ProcessExitConfirmed) {
                $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
                if ($remaining -le 0 -or -not $lease.Wait($remaining)) { throw 'owned_process_wait_unconfirmed' }
                $lease.RecordProcessExited([WindowsSccacheNative]::ExitCodeHandle($lease.Handle))
            }
        } catch { $processExitsComplete = $false; $win32 = if ($_.Exception -is [System.ComponentModel.Win32Exception]) { $_.Exception.NativeErrorCode } else { 'not_win32' }; $cleanErrors.Add('owned_command_exit_unconfirmed:' + $_.Exception.GetType().Name + ':' + $win32) }
    }
    $captureComplete = $true
    foreach ($lease in $leases) {
        $leaseCaptureComplete = $true
        foreach ($capture in @($lease.Stdout, $lease.Stderr)) {
            if (-not $capture) { continue }
            try {
                $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
                if ($remaining -le 0 -or -not $capture.WaitEof($remaining)) { throw 'capture_eof_unconfirmed' }
                $remaining = Get-RemainingMs $Session.CleanupDeadlineTicks
                if ($remaining -le 0 -or -not $capture.WaitReader($remaining)) { throw 'capture_reader_completion_unconfirmed' }
                if ($capture.Error) { throw 'capture_reader_error' }
            } catch { $leaseCaptureComplete = $false; $captureComplete = $false; $cleanErrors.Add('capture_settlement_unconfirmed:' + $_.Exception.GetType().Name) }
        }
        $lease.RecordCapturesSettled($leaseCaptureComplete)
    }
    $notificationComplete = $true
    if ($Session.StartupNotification) { try { $notificationComplete = [WindowsSccacheNative]::CloseStartupNotification($Session.StartupNotification, (Get-RemainingMs $Session.CleanupDeadlineTicks)) } catch { $notificationComplete = $false }; if (-not $notificationComplete) { $cleanErrors.Add('startup_notification_cleanup_unconfirmed') } }
    $Session.Receipt.startup_notification_cleanup_complete = $notificationComplete
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
    foreach ($lease in $leases) {
        try {
            $timing = if ($lease.Pid -ne 0 -and $Session.ProcessTiming.ContainsKey([string]$lease.Pid)) { $Session.ProcessTiming[[string]$lease.Pid] } else { $null }
            if ($timing -and $lease.ProcessExitConfirmed -and -not $timing.elapsed_ms) { $now = [Diagnostics.Stopwatch]::GetTimestamp(); $timing.completed_ticks = $now; $timing.elapsed_ms = [long](($now - $timing.launched_ticks) * 1000 / [Diagnostics.Stopwatch]::Frequency) }
            $Session.Receipt.commands += @(@{ pid = $(if ($lease.Created) { $lease.Pid } else { $null }); assigned = [bool]$lease.Assigned; process_exit_confirmed = [bool]$lease.ProcessExitConfirmed; exit_code = $(if ($lease.ProcessExitConfirmed) { $lease.ExitCode } else { $null }); captures_complete = [bool]$lease.CapturesComplete; timing = $timing; stdout = $(if ($lease.Stdout) { Get-CaptureRecord $lease.Stdout } else { $null }); stderr = $(if ($lease.Stderr) { Get-CaptureRecord $lease.Stderr } else { $null }) })
        } catch { $cleanErrors.Add('process_receipt_error:' + $_.Exception.GetType().Name) }
    }
    $statsCaptureComplete = $false; $statsExit = $null; $statsOutput = ''; $statsOutputComplete = $false; $statsOutputTruncated = $true
    if ($Session.StatsProcess) {
        try {
            $statsExit = if ($Session.StatsProcess.Wait(0)) { $Session.StatsProcess.ExitCode } else { $null }
            $outFacts = [WindowsSccacheNative]::CaptureFacts($Session.StatsProcess.Stdout); $errFacts = [WindowsSccacheNative]::CaptureFacts($Session.StatsProcess.Stderr)
            $statsCaptureComplete = $outFacts.Eof -and $outFacts.ReaderComplete -and -not $outFacts.Error -and $errFacts.Eof -and $errFacts.ReaderComplete -and -not $errFacts.Error
            $statsOutput = $Session.StatsProcess.Stdout.Text; $statsOutputComplete = $statsCaptureComplete; $statsOutputTruncated = $outFacts.Truncated -or $errFacts.Truncated
        } catch { $cleanErrors.Add('stats_completion_unconfirmed') }
    }
    $resourcesSettled = $zeroConfirmed -and $activeAfter -eq 0 -and $processExitsComplete -and $captureComplete -and $notificationComplete
    if ($resourcesSettled) { try { Remove-OperationFiles $Session } catch { $cleanErrors.Add('operation_file_cleanup_failed') } }
    else { $cleanErrors.Add('operation_files_retained_for_unsettled_resources') }

    $handlesClosed = $notificationComplete
    if ($Session.Server -and $Session.Server.Handle -ne [IntPtr]::Zero -and $resourcesSettled) {
        try {
            if ([WindowsSccacheNative]::CloseHandlePublic($Session.Server.Handle)) { $Session.Server.Handle = [IntPtr]::Zero }
            else { $handlesClosed = $false; $cleanErrors.Add('server_handle_close_failed') }
        } catch { $handlesClosed = $false; $cleanErrors.Add('server_handle_close_failed') }
    } elseif ($Session.Server -and $Session.Server.Handle -ne [IntPtr]::Zero) {
        $handlesClosed = $false; $cleanErrors.Add('server_handle_retained_unsettled')
    }
    foreach ($candidate in $Session.CandidateHandles) {
        if ($candidate.Handle -eq [IntPtr]::Zero) { continue }
        if ($resourcesSettled) {
            try { if ([WindowsSccacheNative]::CloseHandlePublic($candidate.Handle)) { $candidate.Handle = [IntPtr]::Zero } else { $handlesClosed = $false; $cleanErrors.Add('unverified_job_member_handle_close_failed') } }
            catch { $handlesClosed = $false; $cleanErrors.Add('unverified_job_member_handle_close_error:' + $_.Exception.GetType().Name) }
        } else { $handlesClosed = $false; $cleanErrors.Add('unverified_job_member_handle_retained_unsettled') }
    }
    foreach ($lease in $leases) {
        try {
            $jobZeroForLease = -not $lease.Assigned -or ($zeroConfirmed -and $activeAfter -eq 0)
            if (-not [WindowsSccacheNative]::CanCloseLaunchHandles($lease, $jobZeroForLease) -or -not [WindowsSccacheNative]::CloseLaunchLease($lease, $jobZeroForLease, (Get-RemainingMs $Session.CleanupDeadlineTicks))) { $handlesClosed = $false; $cleanErrors.Add('owned_launch_lease_handle_close_unconfirmed') }
        } catch { $handlesClosed = $false; $cleanErrors.Add('owned_launch_lease_handle_close_error:' + $_.Exception.GetType().Name) }
    }
    if ($Session.Job) {
        if ($zeroConfirmed -and $activeAfter -eq 0) { try { if (-not $Session.Job.CloseChecked()) { $handlesClosed = $false; $cleanErrors.Add('owned_job_handle_close_failed') } } catch { $handlesClosed = $false; $cleanErrors.Add('owned_job_handle_close_failed') } }
        else { $handlesClosed = $false; $cleanErrors.Add('owned_job_handle_retained_unsettled') }
    }
    if ($Session.PreviousEnvironment) {
        foreach ($key in $Session.PreviousEnvironment.Keys) {
            try { [Environment]::SetEnvironmentVariable($key, $Session.PreviousEnvironment[$key], 'Process') } catch { $handlesClosed = $false; $cleanErrors.Add('process_environment_restore_failed') }
        }
    }
    $nowTicks = [Diagnostics.Stopwatch]::GetTimestamp()
    $Session.Receipt.cleanup_elapsed_ms = [long](($nowTicks - $cleanupStartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
    $Session.Receipt.session_elapsed_ms = [long](($nowTicks - $Session.StartedTicks) * 1000 / [Diagnostics.Stopwatch]::Frequency)
    $facts = @{ StartupNotification = $startupProven; CapturedProcess = [bool]$Session.Server; JobMembership = [bool]$Session.Server; AccountAndImage = [bool]$Session.Server; EndpointOwner = $startupProven; StatsCompleted = ($Session.Receipt.stats -eq 'completed_after_owned_readiness;diagnostic_only'); StatsCaptureComplete = $statsCaptureComplete; StatsOutput = $statsOutput; StatsOutputComplete = $statsOutputComplete; StatsOutputTruncated = $statsOutputTruncated; StatsExitCode = $statsExit; RequireReadiness = $true; CaptureComplete = ($captureComplete -and $cleanErrors.Count -eq 0); ActiveZeroEvent = $zeroConfirmed; ActiveProcesses = $activeAfter; ProcessExited = ($processExitsComplete -and $serverExitConfirmed); HandlesClosed = $handlesClosed; TerminationKind = $Session.TerminationKind; TerminateResult = $Session.TerminateResult; Cancelled = $Cancelled; DeadlineExpired = ((Get-RemainingMs $Session.CleanupDeadlineTicks) -le 0); CleanupError = ($cleanErrors.Count -gt 0); ArtifactFresh = [bool]$Session.ArtifactFresh; PrimaryFailure = $PrimaryFailure }
    $decision = Get-CompletionDecision $facts
    $Session.Receipt.outcome = $decision.Outcome
    $Session.Receipt.completion_reasons = $decision.Reasons
    $Session.Receipt.stats_counters = $decision.StatsCounters
    $Session.Receipt.active_zero_event = $zeroConfirmed
    $Session.Receipt.handles_closed = $handlesClosed
    $rollbackFacts = @{ InstallCreated = [bool]$Session.InstallCreated; OperationFailed = (-not $decision.Success); OwnershipResolved = $resourcesSettled; JobZeroConfirmed = $zeroConfirmed; ActiveProcesses = $activeAfter; ProcessExitsComplete = $processExitsComplete; CaptureComplete = $captureComplete; HandlesClosed = $handlesClosed; Cancelled = $Cancelled; DeadlineExpired = ((Get-RemainingMs $Session.CleanupDeadlineTicks) -le 0); CleanupErrors = @($cleanErrors); TerminationKind = $Session.TerminationKind; TerminateResult = $Session.TerminateResult }
    $rollbackDecision = Get-InstallRollbackDecision $rollbackFacts
    if ($Session.InstallCreated -and -not $decision.Success) {
        $Session.Receipt.install_rollback = 'retained:' + ($rollbackDecision.Reasons -join ',')
        if ($rollbackDecision.Remove -and -not (Remove-CreatedInstall $Session)) { $cleanErrors.Add('owned_install_rollback_failed') }
    } elseif ($Session.InstallCreated) { $Session.Receipt.install_rollback = 'retained_successful_install' }
    $Session.Receipt.rollback_decision = @{ remove = $rollbackDecision.Remove; reasons = $rollbackDecision.Reasons }
    $Session.Receipt.cleanup_errors = @($cleanErrors)
    try { Write-SessionReceipt $Session } catch { $cleanErrors.Add('receipt_write_failed'); $Session.Receipt.cleanup_errors = @($cleanErrors) }
    if ($cleanErrors.Count -ne 0) { $Session.Receipt.outcome = 'unqualified' }
    $Session.CleanupResult = [pscustomobject]@{ Success = ($decision.Success -and $cleanErrors.Count -eq 0); Decision = $decision; TerminationKind = $Session.TerminationKind; TerminateResult = $Session.TerminateResult; Errors = @($cleanErrors); ReceiptPath = $Session.ReceiptPath }
    return $Session.CleanupResult
}

function Remove-CreatedInstall($Session) {
    try {
        $paths = $Session.Paths
        if (-not $paths -or -not (Test-Path -LiteralPath $paths.Destination -PathType Container)) { throw 'created_install_destination_missing' }
        if (-not (Test-InstalledReceipt $paths)) { throw 'created_install_receipt_no_longer_matches' }
        Remove-Item -LiteralPath $paths.Executable -Force
        Remove-Item -LiteralPath $paths.Receipt -Force
        [IO.Directory]::Delete($paths.Destination, $false)
        $Session.Receipt.install_rollback = 'removed_exact_created_receipt_owned_files'
        return $true
    } catch { $Session.Receipt.install_rollback = 'failed:' + $_.Exception.GetType().Name; return $false }
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
if ($PSCmdlet.ParameterSetName -eq 'Contract') { Initialize-WindowsSccacheNative; exit (Invoke-ContractOnly) }
if ($PSCmdlet.ParameterSetName -eq 'Maintenance') { exit (Invoke-Maintenance $Operation) }
