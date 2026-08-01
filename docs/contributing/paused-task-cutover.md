# Legacy parked-task cutover

This is a one-time, operator-controlled conversion for plan `pl_as7ty5`. It must run only while no CodeTerm process can read or write the plan. The application does not accept `parked` as an alias for `paused`.

The only authorized records are:

- `task_pes3i4` — `ci-only-v1-7-2-publish-and-updater-verification.yaml`
- `task_p6kh7f` — `issue-reconciliation-2.yaml`
- `task_sjhx4f` — `release-closeout-and-archive-2.yaml`

## Preflight on the old parser

From the repository containing the live `.codeterm/plans` store, verify all three records still render as `parked` and no fourth record does:

```bash
codeterm task show task_pes3i4 --json
codeterm task show task_p6kh7f --json
codeterm task show task_sjhx4f --json
rg -n '^status: parked([[:space:]]|$)' .codeterm/plans
test "$(rg -l '^status: parked([[:space:]]|$)' .codeterm/plans/active | wc -l | tr -d '[:space:]')" -eq 3
```

Resolve the exact files and require a three-file inventory:

```bash
set -- $(rg -l '^id: task_(pes3i4|p6kh7f|sjhx4f)$' .codeterm/plans/active/pl_as7ty5/tasks | sort)
test "$#" -eq 3
printf '%s\n' "$@"
```

Create a recovery copy outside the plan store, then run the non-mutating audit:

```bash
backup_dir="$(mktemp -d)/pl_as7ty5-parked-backup"
mkdir -p "$backup_dir"
cp -p "$@" "$backup_dir/"
python scripts/convert-legacy-parked-tasks.py \
  --reason "Approved one-time task-state cutover for pl_as7ty5" \
  "$@"
```

The dry run must print exactly three `would convert` lines with distinct audit IDs and strictly increasing timestamps. Do not continue if another task is parked, an expected task is absent, or the backup does not contain three files.

## Stop, apply, and start

1. Quit the old CodeTerm application normally. Do not kill the production process. Verify port 7685 has no listener before touching the sidecars. Stop every other process, editor, sync client, or script that could write the three task files; the migration lock is cooperative on Unix and is not a substitute for the daemon-stop boundary. Run only on a local, same-volume filesystem. NFS, SMB, cloud-synced folders, and other remote or replicated filesystems are unsupported.
2. Run the apply command while CodeTerm is stopped:

   ```bash
   python scripts/convert-legacy-parked-tasks.py \
     --reason "Approved one-time task-state cutover for pl_as7ty5" \
     --apply "$@"
   ```

3. Preserve the `conversion journal` path and the three `converted` output lines as cutover evidence. Each line contains the durable audit ID and timestamp written into that task's milestones.
4. Start the reviewed new CodeTerm build through the normal application launcher. Do not restart the old parser after apply.

The converter keeps each nonblank `park_reason` as `pause_reason`, records the common cutover reason in a decision milestone, clears Tab assignments, and closes open execution attempts. It preserves comments, unknown fields, and nested YAML structures. Apply owns the whole three-file cutover with one lock record containing a transaction UUID, PID, process-start token, host, and journal path, plus an exclusive handle for every source. Windows installation uses `ReplaceFileW` without the unsupported `REPLACEFILE_WRITE_THROUGH` flag and supplies a transaction-owned same-volume backup path. It records hashes for the destination, replacement, and replacement-backup layouts, explicitly including errors 1175, 1176, and 1177, then reacquires a deny-write handle on each installed destination. Unix keeps locks on the staged/new inodes and prefers `renameat2(RENAME_EXCHANGE)` or `renamex_np(RENAME_SWAP)`, with a verified same-directory rename fallback.

