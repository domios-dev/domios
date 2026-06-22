import json, sys
T = sys.argv[1]
d = json.load(open(T))
frames = d["frames"]
segs = []
cur = None
for f in frames:
    st = (f.get("detector") or {}).get("state")
    t = f.get("t_ms")
    if cur is None or st != cur[0]:
        if cur is not None:
            segs.append((cur[0], cur[1], prev_t))
        cur = (st, t)
    prev_t = t
if cur is not None:
    segs.append((cur[0], cur[1], prev_t))

print(f"file: {T}")
print(f"frames: {len(frames)}  total_ms: {frames[-1]['t_ms']}")
from collections import Counter
c = Counter((f.get('detector') or {}).get('state') for f in frames)
print("state counts:", dict(c))
print(f"FINAL state: {segs[-1][0]} (held {segs[-1][2]-segs[-1][1]}ms)")
print(f"segments ({len(segs)}):")
for s,a,b in segs:
    dur = b-a
    flag = "  <-- BLIP(<400ms)" if dur < 400 else ""
    print(f"  {s:9} {a:>7}..{b:>7}ms  dur={dur:>6}ms{flag}")
# stuck-working test: any working segment AFTER the last question that runs to the end with no return to idle
last_q_end = max([b for s,a,b in segs if s=="question"], default=None)
print("last question ends at:", last_q_end)
