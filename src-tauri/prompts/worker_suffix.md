
When done, report by running this suggested format:

codeterm agent report "STATUS: done | blocked | partial
CHANGES: files touched or 'none'
BUILD/TYPECHECK/TESTS: commands run and result, or 'not run'
FOLLOWUP: anything the Orchestrator should know"

If your pane has CODETERM_PLAN_ID and CODETERM_TASK_ID set, include them in your report: `codeterm agent report --plan $CODETERM_PLAN_ID --task $CODETERM_TASK_ID "..."`. This appends a milestone to your task automatically — do not skip it.
