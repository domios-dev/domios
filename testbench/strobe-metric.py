#!/usr/bin/env python3
"""Strobe metric — detect rapid working<->idle<->working oscillation.

A "strobe" is a state SEGMENT shorter than WORKING_GRACE_MS (1500ms) sandwiched
between two segments of the opposite state: e.g. working -> idle(short) -> working,
or idle -> working(short) -> idle. These are the flicker runs the Adversary flagged
for opencode idle-matrix — the classifier toggling because an ambiguous frame
(no status_line / no active footer, but a visible bottom cursor) flips the verdict.

Also exports the AMBIGUOUS frames (no footer, no status_line, cursor visible in the
bottom region) to a bundle so the structural cause is inspectable.

Usage:
  python3 testbench/strobe-metric.py <timeline.json> [--bundle DIR] [--json]
  python3 testbench/strobe-metric.py <dir>/ --glob 'opencode__idle*'   # many
"""
import argparse, glob, json, os, sys
from pathlib import Path

WORKING_GRACE_MS = 1500   # offline_state_classifier.rs
BOTTOM_REGION_ROWS = 8     # composer_cursor_ready bottom band


def det(f): return f.get("detector") or {}


def segments(frames):
    segs = []  # (state, f_start, f_end, t_start, t_end)
    for f in frames:
        s = det(f).get("state")
        if segs and segs[-1][0] == s:
            segs[-1][2] = f["frame_index"]; segs[-1][4] = f["t_ms"]
        else:
            segs.append([s, f["frame_index"], f["frame_index"], f["t_ms"], f["t_ms"]])
    return segs


def strobes(segs):
    out = []
    for i in range(1, len(segs) - 1):
        prev, cur, nxt = segs[i-1], segs[i], segs[i+1]
        dur = cur[4] - cur[3]
        if prev[0] == nxt[0] and cur[0] != prev[0] and dur < WORKING_GRACE_MS:
            out.append({"state": cur[0], "between": prev[0], "f_start": cur[1],
                        "f_end": cur[2], "t_start": cur[3], "t_end": cur[4], "dur_ms": dur})
    return out


def ambiguous_frames(frames, height):
    """No status_line and no active footer in the detector, but cursor visible in the
    bottom region — the structurally ambiguous frames that drive strobing."""
    out = []
    for f in frames:
        d = det(f)
        if d.get("status_line"):
            continue
        cur = f.get("cursor") or {}
        if not cur.get("visible"):
            continue
        row = cur.get("row", 0)
        if row + BOTTOM_REGION_ROWS >= (height or 0):
            ds = f.get("decset") or {}
            out.append({"frame_index": f["frame_index"], "t_ms": f["t_ms"],
                        "state": d.get("state"), "alt_screen": ds.get("alt_screen"),
                        "cursor_row": row, "height": height,
                        "byte_idle_ms": (f.get("quiescence") or {}).get("byte_idle_ms"),
                        "semantic_idle_ms": (f.get("quiescence") or {}).get("semantic_idle_ms")})
    return out


def analyze(path, bundle=None):
    d = json.load(open(path)); fr = d.get("frames", [])
    h = d.get("meta", {}).get("height")
    segs = segments(fr)
    st = strobes(segs)
    amb = ambiguous_frames(fr, h)
    name = Path(path).stem
    if bundle and (st or amb):
        os.makedirs(bundle, exist_ok=True)
        # dump up to 60 ambiguous-frame screens (bottom rows)
        sel = amb[:60]
        with open(os.path.join(bundle, f"{name[:50]}.ambiguous.txt"), "w") as o:
            o.write(f"# {name}\n# strobes={len(st)} ambiguous_frames={len(amb)} (showing {len(sel)})\n")
            byidx = {f["frame_index"]: f for f in fr}
            for a in sel:
                f = byidx[a["frame_index"]]
                o.write(f"\n=== f{a['frame_index']} t={a['t_ms']} state={a['state']} alt={a['alt_screen']} "
                        f"curRow={a['cursor_row']}/{a['height']} byteIdle={a['byte_idle_ms']} semIdle={a['semantic_idle_ms']} ===\n")
                for r in f["screen"][-8:]:
                    if r.strip(): o.write("  " + repr(r[:110]) + "\n")
    return {"name": name, "frames": len(fr), "strobes": st,
            "strobe_count": len(st), "ambiguous_count": len(amb)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--glob", default="*.json")
    ap.add_argument("--bundle")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    paths = [args.path] if os.path.isfile(args.path) else sorted(glob.glob(os.path.join(args.path, args.glob)))
    results = [analyze(p, args.bundle) for p in paths]

    if args.json:
        print(json.dumps(results, indent=2)); return
    for r in results:
        print(f"[{ 'STROBE' if r['strobe_count'] else 'clean ' }] {r['name'][:58]:58} "
              f"frames={r['frames']:>5} strobes={r['strobe_count']:>2} ambiguous_frames={r['ambiguous_count']:>3}")
        for s in r["strobes"]:
            print(f"        strobe: {s['state']} for {s['dur_ms']}ms (<{WORKING_GRACE_MS}) "
                  f"between {s['between']}  f{s['f_start']}-{s['f_end']} t{s['t_start']}-{s['t_end']}")
    total = sum(r["strobe_count"] for r in results)
    print(f"\nTOTAL strobes: {total}  ({'RED' if total else 'GREEN'})  threshold=WORKING_GRACE_MS={WORKING_GRACE_MS}ms")
    if args.bundle:
        print(f"ambiguous-frame screens -> {args.bundle}")


if __name__ == "__main__":
    main()
