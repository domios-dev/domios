#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def load_json(path):
    return json.loads(path.read_text())


def normalize_screen(text):
    lines = []
    for index, line in enumerate(text.replace("\r\n", "\n").replace("\u00a0", " ").splitlines()):
        line = line.rstrip()
        if index == 0 and ("codeterm-spawn-" in line or line.endswith(".sh")):
            continue
        if line:
            lines.append(line)
    return "\n".join(lines).strip()


def count_cast_rows(path):
    counts = {"i": 0, "o": 0, "o+b64": 0, "r": 0, "bad": 0}
    last_t = None
    with path.open(errors="replace") as fh:
        for line in fh:
            try:
                row = json.loads(line)
            except Exception:
                counts["bad"] += 1
                continue
            if isinstance(row, list) and len(row) > 1:
                counts[row[1]] = counts.get(row[1], 0) + 1
                if isinstance(row[0], (int, float)):
                    last_t = row[0]
    return counts, last_t


def render_timeline(
    replay_dir,
    out_path,
    sample_ms,
    rows=None,
    cols=None,
    from_frame=None,
    to_frame=None,
    from_ms=None,
    to_ms=None,
):
    renderer = ROOT / "src-tauri" / "target" / "debug" / "examples" / "render_timeline"
    raw = replay_dir / "raw.cast"
    meta = replay_dir / "meta.json"
    phases = replay_dir / "phases.json"
    if not renderer.exists():
        subprocess.run(
            ["cargo", "build", "--example", "render_timeline"],
            cwd=ROOT / "src-tauri",
            check=True,
        )
    subprocess.run(
        [
            str(renderer),
            str(raw),
            str(out_path),
            "--sample-ms",
            str(sample_ms),
            "--meta",
            str(meta),
            "--phases",
            str(phases),
        ]
        + ([] if rows is None else ["--rows", str(rows)])
        + ([] if cols is None else ["--cols", str(cols)])
        + ([] if from_frame is None else ["--from-frame", str(from_frame)])
        + ([] if to_frame is None else ["--to-frame", str(to_frame)])
        + ([] if from_ms is None else ["--from-ms", str(from_ms)])
        + ([] if to_ms is None else ["--to-ms", str(to_ms)]),
        cwd=ROOT,
        check=True,
    )
    return out_path


