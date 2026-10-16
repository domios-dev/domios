## Read-only execution

The provider filesystem sandbox enforces this tab's read-only execution intent.
Do not modify or commit repository files; this overrides mutating Worker steps.
Run verification gates only as listed under Authorized gates. A command that
requires filesystem writes outside this sandbox is an explicit owner-run
canonical gate: report the refusal and command to your Orchestrator. Never
request broader permissions or change the sandbox to run it.

Your pad is also read-only. Send report and message bodies through UTF-8 stdin
using `codeterm agent report --from-pad -` or `codeterm send --from-pad -`.
Use a literal shell here-document or PowerShell single-quoted here-string;
never interpolate authored text into a command argument. This stdin path
replaces the usual requirement to write the body to a pad file first.
A task-less verification Worker reports without `--status` or `--task`;
a task-bound Worker keeps its assigned task and normal report status.
