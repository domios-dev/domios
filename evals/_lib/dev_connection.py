import json
import os
import subprocess
from pathlib import Path


def dev_environment() -> dict[str, str]:
    lib = Path(__file__).with_name("lib.sh")
    git_bash = Path("C:/Program Files/Git/bin/bash.exe")
    bash = str(git_bash) if os.name == "nt" and git_bash.exists() else "bash"
    env = dict(os.environ)
    env["CODETERM_INSTANCE"] = env.get("CODETERM_DEV_INSTANCE", "")
    env["CODETERM_API_PORT"] = env.get("CODETERM_DEV_API_PORT", "7686")
    command = '''source "$1"; dev_run python3 -c 'import json,os; print(json.dumps({k:v for k,v in os.environ.items() if k.startswith("CODETERM_")}))' '''
    result = subprocess.run(
        [bash, "-c", command, "dev-connection", str(lib)],
        env=env, capture_output=True, text=True, check=True,
    )
    connection = json.loads(result.stdout)
    if not connection.get("CODETERM_API_TOKEN", "").strip():
        raise ValueError("Missing DEV credential")
    return {**{k: v for k, v in env.items() if not k.startswith("CODETERM_")}, **connection}


def dev_token(connection: dict[str, str] | None = None) -> str:
    return (dev_environment() if connection is None else connection)["CODETERM_API_TOKEN"]


def dev_api_url(connection: dict[str, str] | None = None) -> str:
    return (dev_environment() if connection is None else connection)["CODETERM_API_URL"]
