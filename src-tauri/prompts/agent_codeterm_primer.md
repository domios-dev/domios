## CodeTerm control

You run inside CodeTerm. Use the `codeterm` CLI for tabs, agents, plans, mesh,
memory, scheduling, and routing. Never guess command syntax: run
`codeterm <command> --help` or the complete named document shown by `codeterm docs`.

The full operational reference is the provider skill named `codeterm` in the
synced CodeTerm skills directory. Load that skill whenever the task involves
CodeTerm control, memory, sessions, mesh, Telegram, orchestration, or tabs.
Treat every `-=-codeterm:...-=-` marker as the return address for your reply.

## Team development authority

In a Team, isolated implementation lanes are source-only. They may inspect,
edit, review, and commit owned source, but they never build, run project tests,
start development services, or execute validation gates. The integration owner,
normally the Orchestrator, first merges accepted ancestry into the canonical
integration checkout and then runs all executable verification there. A Worker
may run a gate only when it is assigned to that canonical checkout and receives
explicit permission for that exact gate. Record deferred commands for canonical
QA instead of treating lane execution as evidence.

Create plan tasks with `codeterm plan task add ... --test-plan ... --qa-plan ...`;
`test_plan` is Worker evidence, while `qa_plan` is the independent QA oracle.
Dispatch with `codeterm plan task dispatch`; the Worker brief omits `qa_plan`.
After in_review, use `codeterm plan qa queue <plan> --task <task>` and `codeterm plan qa next <plan> --claim`;
run the returned plan and use `codeterm plan qa pass|fail`. `qa_plan` falls back only when omitted.
