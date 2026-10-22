"""Private stratified snapshot queries, one fresh empty session per item."""
import argparse
from collections import Counter, defaultdict
import json
import math
import os
from pathlib import Path
import random
import subprocess
import tempfile


def sample(items):
    groups = defaultdict(list)
    for index, item in enumerate(items):
        groups[item["kind"]].append(index)
    rng = random.Random(20261022)
    total = min(150, len(items))
    quotas = {kind: total * len(indices) / len(items) for kind, indices in groups.items()}
    counts = {kind: math.floor(count) for kind, count in quotas.items()}
    remainder = total - sum(counts.values())
    order = sorted(quotas, key=lambda kind: (-(quotas[kind] - counts[kind]), kind))
    for kind in order[:remainder]:
        counts[kind] += 1
    chosen = []
    for kind in sorted(groups):
        candidates = groups[kind]
        rng.shuffle(candidates)
        chosen.extend(candidates[:counts[kind]])
    rng.shuffle(chosen)
    return [{"gold": gold, "language": "ru" if index < 30 else "en"} for index, gold in enumerate(chosen)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--private", required=True, type=Path)
    parser.add_argument("--claude", required=True, type=Path)
    parser.add_argument("--quota-check", required=True, type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    items = json.loads((args.private / "corpus.json").read_text())
    selection = sample(items)
    selection_path = args.private / "sample.json"
    if selection_path.exists():
        assert json.loads(selection_path.read_text()) == selection
    else:
        selection_path.write_text(json.dumps(selection))
    print(json.dumps({"sampled": len(selection), "languages": Counter(row["language"] for row in selection),
                      "kinds": Counter(items[row["gold"]]["kind"] for row in selection)}), flush=True)
    if args.prepare_only:
        return
    queries_path = args.private / "queries.json"
    queries = json.loads(queries_path.read_text()) if queries_path.exists() else []
    environment = {key: value for key, value in os.environ.items() if not key.startswith("CODETERM_")}
    for index, selected in enumerate(selection):
        if index < len(queries):
            assert queries[index]["gold"] == selected["gold"]
            continue
        quota = subprocess.run(["bash", str(args.quota_check.resolve())], capture_output=True, text=True, timeout=30, check=True)
        usage = json.loads(quota.stdout)
        if usage.get("session_pct", 100) >= 85:
            raise SystemExit("query writers paused: quota threshold")
        item = items[selected["gold"]]
        language = "Russian" if selected["language"] == "ru" else "English"
        prompt = (f"Write one natural memory-search question in {language} that the following item could answer. "
                  "You see only this title and summary. Use different wording; do not mention the item, title, or summary. "
                  "Return JSON only: {\"query\": \"one question\"}.\n"
                  + json.dumps({"title": item["title"], "summary": item["summary"]}, ensure_ascii=False))
        with tempfile.TemporaryDirectory(prefix="snapshot-subject-", dir=args.private) as empty:
            response = subprocess.run([str(args.claude.resolve()), "-p", "--model", "opus", "--effort", "high",
                                       "--output-format", "json", "--tools", "", "--strict-mcp-config", "--no-session-persistence"],
                                      input=prompt, env=environment, cwd=empty, capture_output=True, text=True, timeout=120)
        (args.private / f"subject-{index:03d}.json").write_text(json.dumps({"exit": response.returncode, "stdout": response.stdout, "stderr": response.stderr}))
        if response.returncode:
            raise SystemExit(f"query subject {index} failed; private receipt retained")
        wrapper = json.loads(response.stdout)
        body = wrapper["result"].strip()
        if body.startswith("```"):
            body = "\n".join(body.splitlines()[1:-1])
        text = json.loads(body)["query"].strip()
        if not text or len(text) > 1000:
            raise SystemExit("invalid generated query; stop without retry")
        queries.append({**selected, "text": text})
        queries_path.write_text(json.dumps(queries, ensure_ascii=False))
        print(f"query subjects persisted: {len(queries)}/{len(selection)}", flush=True)


if __name__ == "__main__":
    main()
