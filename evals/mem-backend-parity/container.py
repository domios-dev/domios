import json
import os
from pathlib import Path
import sys
import subprocess
import threading

NAME = "domios-mem-backend-parity"
IMAGE = "pgvector/pgvector:pg17"


def verify_container():
    inspected = subprocess.run(
        ["docker", "inspect", NAME], capture_output=True, text=True,
        check=True, timeout=10,
    )
    record, = json.loads(inspected.stdout)
    if (
        record["Id"] != os.environ["MEM_PARITY_CONTAINER_ID"]
        or record["Name"] != "/" + NAME
        or not record["State"]["Running"]
        or record["Config"]["Image"] != IMAGE
        or record["Config"]["Labels"].get("domios.bench") != "mem-backend-parity"
        or record["NetworkSettings"]["Ports"]["5432/tcp"]
        != [{"HostIp": "127.0.0.1", "HostPort": "55439"}]
        or "/var/lib/postgresql/data" not in record["HostConfig"]["Tmpfs"]
    ):
        raise RuntimeError("container does not match the disposable parity fixture")
    print(json.dumps({"container_id": record["Id"], "image_id": record["Image"]}), flush=True)
    logs = subprocess.Popen(
        ["docker", "logs", "--follow", NAME], stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, text=True,
    )
    deadline = threading.Timer(30, logs.terminate)
    deadline.start()
    listening = False
    ready = False
    try:
        for line in logs.stdout:
            listening |= 'listening on IPv4 address "0.0.0.0", port 5432' in line
            if listening and "database system is ready to accept connections" in line:
                ready = True
                break
    finally:
        deadline.cancel()
        logs.terminate()
        try:
            logs.wait(timeout=5)
        except subprocess.TimeoutExpired:
            logs.kill()
            logs.wait(timeout=5)
        logs.stdout.close()
    if not ready:
        raise RuntimeError("no final TCP-ready Postgres event before the 30-second deadline")


def check_artifacts(directory):
    password = os.environ.get("POSTGRES_PASSWORD")
    if not password:
        raise RuntimeError("artifact password check requires POSTGRES_PASSWORD")
    checked = 0
    for name in ("run.log", "report.json", "inputs.json"):
        path = Path(directory) / name
        if path.exists():
            checked += 1
            if password.encode() in path.read_bytes():
                raise RuntimeError("credential found in " + name)
    print(f"CREDENTIAL_CHECK: password absent from {checked} artifacts", flush=True)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--check-artifacts":
        check_artifacts(sys.argv[2])
    else:
        verify_container()
