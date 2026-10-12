"""Capture the DEV listener's process identity without touching its lifecycle."""

import json
import os
import platform
import subprocess

if os.environ.get("CODETERM_API_PORT") != "7686":
    raise RuntimeError("Process proof requires dev_run targeting DEV port 7686")

if platform.system() == "Windows":
    command = (
        "$ids = @(Get-NetTCPConnection -LocalPort 7686 -State Listen | "
        "Select-Object -ExpandProperty OwningProcess -Unique); "
        "if ($ids.Count -ne 1) { throw 'Expected one DEV listener' }; "
        "$p = Get-Process -Id $ids[0]; "
        "@{pid=$p.Id; startedAt=$p.StartTime.ToUniversalTime().ToString('o')} "
        "| ConvertTo-Json -Compress"
    )
    identity = json.loads(subprocess.check_output(
        ["powershell.exe", "-NoProfile", "-Command", command], text=True
    ))
else:
    pids = subprocess.check_output(
        ["lsof", "-t", "-iTCP:7686", "-sTCP:LISTEN"], text=True
    ).split()
    pids = sorted(set(pids))
    if len(pids) != 1:
        raise RuntimeError("Expected one DEV listener process")
    started = subprocess.check_output(
        ["ps", "-p", pids[0], "-o", "lstart="], text=True
    ).strip()
    if not started:
        raise RuntimeError("DEV listener exited during identity capture")
    identity = {"pid": int(pids[0]), "startedAt": started}
print(json.dumps(identity, sort_keys=True))
