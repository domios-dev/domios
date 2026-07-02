## DOGFOOD PROTOCOL — you are building the system you use (read + follow)
We are DOGFOODING the CodeTerm planning/canvas system AS we build it. You don't just implement —
you USE it, FILL it, and DEBUG it. This also road-tests the prompts that get hardcoded later.
1. WORK THROUGH THE PLAN. As you execute, record your real EXECUTION STAGES as milestones on YOUR task:
   codeterm agent report --kind <status_update|decision|testing|review> --text "<what happened at this stage>"
   Plan/task are resolved automatically from your pane assignment — no `--plan`/`--task` flags needed.
   (use --doc <file.md> to attach a longer discussion/decision doc). Do it at REAL checkpoints, not only at the end.
2. ATTACH YOUR DESIGN DECISIONS as a 'decision' milestone/doc so PROVENANCE stays — anyone can later review what you decided and WHY.
3. ALSO REPORT TO ME (your parent) with a plain 'codeterm agent report --status done "<summary + commit>"'. We are still partly bootstrapping on the old flow, so I need the report HERE too. Both, not either.
4. YOU ARE DEBUGGING THE SYSTEM BY USING IT. If any handle is MISSING, clunky, or broken (a field won't set, the CLI lacks an option, filling something is awkward, a command silently drops input) — SAY SO EXPLICITLY in your report (friction → handle). Your friction is exactly how we improve the tool. This is important.
5. At the end: confirm you dogfooded (which milestones/docs you attached to which task) AND flag any friction you hit.
