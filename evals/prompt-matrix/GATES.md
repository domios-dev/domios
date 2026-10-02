# Canonical gate commands

Source authored only. None executed. Policy cells require final dependency bindings and exact authorization. The focused feed regression and activity-feed cell can run before Auditor binds, only in canonical under their exact authorization.

Run start/stop in a Team-bound utility shell; QA is serialized on the physical machine.

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --prepare
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --start
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --smoke
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --create-owner --provider codex
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --disconnect-jev
```

Reviewed older-version inputs: PM_OLDER_CLAUDE, PM_OLDER_CODEX and PM_OLDER_OPENCODE. Supply exact versions before authorization. Cursor update is pending archived-binary binding.

## Jev off

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case update --jev off --older-version "$PM_OLDER_CLAUDE"
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update --jev off --older-version "$PM_OLDER_CODEX"
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case update --jev off --older-version "$PM_OLDER_OPENCODE"
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-hidden --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-cron --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-visible --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-lane --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case update --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case login --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case permission --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case bad-model --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case crash --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version "$PM_OLDER_CODEX" --jev off --variant default
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version "$PM_OLDER_CODEX" --jev off --variant busy
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version "$PM_OLDER_CODEX" --jev off --variant fail
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant git --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant fake-ci --jev off
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant regex-no-jev --jev off
```

## Jev on

Connect Jev with a token supplied on stdin; do not place it in argv or a committed file.

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --connect-jev
```

```bash
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case update --jev on --older-version "$PM_OLDER_CLAUDE"
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider claude --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update --jev on --older-version "$PM_OLDER_CODEX"
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case update --jev on --older-version "$PM_OLDER_OPENCODE"
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider opencode --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-hidden --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-cron --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-visible --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case trust-lane --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case update --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case login --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case permission --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case bad-model --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider cursor --case crash --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version "$PM_OLDER_CODEX" --jev on --variant default
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version "$PM_OLDER_CODEX" --jev on --variant busy
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --provider codex --case update-coordinator --older-version "$PM_OLDER_CODEX" --jev on --variant fail
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant git --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant fake-ci --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant regex-no-jev --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant budget-exhausted --jev on
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant activity-feed --jev on
```

## Validator, report and teardown

```bash
python3 evals/prompt-matrix/oracle_tests.py --red-demo
python3 evals/prompt-matrix/oracle_tests.py --red-sentinel-demo
python3 evals/prompt-matrix/oracle_tests.py
./build.sh test-one app supporter_sentinel_verdict_is_visible_in_activity_feed
python3 evals/prompt-matrix/report.py --root target-check/evals/prompt-matrix/promptmatrix
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --remove-owner
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --stop
```

Both red demos must exit nonzero for duplicate alert/transition rejection. Pure controls must pass 22 checks. The focused Rust feed test is expected red before the owner's scheduled feed repair, then must pass after that merge. The activity-feed live cell must likewise expose the persisted verdict on the owner API surface. Report is nonzero until all 86 executable cells plus Auditor disagreements pass; the sole budget-off N/A is explicitly owner accepted. Coordinator and Sentinel policy-path cells remain PENDING until Courier/Auditor bind.

No command is requested for Sentinel budget-exhausted/Jev-off: Brahms accepted N/A because regex classification never spends model budget. Exact approval must name each command above and the one named instance. All commands remain deferred; none ran in this lane.
