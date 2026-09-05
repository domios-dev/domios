#!/usr/bin/env python3
"""Correlate each submit transaction in a CodeTerm log with its burst outcome.

Read-only. Groups submit-audit lines per pty into transactions and reports
payload size against submit-key count, recovery verdict and the turn transition
that followed, so the burst window can be compared with ingestion time.

Usage: analyze-submit-bursts.py <codeterm.log> [more logs...]
"""
import re
import sys
from datetime import datetime

TS = r"\[(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})Z"
PASTE = re.compile(
    TS + r".*\[submit-audit\] (initial-submit|paste-encoding) pty=(\S+) "
    r"live_bracketed_paste_mode=(\S+) paste_capability=(\S+) "
    r"source_bytes=(\d+) encoded_bytes=(\d+)"
)
KEY = re.compile(TS + r".*\[submit-audit\] key-write pty=(\S+).*queue_result=(\S+)")
REC = re.compile(
    TS + r".*\[submit-recovery\] pty=(\S+) recovered=(\S+) bytes_written=(\d+) "
    r"key_writes=(\d+) elapsed_ms=(\d+)"
)
STATUS = re.compile(TS + r".*\[status\] (\S+) (\S+) -> (\S+)")
IDLE = re.compile(TS + r".*\[idle-poll\] (\S+) (\S+)")


def parse(paths):
    txns, open_txn, events = [], {}, []
    for path in paths:
        with open(path, "r", encoding="utf-8", errors="replace") as fh:
            for line in fh:
                m = PASTE.search(line)
                if m:
                    ts, kind, pty, live, cap, src, enc = m.groups()
                    txn = dict(ts=ts, kind=kind, pty=pty, live=live, cap=cap,
                               src=int(src), enc=int(enc), keys=[],
                               recovered=None, key_writes=None, elapsed=None)
                    # an initial-submit already carries submit-key frame 1
                    if kind == "initial-submit":
                        txn["keys"].append((ts, "ok(framed)"))
                    open_txn[pty] = txn
                    txns.append(txn)
                    continue
                m = KEY.search(line)
                if m:
                    ts, pty, result = m.groups()
                    if pty in open_txn:
                        open_txn[pty]["keys"].append((ts, result))
                    continue
                m = REC.search(line)
                if m:
                    ts, pty, rec, written, kw, el = m.groups()
                    if pty in open_txn:
                        t = open_txn[pty]
                        t["recovered"] = rec
                        t["key_writes"] = int(kw)
                        t["elapsed"] = int(el)
                    continue
                m = STATUS.search(line)
                if m:
                    events.append((m.group(1), m.group(2), "status",
                                   f"{m.group(3)} -> {m.group(4)}"))
                    continue
                m = IDLE.search(line)
                if m:
                    events.append((m.group(1), m.group(2), "idle-poll", m.group(3)))
    return txns, events


def secs(a, b):
    fmt = "%Y-%m-%dT%H:%M:%S"
    return (datetime.strptime(b, fmt) - datetime.strptime(a, fmt)).total_seconds()


def main():
    txns, events = parse(sys.argv[1:])
    print(f"{'time':>8}  {'pty':>8} {'kind':>14} {'bytes':>7} {'keys':>4} "
          f"{'recov':>6} {'ms':>5}  next Working transition")
    for t in txns:
        pty8 = t["pty"][:8]
        follow = ""
        for ts, p, kind, detail in events:
            if p[:8] != pty8 or kind != "status":
                continue
            d = secs(t["ts"], ts)
            if 0 <= d <= 30 and "-> Working" in detail:
                follow = f"+{d:.0f}s {detail}"
                break
        keys = t["key_writes"] if t["key_writes"] is not None else len(t["keys"])
        rec = t["recovered"] if t["recovered"] is not None else "n/a"
        print(f"{t['ts'][11:]:>8}  {pty8:>8} {t['kind']:>14} {t['src']:>7} "
              f"{keys:>4} {rec:>6} {str(t['elapsed'] or ''):>5}  {follow}")

    print("\n--- size bucket vs burst outcome ---")
    buckets = [(0, 1000), (1000, 5000), (5000, 15000), (15000, 30000), (30000, 10**9)]
    for lo, hi in buckets:
        sel = [t for t in txns if lo <= t["src"] < hi]
        if not sel:
            continue
        burst = [t for t in sel if (t["key_writes"] or len(t["keys"])) > 1]
        norec = [t for t in burst if t["recovered"] == "false"]
        print(f"{lo:>6}-{hi if hi < 10**8 else '+':>7} n={len(sel):>3} "
              f"burst(>1 key)={len(burst):>3} recovered=false={len(norec):>3}")


if __name__ == "__main__":
    main()
