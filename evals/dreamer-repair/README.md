# dreamer-repair eval

End-to-end smoke test for the `codeterm mem graph` repair CLI suite
introduced for issue #112. Verifies that the typed primitives
(`find-duplicate-relations`, `delete-relations`, `merge-relations`,
`find-duplicate-entities`, `augment-entity`, plus the carryover-extended
`alias`) work against a running dev daemon, produce the expected state
changes, and write audit entries to `graph_purge_log`.

## What this eval is NOT

This is a smoke test for the **API**, not a behavioural eval for the
**Dreamer prompt**. `cleanup.sh` is a deterministic stand-in for the
Dreamer — it makes the same decisions the prompt would make on this
seed, but without an LLM in the loop. Pass means the API works.
The behavioural aspect (does Dreamer pick the right action?) belongs
in `evals/dreaming/`.

## Run

```bash
# Dev daemon must be running (port 7686) with the latest CLI built:
./build.sh dev --wait
./build.sh cli

bash evals/dreamer-repair/run.sh
```

Final line is `PASS — all assertions hold` on success.

## Seed scenario

The seed creates four entities and five relations under the `seed_`
prefix:

- `seed_person_owner` (name="SeedAndrey") and `seed_person_ownery`
  (name="seedandrey ") — two Person entities that normalise to the same
  exact-name. Used to exercise `find-duplicate-entities` + `alias`.
- `seed_person_anna`, `seed_proj_test` — endpoints for the relations.
- 3× `(andrey, knows, anna)` at distinct `valid_from` — identical
  otherwise. Exercises `find-duplicate-relations` + `delete-relations`.
- 2× `(andrey, works_on, test)` with confidence 0.7 / 0.9 and distinct
  properties. Exercises `merge-relations` (max-conf + property union).

## What it verifies (assertions in `assert.sh`)

1. `find-duplicate-relations` returns zero seed-related groups after cleanup.
2. The surviving `works_on` row has confidence 0.9 (max).
3. Exactly one `works_on` row remains.
4. Exactly one `knows` row remains.
5. At most one `seed_person_owner*` entity remains (alias removed alias entity).
6. `entity_aliases` has a row referencing the seeds (alias bookkeeping intact).
7. The canonical entity has `city="Riga"` and `role="Senior iOS dev"`
   patched in via `augment-entity`.
8. `graph_purge_log` has ≥2 rows with `actor='dreamer-eval'`
   (`delete-relations` + `merge-relations` + `augment-entity`).

## Idempotent re-runs

`seed.sh` deletes all `seed_*` rows + `actor='dreamer-eval'` audit
entries before re-inserting, so the eval can be looped without manual
cleanup. To wipe by hand:

```bash
sqlite3 ~/.codeterm-dev/mem/knowledge.db "
    DELETE FROM relations       WHERE from_id LIKE 'seed_%' OR to_id LIKE 'seed_%';
    DELETE FROM entities        WHERE id LIKE 'seed_%';
    DELETE FROM entity_aliases  WHERE alias_id LIKE 'seed_%' OR canonical_id LIKE 'seed_%';
    DELETE FROM graph_purge_log WHERE actor='dreamer-eval';
"
```
