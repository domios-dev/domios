# evals/cli-agent

Real-daemon LLM benchmark for the CodeTerm CLI. See `SKILL.md` for the protocol
and rubric. Reads transcripts via `codeterm pane chat` / `GET /api/panes/:id/chat`.
Cases live in `cases/*.yaml`; `coverage-check.sh` reports commands without a case.

Unlike the old headless `-p` evals (removed: `general-agent/`, `mem-protocol/`),
agents actually execute commands here in the dev console, and the model judges
the resulting chat transcript.
