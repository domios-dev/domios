import argparse
import ctypes
import json
import math
import os
import queue
from pathlib import Path
import subprocess
import sys
import time
import threading


def resident_bytes(pid):
    if sys.platform == "win32":
        class Counters(ctypes.Structure):
            _fields_ = [("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong)] + [
                (name, ctypes.c_size_t) for name in ["PeakWorkingSetSize", "WorkingSetSize",
                "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage"]]
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel.OpenProcess(0x1010, False, pid)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            counters = Counters()
            counters.cb = ctypes.sizeof(counters)
            getter = ctypes.WinDLL("psapi").GetProcessMemoryInfo
            getter.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
            if not getter(handle, ctypes.byref(counters), counters.cb):
                raise ctypes.WinError(ctypes.get_last_error())
            return counters.WorkingSetSize
        finally:
            kernel.CloseHandle(handle)
    if sys.platform.startswith("linux"):
        pages = int(Path(f"/proc/{pid}/statm").read_text().split()[1])
        return pages * os.sysconf("SC_PAGE_SIZE")
    return int(subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True)) * 1024


def measure(binary, sentences, legacy=False):
    launched = time.perf_counter()
    process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
    responses, readiness = queue.Queue(), queue.Queue()

    def read_output():
        for line in process.stdout:
            responses.put(line)
        responses.put(None)

    def read_errors():
        for line in process.stderr:
            sys.stderr.write(line)
            if "[codeterm-embed] ready" in line:
                readiness.put(True)
        readiness.put(False)

    threading.Thread(target=read_output, daemon=True).start()
    threading.Thread(target=read_errors, daemon=True).start()
    vectors, latencies, resident = [], [], []
    try:
        if legacy:
            if not readiness.get(timeout=120):
                raise RuntimeError("old embedding runtime exited before readiness")
        elif not json.loads(responses.get(timeout=120)).get("ready"):
            raise RuntimeError("new embedding runtime did not report readiness")
        startup_ms = (time.perf_counter() - launched) * 1000
        idle = resident_bytes(process.pid)
        for index, text in enumerate(sentences):
            started = time.perf_counter()
            process.stdin.write(json.dumps({"id": str(index), "text": text}, ensure_ascii=False) + "\n")
            process.stdin.flush()
            line = responses.get(timeout=120)
            if line is None:
                raise RuntimeError("embedding runtime closed stdout")
            response = json.loads(line)
            latency = (time.perf_counter() - started) * 1000
            if index == 0:
                cold_first_result_ms = (time.perf_counter() - launched) * 1000
            if response.get("error"):
                raise RuntimeError(response["error"])
            vector = response["embedding"]
            if len(vector) != 384 or not all(math.isfinite(v) for v in vector):
                raise RuntimeError("invalid embedding")
            vectors.append(vector)
            latencies.append(latency)
            resident.append(resident_bytes(process.pid))
        return {"vectors": vectors, "latencies_ms": latencies, "startup_ms": startup_ms,
                "cold_first_result_ms": cold_first_result_ms, "initial_idle_rss_bytes": idle,
                "warm_rss_bytes": resident[-1], "peak_observed_rss_bytes": max(resident),
                "warm_p50_ms": sorted(latencies[1:])[math.ceil(0.50 * (len(latencies) - 1)) - 1],
                "warm_p95_ms": sorted(latencies[1:])[math.ceil(0.95 * (len(latencies) - 1)) - 1]}
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def cosine(a, b):
    if len(a) != len(b):
        raise ValueError("embedding dimensions differ")
    return sum(x * y for x, y in zip(a, b)) / math.sqrt(sum(x*x for x in a) * sum(x*x for x in b))


def compare_vectors(old, new, count):
    if len(old) != count or len(new) != count:
        raise ValueError("embedding count differs from sentence count")
    for vector in old + new:
        if len(vector) != 384 or not all(math.isfinite(v) for v in vector):
            raise ValueError("invalid embedding")
        if abs(math.sqrt(sum(v * v for v in vector)) - 1) > 1e-3:
            raise ValueError("embedding is not normalized")
    cosines = [cosine(a, b) for a, b in zip(old, new)]
    if min(cosines) < 0.999:
        raise ValueError(f"embedding parity failed: minimum cosine {min(cosines)}")
    return cosines


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-binary", type=Path)
    parser.add_argument("--baseline", type=Path, default=Path(__file__).resolve().parent.parent / "src-tauri/crates/codeterm-mem/tests/fixtures/embed_parity.json")
    parser.add_argument("--new-binary", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    sentences = json.loads((Path(__file__).parent / "fixtures/embedding-parity.json").read_text(encoding="utf-8"))
    if args.old_binary:
        old = measure(args.old_binary, sentences, legacy=True)
        args.baseline.write_text(json.dumps(old), encoding="utf-8")
    else:
        old = json.loads(args.baseline.read_text(encoding="utf-8"))
        if "cases" in old:
            if [case["text"] for case in old["cases"]] != sentences:
                raise RuntimeError("golden sentences differ from oracle inputs")
            old = {"vectors": [case["vector"] for case in old["cases"]]}
    if args.new_binary:
        new = measure(args.new_binary, sentences)
        cosines = compare_vectors(old["vectors"], new["vectors"], len(sentences))
        for index, value in enumerate(cosines):
            print(f"{index + 1:02}: {value:.9f}")
        report = {"cosines": cosines, "old": old, "new": new}
        if args.output:
            args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(json.dumps({name: {key: value for key, value in data.items() if key != "vectors"}
                          for name, data in [("old", old), ("new", new)]}, indent=2))


if __name__ == "__main__":
    main()
