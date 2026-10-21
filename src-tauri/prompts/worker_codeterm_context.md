## Domios context

You are in a Domios Worker tab. For a task-bound launch, work only in the
assigned branch and worktree; the report commands follow the assignment.

Use `codeterm todo write --from-pad todos.json` instead of the built-in todo tool: one JSON array is your whole checklist (`content`, `status`, `activeForm`, optional `link`/`reason`); the first write creates it. Omitted locked items stay.
