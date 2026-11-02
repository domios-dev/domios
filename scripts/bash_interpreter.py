from functools import cache
import os
from pathlib import Path
import subprocess


@cache
def bash_interpreter() -> str:
    if os.name != "nt":
        return "bash"
    return subprocess.check_output(
        ["node", str(Path(__file__).with_name("bash-interpreter.mjs"))],
        text=True,
    ).strip()
