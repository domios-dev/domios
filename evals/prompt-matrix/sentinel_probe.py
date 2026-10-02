"""WHAT: Produce stable git/CI observations and acknowledge each probe read.
WHY: An unchanged-probe barrier needs a process signal, never a settling delay.
ORACLE: A nonce-bound localhost frame identifies the exact fixture read.
"""

import argparse
import json
import socket
import subprocess
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--state", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--nonce", required=True)
    args = parser.parse_args()
    with open(args.state) as source:
        state = json.load(source)
    if state.get("repo"):
        completed = subprocess.run(
            ["git", "-C", state["repo"], "merge-base", "--is-ancestor",
             state["commit"], state["branch"]], capture_output=True, timeout=1,
        )
        if completed.returncode not in (0, 1):
            raise RuntimeError("Git probe could not establish ancestry")
        state = {"status": "landed" if completed.returncode == 0 else "waiting"}
    output = json.dumps(state, sort_keys=True)
    frame = json.dumps({"nonce": args.nonce, "id": uuid.uuid4().hex,
                        "state": state, "output": output}).encode() + b"\n"
    with socket.create_connection(("127.0.0.1", args.port), timeout=1) as connection:
        connection.sendall(frame)
    print(output)
    return {"running": 2, "success": 0, "failure": 1}.get(state["status"], 0)


if __name__ == "__main__":
    raise SystemExit(main())
