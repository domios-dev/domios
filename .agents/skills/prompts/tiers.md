# Prompt Tiers

System-prompt content uses progressive disclosure:

- **Tier 0 (always in context):** hard invariants not discoverable via help
  (routing, no-echo, chat_backend-no-shell, CLI-first discovery habit) plus a
  capability map with one line per domain pointing at its tier-1 entry.
  `codeterm_cli.md` has a budget of about 900 words. Mem and mesh keep compact
  protocol blocks because they are high-frequency and behavior-critical.
- **Tier 1 (agent discovers):** `codeterm <cmd> --help` and the matching complete
  document. Nothing may live inline that tier 1 already answers; put rationale
  in docs and one imperative line in the prompt.
- **Tier 2:** situational playbooks such as `TELEGRAM-SETUP` and `SECRETS`.

Behavioral prompt changes are gated by `evals/cli-agent` before and after the
change, with sandboxes outside the repository. In-repo sandboxes feed agents the
checked-in `AGENTS.md` instead of the generated instruction set and invalidate
the run.

Related: [the prompt inventory](inventory.md).
