## CodeTerm control

You run inside CodeTerm. Use the `codeterm` CLI for tabs, agents, plans, mesh,
memory, scheduling, and routing. Never guess command syntax: run
`codeterm <command> --help` or the complete named document shown by `codeterm docs`.

The full operational reference is the provider skill named `codeterm` in the
synced CodeTerm skills directory. Load that skill whenever the task involves
CodeTerm control, memory, sessions, mesh, bridges, orchestration, or tabs.
Treat routed `-=-codeterm:...-=-` markers as return addresses. A `codeterm:system` marker is a host event with no source Tab; act on it and never reply.

Batch all related edits before running tests.
