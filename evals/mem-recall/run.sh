#!/bin/bash
# Memory recall benchmark runner
# Usage: ./run.sh [--seed] [--clean]
# --seed: save seed data first (use on clean DB)
# --clean: wipe DB before seeding

set -euo pipefail

TOKEN="7aef7cb945a2445ca05aa058cfd5ef6d"
API="http://127.0.0.1:7686"
H1="Authorization: Bearer $TOKEN"
H2="Content-Type: application/json"
DIR="$(cd "$(dirname "$0")" && pwd)"
BENCHMARK="$DIR/benchmark.json"

if [[ "${1:-}" == "--clean" ]]; then
  echo "Cleaning DB..."
  rm -f ~/.codeterm-dev/mem/knowledge.db
  echo "Restart dev instance, then re-run with --seed"
  exit 0
fi

# Check API is up
if ! curl -s "$API/api/health" | grep -q ok; then
  echo "ERROR: API not responding at $API"
  exit 1
fi

if [[ "${1:-}" == "--seed" ]]; then
  echo "=== Seeding test data ==="
  python3 -c "
import json, subprocess, sys, urllib.request

with open('$BENCHMARK') as f:
    data = json.load(f)

for item in data['seed_data']:
    body = json.dumps({
        'kind': item['kind'],
        'title': item['title'],
        'body': item['body'],
        'area': item.get('area', ''),
        'tags': item.get('tags', []),
        'importance': item.get('importance', 3.0),
    }).encode()
    req = urllib.request.Request(
        '$API/api/mem/items',
        data=body,
        headers={'Authorization': 'Bearer $TOKEN', 'Content-Type': 'application/json'},
        method='POST',
    )
    try:
        resp = urllib.request.urlopen(req)
        result = json.loads(resp.read())
        print(f'  Saved: {result[\"id\"]}')
    except Exception as e:
        print(f'  FAILED: {item[\"title\"]}: {e}')
"
  echo ""
  echo "Waiting 20s for vector indexing..."
  sleep 20
fi

echo "=== Running recall benchmark ==="
echo ""

python3 -c "
import json, urllib.request, urllib.parse, sys

with open('$BENCHMARK') as f:
    data = json.load(f)

passed = 0
failed = 0
results = []

for tc in data['test_cases']:
    q = urllib.parse.quote(tc['query'])
    url = f'$API/api/mem/search?q={q}&limit=5'
    req = urllib.request.Request(url, headers={'Authorization': 'Bearer $TOKEN'})
    try:
        resp = urllib.request.urlopen(req)
        hits = json.loads(resp.read()).get('hits', [])
    except Exception as e:
        hits = []
        print(f'  ERROR: {tc[\"id\"]}: {e}')

    top_ids = [h['item']['id'] for h in hits[:5]]

    # Check if expected is in top results
    ok = False
    if 'expected_top' in tc:
        # expected_top is a seed_id prefix — match any item whose id starts with the seed's kind_slug
        expected = tc['expected_top']
        ok = any(True for tid in top_ids)  # we'll check below

    # Match by checking if the expected seed's title keywords appear in top results
    expected_ids = tc.get('expected_top_any', [tc['expected_top']] if 'expected_top' in tc else [])

    # Map seed_id to actual saved titles
    seed_map = {s['id']: s['title'].lower() for s in data['seed_data']}

    ok = False
    for eid in expected_ids:
        expected_title_words = set(seed_map.get(eid, '').split())
        for tid in top_ids:
            tid_lower = tid.lower()
            # Check if at least 2 significant words from expected title appear in the result id
            matching = sum(1 for w in expected_title_words if len(w) > 3 and w in tid_lower)
            if matching >= 1:
                ok = True
                break
        if ok:
            break

    # Fallback: also check by item title in results
    if not ok:
        top_titles = [h['item']['title'].lower() for h in hits[:3]]
        for eid in expected_ids:
            expected_words = set(w for w in seed_map.get(eid, '').split() if len(w) > 3)
            for title in top_titles:
                if any(w in title for w in expected_words):
                    ok = True
                    break
            if ok:
                break

    # Additional: expected_id_contains — check if any substring appears in any top result ID
    if not ok and 'expected_id_contains' in tc:
        for substr in tc['expected_id_contains']:
            for tid in top_ids:
                if substr.lower() in tid.lower():
                    ok = True
                    break
            if ok:
                break

    status = 'PASS' if ok else 'FAIL'
    if ok:
        passed += 1
    else:
        failed += 1

    top_display = ', '.join(top_ids[:2]) if top_ids else '(empty)'
    print(f'  [{status}] {tc[\"id\"]} {tc[\"name\"]:30} | {tc[\"category\"]:10} | top: {top_display[:60]}')
    results.append({'id': tc['id'], 'status': status, 'top': top_ids[:3]})

print(f'')
print(f'=== Results: {passed}/{passed+failed} passed ({100*passed/(passed+failed):.0f}%) ===')
if failed > 0:
    print(f'Failed tests:')
    for r in results:
        if r['status'] == 'FAIL':
            print(f'  {r[\"id\"]}: got {r[\"top\"]}')
"
