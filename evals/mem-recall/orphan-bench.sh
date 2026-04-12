#!/usr/bin/env bash
set -euo pipefail

# Orphan connectivity bench — measures what % of mem items end up without
# any graph relations after extraction workers finish.
#
# Requires a running dev instance on port 7686.
# Wipes mem database, saves fixture items, waits for workers, reports.
#
# Usage:
#   ./orphan-bench.sh              # run bench
#   ./orphan-bench.sh --baseline   # save result as baseline

API="${CODETERM_API_URL:-http://127.0.0.1:7686}"
DB="${HOME}/.codeterm-dev/mem/knowledge.db"
BASELINE_FILE="$(dirname "$0")/baselines/orphan-baseline.json"

# Resolve auth token
TOKEN=$(grep 'token' ~/.codeterm-dev/config.toml 2>/dev/null | head -1 | sed 's/.*= *"\(.*\)"/\1/')
if [ -z "$TOKEN" ]; then
  echo "ERROR: no API token in ~/.codeterm-dev/config.toml"
  exit 1
fi

H="Authorization: Bearer $TOKEN"

save() {
  curl -s -X POST -H "$H" -H "Content-Type: application/json" "$API/api/mem/items" -d "$1" >/dev/null
}

echo "=== Orphan connectivity bench ==="
echo "API: $API"

# Step 1: Verify clean state (caller must wipe DB + restart dev before running)
echo "[1/5] Checking clean state..."
for i in $(seq 1 30); do
  h=$(curl -s "$API/api/health" 2>/dev/null)
  if [ -n "$h" ]; then break; fi
  sleep 1
done
items=$(curl -s -H "$H" "$API/api/mem/stats" 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin).get('total_items',0))" 2>/dev/null)
if [ "${items:-0}" -gt 5 ]; then
  echo "ERROR: DB has $items items. Wipe first:"
  echo "  rm -f ~/.codeterm-dev/mem/knowledge.db* ~/.codeterm-dev/mem/workers.json"
  echo "  rm -rf ~/.codeterm-dev/mem/workers/"
  echo "  ./build.sh kill && ./build.sh dev"
  exit 1
fi
echo "  DB clean ($items seed items)"

# Step 2: Save fixture items (diverse kinds, some with entities, some generic)
echo "[2/5] Saving 15 fixture items..."
save '{"kind":"fact","title":"postgres pooling config","body":"PgBouncer with transaction mode, max_client_conn=400. Deployed on Mac Mini alongside the Rekap backend. Tima configured it.","area":"rekap","scope":"global"}'
save '{"kind":"fact","title":"soundcalm audio pipeline","body":"Core Audio graph with 3 mixer layers. Anya designed the UX. Zheka built the backend API for preset sync.","area":"soundcalm","scope":"global"}'
save '{"kind":"fact","title":"mesh discovery protocol","body":"CodeTerm mesh uses mDNS for LAN and Tailscale for WAN. Ed25519 identity per node.","area":"codeterm","scope":"global"}'
save '{"kind":"rule","title":"never commit env files","body":"API keys go into env vars or secret managers. Use .env.example with placeholders.","scope":"global"}'
save '{"kind":"rule","title":"validate at boundaries","body":"All external input validated. Pydantic in Python, Zod in TypeScript, serde in Rust.","scope":"global"}'
save '{"kind":"rule","title":"one-way migrations","body":"Never write down migrations in production. Write a new forward migration to fix.","scope":"global"}'
save '{"kind":"preference","title":"dark theme","body":"Always use dark themes, Dracula preferred. Tab width 2 spaces. JetBrains Mono.","scope":"global"}'
save '{"kind":"preference","title":"postgresql over mysql","body":"For new projects pick PostgreSQL over MySQL. Prefer raw SQL over ORMs.","scope":"global"}'
save '{"kind":"note","title":"investor chat alex","body":"Alex from Gradient Ventures wants 500K MAU before Series A. Suggested B2B wellness angle. Anya preparing deck.","area":"binaura","scope":"global"}'
save '{"kind":"note","title":"weekend ideas","body":"Ideas: AI sleep coach, terminal code review, distributed build cache across mesh.","scope":"global"}'
save '{"kind":"learning","title":"swift actor reentrancy","body":"Actor reentrancy in Swift causes state mutations when methods await. Fixed with serial TaskQueue in SoundCalm.","area":"soundcalm","scope":"global"}'
save '{"kind":"learning","title":"whisper silence hallucination","body":"Whisper hallucinates on silent audio. Fixed with VAD pre-filter at -40dB RMS.","area":"rekap","scope":"global"}'
save '{"kind":"peer","title":"blackstar pc","body":"Desktop workstation with a large GPU. Windows 11. Media server, ML, gaming. Tailscale.","area":"home","scope":"global"}'
save '{"kind":"peer","title":"mac mini server","body":"Small always-on server. Always-on Docker: Home Assistant, Mosquitto, PostgreSQL, Grafana.","area":"home","scope":"global"}'
save '{"kind":"peer","title":"Home router","body":"Home router. Cloudflare DNS. WireGuard VPN.","area":"home","scope":"global"}'

