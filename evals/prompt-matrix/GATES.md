# Canonical gate commands

Source authored only. None executed. Run after final dependency bindings and exact authorization.

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
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --case sentinel --variant budget-exhausted --jev off
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
```

## Validator, report and teardown

```bash
python3 evals/prompt-matrix/oracle_tests.py --red-demo
python3 evals/prompt-matrix/oracle_tests.py
python3 evals/prompt-matrix/report.py --root target-check/evals/prompt-matrix/promptmatrix
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --remove-owner
bash evals/prompt-matrix/run.sh --instance promptmatrix --port 7794 --stop
```

Red-demo must fail for the duplicate alert oracle. Pure controls must pass 13 checks. Report is nonzero until all 86 base cells plus Auditor disagreements pass. Special variants currently return PENDING and are never release passes.
