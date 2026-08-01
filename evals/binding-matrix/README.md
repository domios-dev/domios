# binding-matrix — session-binding stress matrix

Stress-tests provider-reported tab→session binding under churn and asserts the
core invariant never slips: **every live agent tab is bound to its own unique
provider session id**, even with many tabs in the same cwd, `/new` resets, and
close+respawn cycles.

## Run

```bash
./build.sh dev --wait                 # dev daemon must be up (port 7686)
bash evals/binding-matrix/run.sh
```

Tunables (env):

| var | default | meaning |
|-----|---------|---------|
| `BM_PROVIDERS` | `claude` | space/comma list: `claude codex opencode` |
| `BM_WS` | `3` | number of workspaces (each is one same-cwd cluster) |
| `BM_PER_WS` | `4` | agents per workspace |
| `BM_BIND_TIMEOUT` | `75` | secs to wait for a fleet to bind |
| `BM_KEEP` | `0` | `1` keeps test workspaces instead of cleaning up |

```bash
# mass: 4 workspaces x 6 agents = 24, mixed providers
BM_PROVIDERS="claude codex" BM_WS=4 BM_PER_WS=6 BM_BIND_TIMEOUT=150 bash evals/binding-matrix/run.sh
```

## Scenarios

Each is followed by a full-fleet invariant check.

- **A+B — mass spawn / same-cwd clusters.** `BM_WS` workspaces, each one cwd,
  `BM_PER_WS` agents spawned back-to-back into it. This is the hard case the old
  filesystem detector failed: N agents sharing a cwd must each bind to a distinct
  session, not collide on "newest for cwd".
- **C — `/new` churn.** Half the claude tabs get `/clear`; the fleet must stay
  fully bound and unique afterwards. (Note: claude reuses its session id across
  `/clear`, so the sid usually does not change — the assertion is *no slip*, not
  *sid changed*.)
- **D — close + respawn.** A third of the tabs are closed and replaced in the
  same cwd; the closed sessions must release and the replacements bind fresh,
  with no stale/duplicate bindings left behind.

## Oracle

Per check (`check_invariants`):

- **bound** — every tracked, still-live agent tab has a non-null `agent_session_id`.
- **unique** — no two live tabs share an `agent_session_id` (binding is injective).

Plus a run-wide guard: **zero `binding nonce mismatch`** in the dev log — i.e. no
legitimate report was wrongly rejected by the nonce check.

### Why bound+unique proves correctness (not just absence of collisions)

A tab could in principle be unique yet *swapped* (A holds B's session, B holds
A's). That cannot happen here by construction: each provider's session-start
report runs **inside that tab's own process**, carrying the tab's
`CODETERM_TAB_ID` (in the report URL) and its per-launch
`CODETERM_SESSION_BINDING_NONCE`. The daemon binds the reported sid to *that*
tab and rejects a nonce that doesn't match. So a report can only bind the tab
it came from — uniqueness across the fleet is then sufficient to prove every
tab owns exactly its own session.

## Output

Snapshots + `tracked.txt` (tab_id/provider/ws/cwd) land under
`target-check/evals/binding-matrix/<ts>/`. Exit 0 = all checks passed.

<!-- revision: 1 -->
