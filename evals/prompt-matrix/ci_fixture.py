"""WHAT: Expose a controllable running/success/failure CI result to a sentinel.
WHY: A watch must route each transition once and remain silent while unchanged.
ORACLE: Each invocation reads one atomic generation and returns its status.
"""

import argparse
import json
from pathlib import Path
import sys

from fixtures import write_json


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--set", choices=("running", "success", "failure"))
    args = parser.parse_args()
    if args.set:
        previous = json.loads(args.state.read_text()) if args.state.exists() else {"generation": 0}
        write_json(args.state, {"generation": previous["generation"] + 1, "status": args.set})
    state = json.loads(args.state.read_text())
    print(json.dumps(state, sort_keys=True))
    return {"running": 2, "success": 0, "failure": 1}[state["status"]]


if __name__ == "__main__":
    sys.exit(main())