The journal separates `logical_state` from `durability`. Every entry records original, installed, backup, staged, and replacement-backup paths and hashes. Each replacement is preceded by a durable `replace_intent`, followed by `installed` only after file-data and namespace barriers pass. The final full-set hashes are checked while current-inode locks are held. On macOS, file data, journals, and installed files use `F_FULLFSYNC`, followed by directory `fsync`. On Windows, the namespace barrier is `FlushFileBuffers` on the local volume and normally requires an elevated process; ordinary file flush and `ReplaceFileW` alone are not treated as a namespace durability guarantee.

If the platform cannot provide the required namespace barrier before the first destination replacement, apply stops with `logical_state: prepared` and `durability: durability_unverified`; no task sidecar is changed, and the lock, backups, and journal remain for recovery. A barrier failure after any replacement records `durability: repair_required`, retains all evidence, and never reports a verified `committed` or `rolled_back` outcome even if hashes were restored. Do not bypass this state by deleting the lock. Keep all writers stopped and recover from the recorded journal.

## Validation

After the new parser is healthy, verify serialized state and the legacy-state absence:

```bash
codeterm task show task_pes3i4 --json
codeterm task show task_p6kh7f --json
codeterm task show task_sjhx4f --json
if rg -n '^status: parked([[:space:]]|$)|^park_reason:' "$@"; then
  echo "legacy task state remains after cutover" >&2
  exit 1
fi
rg -n '^status: paused([[:space:]]|$)|^pause_reason:' "$@"
```

Each CLI result must show `status: paused`, a nonblank pause reason, no active Tab assignment, and the conversion milestone. Re-running the converter against the same files must print three `already converted` lines and make no changes.

## Rollback and recovery

If a source changes after staging, the converter preserves that concurrent edit, restores any sources it already replaced, and returns an error. A replace or sync failure also restores every source already replaced by the transaction. Rollback is hash-guarded: an original hash needs no action, an installed hash may be restored, and any third hash is preserved as a conflict. A missing destination is restored only when destination, replacement, and replacement-backup hashes form a complete known layout; an incomplete layout is preserved for manual repair. Inspect `logical_state` and `durability` together: `rolled_back` plus `verified` is the only verified rollback, while `rollback_failed`, `durability_unverified`, or `repair_required` must remain stopped and locked for recovery.

The migration lock is removed only for `committed` or fully `rolled_back` outcomes. If the process is interrupted or rollback fails, keep CodeTerm and all external writers stopped. Do not delete or edit the lock, journal, staged files, or backups. Run:

```bash
python scripts/convert-legacy-parked-tasks.py \
  --recover /absolute/path/from-lock-record/journal.json
```

Recovery acquires the existing OS lock and refuses unless the recorded host matches and the PID plus process-start token prove the prior owner is stale. An owner on another host, a live matching process, or an identity that cannot be proven stale is a hard stop. Recovery validates transaction paths, schema, and every retained hash; it never resumes forward installation. For `prepared`, `applying`, or `rollback_failed`, it classifies all destination/replacement/backup layouts and attempts only guarded rollback. A `committed` layout is confirmed only when all installed hashes match and durability was already verified. Unverified committed durability, a third hash, an incomplete missing-destination layout, or a failed recovery barrier remains `repair_required` and locked.

After successful recovery, require either `conversion recovery rolled_back` with all three original hashes or `conversion recovery committed` with all three installed hashes. Preserve the updated journal as evidence. Only a successful terminal recovery removes the global lock; never remove an unprovable or conflicted lock by hand.

For manual recovery, or if validation fails after the new parser starts, restore all three files together from the independent preflight backup:

```bash
cp -p "$backup_dir/"*.yaml .codeterm/plans/active/pl_as7ty5/tasks/
```

Then rerun the old-parser inventory. If the new parser fails after startup, quit it normally, perform the same three-file restore, then start the old build. Never run the old parser against converted sidecars or the new parser against restored legacy sidecars.

Keep the backup and captured apply output until the new parser has loaded the plan and all validation commands pass. A partial restore is invalid; the three records are one cutover set.

<!-- revision: 2 -->
