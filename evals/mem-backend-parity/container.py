import json
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
        record["Name"] != "/" + NAME
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


if __name__ == "__main__":
    verify_container()
