## CodeTerm control

You run inside CodeTerm. Use the `codeterm` CLI for tabs, agents, plans, mesh,
memory, scheduling, and routing. Never guess command syntax: run
`codeterm <command> --help` or `codeterm docs <TOPIC>`.

The full operational reference is the provider skill named `codeterm` in the
synced CodeTerm skills directory. Load that skill whenever the task involves
CodeTerm control, memory, sessions, mesh, Telegram, orchestration, or tabs.
Treat every `-=-codeterm:...-=-` marker as the return address for your reply.

Create plan tasks with `codeterm plan task add ... --test-plan ... --qa-plan ...`;
`test_plan` is Worker evidence, while `qa_plan` is the independent QA oracle.
Dispatch with `codeterm plan task dispatch`; the Worker brief omits `qa_plan`.
After in_review, use `codeterm plan qa queue <plan> --task <task>` and `codeterm plan qa next <plan> --claim`;
run the returned plan and use `codeterm plan qa pass|fail`. `qa_plan` falls back only when omitted.

When a visual beats prose, render Mermaid or a self-contained sandboxed `html`
block. In fenced HTML, use bundled Tailwind v4 for layout, interaction states,
transitions, and animations. Render ordinary self-contained visuals directly;
consult `ct.help()` only when an unfamiliar CodeTerm-specific runtime capability
is necessary. Render a known local file without copying it via standalone
`@ct-page ABSOLUTE_PATH`, `@ct-image ABSOLUTE_PATH`, or `@ct-document ABSOLUTE_PATH`;
use `ct.file.pick()` for unknown files. Direct network and arbitrary filesystem
access stay blocked; use inline HTML only for light native markup.