#!/usr/bin/env python3
"""
Realistic mem recall bench against loaded.py seed data (50 items).
Saves through full pipeline (with extraction workers), waits, then runs queries.
"""

import json, os, subprocess, sys, time
from pathlib import Path

EVAL_LIB = Path(__file__).resolve().parents[1] / "_lib" / "lib.sh"
RECORD_CACHE = {}


def api(method, path, body=None):
    result = subprocess.run(
        [
            "bash",
            "-c",
            'source "$1"; case "$2" in GET) dev_get "$3" ;; POST) dev_post "$3" "$4" ;; *) exit 2 ;; esac',
            "mem-recall",
            str(EVAL_LIB),
            method,
            path,
            json.dumps(body or {}),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    return json.loads(result.stdout)


def search(query, limit=10):
    return dc_json("mem", "search", query, "--limit", str(limit), "--json")


def dc_json(*args):
    result = subprocess.run(
        ["bash", "-c", 'source "$1"; shift; dc "$@"', "mem-recall", str(EVAL_LIB), *args],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    return json.loads(result.stdout)


def hydrate_hits(hits):
    hydrated = []
    for hit in hits:
        item_id = hit.get("id")
        if not isinstance(item_id, str) or hit.get("source") == "doc":
            hydrated.append({**hit, "record": {}})
            continue
        if item_id not in RECORD_CACHE:
            record = dc_json("mem", "get", item_id, "--json")
            if record.get("id") != item_id:
                raise RuntimeError(f"mem get returned a mismatched record for {item_id}")
            RECORD_CACHE[item_id] = record
        hydrated.append({**hit, "record": RECORD_CACHE[item_id]})
    return hydrated


# ── Test cases: query → what we expect in top 3 ───────────────────────

TESTS = [
    # keyword
    {"id": "K01", "cat": "keyword",  "q": "PgBouncer connection pooling",       "expect": ["pgbouncer", "pooling", "bouncer"]},
    {"id": "K02", "cat": "keyword",  "q": "Core Audio AVAudioEngine",           "expect": ["audio engine", "avaudioengine", "core audio"]},
    {"id": "K03", "cat": "keyword",  "q": "StoreKit 2 subscriptions",           "expect": ["storekit", "subscription"]},
    {"id": "K04", "cat": "keyword",  "q": "Deepgram transcription",             "expect": ["deepgram", "transcription", "whisper"]},
    {"id": "K05", "cat": "keyword",  "q": "LangGraph agent pipeline",           "expect": ["langgraph", "agent"]},

    # semantic (no exact keyword match)
    {"id": "S01", "cat": "semantic", "q": "database connection issues",          "expect": ["pgbouncer", "pooling", "postgres"]},
    {"id": "S02", "cat": "semantic", "q": "audio processing architecture",       "expect": ["audio engine", "core audio", "mixer"]},
    {"id": "S03", "cat": "semantic", "q": "how to handle payments in the app",   "expect": ["storekit", "subscription", "purchase"]},
    {"id": "S04", "cat": "semantic", "q": "speech recognition performance",      "expect": ["whisper", "transcription", "latency"]},
    {"id": "S05", "cat": "semantic", "q": "background task reliability",         "expect": ["celery", "idempotent", "retry"]},

    # person
    {"id": "P01", "cat": "person",   "q": "what does Alex work on",             "expect": ["alex", "mixer", "refactor"]},
    {"id": "P02", "cat": "person",   "q": "Morgan's design work",               "expect": ["morgan", "onboarding", "design"]},
    {"id": "P03", "cat": "person",   "q": "what did Taylor improve",            "expect": ["taylor", "whisper", "latency"]},
    {"id": "P04", "cat": "person",   "q": "Jordan's proposal",                  "expect": ["jordan", "django"]},
    {"id": "P05", "cat": "person",   "q": "Sam audio tuning",                   "expect": ["sam", "reverb", "preset"]},

    # gotcha / learning
    {"id": "L01", "cat": "learning", "q": "JWT authentication bug",             "expect": ["jwt", "kid", "401", "auth0"]},
    {"id": "L02", "cat": "learning", "q": "audio crashes in background",        "expect": ["background", "interruption", "audio"]},
    {"id": "L03", "cat": "learning", "q": "memory leak in Combine",             "expect": ["combine", "retain", "memory"]},
    {"id": "L04", "cat": "learning", "q": "SwiftUI performance problems",       "expect": ["swiftui", "list", "scroll", "performance"]},
    {"id": "L05", "cat": "learning", "q": "purchase not completing",            "expect": ["transaction", "finish", "storekit"]},

    # cross-project
    {"id": "X01", "cat": "cross",    "q": "which project uses LiveKit",         "expect": ["livekit", "beacon"]},
    {"id": "X02", "cat": "cross",    "q": "which apps use HealthKit",           "expect": ["healthkit", "aurora", "sleep"]},
    {"id": "X03", "cat": "cross",    "q": "where do we use Redis",              "expect": ["redis", "beacon", "celery"]},

    # skill / how-to
    {"id": "H01", "cat": "howto",    "q": "how to run tests locally",           "expect": ["xcodebuild", "test", "simulator"]},
    {"id": "H02", "cat": "howto",    "q": "how to deploy to staging",           "expect": ["staging", "deploy", "ecs"]},

    # rule
    {"id": "R01", "cat": "rule",     "q": "audio thread safety rules",          "expect": ["realtime", "lock", "malloc", "callback"]},
    {"id": "R02", "cat": "rule",     "q": "AWS credentials policy",             "expect": ["aws", "key", "commit", "secret"]},

    # metrics
    {"id": "M01", "cat": "metrics",  "q": "Aurora monthly active users",        "expect": ["12k", "monthly", "active", "growth"]},
    {"id": "M02", "cat": "metrics",  "q": "app crash rate",                     "expect": ["crash", "99.6", "99.2"]},

    # dedup detection (near-duplicates exist for PgBouncer)
    {"id": "D01", "cat": "dedup",    "q": "Beacon database connection pool config", "expect": ["pgbouncer", "400", "transaction"]},
]


def check_hit(hits, expect_words):
    """Check if any of the top-3 results contain expected keywords."""
    for h in hits[:3]:
        record = h["record"]
        text = (record.get("title", "") + " " + record.get("body", "")).lower()
        matches = sum(1 for w in expect_words if w.lower() in text)
        if matches >= 2 or (len(expect_words) <= 2 and matches >= 1):
            return True
    return False


# ── Phase 1: Save 50 items through full pipeline ──────────────────────

if "--skip-save" not in sys.argv:
    print("=== Phase 1: Saving 50 items (full pipeline, with extraction) ===\n")
    sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "seeds"))
    from loaded import ITEMS

    for i, item in enumerate(ITEMS[:50]):
        body = {
            "kind": item["kind"],
            "title": item["title"],
            "body": item["body"],
            "area": item.get("area", ""),
            "tags": item.get("tags", []),
            "importance": item.get("importance", 3.0),
            "skip_extract": False,
        }
        try:
            result = api("POST", "/api/mem/items", body)
            if (i + 1) % 10 == 0:
                print(f"  saved {i+1}/50")
        except Exception as e:
            print(f"  FAILED #{i+1}: {e}")
        time.sleep(0.3)

    print("\n=== Waiting for extraction workers to finish ===\n")
    prev_ents = 0
    stable = 0
    for _ in range(40):  # up to 10 min
        stats = api("GET", "/api/mem/stats")
        ents = stats["total_entities"]
        items = stats["total_items"]
        vecs = stats["total_vectors"]
        print(f"  items={items} entities={ents} vectors={vecs}", end="\r")
        if ents == prev_ents:
            stable += 1
            if stable >= 4:
                break
        else:
            stable = 0
        prev_ents = ents
        time.sleep(15)

    stats = api("GET", "/api/mem/stats")
    print(f"\n  Final: items={stats['total_items']} entities={stats['total_entities']} "
          f"relations={stats['total_relations']} vectors={stats['total_vectors']}\n")


# ── Phase 2: Run recall queries ───────────────────────────────────────

print("=== Phase 2: Recall benchmark ({} queries) ===\n".format(len(TESTS)))

passed = 0
failed = 0
results_by_cat = {}
latencies = []

for tc in TESTS:
    t0 = time.time()
    try:
        resp = search(tc["q"])
        elapsed = (time.time() - t0) * 1000
        hits = hydrate_hits(resp.get("hits", []))
    except Exception as e:
        hits = []
        elapsed = (time.time() - t0) * 1000
        print(f"  ERROR {tc['id']}: {e}")
    latencies.append(elapsed)

    ok = check_hit(hits, tc["expect"])
    status = "PASS" if ok else "FAIL"
    if ok:
        passed += 1
    else:
        failed += 1

    cat = tc["cat"]
    if cat not in results_by_cat:
        results_by_cat[cat] = {"pass": 0, "fail": 0}
    results_by_cat[cat]["pass" if ok else "fail"] += 1

    top2 = [h["record"].get("title", "")[:40] for h in hits[:2]]
    print(f"  [{status}] {tc['id']:4} {tc['cat']:9} | {tc['q'][:45]:45} | {', '.join(top2)[:60]}")

total = passed + failed
print(f"\n{'='*70}")
print(f"TOTAL: {passed}/{total} passed ({100*passed/total:.0f}%)")
print(f"p50 latency: {sorted(latencies)[len(latencies)//2]:.1f}ms")
print(f"p95 latency: {sorted(latencies)[int(len(latencies)*0.95)]:.1f}ms")
print(f"\nBy category:")
for cat, r in sorted(results_by_cat.items()):
    t = r["pass"] + r["fail"]
    print(f"  {cat:10} {r['pass']}/{t} ({100*r['pass']/t:.0f}%)")
