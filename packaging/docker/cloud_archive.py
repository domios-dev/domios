"""Portable essentials archive export, verify-before-evict, and safe restore engine
for Domios managed cloud machines (`domios.cloud-archive/1`).

Invariants:
- Never archive or restore login credentials, keys, vault, running authority, or replayable sends.
- Refuse eviction on inaccessible/corrupt repo state, credential-bearing remotes, missing baseline
  commits, unknown/ambiguous user files, failed capture/checksum, stale generation, or size overflow.
- Bind authenticated encryption strictly to (owner_uid, machine_id, generation).
- Never delete or mutate an archive on restore failure; report phases, bytes, and timings honestly.
"""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor, as_completed
import errno
import hashlib
import hmac
import io
import json
import os
import pathlib
import posixpath
import re
import secrets
import shutil
import struct
import subprocess
import tarfile
import tempfile
import time
import urllib.parse
from dataclasses import dataclass, field
from typing import Any, Callable

ARCHIVE_FORMAT = "domios.cloud-archive/1"
ARCHIVE_MAGIC = b"DMCA0001"
ARCHIVE_ALG = "hkdf-sha256-ctr-hmac-sha256"
DEFAULT_MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
DEFAULT_MAX_AGENT_CONFIG_FILE_BYTES = 2 * 1024 * 1024
DEFAULT_MAX_PARALLEL_CLONES = 4

# Strict secret, authority, and replayable-send exclusion patterns (matched against POSIX relative paths).
EXCLUDED_EXACT_NAMES = frozenset({
    ".secrets",
    ".netrc",
    "auth.json",
    "credentials.json",
    ".credentials.json",
    "cloud_wake_queue.json",
    "chat_send_ledger.json",
    "server.pid",
    "domios.pid",
})

EXCLUDED_PATH_PREFIXES = (
    ".ssh/",
    ".aws/",
    ".gnupg/",
    ".kube/",
    ".domios/neighbor-keys/",
    ".domios-dev/neighbor-keys/",
    ".domios/runtime/",
    ".domios-dev/runtime/",
    ".domios/vault/",
    ".domios-dev/vault/",
    ".domios/outbox/",
    ".domios-dev/outbox/",
    ".domios-dev/outbox/",
    ".claude/.credentials",
    ".claude/credentials",
    ".codex/auth",
    ".config/opencode/auth",
)

EXCLUDED_SUBSTRINGS = (
    "/neighbor-keys/",
    "/.secrets",
    "/chat_send_ledger",
    "/cloud_wake_queue",
    "/outbox/",
    "/vault/",
)

# Eligible small portable agent-home configuration paths (explicit allowlist).
ELIGIBLE_AGENT_HOME_FILES = frozenset({
    ".gitconfig",
    ".claude/settings.json",
    ".claude/CLAUDE.md",
    ".codex/config.toml",
    ".config/opencode/config.json",
    ".config/git/ignore",
})

# Known regenerable directories omitted ONLY when a provable recovery input exists in the same repo.
REGENERABLE_DIR_RECOVERY_INPUTS: dict[str, tuple[str, ...]] = {
    "node_modules": ("package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock"),
    "target": ("Cargo.toml",),
    ".venv": ("pyproject.toml", "requirements.txt", "uv.lock", "poetry.lock"),
    "venv": ("pyproject.toml", "requirements.txt"),
    "__pycache__": ("pyproject.toml", "requirements.txt", "setup.py"),
    ".pytest_cache": ("pyproject.toml", "pytest.ini"),
    ".mypy_cache": ("pyproject.toml", "mypy.ini"),
    ".cache": ("package.json", "Cargo.toml", "pyproject.toml"),
    "dist": ("package.json", "Cargo.toml", "pyproject.toml"),
    "build": ("package.json", "Cargo.toml", "pyproject.toml", "Makefile"),
}


class ArchiveError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.detail = message


class ArchiveRefusalError(ArchiveError):
    """Raised when eviction MUST be refused and live storage preserved."""


class ArchiveVerificationError(ArchiveError):
    """Raised when encrypted archive verification or binding fails."""


class RestoreDiskFullError(ArchiveError):
    """Raised on local ENOSPC during restore without deleting the source archive."""


@dataclass
class ExportResult:
    archive_path: pathlib.Path
    manifest: dict[str, Any]
    manifest_sha256: str
    ciphertext_sha256: str
    archive_bytes: int
    uncompressed_bytes: int
    timings_ms: dict[str, int]
    excluded_secrets_verified: bool = True


@dataclass
class RestoreResult:
    recovery_ready: bool
    archive_id: str
    owner_uid: str
    machine_id: str
    generation: int
    bytes_restored: int
    files_restored: int
    repos_restored: list[dict[str, Any]]
    deferred_dependency_installs: list[dict[str, Any]]
    missing_or_partial: list[str]
    timings_ms: dict[str, int]
    workspace_identities_compatible: bool


def _now_ms() -> int:
    return int(time.monotonic() * 1000)


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def _to_posix_rel(path: pathlib.Path, root: pathlib.Path) -> str:
    rel = path.relative_to(root).as_posix()
    norm = posixpath.normpath(rel)
    if norm in (".", "") or norm.startswith("../") or norm.startswith("/"):
        raise ArchiveRefusalError("unsafe_path", f"Path escapes root: {rel}")
    return norm


