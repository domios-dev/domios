# Repository guidance removed from shipped content

## Protocol version skew

The socket server selects recovery by build mode: debug builds prescribe
`./build.sh cli`; release builds prescribe `codeterm update install --auto`.
The CLI prints the daemon-composed typed error unchanged. Keep repository
rebuild instructions here and release update instructions in product help.

## Lane ancestry audit

Before final QA and the master PR, run
`scripts/merge-stack-audit.sh --target <candidate> <accepted-lane>...`.
`MERGED` is clean. `PATCH_LANDED_STALE` proves patch equivalence only;
that result and `UNLANDED` require an ancestry merge or explicit deferral.
This repository script must never be prescribed by the embedded LANES manual.

## Backup restore drills

`sessions backups import --file <file>` requires a disposable named
upgrade-restore DEV instance. Keep this drill out of the shared production CLI
primer; production agents can export sanitized backups without a test instance.

`POST /api/session/backups/import` replaces a disposable named `upgrade-restore-*`
DEV desk without PTYs; PROD refuses it. CLI: `codeterm sessions backups import --file /tmp/desk.json`.
Import returns `{imported_tabs, restart_required, boot_id}`; `/api/health` boot_id proves a subsequent restart.