# Step 3: Wait for all extraction workers
echo "[3/5] Waiting for extraction workers..."
for i in $(seq 1 240); do
  active=$(curl -s -H "$H" "$API/api/mem/workers" 2>/dev/null | python3 -c "
import json,sys
ws=json.load(sys.stdin)
print(len([w for w in ws if w['status'] not in ('done','error','orphaned')]))" 2>/dev/null)
  if [ "$active" = "0" ]; then
    echo "  Workers done after $((i*3))s"
    break
  fi
  [ $((i % 20)) -eq 0 ] && echo "  $active workers remaining... ($((i*3))s)"
  sleep 3
done

# Step 4: Count
echo "[4/5] Counting orphans..."
total=$(sqlite3 "$DB" "SELECT COUNT(*) FROM mem_items;")
orphans=$(sqlite3 "$DB" "SELECT COUNT(*) FROM mem_items WHERE id NOT IN (SELECT from_id FROM relations) AND id NOT IN (SELECT to_id FROM relations);")
connected=$((total - orphans))
pct=$((orphans * 100 / total))
entities=$(sqlite3 "$DB" "SELECT COUNT(DISTINCT id) FROM entities;")
relations=$(sqlite3 "$DB" "SELECT COUNT(*) FROM relations;")

workers_done=$(curl -s -H "$H" "$API/api/mem/workers" | python3 -c "import json,sys; ws=json.load(sys.stdin); print(len([w for w in ws if w['status']=='done']))")
workers_err=$(curl -s -H "$H" "$API/api/mem/workers" | python3 -c "import json,sys; ws=json.load(sys.stdin); print(len([w for w in ws if w['status'] in ('error','orphaned')]))")

# Step 5: Report
echo "[5/5] Results:"
echo "  Saved:      15 fixture items"
echo "  Total:      $total (incl. auto-extracted)"
echo "  Connected:  $connected"
echo "  Orphans:    $orphans ($pct%)"
echo "  Entities:   $entities"
echo "  Relations:  $relations"
echo "  Workers OK: $workers_done"
echo "  Workers ERR: $workers_err"

# Compare with baseline if exists
if [ -f "$BASELINE_FILE" ]; then
  prev_orphan_pct=$(python3 -c "import json; d=json.load(open('$BASELINE_FILE')); print(d['orphan_pct'])")
  echo ""
  if [ "$pct" -le "$prev_orphan_pct" ]; then
    echo "  PASS: orphan rate $pct% <= baseline $prev_orphan_pct%"
  else
    echo "  FAIL: orphan rate $pct% > baseline $prev_orphan_pct%"
    exit 1
  fi
fi

# Save baseline if requested
if [ "${1:-}" = "--baseline" ]; then
  mkdir -p "$(dirname "$BASELINE_FILE")"
  python3 -c "
import json
d = {
    'total': $total,
    'orphans': $orphans,
    'orphan_pct': $pct,
    'connected': $connected,
    'entities': $entities,
    'relations': $relations,
    'workers_done': $workers_done,
    'workers_err': $workers_err,
    'commit': '$(git rev-parse --short HEAD 2>/dev/null || echo unknown)',
}
json.dump(d, open('$BASELINE_FILE', 'w'), indent=2)
print(f'Baseline saved: {\"$BASELINE_FILE\"}')"
fi
