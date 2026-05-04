"""
Seed dataset for the "loaded" fixture — ~200 anonymized mem items.

All names/projects/people are fictional. Technologies are real (PgBouncer,
SwiftUI, Docker, etc.) because they're public and needed for realism.

Personas:
  project_aurora   — cross-platform audio wellness app (iOS-focused)
  project_beacon   — AI meeting assistant (Python backend, transcription)
  project_cobalt   — internal enterprise platform (React frontend)
  project_delta    — home-lab infra / mesh
  project_echo     — CLI tool being built (meta)

  person_alex      — aurora lead dev (iOS)
  person_morgan    — aurora designer
  person_sam       — aurora audio engineer
  person_jordan    — beacon backend
  person_taylor    — beacon ML/transcription
  person_riley     — cobalt fullstack
  person_kai       — delta infra

Areas: aurora, beacon, cobalt, infra, automation, echo, personal

The file exports ITEMS (list of dicts). Run `python load.py` to POST them.
Intentional near-duplicates and supersede pairs are marked with comments.
"""

ITEMS: list[dict] = []


def add(**kw):
    ITEMS.append(kw)

from . import aurora, beacon, cobalt, cross, delta, echo

ITEMS: list[dict] = (
    aurora.ITEMS
    + beacon.ITEMS
    + cobalt.ITEMS
    + delta.ITEMS
    + echo.ITEMS
    + cross.ITEMS
)


# ─────────────────────────────────────────────────────────────
# Counts
# ─────────────────────────────────────────────────────────────
if __name__ == "__main__":
    from collections import Counter
    kinds = Counter(item["kind"] for item in ITEMS)
    areas = Counter(item.get("area") or "(none)" for item in ITEMS)
    print(f"Total items: {len(ITEMS)}")
    print("\nBy kind:")
    for k, v in sorted(kinds.items(), key=lambda x: -x[1]):
        print(f"  {k:14} {v}")
    print("\nBy area:")
    for a, v in sorted(areas.items(), key=lambda x: -x[1]):
        print(f"  {a:14} {v}")
