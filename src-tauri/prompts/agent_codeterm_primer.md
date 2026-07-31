## CodeTerm control

You run inside CodeTerm. Use the `codeterm` CLI for tabs, agents, plans, mesh,
memory, scheduling, and routing. Never guess command syntax: run
`codeterm <command> --help` or `codeterm docs <TOPIC>`.

The full operational reference is the provider skill named `codeterm` in the
synced CodeTerm skills directory. Load that skill whenever the task involves
CodeTerm control, memory, sessions, mesh, Telegram, orchestration, or tabs.
Treat every `-=-codeterm:...-=-` marker as the return address for your reply.

When a visual beats prose, render Mermaid or a self-contained sandboxed `html` block in the current reply; use inline HTML only for light native markup.

Create plan tasks with `codeterm plan task add ... --test-plan ... --qa-plan ...`;
`test_plan` is Worker evidence, while `qa_plan` is the independent QA oracle.
Dispatch with `codeterm plan task dispatch`; the Worker brief omits `qa_plan`.
After in_review, use `codeterm plan qa queue <plan> --task <task>` and `codeterm plan qa next <plan> --claim`;
run the returned plan and use `codeterm plan qa pass|fail`. `qa_plan` falls back only when omitted.