def is_excluded_secret_or_runtime_path(rel_posix: str) -> bool:
    norm = posixpath.normpath(rel_posix).lstrip("./")
    base = posixpath.basename(norm)
    if base in EXCLUDED_EXACT_NAMES or base.endswith(".lock") or base.endswith(".pid") or base.endswith(".key"):
        return True
    if any(norm == p.rstrip("/") or norm.startswith(p) for p in EXCLUDED_PATH_PREFIXES):
        return True
    wrapped = f"/{norm}"
    if any(sub in wrapped for sub in EXCLUDED_SUBSTRINGS):
        return True
    if re.search(r"(^|/)\.env(\.|$)", norm) and not norm.endswith(".env.example"):
        return True
    return False


def validate_credential_free_remote_url(url: str) -> str:
    """Validates a repository remote URL and strictly refuses embedded credentials."""
    trimmed = url.strip()
    if not trimmed:
        raise ArchiveRefusalError("missing_remote_url", "Managed repo has an empty remote URL")
    if "://" in trimmed:
        parsed = urllib.parse.urlsplit(trimmed)
        if parsed.username or parsed.password:
            raise ArchiveRefusalError("credential_in_remote_url", "Remote URL contains embedded credentials")
        if parsed.scheme not in ("https", "http", "ssh", "git", "file"):
            raise ArchiveRefusalError("unsupported_remote_scheme", f"Unsupported remote scheme: {parsed.scheme}")
        return trimmed
    # SCP-style SSH URL (git@github.com:org/repo.git) or absolute local path (for test fixtures).
    if re.match(r"^[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+:.+", trimmed):
        # Refuse token-like user@host where user contains ':' (user:pass@host)
        if ":" in trimmed.split("@", 1)[0]:
            raise ArchiveRefusalError("credential_in_remote_url", "SSH remote URL contains embedded password")
        return trimmed
    if os.path.isabs(trimmed) or re.match(r"^[A-Za-z]:[/\\]", trimmed):
        return trimmed
    raise ArchiveRefusalError("unsupported_remote_url", f"Unrecognized remote URL format: {trimmed}")


def _hkdf_sha256(ikm: bytes, salt: bytes, info: bytes, length: int) -> bytes:
    prk = hmac.new(salt, ikm, hashlib.sha256).digest()
    out = bytearray()
    t = b""
    counter = 1
    while len(out) < length:
        t = hmac.new(prk, t + info + bytes([counter]), hashlib.sha256).digest()
        out.extend(t)
        counter += 1
    return bytes(out[:length])


def _ctr_xor(key: bytes, nonce: bytes, data: bytes) -> bytes:
    """Pure-stdlib SHA-256-CTR keystream XOR (authenticated via Encrypt-then-MAC HMAC-SHA256)."""
    out = bytearray(len(data))
    mv = memoryview(data)
    block_idx = 0
    pos = 0
    total = len(data)
    while pos < total:
        stream = hmac.new(key, nonce + struct.pack(">Q", block_idx), hashlib.sha256).digest()
        end = min(pos + 32, total)
        chunk_len = end - pos
        for i in range(chunk_len):
            out[pos + i] = mv[pos + i] ^ stream[i]
        pos = end
        block_idx += 1
    return bytes(out)


def encrypt_archive_payload(
    plaintext: bytes,
    *,
    owner_key: bytes,
    owner_uid: str,
    machine_id: str,
    generation: int,
    archive_id: str,
    revision: int,
    manifest_sha256: str,
) -> bytes:
    if not owner_key or len(owner_key) < 16:
        raise ArchiveRefusalError("invalid_owner_key", "Owner archive key must be at least 16 bytes")
    nonce = secrets.token_bytes(16)
    aad_info = f"{ARCHIVE_FORMAT}|{owner_uid}|{machine_id}|{generation}|{archive_id}".encode("utf-8")
    derived = _hkdf_sha256(owner_key, nonce, aad_info, 64)
    enc_key, mac_key = derived[:32], derived[32:]
    ciphertext = _ctr_xor(enc_key, nonce, plaintext)
    header = json.dumps({
        "format": ARCHIVE_FORMAT,
        "alg": ARCHIVE_ALG,
        "archive_id": archive_id,
        "owner_uid": owner_uid,
        "machine_id": machine_id,
        "generation": generation,
        "revision": revision,
        "manifest_sha256": manifest_sha256,
        "plaintext_bytes": len(plaintext),
        "nonce_hex": nonce.hex(),
    }, sort_keys=True, separators=(",", ":")).encode("utf-8")
    frame = ARCHIVE_MAGIC + struct.pack(">I", len(header)) + header + ciphertext
    tag = hmac.new(mac_key, frame + aad_info, hashlib.sha256).digest()
    return frame + tag


