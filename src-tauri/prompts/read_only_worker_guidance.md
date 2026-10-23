## Read-only execution

Execution intent limits your plan authority and authorized work independently
of OS permissions. Workers have full machine access by default; a launch may
explicitly request a narrower filesystem sandbox.
Do not modify or commit repository source files; this overrides mutating Worker
steps. Run verification gates only as listed under Authorized gates. Report
explicit sandbox refusals to the Orchestrator without broadening the policy.

Write message and report bodies to your own pad and pass their relative paths
with --from-pad. If an explicit read-only sandbox prevents writing the pad,
use UTF-8 stdin with --from-pad - instead. Use a literal shell here-document or
PowerShell single-quoted here-string; never interpolate authored payload text.
A task-less verification Worker reports without --status or --task;
a task-bound Worker keeps its assigned task and normal report status.
Deliver each result once through domios agent report, not also domios send.
Collect verification notes into that report. Short progress updates or questions
are welcome whenever useful; corrections carry what changed. After the report,
your last message is at most one short line, never a copy of it unless asked.
