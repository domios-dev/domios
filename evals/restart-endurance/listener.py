import ctypes
import json
import os
import platform
import subprocess
import sys


def output(command):
    return subprocess.check_output(command, text=True, timeout=30).strip()


def unix_listener(port):
    pids = set(output(["lsof", "-nP", f"-iTCP:{port}", "-sTCP:LISTEN", "-Fp"]).splitlines())
    pids = {int(line[1:]) for line in pids if line.startswith("p")}
    if len(pids) != 1:
        raise RuntimeError(f"Expected one listener on {port}, found {sorted(pids)}")
    return pids.pop()


def mac_start(pid):
    class BsdInfo(ctypes.Structure):
        _fields_ = [(name, ctypes.c_uint32) for name in (
            "flags", "status", "xstatus", "pid", "ppid", "uid", "gid", "ruid",
            "rgid", "svuid", "svgid", "rfu_1",
        )] + [("comm", ctypes.c_char * 16), ("name", ctypes.c_char * 32)] + [
            (name, ctypes.c_uint32) for name in ("nfiles", "pgid", "pjobc", "e_tdev", "e_tpgid")
        ] + [("nice", ctypes.c_int32), ("start_sec", ctypes.c_uint64), ("start_usec", ctypes.c_uint64)]

    library = ctypes.CDLL("/usr/lib/libproc.dylib")
    library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
    info = BsdInfo()
    size = library.proc_pidinfo(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info))
    if size != ctypes.sizeof(info) or info.pid != pid or not info.start_sec:
        raise RuntimeError(f"Cannot verify process start for {pid}: {size} bytes")
    return info.start_sec * 1_000_000 + info.start_usec


def linux_start(pid):
    with open(f"/proc/{pid}/stat") as source:
        fields = source.read().rsplit(")", 1)[1].split()
    with open("/proc/stat") as source:
        boot = next(int(line.split()[1]) for line in source if line.startswith("btime "))
    return boot * 1_000_000 + int(fields[19]) * 1_000_000 // os.sysconf("SC_CLK_TCK")


def identity(port):
    if platform.system() == "Windows":
        script = f"""$ErrorActionPreference='Stop'
$ids=@(Get-NetTCPConnection -LocalPort {port} -State Listen | Select-Object -ExpandProperty OwningProcess -Unique)
if ($ids.Count -ne 1) {{ throw 'Expected one listener' }}
$process=Get-Process -Id $ids[0]
$epoch=[DateTime]::SpecifyKind([DateTime]'1970-01-01',[DateTimeKind]::Utc)
@{{pid=$process.Id;started_us=[long](($process.StartTime.ToUniversalTime().Ticks-$epoch.Ticks)/10)}} | ConvertTo-Json -Compress
"""
        return json.loads(output(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script]))
    pid = unix_listener(port)
    return {"pid": pid, "started_us": mac_start(pid) if platform.system() == "Darwin" else linux_start(pid)}


if __name__ == "__main__":
    port = int(sys.argv[1])
    if port not in (7686, 7697):
        raise RuntimeError("Unapproved DEV listener port")
    print(json.dumps(identity(port)))
