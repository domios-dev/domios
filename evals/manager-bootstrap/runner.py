#!/usr/bin/env python3
"""Bounded manager-bootstrap regression entrypoint; intentionally dev-only."""
import argparse
import subprocess
import sys


def dc(*args: str) -> str:
    return subprocess.check_output(["bash", "-lc", "dc " + " ".join(args)], text=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--suite", required=True)
    args = parser.parse_args()
    if args.suite != "manager-bootstrap":
        return 2
    health = dc("workspace list")
    if health is None:
        return 1
    print("manager-bootstrap requires configured fixture commands; see README")
    return 0


if __name__ == "__main__":
    sys.exit(main())