def main():
    parser = argparse.ArgumentParser(description="Validate that a detection replay is a trustworthy artifact.")
    parser.add_argument("replay_dir", type=Path)
    parser.add_argument("--sample-ms", type=int, default=250)
    parser.add_argument("--fast-full", action="store_true", help="Use coarse 500ms sampling for a faster full-cast gate.")
    parser.add_argument("--render", action="store_true", help="Refresh timeline.json before validating.")
    parser.add_argument("--screen-rows", type=int)
    parser.add_argument("--screen-cols", type=int)
    parser.add_argument("--from-frame", type=int)
    parser.add_argument("--to-frame", type=int)
    parser.add_argument("--from-ms", type=int)
    parser.add_argument("--to-ms", type=int)
    parser.add_argument("--strict-screen", action="store_true")
    args = parser.parse_args()
    has_frame_window = args.from_frame is not None or args.to_frame is not None
    if has_frame_window and args.sample_ms == 250:
        args.sample_ms = 75
    if args.fast_full and args.sample_ms == 250:
        args.sample_ms = 500
    windowed = any(v is not None for v in (args.from_frame, args.to_frame, args.from_ms, args.to_ms))

    replay_dir = args.replay_dir
    failures = []
    required = ["raw.cast", "screen.txt", "meta.json", "phases.json", "capture.log.json", "scenario.json"]
    for name in required:
        if not (replay_dir / name).exists():
            failures.append(f"missing {name}")

    if failures:
        for failure in failures:
            print(f"BAD {failure}")
        return 1

    counts, last_t = count_cast_rows(replay_dir / "raw.cast")
    if counts["bad"]:
        failures.append(f"raw.cast has {counts['bad']} malformed rows")
    if counts["o+b64"]:
        failures.append(f"raw.cast has {counts['o+b64']} lossy o+b64 rows")
    if counts["i"] == 0:
        failures.append("raw.cast has no input anchors")
    if counts["o"] == 0:
        failures.append("raw.cast has no output rows")

    phases_doc = load_json(replay_dir / "phases.json")
    phases = phases_doc.get("phases", phases_doc if isinstance(phases_doc, list) else [])
    if not phases:
        failures.append("phases.json has no phases")
    if phases and phases[-1].get("to_ms", 0) <= phases[-1].get("from_ms", 0):
        failures.append("last phase has invalid time range")
    timed_out = [
        phase
        for phase in phases
        if isinstance(phase, dict) and (phase.get("pacing") or {}).get("timed_out")
    ]
    if timed_out:
        failures.append(f"{len(timed_out)} phase(s) timed out during capture")

    timeline = replay_dir / "timeline.json"
    transient_timeline_dir = None
    if windowed or args.fast_full:
        transient_timeline_dir = tempfile.TemporaryDirectory(prefix="ct-replay-main-")
        timeline = Path(transient_timeline_dir.name) / "timeline.json"
        timeline = render_timeline(
            replay_dir,
            timeline,
            args.sample_ms,
            from_frame=args.from_frame,
            to_frame=args.to_frame,
            from_ms=args.from_ms,
            to_ms=args.to_ms,
        )
    elif args.render or not timeline.exists():
        timeline = render_timeline(
            replay_dir,
            timeline,
            args.sample_ms,
            from_frame=args.from_frame,
            to_frame=args.to_frame,
            from_ms=args.from_ms,
            to_ms=args.to_ms,
        )
    if not timeline.exists():
        failures.append("timeline.json missing")
        frames = []
    else:
        timeline_doc = load_json(timeline)
        frames = timeline_doc.get("frames", [])
        if not frames:
            failures.append("timeline.json has no frames")

    with tempfile.TemporaryDirectory(prefix="ct-replay-doctor-") as td:
        doctor_timeline = Path(td) / "timeline.json"
        render_timeline(
            replay_dir,
            doctor_timeline,
            args.sample_ms,
            rows=args.screen_rows,
            cols=args.screen_cols,
            from_frame=args.from_frame,
            to_frame=args.to_frame,
            from_ms=args.from_ms,
            to_ms=args.to_ms,
        )
        doctor_doc = load_json(doctor_timeline)
        doctor_frames = doctor_doc.get("frames", [])

    if doctor_frames and not windowed:
        last_screen = "\n".join(doctor_frames[-1].get("screen", []))
        screen_txt = (replay_dir / "screen.txt").read_text(errors="replace")
        raw_screen = replay_dir / "raw.screen.txt"
        candidates = [screen_txt]
        if raw_screen.exists():
            candidates.append(raw_screen.read_text(errors="replace"))
        screen_mismatch = all(
            normalize_screen(last_screen) != normalize_screen(candidate) for candidate in candidates
        )
        if screen_mismatch:
            message = (
                "final replay frame does not match screen.txt/raw.screen.txt "
                f"at {args.screen_rows}x{args.screen_cols}"
            )
            if args.strict_screen:
                failures.append(message)
            else:
                print(f"WARN {message}")
    elif not doctor_frames:
        failures.append("doctor screen render has no frames")

    if frames:
        card_frames = sum(1 for f in frames if (f.get("detector") or {}).get("card"))
        status_frames = sum(1 for f in frames if (f.get("detector") or {}).get("status_line"))
        working_frames = sum(1 for f in frames if (f.get("detector") or {}).get("working"))
    else:
        card_frames = status_frames = working_frames = 0

    print(f"replay: {replay_dir}")
    print(
        f"cast: inputs={counts['i']} outputs={counts['o']} o+b64={counts['o+b64']} "
        f"resize={counts['r']} bad={counts['bad']} last_t={last_t}"
    )
    print(f"phases: {len(phases)}")
    print(
        "screen_compare:",
        f"rows={args.screen_rows if args.screen_rows is not None else 'cast'}",
        f"cols={args.screen_cols if args.screen_cols is not None else 'cast'}",
    )
    if windowed:
        print(
            "window:",
            f"from_frame={args.from_frame}",
            f"to_frame={args.to_frame}",
            f"from_ms={args.from_ms}",
            f"to_ms={args.to_ms}",
        )
    print(
        "timeline:",
        f"frames={len(frames)}",
        f"working={working_frames}",
        f"status_line={status_frames}",
        f"cards={card_frames}",
    )

    if failures:
        for failure in failures:
            print(f"BAD {failure}")
        if transient_timeline_dir:
            transient_timeline_dir.cleanup()
        return 1
    print("OK replay doctor passed")
    if transient_timeline_dir:
        transient_timeline_dir.cleanup()
    return 0


if __name__ == "__main__":
    sys.exit(main())
