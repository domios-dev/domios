# Memory seed snapshots

This fixture tool builds clean and populated development-memory snapshots from
the checked-in `loaded` personas, then restores one into the dev instance for a
repeatable memory scenario.

## Run

These commands replace the dev memory state. Run them only in an isolated dev
instance, never against production:

```bash
bash evals/seeds/build.sh
bash evals/seeds/restore.sh loaded
```

## Pass

The build ends with `[build] done`. Restore reports `[restore] dev ready` and
then `[restore] restored loaded:` with item, entity, and relation counts.

<!-- revision: 1 -->
