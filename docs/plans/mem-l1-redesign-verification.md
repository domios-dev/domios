# L1 Redesign — End-to-End Verification

> **Status:** PASSED on develop @ `c4281f6` (build 309). Verified 2026-05-10.

This doc captures the smoke-test sequence that confirmed every Task 1–13 deliverable works against a live dev daemon. Re-run when shipping a new build to catch regressions.

## Prereqs

- Dev daemon up: `./build.sh dev`, verify `curl http://127.0.0.1:7686/api/health` returns `{"build":"<HEAD-sha>",...}`
- Token: `cat ~/.codeterm-dev/config.toml | grep token`. Pass as `Authorization: Bearer <token>` on every API call.

## What's covered

| Task | Surface verified | How |
|------|------------------|-----|
| 1 — `is_pinned_wakeup` migration | column queryable | `mem pinned` returns rows |
| 2 — `bump_on_get` extension + worker skip | evidence_count moves on user reads, frozen on worker reads | wake-up `--explain` numbers |
| 3 — `UpdateBody.pinned` | `mem pin`/`mem unpin` toggles `is_pinned_wakeup` | `mem wake-up` rendering with/without `★` |
| 4 — `GET /api/mem/pinned` | returns pinned items | `dc mem pinned --limit N` |
| 5 — two-pass scoring | pinned first, ranked fill, ws_match, recency | `--explain` shows correct score = imp + log10(1+ev)·0.5 + ws·0.5 + recency·0.3 |
| 6 — `memory.l1_budget_tokens` config | default 800 | wake-up tokens ≤ 797 |
| 7 — CLI `mem pin/unpin/pinned` | end-to-end |  |
| 8 — `mem save --pin --workspace` | atomic single call | `mem pinned` shows new item with `scope=workspace:W1` |
| 9 — `mem wake-up --explain` | per-item breakdown rendered | greps `[L1 score]` lines |
| 10 — eval script | `evals/general-agent/cases/wakeup_loads_pinned_in_workspace.sh` |  |
| 11 — `docs/MEMORY.md` | revision 4, new sections | manual read |
| 12 — dreamer schedule | `0 6 * * *` ±30min | `dc cron list` |
| 13b — `/api/mem/calibration/sweep` | server-side sweep with all caps |  |
| 13c — CLI `mem calibration log/sweep` | dispatches to server |  |
| 13d — dreamer STAGE D | conditional Sunday block in prompt | manual `grep "STAGE D" src-tauri/prompts/dreaming_rem.md` |

## Smoke-test sequence

```bash
TOKEN=$(grep '^token' ~/.codeterm-dev/config.toml | cut -d'"' -f2)
BASE="http://127.0.0.1:7686"

# Pin / workspace / atomic save
codeterm mem save --kind fact --workspace W1 --pin --title "smoke-test-pin" "body" >/dev/null
codeterm mem pinned --limit 5

# Workspace scoping (positive + negative)
codeterm mem wake-up --workspace W1 --no-l0 | grep "smoke-test-pin"   # PASS expected
codeterm mem wake-up --workspace W2 --no-l0 | grep "smoke-test-pin"   # NO MATCH expected

# Pin/unpin toggle
codeterm mem unpin fact_smoke_test_pin
codeterm mem wake-up --workspace W1 --no-l0 | grep "★ smoke-test-pin" # NO MATCH (unpinned, no star)
codeterm mem pin fact_smoke_test_pin
codeterm mem wake-up --workspace W1 --no-l0 | grep "★ smoke-test-pin" # PASS (pinned, star)

# Score breakdown
codeterm mem wake-up --workspace W1 --explain | grep "[L1 score]"      # PASS

# Evidence count: user reads bump, worker reads skip
curl -s "$BASE/api/mem/items/fact_smoke_test_pin" -H "Authorization: Bearer $TOKEN" >/dev/null  # bump
curl -s "$BASE/api/mem/items/fact_smoke_test_pin" -H "Authorization: Bearer $TOKEN" -H "X-CodeTerm-Worker-Id: t" >/dev/null  # NO bump

# Calibration
curl -s -X POST "$BASE/api/mem/calibration/sweep?force=true" -H "Authorization: Bearer $TOKEN"
# Expect: {"applied":N, "archived":N, "day_of_week":"0..6", "examined":N}
codeterm mem calibration log --count
# Expect: {count: N}

# Re-entry guard (run sweep twice within 6h)
curl -s -X POST "$BASE/api/mem/calibration/sweep?force=true" -H "Authorization: Bearer $TOKEN"
# Expect: skipped_reason="recent_run" (if first sweep wrote any history rows)

# Cleanup
codeterm mem delete fact_smoke_test_pin
```

## Gotchas surfaced

1. **`MemItem` API response does NOT expose `evidence_count`.** It's read internally for scoring but not serialized. Verify via `--explain` instead.
2. **`mem.db` shows 0B at startup** — false alarm. WAL mode delays main-file flush. Real data lives in WAL until checkpoint.
3. **`evals/_lib/lib.sh` requires `set +u` or it errors on bare `BASH_SOURCE[0]`** under zsh. Smoke tests use direct curl + `dc` only when running under bash.
4. **`dev_post` helper relies on curl in PATH** — interactive shell may not have it (zsh path quirk). Use `curl` directly.

## When to rerun

- Before merging any feature touching `src-tauri/src/mem/` or `src-tauri/src/api/mem_routes/`
- After any schema migration in `crates/mem-store/src/migrations.rs`
- Before a release build

## What's NOT covered here

- Real Sunday calibration end-to-end (waits for actual Sunday dreamer run; mock with `--force`)
- Eviction warning behavior under realistic 30+ pinned-item load (would need fixture seeding)
- L0/L1 dedup heuristic (deferred to Task 13d's prompt-side L0 overlap detection — not implemented yet)
- Memory Panel UI rendering of `is_pinned_wakeup` (frontend untouched in this redesign)

<!-- revision: 1 -->
