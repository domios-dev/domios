#!/usr/bin/env python3
import argparse
import json
from collections import Counter
from pathlib import Path


def frame_time(frame):
    return int(frame.get("t_ms") or frame.get("t") or frame.get("time_ms") or 0)


def detector(frame):
    return frame.get("detector") or frame.get("agent_state") or {}


def is_working(frame):
    state = detector(frame)
    return bool(state.get("working") or state.get("state") == "working")


def has_card(frame):
    state = detector(frame)
    return bool(state.get("card") or state.get("pending_prompt") or frame.get("card"))


def has_native_picker(frame):
    state = detector(frame)
    return bool(state.get("native_picker") or frame.get("native_picker"))


def status_line(frame):
    state = detector(frame)
    return state.get("status_line") or frame.get("status_line") or ""


def runs(frames, predicate):
    out = []
    start = None
    for i, frame in enumerate(frames):
        active = predicate(frame)
        if active and start is None:
            start = i
        if (not active or i == len(frames) - 1) and start is not None:
            end = i if active and i == len(frames) - 1 else i - 1
            out.append((start, end))
            start = None
    return out


def load_phases(doc):
    phases = doc.get("expected_phases") or doc.get("phases") or []
    if isinstance(phases, dict):
        phases = phases.get("phases", [])
    return phases


def main():
    parser = argparse.ArgumentParser(description="Summarize detection replay timeline quality.")
    parser.add_argument("timeline", type=Path)
    parser.add_argument("--short-idle-ms", type=int, default=600)
    args = parser.parse_args()

    doc = json.loads(args.timeline.read_text())
    frames = doc.get("frames") or []
    if not frames:
        raise SystemExit("timeline has no frames")

    states = Counter(detector(frame).get("state") for frame in frames)
    working_frames = sum(1 for frame in frames if is_working(frame))
    card_frames = sum(1 for frame in frames if has_card(frame))
    native_picker_frames = sum(1 for frame in frames if has_native_picker(frame))
    status_frames = sum(1 for frame in frames if status_line(frame))

    transitions = []
    previous = None
    for frame in frames:
        state = detector(frame).get("state")
        if state != previous:
            transitions.append((frame_time(frame), previous, state))
            previous = state

    idle_runs = runs(frames, lambda frame: not is_working(frame))
    short_idle = []
    for start, end in idle_runs:
        duration = frame_time(frames[end]) - frame_time(frames[start])
        if duration < args.short_idle_ms:
            short_idle.append((start, end, duration, frame_time(frames[start])))

    duration_ms = frame_time(frames[-1]) - frame_time(frames[0])
    print(f"timeline: {args.timeline}")
    print(
        "summary:",
        f"frames={len(frames)}",
        f"duration_ms={duration_ms}",
        f"inputs={len(doc.get('inputs') or [])}",
    )
    print(
        "detector:",
        f"states={dict(states)}",
        f"working_frames={working_frames}",
        f"status_line_frames={status_frames}",
        f"card_frames={card_frames}",
        f"native_picker_frames={native_picker_frames}",
        f"transitions={len(transitions)}",
        f"idle_runs={len(idle_runs)}",
        f"short_idle_blips={len(short_idle)}",
    )
    if short_idle:
        for start, end, duration, at_ms in short_idle[:20]:
            print(f"short_idle: t_ms={at_ms} frames={start}-{end} duration_ms={duration}")

    phases = load_phases(doc)
    if phases:
        print("per_turn:")
        for phase in phases:
            start = int(phase.get("from_ms") or 0)
            end = int(phase.get("to_ms") or start)
            scoped = [frame for frame in frames if start <= frame_time(frame) <= end]
            if not scoped:
                continue
            turn = phase.get("turn") or phase.get("id") or "?"
            turn_working = sum(1 for frame in scoped if is_working(frame))
            turn_status = sum(1 for frame in scoped if status_line(frame))
            turn_cards = sum(1 for frame in scoped if has_card(frame))
            turn_native = sum(1 for frame in scoped if has_native_picker(frame))
            timed_out = (phase.get("pacing") or {}).get("timed_out")
            print(
                f"  turn={turn}",
                f"frames={len(scoped)}",
                f"working_pct={turn_working * 100 / len(scoped):.1f}",
                f"status_frames={turn_status}",
                f"card_frames={turn_cards}",
                f"native_picker_frames={turn_native}",
                f"timed_out={timed_out}",
            )


if __name__ == "__main__":
    main()
