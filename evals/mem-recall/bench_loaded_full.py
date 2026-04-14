#!/usr/bin/env python3
"""Expanded bench: full 189-item loaded.py + 50+ queries across all projects."""

import json, os, sys, time, urllib.request, urllib.parse

API = os.environ.get("CODETERM_API_URL", "http://127.0.0.1:7686")
TOKEN = os.environ.get("CODETERM_API_TOKEN", "")

if not TOKEN:
    import subprocess
    r = subprocess.run(
        ["grep", "^token", os.path.expanduser("~/.codeterm-dev/config.toml")],
        capture_output=True, text=True
    )
    TOKEN = r.stdout.strip().split('"')[1] if '"' in r.stdout else ""

HEADERS = {"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"}


def api(method, path, body=None):
    data = json.dumps(body).encode() if body else None
    req = urllib.request.Request(f"{API}{path}", data=data, headers=HEADERS, method=method)
    resp = urllib.request.urlopen(req)
    return json.loads(resp.read())


def search(query, limit=10):
    q = urllib.parse.quote(query)
    return api("GET", f"/api/mem/search?q={q}&limit={limit}")


TESTS = [
    # AURORA (iOS audio app) - 37 items
    {"cat":"keyword",  "q":"AVAudioEngine background",               "expect":["avaudioengine","background","interruption"]},
    {"cat":"keyword",  "q":"StoreKit 2 transaction",                 "expect":["storekit","transaction","finish"]},
    {"cat":"keyword",  "q":"HealthKit sleep data",                   "expect":["healthkit","sleep","aurora"]},
    {"cat":"keyword",  "q":"SwiftUI List performance",               "expect":["swiftui","list","performance"]},
    {"cat":"semantic", "q":"app freezes during phone call",          "expect":["avaudioengine","background","interruption"]},
    {"cat":"semantic", "q":"purchase not completing",                "expect":["storekit","transaction","finish"]},
    {"cat":"semantic", "q":"which ios versions do we support",       "expect":["ios","16","deployment"]},
    {"cat":"person",   "q":"Alex mixer",                             "expect":["alex","mixer"]},
    {"cat":"person",   "q":"Morgan onboarding",                      "expect":["morgan","onboarding","design"]},
    {"cat":"person",   "q":"Sam reverb",                             "expect":["sam","reverb","preset"]},
    {"cat":"howto",    "q":"how to run Aurora tests",                "expect":["xcodebuild","test","simulator"]},
    {"cat":"howto",    "q":"Aurora TestFlight release",              "expect":["testflight","archive","release"]},
    {"cat":"rule",     "q":"audio thread rules",                     "expect":["realtime","lock","malloc","callback"]},

    # BEACON (Python/Django AI meeting) - 44 items
    {"cat":"keyword",  "q":"PgBouncer transaction pooling",          "expect":["pgbouncer","transaction","pooling"]},
    {"cat":"keyword",  "q":"LangGraph nodes",                        "expect":["langgraph","nodes","pipeline"]},
    {"cat":"keyword",  "q":"Deepgram Nova-2",                        "expect":["deepgram","nova","transcription"]},
    {"cat":"keyword",  "q":"Celery retry idempotent",                "expect":["celery","idempotent","retry"]},
    {"cat":"semantic", "q":"database issues",                        "expect":["pgbouncer","postgres","pooling"]},
    {"cat":"semantic", "q":"authentication bug",                     "expect":["jwt","auth0","kid"]},
    {"cat":"person",   "q":"Jordan Django upgrade",                  "expect":["jordan","django"]},
    {"cat":"person",   "q":"Taylor Whisper optimization",            "expect":["taylor","whisper","latency"]},
    {"cat":"howto",    "q":"Beacon local dev",                       "expect":["docker","compose","beacon"]},
    {"cat":"howto",    "q":"deploy Beacon to staging",               "expect":["staging","deploy","ecs","release"]},

    # COBALT (React enterprise platform) - 23 items
    {"cat":"keyword",  "q":"Cobalt React version",                   "expect":["cobalt","react","18"]},
    {"cat":"keyword",  "q":"TypeScript strict mode",                 "expect":["typescript","strict","any"]},
    {"cat":"person",   "q":"Riley work",                             "expect":["riley"]},
    {"cat":"semantic", "q":"form validation approach",               "expect":["zod","react","form","validation"]},

    # INFRA / DELTA (home lab / infra) - 32 items
    {"cat":"keyword",  "q":"Tailscale mesh",                         "expect":["tailscale","mesh","vpn"]},
    {"cat":"keyword",  "q":"Home Assistant automation",              "expect":["home","assistant","automation"]},
    {"cat":"person",   "q":"Kai home lab",                           "expect":["kai","lab","infra","home"]},
    {"cat":"semantic", "q":"secure private network",                 "expect":["tailscale","mesh","vpn"]},

    # AUTOMATION - 11 items
    {"cat":"keyword",  "q":"cron schedule",                          "expect":["cron","schedule"]},

    # ECHO (CLI tool, meta) - 21 items
    {"cat":"keyword",  "q":"terminal multiplexer",                   "expect":["codeterm","multiplexer","terminal"]},

    # PERSONAL - 15 items
    {"cat":"semantic", "q":"vacation plans",                         "expect":["trip","vacation","plan"]},

    # Cross-project
    {"cat":"cross",    "q":"which projects use Postgres",            "expect":["beacon","postgres"]},
    {"cat":"cross",    "q":"where do we use Redis",                  "expect":["redis","beacon","celery"]},
    {"cat":"cross",    "q":"which apps have iOS",                    "expect":["aurora","ios","swift"]},
    {"cat":"cross",    "q":"which apps use LiveKit",                 "expect":["livekit","beacon"]},

    # Gotchas / learnings
    {"cat":"learning", "q":"JWT kid header",                         "expect":["jwt","kid","auth0"]},
    {"cat":"learning", "q":"memory leak Combine",                    "expect":["combine","retain","memory"]},
    {"cat":"learning", "q":"Core Audio sample rate",                 "expect":["sample","rate","avaudioengine"]},

    # Near-duplicates (dedup targets)
    {"cat":"dedup",    "q":"Beacon connection pooler",               "expect":["pgbouncer","pooling","beacon"]},
    {"cat":"dedup",    "q":"Beacon database pooling",                "expect":["pgbouncer","pooling","beacon"]},

    # Metrics
    {"cat":"metrics",  "q":"Aurora monthly active users",            "expect":["monthly","active","12k"]},
    {"cat":"metrics",  "q":"Beacon Postgres disk size",              "expect":["postgres","500gb","disk"]},
    {"cat":"metrics",  "q":"crash rate ios",                         "expect":["crash","99.6","99.2"]},

    # Security/rules
    {"cat":"rule",     "q":"AWS credentials commit",                 "expect":["aws","key","commit","secret"]},
]