def decrypt_archive_payload(
    blob: bytes,
    *,
    owner_key: bytes,
    expected_owner_uid: str,
    expected_machine_id: str,
    expected_generation: int | None = None,
) -> tuple[dict[str, Any], bytes]:
    if len(blob) < len(ARCHIVE_MAGIC) + 4 + 32 or not blob.startswith(ARCHIVE_MAGIC):
        raise ArchiveVerificationError("corrupt_archive", "Invalid archive magic or truncated envelope")
    header_len = struct.unpack(">I", blob[len(ARCHIVE_MAGIC):len(ARCHIVE_MAGIC) + 4])[0]
    header_start = len(ARCHIVE_MAGIC) + 4
    header_end = header_start + header_len
    if header_end + 32 > len(blob):
        raise ArchiveVerificationError("corrupt_archive", "Truncated archive header")
    try:
        header = json.loads(blob[header_start:header_end].decode("utf-8"))
    except Exception as exc:
        raise ArchiveVerificationError("corrupt_archive", f"Malformed archive header: {exc}") from exc
    if header.get("format") != ARCHIVE_FORMAT or header.get("alg") != ARCHIVE_ALG:
        raise ArchiveVerificationError("unsupported_format", "Unsupported archive format or algorithm")
    if header.get("owner_uid") != expected_owner_uid:
        raise ArchiveVerificationError("owner_mismatch", "Archive owner does not match requester")
    if header.get("machine_id") != expected_machine_id:
        raise ArchiveVerificationError("machine_mismatch", "Archive machine_id does not match target")
    if expected_generation is not None and header.get("generation") != expected_generation:
        raise ArchiveVerificationError("generation_mismatch", "Archive generation does not match expected")

    nonce = bytes.fromhex(header["nonce_hex"])
    aad_info = (
        f"{ARCHIVE_FORMAT}|{header['owner_uid']}|{header['machine_id']}"
        f"|{header['generation']}|{header['archive_id']}"
    ).encode("utf-8")
    derived = _hkdf_sha256(owner_key, nonce, aad_info, 64)
    enc_key, mac_key = derived[:32], derived[32:]
    frame = blob[:-32]
    tag = blob[-32:]
    expected_tag = hmac.new(mac_key, frame + aad_info, hashlib.sha256).digest()
    if not hmac.compare_digest(tag, expected_tag):
        raise ArchiveVerificationError("mac_verification_failed", "Archive authentication tag mismatch")
    ciphertext = blob[header_end:-32]
    plaintext = _ctr_xor(enc_key, nonce, ciphertext)
    if len(plaintext) != header.get("plaintext_bytes"):
        raise ArchiveVerificationError("plaintext_length_mismatch", "Decrypted payload length mismatch")
    return header, plaintext


