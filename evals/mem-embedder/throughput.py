"""Measure warm production embedding throughput on an otherwise idle build host."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

MODELS = [
    ("l12", "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2"),
    ("multilingual-e5-small", "intfloat/multilingual-e5-small"),
    ("bge-small-en-v1.5", "BAAI/bge-small-en-v1.5"),
    ("e5-small-v2", "intfloat/e5-small-v2"),
]
TEXTS = [
    "Find the release checklist for a failed deployment",
    "Какие действия нужны при отказе резервного копирования?",
    "GitHub runners, permissions, and scheduled notifications",
    " Mixed  CASE\n and whitespace ",
    "Café naïve Straße 東京 🦀",
    "",
    "a",
    "How can I recover my terminal session?",
]


def response(process):
    row = json.loads(process.stdout.readline())
    if "error" in row:
        raise RuntimeError(row["error"])
    return row


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    result = {"command": __import__("sys").argv, "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              "warmup_per_model": 1, "repeats": 3, "corpus": TEXTS, "results": []}
    for label, model in MODELS:
        process = subprocess.Popen([str(args.binary.resolve()), model], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            assert response(process)["ready"]
            process.stdin.write(json.dumps({"text": TEXTS[0]}) + "\n")
            process.stdin.flush()
            response(process)
            timings = []
            for _ in range(3):
                for text in TEXTS:
                    start = time.perf_counter()
                    process.stdin.write(json.dumps({"text": text}) + "\n")
                    process.stdin.flush()
                    response(process)
                    timings.append(time.perf_counter() - start)
        finally:
            process.stdin.close()
            process.wait(timeout=15)
            if process.returncode:
                raise RuntimeError(f"production oracle exit {process.returncode}")
        row = {"label": label, "model": model, "samples": len(timings),
               "warm_embeds_per_second": len(timings) / sum(timings), "elapsed_seconds": sum(timings)}
        result["results"].append(row)
        args.out.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps(row), flush=True)


if __name__ == "__main__":
    main()
