"""Independent cached Transformers reference against the production provider."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

import torch
from transformers import AutoModel, AutoTokenizer

MODELS = [
    ("l12", "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2", "e8f8c211226b894fcb81acc59f3b34ba3efd5f42", "mean", "", ""),
    ("multilingual-e5-small", "intfloat/multilingual-e5-small", "614241f622f53c4eeff9890bdc4f31cfecc418b3", "mean", "query: ", "passage: "),
    ("bge-small-en-v1.5", "BAAI/bge-small-en-v1.5", "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a", "cls", "Represent this sentence for searching relevant passages: ", ""),
    ("e5-small-v2", "intfloat/e5-small-v2", "ffb93f3bd4047442299a41ebb6fa998a38507c52", "mean", "query: ", "passage: "),
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
    "многоязычный поиск памяти " * 300,
    "release quality gates and failure notifications " * 300,
]

GOLDEN_TEXTS = json.loads((Path(__file__).resolve().parents[2] / "scripts/fixtures/embedding-parity.json").read_text())
TEXTS += GOLDEN_TEXTS

def read_response(process):
    line = process.stdout.readline()
    if not line:
        raise RuntimeError(f"oracle exited before response: {process.poll()}")
    row = json.loads(line)
    if "error" in row:
        raise RuntimeError(row["error"])
    return row


def measure(binary, cache, descriptor, reference_dir):
    label, model_id, revision, pooling, query_prefix, passage_prefix = descriptor
    path = cache / ("models--" + model_id.replace("/", "--")) / "snapshots" / revision
    tokenizer = AutoTokenizer.from_pretrained(path, local_files_only=True)
    model = AutoModel.from_pretrained(path, local_files_only=True).eval()
    process = subprocess.Popen([str(binary), model_id], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    rows = []
    golden = []
    try:
        assert read_response(process)["ready"]
        for query in [False, True]:
            for index, original in enumerate(TEXTS):
                text = " ".join(original.lower().split())
                prefixed = (query_prefix if query else passage_prefix) + text
                tokens = tokenizer(prefixed, max_length=512, padding=True, truncation=True, return_tensors="pt")
                with torch.inference_mode():
                    hidden = model(**tokens).last_hidden_state
                    if pooling == "cls":
                        expected = hidden[:, 0]
                    else:
                        mask = tokens["attention_mask"].unsqueeze(-1)
                        expected = (hidden * mask).sum(1) / mask.sum(1)
                    expected = torch.nn.functional.normalize(expected, p=2, dim=1)[0]
                if not query and index >= len(TEXTS) - len(GOLDEN_TEXTS):
                    golden.append({"text": original, "vector": expected.tolist()})
                process.stdin.write(json.dumps({"text": original, "query": query}) + "\n")
                process.stdin.flush()
                response = read_response(process)
                actual = torch.tensor(response["embedding"])
                cosine = torch.nn.functional.cosine_similarity(actual.unsqueeze(0), expected.unsqueeze(0)).item()
                rows.append({"case": index, "query": query, "tokens": tokens["input_ids"].shape[1], "cosine": cosine,
                             "max_abs_error": (actual - expected).abs().max().item(), "elapsed_ms": response["elapsed_ms"]})
        warm = []
        for _ in range(3):
            for text in TEXTS[:8]:
                start = time.perf_counter()
                process.stdin.write(json.dumps({"text": text}) + "\n")
                process.stdin.flush()
                read_response(process)
                warm.append(time.perf_counter() - start)
    finally:
        process.stdin.close()
        process.wait(timeout=15)
        if process.returncode:
            raise RuntimeError(f"oracle exit {process.returncode}")
    reference_dir.mkdir(parents=True, exist_ok=True)
    (reference_dir / f"{label}.json").write_text(json.dumps({"model": model_id, "revision": revision, "dimension": 384, "cases": golden}) + "\n")
    files = ["config.json", "tokenizer.json", "tokenizer_config.json", "special_tokens_map.json", "model.safetensors"]
    artifacts = {name: {"bytes": (path / name).stat().st_size,
                        "sha256": hashlib.file_digest((path / name).open("rb"), "sha256").hexdigest()} for name in files}
    return {"label": label, "model": model_id, "revision": revision, "cases": rows,
            "min_cosine": min(row["cosine"] for row in rows), "tolerance_cosine": 0.999,
            "passed": all(row["cosine"] >= 0.999 for row in rows), "artifacts": artifacts,
            "disk_bytes": sum(row["bytes"] for row in artifacts.values()),
            "warm_embeds_per_second": len(warm) / sum(warm), "warm_samples": len(warm)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    torch.set_num_threads(4)
    results = json.loads(args.out.read_text())["results"] if args.resume and args.out.exists() else []
    for descriptor in MODELS:
        if any(row["label"] == descriptor[0] and row["passed"] for row in results):
            continue
        result = measure(args.binary.resolve(), args.cache, descriptor, args.out.parent / "oracle-refs")
        results.append(result)
        args.out.write_text(json.dumps({"command": os.sys.argv, "torch": torch.__version__, "results": results}, indent=2) + "\n")
        print(result["label"], "min_cosine", result["min_cosine"], "embeds/sec", result["warm_embeds_per_second"], flush=True)
        if not result["passed"]:
            raise SystemExit("Candle/reference mismatch; stop before recall")


if __name__ == "__main__":
    main()