def _run_git(repo_dir: pathlib.Path, args: list[str], *, check: bool = True, binary: bool = False) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    env["GIT_TERMINAL_PROMPT"] = "0"
    proc = subprocess.run(
        ["git", "-C", str(repo_dir), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        check=False,
    )
    if check and proc.returncode != 0:
        err = proc.stderr.decode("utf-8", errors="replace").strip()
        raise ArchiveRefusalError("git_command_failed", f"git {' '.join(args)} failed in {repo_dir}: {err}")
    if not binary:
        proc.stdout = proc.stdout.decode("utf-8", errors="replace")
        proc.stderr = proc.stderr.decode("utf-8", errors="replace")
    return proc


def _check_repo_state_supported(repo_dir: pathlib.Path) -> None:
    git_dir_proc = _run_git(repo_dir, ["rev-parse", "--git-dir"], check=False)
    if git_dir_proc.returncode != 0:
        raise ArchiveRefusalError("repo_inaccessible", f"Repository is inaccessible or corrupt: {repo_dir}")
    git_dir = (repo_dir / git_dir_proc.stdout.strip()).resolve()
    for sentinel in ("rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD", "BISECT_LOG"):
        if (git_dir / sentinel).exists():
            raise ArchiveRefusalError("repo_mid_operation", f"Repository has in-progress {sentinel}: {repo_dir}")


def _capture_repo(
    repo_dir: pathlib.Path,
    cloud_home: pathlib.Path,
    staging_dir: pathlib.Path,
    repo_index: int,
) -> tuple[dict[str, Any], list[tuple[str, bytes]]]:
    _check_repo_state_supported(repo_dir)
    rel_repo = _to_posix_rel(repo_dir, cloud_home)
    repo_id = f"repo_{repo_index:03d}"

    remotes_out = _run_git(repo_dir, ["remote"], check=True).stdout.splitlines()
    remote_name = "origin" if "origin" in remotes_out else (remotes_out[0] if remotes_out else "")
    if not remote_name:
        raise ArchiveRefusalError("repo_missing_remote", f"Repository {rel_repo} has no configured remote")
    raw_remote_url = _run_git(repo_dir, ["remote", "get-url", remote_name], check=True).stdout.strip()
    remote_url = validate_credential_free_remote_url(raw_remote_url)

    head_proc = _run_git(repo_dir, ["rev-parse", "--verify", "HEAD"], check=False)
    if head_proc.returncode != 0:
        raise ArchiveRefusalError("repo_missing_head", f"Repository {rel_repo} has no valid HEAD commit")
    head_sha = head_proc.stdout.strip()

    branch_proc = _run_git(repo_dir, ["symbolic-ref", "--short", "-q", "HEAD"], check=False)
    branch = branch_proc.stdout.strip() if branch_proc.returncode == 0 and branch_proc.stdout.strip() else None

    # Verify whether ignored/regenerable directories have provable recovery inputs.
    omitted_regenerable: list[str] = []
    for child in sorted(repo_dir.iterdir()):
        if child.name == ".git":
            continue
        if child.is_dir() and child.name in REGENERABLE_DIR_RECOVERY_INPUTS:
            recovery_inputs = REGENERABLE_DIR_RECOVERY_INPUTS[child.name]
            if any((repo_dir / req).exists() for req in recovery_inputs):
                omitted_regenerable.append(child.name)
            else:
                raise ArchiveRefusalError(
                    "ambiguous_regenerable_dir",
                    f"Directory {rel_repo}/{child.name} lacks provable recovery inputs ({', '.join(recovery_inputs)})",
                )

    artifacts: list[tuple[str, bytes]] = []

    # Determine unpushed commits via upstream tracking ref or remote-tracking branch.
    upstream_proc = _run_git(repo_dir, ["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], check=False)
    baseline_ref: str | None = None
    if upstream_proc.returncode == 0 and upstream_proc.stdout.strip():
        baseline_ref = upstream_proc.stdout.strip()
    elif branch:
        candidate = f"refs/remotes/{remote_name}/{branch}"
        chk = _run_git(repo_dir, ["rev-parse", "--verify", candidate], check=False)
        if chk.returncode == 0:
            baseline_ref = candidate

    has_bundle = False
    bundle_arc_name: str | None = None
    bundle_ref = f"refs/heads/{branch}" if branch else "HEAD"
    if baseline_ref:
        baseline_sha = _run_git(repo_dir, ["rev-parse", "--verify", baseline_ref], check=True).stdout.strip()
        # Verify baseline is an ancestor or reachable commit.
        ahead_count = int(_run_git(repo_dir, ["rev-list", "--count", f"{baseline_ref}..HEAD"], check=True).stdout.strip() or "0")
        if ahead_count > 0:
            bundle_file = staging_dir / f"{repo_id}.bundle"
            _run_git(repo_dir, ["bundle", "create", str(bundle_file), f"{baseline_sha}..HEAD", bundle_ref], check=True)
            bundle_bytes = bundle_file.read_bytes()
            bundle_arc_name = f"repos/{repo_id}/unpushed.bundle"
            artifacts.append((bundle_arc_name, bundle_bytes))
            has_bundle = True
    else:
        # No remote tracking ref locally: bundle full HEAD history so restore never loses commits.
        bundle_file = staging_dir / f"{repo_id}.bundle"
        _run_git(repo_dir, ["bundle", "create", str(bundle_file), bundle_ref], check=True)
        bundle_bytes = bundle_file.read_bytes()
        bundle_arc_name = f"repos/{repo_id}/unpushed.bundle"
        artifacts.append((bundle_arc_name, bundle_bytes))
        has_bundle = True

    # Staged and unstaged patches (binary-safe).
    staged_bytes = _run_git(repo_dir, ["diff", "--cached", "--binary"], check=True, binary=True).stdout
    unstaged_bytes = _run_git(repo_dir, ["diff", "--binary"], check=True, binary=True).stdout
    staged_arc_name: str | None = None
    unstaged_arc_name: str | None = None
    if staged_bytes:
        staged_arc_name = f"repos/{repo_id}/staged.patch"
        artifacts.append((staged_arc_name, staged_bytes))
    if unstaged_bytes:
        unstaged_arc_name = f"repos/{repo_id}/unstaged.patch"
        artifacts.append((unstaged_arc_name, unstaged_bytes))

    # Explicit untracked user files (non-ignored, excluding secrets).
    untracked_raw = _run_git(repo_dir, ["ls-files", "--others", "--exclude-standard", "-z"], check=True, binary=True).stdout
    untracked_entries: list[dict[str, Any]] = []
    for raw_item in untracked_raw.split(b"\x00"):
        if not raw_item:
            continue
        item_rel = raw_item.decode("utf-8")
        first_seg = item_rel.split("/", 1)[0]
        if first_seg in omitted_regenerable:
            continue
        full_rel = f"{rel_repo}/{item_rel}"
        if is_excluded_secret_or_runtime_path(full_rel):
            continue
        src = repo_dir / item_rel
        if src.is_symlink() or not src.is_file():
            raise ArchiveRefusalError("unsupported_untracked_entry", f"Unsupported untracked entry: {full_rel}")
        data = src.read_bytes()
        arc_path = f"repos/{repo_id}/untracked/{item_rel}"
        artifacts.append((arc_path, data))
        untracked_entries.append({
            "relative_path": item_rel,
            "archive_path": arc_path,
            "sha256": _sha256_bytes(data),
            "bytes": len(data),
        })

    deferred_deps = [
        req
        for d in omitted_regenerable
        for req in REGENERABLE_DIR_RECOVERY_INPUTS[d]
        if (repo_dir / req).exists()
    ]

    meta = {
        "repo_id": repo_id,
        "relative_path": rel_repo,
        "remote_name": remote_name,
        "remote_url": remote_url,
        "branch": branch,
        "head_sha": head_sha,
        "has_bundle": has_bundle,
        "bundle_archive_path": bundle_arc_name,
        "has_staged_patch": staged_arc_name is not None,
        "staged_patch_archive_path": staged_arc_name,
        "has_unstaged_patch": unstaged_arc_name is not None,
        "unstaged_patch_archive_path": unstaged_arc_name,
        "untracked_files": untracked_entries,
        "omitted_regenerable": sorted(set(omitted_regenerable)),
        "deferred_dependency_manifests": sorted(set(deferred_deps)),
    }
    return meta, artifacts


def export_essentials_archive(
    cloud_home: pathlib.Path,
    output_path: pathlib.Path,
    *,
    owner_uid: str,
    machine_id: str,
    generation: int,
    owner_key: bytes,
    archive_id: str,
    revision: int = 1,
    data_dir_name: str = ".domios",
    named_user_files: list[str] | None = None,
    managed_repo_paths: list[str] | None = None,
    max_archive_bytes: int = DEFAULT_MAX_ARCHIVE_BYTES,
    generation_fence_check: Callable[[], bool] | None = None,
) -> ExportResult:
    """Exports, encrypts, and durably verifies an owner-bound portable essentials archive."""
    t_start = _now_ms()
    cloud_home = cloud_home.resolve()
    if not cloud_home.is_dir():
        raise ArchiveRefusalError("missing_cloud_home", f"Cloud home not found: {cloud_home}")

    if generation_fence_check and not generation_fence_check():
        raise ArchiveRefusalError("stale_generation", "Generation fence failed before export")

    named_set = {posixpath.normpath(p).lstrip("./") for p in (named_user_files or [])}
    repo_rel_set = {posixpath.normpath(p).lstrip("./") for p in (managed_repo_paths or [])}

    # Auto-discover managed git repos under cloud_home if not explicitly passed.
    discovered_repos: list[pathlib.Path] = []
    if managed_repo_paths is not None:
        for rel in sorted(repo_rel_set):
            repo_p = (cloud_home / rel).resolve()
            if not repo_p.is_dir():
                raise ArchiveRefusalError("repo_inaccessible", f"Managed repo missing: {rel}")
            discovered_repos.append(repo_p)
    else:
        for dirpath, dirnames, _ in os.walk(cloud_home):
            cur = pathlib.Path(dirpath)
            rel_cur = "" if cur == cloud_home else _to_posix_rel(cur, cloud_home)
            if rel_cur.startswith(data_dir_name):
                dirnames[:] = []
                continue
            if ".git" in dirnames or (cur / ".git").is_file():
                discovered_repos.append(cur)
                repo_rel_set.add(_to_posix_rel(cur, cloud_home))
                dirnames[:] = []
                continue
            dirnames[:] = [d for d in sorted(dirnames) if d not in (".cache", ".local")]

    domios_entries: list[dict[str, Any]] = []
    agent_home_entries: list[dict[str, Any]] = []
    user_file_entries: list[dict[str, Any]] = []
    payload_blobs: list[tuple[str, bytes]] = []
    omitted_excluded: list[str] = []

    # 1. Inventory portable Domios data, eligible agent-home config, and named user files;
    #    refuse on unknown/ambiguous files outside known managed roots!
    for dirpath, dirnames, filenames in os.walk(cloud_home):
        cur = pathlib.Path(dirpath)
        rel_dir = "" if cur == cloud_home else _to_posix_rel(cur, cloud_home)
        # Skip inside managed repos (handled by _capture_repo)
        if rel_dir and any(rel_dir == r or rel_dir.startswith(f"{r}/") for r in repo_rel_set):
            dirnames[:] = []
            continue
        # Skip known OS XDG cache directories
        if rel_dir in (".cache", ".local/share", ".local/state") or rel_dir.startswith(".cache/"):
            dirnames[:] = []
            continue

        dirnames.sort()
        for fname in sorted(filenames):
            fpath = cur / fname
            rel = _to_posix_rel(fpath, cloud_home)
            if is_excluded_secret_or_runtime_path(rel):
                omitted_excluded.append(rel)
                continue
            if fpath.is_symlink():
                raise ArchiveRefusalError("unsupported_symlink", f"Refusing symlink in essentials: {rel}")

            if rel.startswith(f"{data_dir_name}/"):
                data = fpath.read_bytes()
                arc_path = f"domios/{rel}"
                payload_blobs.append((arc_path, data))
                domios_entries.append({"relative_path": rel, "archive_path": arc_path, "sha256": _sha256_bytes(data), "bytes": len(data)})
            elif rel in ELIGIBLE_AGENT_HOME_FILES:
                size = fpath.stat().st_size
                if size > DEFAULT_MAX_AGENT_CONFIG_FILE_BYTES:
                    raise ArchiveRefusalError("agent_config_too_large", f"Agent config exceeds cap: {rel}")
                data = fpath.read_bytes()
                arc_path = f"agent_home/{rel}"
                payload_blobs.append((arc_path, data))
                agent_home_entries.append({"relative_path": rel, "archive_path": arc_path, "sha256": _sha256_bytes(data), "bytes": len(data)})
            elif rel in named_set:
                data = fpath.read_bytes()
                arc_path = f"user_files/{rel}"
                payload_blobs.append((arc_path, data))
                user_file_entries.append({"relative_path": rel, "archive_path": arc_path, "sha256": _sha256_bytes(data), "bytes": len(data)})
            else:
                raise ArchiveRefusalError("unknown_user_file", f"Refusing eviction: unclassified user file {rel}")

    # Ensure every explicitly named user file was captured.
    captured_named = {e["relative_path"] for e in user_file_entries}
    missing_named = sorted(named_set - captured_named)
    if missing_named:
        raise ArchiveRefusalError("missing_named_user_file", f"Named user file missing: {', '.join(missing_named)}")

    t_inv = _now_ms()

    # 2. Capture managed Git repositories.
    repo_metas: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="domios-arc-stage-") as stage_tmp:
        stage_dir = pathlib.Path(stage_tmp)
        for idx, repo_dir in enumerate(discovered_repos):
            meta, repo_blobs = _capture_repo(repo_dir, cloud_home, stage_dir, idx)
            repo_metas.append(meta)
            payload_blobs.extend(repo_blobs)

    t_repos = _now_ms()

    # Re-check fence before packing/sealing (catches competing wake or write).
    if generation_fence_check and not generation_fence_check():
        raise ArchiveRefusalError("competing_wake_or_write", "Generation fence tripped during archive export")

    # Assert strict secret exclusion across every staged entry.
    for arc_path, _ in payload_blobs:
        candidate = arc_path.split("/", 1)[1] if "/" in arc_path else arc_path
        if is_excluded_secret_or_runtime_path(candidate):
            raise ArchiveRefusalError("secret_leak_detected", f"Refusing archive containing excluded path: {arc_path}")

    uncompressed_bytes = sum(len(b) for _, b in payload_blobs)
    manifest: dict[str, Any] = {
        "schema_version": 1,
        "format": ARCHIVE_FORMAT,
        "archive_id": archive_id,
        "uid": owner_uid,
        "machine_id": machine_id,
        "generation": generation,
        "revision": revision,
        "data_dir_name": data_dir_name,
        "domios_files": domios_entries,
        "agent_home_files": agent_home_entries,
        "user_files": user_file_entries,
        "repos": repo_metas,
        "omitted_excluded_count": len(omitted_excluded),
        "excluded_secrets_verified": True,
        "uncompressed_bytes": uncompressed_bytes,
        "created_at": int(time.time()),
    }
    manifest_bytes = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode("utf-8")
    manifest_sha256 = _sha256_bytes(manifest_bytes)

    # 3. Pack deterministic compressed tar.gz in memory.
    tar_buf = io.BytesIO()
    with tarfile.open(fileobj=tar_buf, mode="w:gz", compresslevel=6) as tf:
        def _add_bytes(name: str, content: bytes) -> None:
            ti = tarfile.TarInfo(name=name)
            ti.size = len(content)
            ti.mtime = 0
            ti.mode = 0o600
            tf.addfile(ti, io.BytesIO(content))

        _add_bytes("manifest.json", manifest_bytes)
        for arc_path, content in sorted(payload_blobs, key=lambda x: x[0]):
            _add_bytes(arc_path, content)
    plaintext_tar = tar_buf.getvalue()
    t_pack = _now_ms()

    # 4. Encrypt & authenticate bound to (owner_uid, machine_id, generation).
    encrypted = encrypt_archive_payload(
        plaintext_tar,
        owner_key=owner_key,
        owner_uid=owner_uid,
        machine_id=machine_id,
        generation=generation,
        archive_id=archive_id,
        revision=revision,
        manifest_sha256=manifest_sha256,
    )
    if len(encrypted) > max_archive_bytes:
        raise ArchiveRefusalError(
            "archive_size_exceeded",
            f"Archive size {len(encrypted)} exceeds max {max_archive_bytes} bytes",
        )
    t_enc = _now_ms()

    output_path.parent.mkdir(parents=True, exist_ok=True)
    tmp_out = output_path.with_suffix(output_path.suffix + ".tmp")
    try:
        with tmp_out.open("wb") as f:
            f.write(encrypted)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp_out, output_path)
    finally:
        tmp_out.unlink(missing_ok=True)

    # 5. Durably re-read & verify the exact written archive before returning success!
    verified = verify_archive(
        output_path,
        owner_key=owner_key,
        expected_owner_uid=owner_uid,
        expected_machine_id=machine_id,
        expected_generation=generation,
        expected_ciphertext_sha256=_sha256_bytes(encrypted),
        max_archive_bytes=max_archive_bytes,
    )
    t_verify = _now_ms()

    return ExportResult(
        archive_path=output_path,
        manifest=verified["manifest"],
        manifest_sha256=verified["manifest_sha256"],
        ciphertext_sha256=verified["ciphertext_sha256"],
        archive_bytes=verified["archive_bytes"],
        uncompressed_bytes=uncompressed_bytes,
        timings_ms={
            "inventory_ms": t_inv - t_start,
            "repos_ms": t_repos - t_inv,
            "pack_ms": t_pack - t_repos,
            "encrypt_ms": t_enc - t_pack,
            "verify_ms": t_verify - t_enc,
            "total_ms": t_verify - t_start,
        },
        excluded_secrets_verified=True,
    )


def verify_archive(
    archive_path: pathlib.Path,
    *,
    owner_key: bytes,
    expected_owner_uid: str,
    expected_machine_id: str,
    expected_generation: int | None = None,
    expected_ciphertext_sha256: str | None = None,
    max_archive_bytes: int = DEFAULT_MAX_ARCHIVE_BYTES,
) -> dict[str, Any]:
    """Durably reads and verifies ciphertext hash, AEAD tag, owner/machine/generation binding,
    manifest integrity, file digests, and strict secret exclusion."""
    if not archive_path.is_file():
        raise ArchiveVerificationError("archive_missing", f"Archive file missing: {archive_path}")
    blob = archive_path.read_bytes()
    if len(blob) == 0 or len(blob) > max_archive_bytes:
        raise ArchiveVerificationError("archive_size_invalid", f"Invalid archive size: {len(blob)}")
    actual_cipher_sha = _sha256_bytes(blob)
    if expected_ciphertext_sha256 and actual_cipher_sha != expected_ciphertext_sha256:
        raise ArchiveVerificationError("ciphertext_hash_mismatch", "Archive ciphertext SHA-256 mismatch")

    header, plaintext = decrypt_archive_payload(
        blob,
        owner_key=owner_key,
        expected_owner_uid=expected_owner_uid,
        expected_machine_id=expected_machine_id,
        expected_generation=expected_generation,
    )

    try:
        with tarfile.open(fileobj=io.BytesIO(plaintext), mode="r:gz") as tf:
            members = {m.name: m for m in tf.getmembers()}
            if "manifest.json" not in members:
                raise ArchiveVerificationError("missing_manifest", "Archive lacks manifest.json")
            manifest_bytes = tf.extractfile(members["manifest.json"]).read()
            manifest_sha = _sha256_bytes(manifest_bytes)
            if manifest_sha != header.get("manifest_sha256"):
                raise ArchiveVerificationError("manifest_hash_mismatch", "Manifest SHA-256 mismatch")
            manifest = json.loads(manifest_bytes.decode("utf-8"))
            if not manifest.get("excluded_secrets_verified"):
                raise ArchiveVerificationError("secrets_not_verified", "Manifest did not verify secret exclusion")
            if manifest.get("uid") != expected_owner_uid or manifest.get("machine_id") != expected_machine_id:
                raise ArchiveVerificationError("manifest_binding_mismatch", "Manifest owner/machine binding mismatch")

            # Verify every file entry digest and confirm zero excluded secret paths.
            for section in ("domios_files", "agent_home_files", "user_files"):
                for entry in manifest.get(section, []):
                    arc_p = entry["archive_path"]
                    if is_excluded_secret_or_runtime_path(entry["relative_path"]):
                        raise ArchiveVerificationError("secret_present_in_archive", f"Excluded path found: {entry['relative_path']}")
                    if arc_p not in members:
                        raise ArchiveVerificationError("missing_entry", f"Missing entry in archive: {arc_p}")
                    data = tf.extractfile(members[arc_p]).read()
                    if _sha256_bytes(data) != entry["sha256"]:
                        raise ArchiveVerificationError("entry_checksum_mismatch", f"Checksum mismatch for {arc_p}")

            for repo in manifest.get("repos", []):
                validate_credential_free_remote_url(repo["remote_url"])
                for ut in repo.get("untracked_files", []):
                    arc_p = ut["archive_path"]
                    if arc_p not in members:
                        raise ArchiveVerificationError("missing_entry", f"Missing untracked entry: {arc_p}")
                    data = tf.extractfile(members[arc_p]).read()
                    if _sha256_bytes(data) != ut["sha256"]:
                        raise ArchiveVerificationError("entry_checksum_mismatch", f"Checksum mismatch for {arc_p}")
    except ArchiveVerificationError:
        raise
    except Exception as exc:
        raise ArchiveVerificationError("corrupt_archive_tar", f"Failed to inspect archive payload: {exc}") from exc

    return {
        "header": header,
        "manifest": manifest,
        "manifest_sha256": manifest_sha,
        "ciphertext_sha256": actual_cipher_sha,
        "archive_bytes": len(blob),
    }


def _write_file_checked(dest: pathlib.Path, data: bytes, writer: Callable[[pathlib.Path, bytes], None] | None = None) -> None:
    try:
        dest.parent.mkdir(parents=True, exist_ok=True)
        if writer is not None:
            writer(dest, data)
        else:
            dest.write_bytes(data)
    except OSError as exc:
        if exc.errno == errno.ENOSPC:
            raise RestoreDiskFullError("disk_full", f"Local volume full while restoring {dest}") from exc
        raise


def _restore_one_repo(
    repo: dict[str, Any],
    target_home: pathlib.Path,
    extracted_payloads: dict[str, bytes],
    staging_dir: pathlib.Path,
    file_writer: Callable[[pathlib.Path, bytes], None] | None,
) -> dict[str, Any]:
    repo_rel = repo["relative_path"]
    dest_dir = (target_home / repo_rel).resolve()
    if not str(dest_dir).startswith(str(target_home.resolve())):
        raise ArchiveVerificationError("unsafe_repo_path", f"Repo path escapes target: {repo_rel}")
    dest_dir.parent.mkdir(parents=True, exist_ok=True)

    remote_url = validate_credential_free_remote_url(repo["remote_url"])
    remote_name = repo.get("remote_name") or "origin"
    expected_head = repo["head_sha"]
    branch = repo.get("branch")

    # 1. Re-clone from credential-free remote (or init + fetch from bundle if remote is unreachable/local-only).
    clone_proc = subprocess.run(
        ["git", "clone", "--origin", remote_name, remote_url, str(dest_dir)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
        check=False,
    )
    if clone_proc.returncode != 0:
        err = clone_proc.stderr.decode("utf-8", errors="replace").strip()
        raise ArchiveError("repo_clone_failed", f"Failed to clone {remote_url}: {err}")

    # 2. Apply unpushed commits bundle if present (fetch into non-checked-out namespace to avoid ref lock).
    if repo.get("has_bundle") and repo.get("bundle_archive_path"):
        bundle_bytes = extracted_payloads.get(repo["bundle_archive_path"])
        if not bundle_bytes:
            raise ArchiveVerificationError("missing_bundle", f"Missing Git bundle for {repo_rel}")
        bundle_path = staging_dir / f"{repo['repo_id']}.bundle"
        bundle_path.write_bytes(bundle_bytes)
        refspec = f"refs/heads/{branch}:refs/domios-restore/{branch}" if branch else "HEAD:refs/domios-restore/head"
        _run_git(dest_dir, ["fetch", "--force", str(bundle_path), refspec], check=True)

    # 3. Checkout branch and pin exact expected HEAD!
    if branch:
        _run_git(dest_dir, ["checkout", "-B", branch, expected_head], check=True)
    else:
        _run_git(dest_dir, ["checkout", "--detach", expected_head], check=True)

    actual_head = _run_git(dest_dir, ["rev-parse", "--verify", "HEAD"], check=True).stdout.strip()
    if actual_head != expected_head:
        raise ArchiveVerificationError(
            "head_pin_mismatch",
            f"Restored HEAD {actual_head} != expected {expected_head} in {repo_rel}",
        )

    # 4. Reapply staged patch (into index + working tree) and unstaged patch (working tree only).
    if repo.get("has_staged_patch") and repo.get("staged_patch_archive_path"):
        staged_bytes = extracted_payloads.get(repo["staged_patch_archive_path"])
        if staged_bytes is None:
            raise ArchiveVerificationError("missing_staged_patch", f"Missing staged patch for {repo_rel}")
        patch_file = staging_dir / f"{repo['repo_id']}.staged.patch"
        patch_file.write_bytes(staged_bytes)
        _run_git(dest_dir, ["apply", "--index", "--binary", str(patch_file)], check=True)

    if repo.get("has_unstaged_patch") and repo.get("unstaged_patch_archive_path"):
        unstaged_bytes = extracted_payloads.get(repo["unstaged_patch_archive_path"])
        if unstaged_bytes is None:
            raise ArchiveVerificationError("missing_unstaged_patch", f"Missing unstaged patch for {repo_rel}")
        patch_file = staging_dir / f"{repo['repo_id']}.unstaged.patch"
        patch_file.write_bytes(unstaged_bytes)
        _run_git(dest_dir, ["apply", "--binary", str(patch_file)], check=True)

    # 5. Restore untracked user files and verify digests.
    restored_untracked = 0
    for ut in repo.get("untracked_files", []):
        data = extracted_payloads.get(ut["archive_path"])
        if data is None or _sha256_bytes(data) != ut["sha256"]:
            raise ArchiveVerificationError("untracked_checksum_mismatch", f"Corrupt untracked file {ut['relative_path']}")
        _write_file_checked(dest_dir / ut["relative_path"], data, file_writer)
        restored_untracked += 1

    return {
        "repo_id": repo["repo_id"],
        "relative_path": repo_rel,
        "branch": branch,
        "head_sha": actual_head,
        "untracked_restored": restored_untracked,
        "omitted_regenerable": repo.get("omitted_regenerable", []),
        "deferred_dependency_manifests": repo.get("deferred_dependency_manifests", []),
    }


def restore_essentials_archive(
    archive_path: pathlib.Path,
    target_home: pathlib.Path,
    *,
    owner_key: bytes,
    requester_uid: str,
    target_machine_id: str,
    expected_generation: int | None = None,
    max_parallel_clones: int = DEFAULT_MAX_PARALLEL_CLONES,
    file_writer: Callable[[pathlib.Path, bytes], None] | None = None,
) -> RestoreResult:
    """Safely restores essentials and managed repos into a fresh volume before publishing recovery-ready.
    Never deletes or mutates `archive_path` on failure."""
    t_start = _now_ms()
    target_home = target_home.resolve()
    target_home.mkdir(parents=True, exist_ok=True)

    # 1. Verify archive integrity and strict owner/machine/generation binding before touching target disk.
    verified = verify_archive(
        archive_path,
        owner_key=owner_key,
        expected_owner_uid=requester_uid,
        expected_machine_id=target_machine_id,
        expected_generation=expected_generation,
    )
    manifest = verified["manifest"]
    header = verified["header"]
    t_verify = _now_ms()

    # 2. Extract verified files into memory and restore portable essentials first.
    blob = archive_path.read_bytes()
    _, plaintext = decrypt_archive_payload(
        blob,
        owner_key=owner_key,
        expected_owner_uid=requester_uid,
        expected_machine_id=target_machine_id,
        expected_generation=expected_generation,
    )
    extracted_payloads: dict[str, bytes] = {}
    with tarfile.open(fileobj=io.BytesIO(plaintext), mode="r:gz") as tf:
        for member in tf.getmembers():
            if member.isfile():
                extracted_payloads[member.name] = tf.extractfile(member).read()

    files_restored = 0
    bytes_restored = 0
    missing_or_partial: list[str] = []

    for section in ("domios_files", "agent_home_files", "user_files"):
        for entry in manifest.get(section, []):
            rel = entry["relative_path"]
            if is_excluded_secret_or_runtime_path(rel):
                missing_or_partial.append(f"skipped_excluded:{rel}")
                continue
            data = extracted_payloads.get(entry["archive_path"])
            if data is None:
                missing_or_partial.append(f"missing:{rel}")
                continue
            dest = (target_home / rel).resolve()
            if not str(dest).startswith(str(target_home)):
                raise ArchiveVerificationError("unsafe_restore_path", f"Restore path escapes target: {rel}")
            _write_file_checked(dest, data, file_writer)
            files_restored += 1
            bytes_restored += len(data)
    t_extract = _now_ms()

    # 3. Re-clone managed repos with bounded parallelism and reapply bundles/patches/untracked files.
    repos = manifest.get("repos", [])
    repos_restored: list[dict[str, Any]] = []
    deferred_deps: list[dict[str, Any]] = []
    workers = max(1, min(max_parallel_clones, len(repos))) if repos else 1

    with tempfile.TemporaryDirectory(prefix="domios-arc-restore-") as stage_tmp:
        stage_dir = pathlib.Path(stage_tmp)
        if len(repos) <= 1:
            for repo in repos:
                res = _restore_one_repo(repo, target_home, extracted_payloads, stage_dir, file_writer)
                repos_restored.append(res)
                if res["deferred_dependency_manifests"]:
                    deferred_deps.append({
                        "repo": res["relative_path"],
                        "manifests": res["deferred_dependency_manifests"],
                        "omitted_dirs": res["omitted_regenerable"],
                    })
        else:
            with ThreadPoolExecutor(max_workers=workers) as pool:
                futures = {
                    pool.submit(_restore_one_repo, repo, target_home, extracted_payloads, stage_dir, file_writer): repo
                    for repo in repos
                }
                for fut in as_completed(futures):
                    res = fut.result()
                    repos_restored.append(res)
                    if res["deferred_dependency_manifests"]:
                        deferred_deps.append({
                            "repo": res["relative_path"],
                            "manifests": res["deferred_dependency_manifests"],
                            "omitted_dirs": res["omitted_regenerable"],
                        })
    t_repos = _now_ms()

    # Workspace/session identity compatibility check: compatible iff zero missing essentials and all repos pinned.
    identities_compatible = len(missing_or_partial) == 0 and len(repos_restored) == len(repos)
    t_apply = _now_ms()

    return RestoreResult(
        recovery_ready=identities_compatible,
        archive_id=header["archive_id"],
        owner_uid=header["owner_uid"],
        machine_id=header["machine_id"],
        generation=int(header["generation"]),
        bytes_restored=bytes_restored,
        files_restored=files_restored,
        repos_restored=sorted(repos_restored, key=lambda r: r["repo_id"]),
        deferred_dependency_installs=deferred_deps,
        missing_or_partial=missing_or_partial,
        timings_ms={
            "verify_ms": t_verify - t_start,
            "extract_ms": t_extract - t_verify,
            "repos_ms": t_repos - t_extract,
            "apply_ms": t_apply - t_repos,
            "total_ms": t_apply - t_start,
        },
        workspace_identities_compatible=identities_compatible,
    )
