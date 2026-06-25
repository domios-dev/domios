---
tags:
- friction
- state-integrity
---

Dogfood finding: handoff #3 text/instructions previously said to read a concrete handoff with `codeterm plan handoff show <plan> --seq 3`, but current CLI help is `show <PLAN_ID> [SELECTOR]` and `--seq` fails as an unexpected argument. Current working commands are positional selector `show <plan> 3 --markdown`, id selector, and `--latest --markdown`. Inspect the clap/parser and handoff resume_prompt/docs generation. Either add a backwards-compatible `--seq <N>` alias or update all generated/docs references to the positional selector, with tests so this does not regress.