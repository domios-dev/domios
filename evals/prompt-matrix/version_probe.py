"""Own-tab tool invokes codex --version in that tab's actual environment."""

import argparse
import json
import os
from pathlib import Path
import sys

from transport import run_group


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--nonce", required=True)
    args = parser.parse_args()
    if not os.environ.get("CODETERM_TAB_ID"):
        raise RuntimeError("Version probe requires its actual provider tab environment")
    argv = ["codex", "--version"]
    code, out, err = run_group(argv, dict(os.environ), 10, cwd=Path.cwd())
    print(json.dumps({"nonce": args.nonce, "argv": argv, "exit_code": code, "stdout": out, "stderr": err}))
    return code


if __name__ == "__main__":
    sys.exit(main())
