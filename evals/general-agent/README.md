# General Agent provider matrix

This bench validates the real first-run General Agent path against a clean dev
profile. It uses only the authenticated dev API on port 7686 and subscribes to
SSE before launch. It never targets prod, invokes Playwright, polls, sleeps, or
treats elapsed time as successful readiness.

Run one provider per newly created `~/.codeterm-dev` profile:

```powershell
$env:GA_PROVIDER='claude'; npm run test:general-agent-provider
$env:GA_PROVIDER='codex'; npm run test:general-agent-provider
$env:GA_PROVIDER='opencode'; npm run test:general-agent-provider
$env:GA_PROVIDER='cursor'; npm run test:general-agent-provider
```

Claude, Codex, and OpenCode must:

1. replace the untouched Get started surface;
2. bind an authoritative session; providers with a system-prompt channel carry
   bootstrap there and create no interactive PTY delivery, while providers
   without one confirm the fallback delivery;
3. produce a first assistant reply;
4. restart with the same Tab and session, a new PTY, and produce an exact
   post-resume reply.

Cursor is intentionally launch-only because a limited account may not produce
a model reply. It must still reach `agent_ready`, bind a session, and use the
delivery surface declared by its provider capability.

The runner refuses a reused profile. Before each invocation, stop dev with
`.\build.ps1 kill`, preserve or remove only `~/.codeterm-dev`, then start a
fresh visible dev instance with `.\build.ps1 dev`. Never remove
`~/.codeterm`, which is prod.

## First-run wording and Guide navigation

`evals.json` includes first-run ids 14–16. They replace user-facing output with
opaque fragment tokens and judge ordering: deleting `bootstrap.md`, then
`bootstrap:setup-complete`, then a single `bootstrap:post-setup-supporter`.
Missing identity must not reach the optional fragment, and a finished flow must
not repeat it. No expectation asserts the announcement's prose.

The bootstrap is delivered alongside the General Agent instructions. Evaluating
only `bootstrap_instructions.md` against the existing machine-control cases
would omit their instructions. `prepare-prompts.py` reads the old arm from the
pinned revision and the new arm from current sources. It composes the GA primer,
GA template, shared CLI with its memory section removed, pad contract, child
recovery, framework rule and memory protocol through their production
substitutions. The `prompt_family` field selects first-run cases for the composed
bootstrap; existing mature-agent cases use the composition without bootstrap,
so first-run instructions do not override their scenario. It also composes the
spawned-agent primer with its pad and rendering fragments, matching its
production composition.

Isolated lanes author source only. The Orchestrator runs these commands in
canonical, serialized through the bounded runner. The fixed old revision below
is the Herald lane's unchanged prompt baseline; do not replace it with a moving
`HEAD~1` after merges.

```bash
python3 evals/general-agent/prepare-prompts.py \
  --baseline-ref a004b77d4c6c8d021231248b6390f2ee05bf90ce \
  --output-dir /tmp/herald-prompt-bench

TEST_CAP_SECS=3600 scripts/test-capped.sh --exec \
  python3 .agents/skills/prompt-evaluator/runner.py \
  --prompt-a /tmp/herald-prompt-bench/general-agent-old.md \
  --prompt-b /tmp/herald-prompt-bench/general-agent-new.md \
  --eval-set /tmp/herald-prompt-bench/general-agent-evals.json --runs 3 --no-cache \
  --report markdown

TEST_CAP_SECS=3600 scripts/test-capped.sh --exec \
  python3 .agents/skills/prompt-evaluator/runner.py \
  --prompt-a /tmp/herald-prompt-bench/bootstrap-old.md \
  --prompt-b /tmp/herald-prompt-bench/bootstrap-new.md \
  --eval-set /tmp/herald-prompt-bench/bootstrap-evals.json --runs 3 --no-cache \
  --report markdown

TEST_CAP_SECS=3600 scripts/test-capped.sh --exec \
  python3 .agents/skills/prompt-evaluator/runner.py \
  --prompt-a /tmp/herald-prompt-bench/spawned-agent-old.md \
  --prompt-b /tmp/herald-prompt-bench/spawned-agent-new.md \
  --eval-set evals/spawned-agent-primer/evals.json --runs 3 --no-cache \
  --report markdown

scripts/test-capped.sh --exec python3 -m unittest discover \
  -s evals/general-agent -p 'test_*.py'
scripts/test-capped.sh --exec npm run test:unit -- src/help
scripts/test-capped.sh --exec npm run test:unit -- packages/chat-ui/src/MarkdownBody
```

Each A/B report contains the old and new arm, distinct live samples and delta.
Existing general-agent ids 1–13 and primer ids 1–7 must not regress. New
first-run ids 14–16 and primer id 8 must pass 3/3 on the new arm. The old arm is
expected to fail the new positive placement/navigation cases. Read sample
responses for any failed or flaky case; a judge timeout is not behavioral proof.
Record reports and the observed delta as a plan stride. A source edit or a
prepared prompt is not an executed bench result.

The offline test checks fragment marker uniqueness and placement after the last
setup action, plus the Guide directive's containment in that fragment. The
happy-dom Guide test renders the real Markdown and SettingsPanel, clicks the
native control and checks section selection, scroll, heading focus, repeat
navigation and unknown-id fallback. It rejects inline text, quotes, code fences
and controls inside file previews.

Manual far-end proof on a named dev instance with a fresh first-run profile:
verify its build first; observe no optional mention in the greeting, tour or
identity capture; finish identity, soul, memory save and bootstrap deletion;
observe one post-setup note and `@ds-guide supporter-agent` control; activate it
with the keyboard and confirm Settings > Guide selects the section and focuses
its heading. Continue the conversation and confirm the note is not repeated.
This checks the real first-run journey beyond the sandbox bench and DOM harness.
Never reset the production profile or use Playwright for this check.

<!-- revision: 4 -->