def check_hit(hits, expect_words):
    for h in hits[:3]:
        text = (h["item"]["title"] + " " + h["item"].get("body", "")).lower()
        matches = sum(1 for w in expect_words if w.lower() in text)
        if matches >= 2 or (len(expect_words) <= 2 and matches >= 1):
            return True
    return False


def check_hit_rank(hits, expect_words):
    for rank, h in enumerate(hits[:10], 1):
        text = (h["item"]["title"] + " " + h["item"].get("body", "")).lower()
        matches = sum(1 for w in expect_words if w.lower() in text)
        if matches >= 2 or (len(expect_words) <= 2 and matches >= 1):
            return rank
    return None


if "--skip-save" not in sys.argv:
    print("=== Phase 1: Saving 189 items ===\n")
    sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "seeds"))
    from loaded import ITEMS

    for i, item in enumerate(ITEMS):
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
            api("POST", "/api/mem/items", body)
            if (i + 1) % 25 == 0:
                print(f"  saved {i+1}/{len(ITEMS)}")
        except Exception as e:
            print(f"  FAILED #{i+1}: {e}")
        time.sleep(0.2)

    print(f"\n=== Waiting for extraction workers ===\n")
    prev_ents = 0
    stable = 0
    for _ in range(80):
        stats = api("GET", "/api/mem/stats")
        print(f"  items={stats['total_items']} entities={stats['total_entities']} "
              f"relations={stats['total_relations']} vectors={stats['total_vectors']}", end="\r")
        if stats["total_entities"] == prev_ents:
            stable += 1
            if stable >= 4:
                break
        else:
            stable = 0
        prev_ents = stats["total_entities"]
        time.sleep(20)

    stats = api("GET", "/api/mem/stats")
    print(f"\n  Final: items={stats['total_items']} entities={stats['total_entities']} "
          f"relations={stats['total_relations']} vectors={stats['total_vectors']}\n")


print(f"=== Phase 2: Recall benchmark ({len(TESTS)} queries) ===\n")

passed_at_1 = 0
passed_at_3 = 0
passed_at_5 = 0
mrr_sum = 0.0
cat_stats = {}
latencies = []

for i, tc in enumerate(TESTS):
    t0 = time.time()
    try:
        resp = search(tc["q"])
        hits = resp.get("hits", [])
    except Exception as e:
        hits = []
        print(f"  ERROR {tc['q']}: {e}")
    elapsed = (time.time() - t0) * 1000
    latencies.append(elapsed)

    rank = check_hit_rank(hits, tc["expect"])
    cat = tc["cat"]
    if cat not in cat_stats:
        cat_stats[cat] = {"pass1": 0, "pass3": 0, "total": 0}
    cat_stats[cat]["total"] += 1

    if rank:
        mrr_sum += 1.0 / rank
        if rank == 1:
            passed_at_1 += 1
            cat_stats[cat]["pass1"] += 1
        if rank <= 3:
            passed_at_3 += 1
            cat_stats[cat]["pass3"] += 1
        if rank <= 5:
            passed_at_5 += 1

    status = f"@{rank}" if rank else "MISS"
    top2 = [h["item"]["title"][:35] for h in hits[:2]]
    print(f"  [{status:5}] {tc['cat']:9} | {tc['q'][:38]:38} | {', '.join(top2)[:55]}")

n = len(TESTS)
print(f"\n{'='*80}")
print(f"RECALL@1: {passed_at_1}/{n} ({100*passed_at_1/n:.1f}%)")
print(f"RECALL@3: {passed_at_3}/{n} ({100*passed_at_3/n:.1f}%)")
print(f"RECALL@5: {passed_at_5}/{n} ({100*passed_at_5/n:.1f}%)")
print(f"MRR:      {mrr_sum/n:.3f}")
print(f"p50/p95:  {sorted(latencies)[len(latencies)//2]:.1f}ms / {sorted(latencies)[int(len(latencies)*0.95)]:.1f}ms")
print(f"\nBy category (pass@1 / pass@3 / total):")
for cat, r in sorted(cat_stats.items()):
    print(f"  {cat:10} {r['pass1']}/{r['pass3']}/{r['total']}")
