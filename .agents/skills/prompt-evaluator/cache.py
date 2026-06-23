"""On-disk response cache for prompt-evaluator.

Wraps any (model, system_prompt, user_prompt) → response.text call so
re-runs (e.g. just the judge changed, or A/B again after fixing a flake)
don't repay the LLM cost. Pure stdlib (hashlib + json + pathlib).

Cache lives under ~/.cache/codeterm-evals/ — durable across sessions.
Each entry is a tiny JSON file named <sha256>.json. No TTL; the cache
is content-addressed so prompt edits invalidate naturally. Use
`prune` or `nuke` for explicit eviction.
"""
from __future__ import annotations

import hashlib
import json
import os
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any


_DEFAULT_DIR = Path(os.environ.get("CODETERM_EVAL_CACHE_DIR", "")) \
    if os.environ.get("CODETERM_EVAL_CACHE_DIR") \
    else Path.home() / ".cache" / "codeterm-evals"

_lock = threading.Lock()


def cache_dir() -> Path:
    _DEFAULT_DIR.mkdir(parents=True, exist_ok=True)
    return _DEFAULT_DIR


def _key(model: str, system_prompt: str, user_prompt: str, namespace: str) -> str:
    h = hashlib.sha256()
    h.update(namespace.encode("utf-8"))
    h.update(b"\x00")
    h.update(model.encode("utf-8"))
    h.update(b"\x00")
    h.update(system_prompt.encode("utf-8"))
    h.update(b"\x00")
    h.update(user_prompt.encode("utf-8"))
    return h.hexdigest()


@dataclass
class CacheStats:
    hits: int = 0
    misses: int = 0
    writes: int = 0


_stats = CacheStats()


def stats() -> CacheStats:
    return _stats


def get(model: str, system_prompt: str, user_prompt: str, namespace: str = "agent") -> str | None:
    """Return cached response text, or None on miss."""
    path = cache_dir() / f"{_key(model, system_prompt, user_prompt, namespace)}.json"
    try:
        with _lock:
            data = json.loads(path.read_text(encoding="utf-8"))
        with _lock:
            _stats.hits += 1
        return data.get("response")
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        with _lock:
            _stats.misses += 1
        return None


def put(
    model: str,
    system_prompt: str,
    user_prompt: str,
    response: str,
    namespace: str = "agent",
) -> None:
    path = cache_dir() / f"{_key(model, system_prompt, user_prompt, namespace)}.json"
    payload = {"model": model, "namespace": namespace, "response": response}
    tmp = path.with_suffix(".tmp")
    try:
        with _lock:
            tmp.write_text(json.dumps(payload), encoding="utf-8")
            tmp.replace(path)
            _stats.writes += 1
    except OSError:
        # Cache is best-effort; failures don't break the run.
        pass


def nuke() -> int:
    """Wipe every cached entry. Returns the count removed."""
    n = 0
    if not _DEFAULT_DIR.exists():
        return 0
    for entry in _DEFAULT_DIR.glob("*.json"):
        try:
            entry.unlink()
            n += 1
        except OSError:
            pass
    return n
