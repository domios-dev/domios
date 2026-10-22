import os
import subprocess
from pathlib import Path


def dev_token() -> str:
    lib = Path(__file__).with_name("lib.sh")
    git_bash = Path("C:/Program Files/Git/bin/bash.exe")
    bash = str(git_bash) if os.name == "nt" and git_bash.exists() else "bash"
    env = dict(os.environ)
    env.pop("CODETERM_INSTANCE", None)
    env["CODETERM_API_PORT"] = "7686"
    result = subprocess.run(
        [bash, "-c", 'source "$1"; dev_token', "dev-connection", str(lib)],
        env=env, capture_output=True, text=True, check=True,
    )
    return result.stdout.strip()
