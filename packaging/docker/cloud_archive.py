"""Portable essentials archive (`domios.cloud-archive/1`) for managed cloud machines.

Owns one manifest from verified export through storage release and safe restore:
- Captures Domios desk/sessions/memory/settings, portable agent-home config/history,
  owner data, and repository state (credential-free remote URL + bundle of unpushed commits +
  binary-safe staged/unstaged patches + explicit untracked files + regenerable omission list).
- Strictly excludes provider credentials/OAuth tokens/API keys, SSH/GPG private keys,
  vault/secret stores, running daemon authority/locks, and replayable sends.
- Delegates streaming chunked AES-256-GCM AEAD and signed-URL HTTP transfer to `cloud_archive_crypto`.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
import errno
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from typing import Any, Callable
import urllib.parse


def _load_crypto_module():
    sibling = Path(__file__).with_name("cloud_archive_crypto.py")
    if not sibling.exists():
        sibling = Path("/usr/local/lib/cloud_archive_crypto.py")
    spec = importlib.util.spec_from_file_location("domios_cloud_archive_crypto", sibling)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    spec.loader.exec_module(mod)
    return mod


_crypto = _load_crypto_module()
WIRE_MAGIC = _crypto.WIRE_MAGIC
WIRE_FLAGS_AES_256_GCM = _crypto.WIRE_FLAGS_AES_256_GCM
WIRE_HEADER_STRUCT = _crypto.WIRE_HEADER_STRUCT
WIRE_CHUNK_STRUCT = _crypto.WIRE_CHUNK_STRUCT
DEFAULT_CHUNK_SIZE = _crypto.DEFAULT_CHUNK_SIZE
DEFAULT_MAX_ARCHIVE_BYTES = _crypto.DEFAULT_MAX_ARCHIVE_BYTES
ArchiveError = _crypto.ArchiveError
RestoreDiskFullError = _crypto.RestoreDiskFullError
encrypt_stream_file = _crypto.encrypt_stream_file
decrypt_stream_file = _crypto.decrypt_stream_file
encrypt_archive_bytes = _crypto.encrypt_archive_bytes
decrypt_archive_bytes = _crypto.decrypt_archive_bytes
upload_encrypted_archive = _crypto.upload_encrypted_archive
download_encrypted_archive = _crypto.download_encrypted_archive
_validate_owner_key = _crypto.validate_owner_key

ARCHIVE_FORMAT_VERSION = "domios.cloud-archive/1"
MAX_UNTRACKED_FILE_BYTES = 64 * 1024 * 1024  # 64 MiB single untracked file guard

REGENERABLE_DIR_RULES: dict[str, tuple[str, ...]] = {
    "node_modules": ("package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb"),
    "target": ("Cargo.toml", "Cargo.lock"),
    ".venv": ("pyproject.toml", "requirements.txt", "poetry.lock", "uv.lock", "Pipfile"),
    "venv": ("pyproject.toml", "requirements.txt", "poetry.lock", "uv.lock", "Pipfile"),
    "__pycache__": (),
    ".pytest_cache": (),
    ".mypy_cache": (),
    ".ruff_cache": (),
    ".next": ("package.json",),
    ".nuxt": ("package.json",),
    ".turbo": ("package.json",),
    ".gradle": ("build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"),
    ".cargo": ("Cargo.toml",),
}

EXCLUDED_EXACT_NAMES = frozenset({
    ".secrets", "secrets.json", "credentials.json", ".credentials.json",
    "auth.json", "oauth.json", "oauth_creds.json", "tokens.json", "token.json",
    "api_keys.json", "vault.json", "vault.enc", "vault.db", "keychain.json",
    "id_rsa", "id_ed25519", "id_ecdsa", "id_dsa",
    ".netrc", ".npmrc", ".pypirc", ".git-credentials",
    ".env", ".env.local", ".env.production",
    "daemon.pid", "daemon.lock", "server.lock",
    "mesh-authority.json", "installation-key.json", "installation-key.pem",
    "cloud-binding.json", "held-sends.json", "pending-sends.json",
    "send-ledger.json", "wake-queue.json", "cloud_wake_queue.json",
})

EXCLUDED_PATH_SEGMENTS = frozenset({
    ".ssh", ".gnupg", ".aws", ".azure", ".kube", "neighbor-keys",
    "vault", "credentials", "oauth", "held_sends", "pending_sends",
    "replay_sends", "runtime-locks", "sockets", ".Trash", "cache", ".cache",
})

EXCLUDED_SUFFIXES = (".pem", ".key", ".p12", ".pfx", ".jks", ".kdbx", ".sock", ".pid", ".lock")


@dataclass(frozen=True)
class RepoArchiveEntry:
    rel_path: str
    remote_name: str | None
    remote_url: str | None
    branch: str | None
    head_sha: str | None
    bundle_member: str | None
    unpushed_commits: int
    staged_patch_member: str | None
    unstaged_patch_member: str | None
    untracked_files: list[str]
    omitted_regenerable_dirs: list[str]
    reinstall_manifests: list[str]


@dataclass(frozen=True)
class ArchivePhaseTimingsMs:
    inventory_ms: int
    repos_ms: int
    pack_ms: int
    encrypt_ms: int
    verify_ms: int
    total_ms: int


@dataclass(frozen=True)
class RestorePhaseTimingsMs:
    verify_ms: int
    extract_ms: int
    repos_ms: int
    apply_ms: int
    total_ms: int


@dataclass(frozen=True)
class ArchiveExportResult:
    format_version: str
    archive_id: str
    owner_uid: str
    machine_id: str
    generation: int
    revision: int
    created_at: int
    archive_path: str
    archive_bytes: int
    uncompressed_bytes: int
    manifest_sha256: str
    ciphertext_sha256: str
    excluded_secrets_verified: bool
    files_archived: int
    excluded_secret_paths: list[str]
    omitted_regenerable_dirs: list[str]
    repos: list[RepoArchiveEntry]
    timings_ms: ArchivePhaseTimingsMs


@dataclass(frozen=True)
class ArchiveRestoreResult:
    archive_id: str
    owner_uid: str
    machine_id: str
    generation: int
    files_restored: int
    bytes_restored: int
    repos_restored: int
    deferred_installs: list[dict[str, Any]]
    missing_or_partial: list[str]
    recovery_ready: bool
    timings_ms: RestorePhaseTimingsMs


def is_excluded_secret_path(rel_posix: str) -> bool:
    parts = [p for p in rel_posix.split("/") if p and p != "."]
    if not parts:
        return False
    name = parts[-1]
    if name in EXCLUDED_EXACT_NAMES:
        return True
    if any(seg in EXCLUDED_PATH_SEGMENTS for seg in parts):
        return True
    lower = name.lower()
    if any(lower.endswith(suf) for suf in EXCLUDED_SUFFIXES):
        return True
    if any(tok in lower for tok in ("oauth", "credential", "secret", "auth_token")):
        if lower.endswith((".json", ".toml", ".yaml", ".yml", ".txt", ".db", ".sqlite", ".enc")):
            return True
    return False


def _has_embedded_credentials(remote_url: str) -> bool:
    if not remote_url or "://" not in remote_url:
        return False
    parsed = urllib.parse.urlsplit(remote_url)
    return bool(parsed.username or parsed.password)


def _is_regenerable_dir(dir_path: Path) -> bool:
    name = dir_path.name
    if name not in REGENERABLE_DIR_RULES:
        return False
    markers = REGENERABLE_DIR_RULES[name]
    if not markers:
        return True
    return any((dir_path.parent / marker).exists() for marker in markers)


def _run_git(repo_dir: Path, args: list[str], *, check: bool = True, binary: bool = False) -> subprocess.CompletedProcess:
    proc = subprocess.run(["git", *args], cwd=str(repo_dir), capture_output=True, check=False)
    if check and proc.returncode != 0:
        err = proc.stderr.decode("utf-8", errors="replace").strip()
        raise ArchiveError("git_failed", f"git {' '.join(args)} failed in {repo_dir.name}: {err}")
    if not binary:
        proc.stdout = proc.stdout.decode("utf-8", errors="replace")
        proc.stderr = proc.stderr.decode("utf-8", errors="replace")
    return proc


def _discover_git_repos(home_dir: Path) -> list[Path]:
    repos: list[Path] = []
    for root, dirs, _files in os.walk(home_dir, followlinks=False):
        root_path = Path(root)
        rel = root_path.relative_to(home_dir).as_posix()
        if rel != "." and is_excluded_secret_path(rel):
            dirs[:] = []
            continue
        if ".git" in dirs:
            repos.append(root_path)
            dirs[:] = []
            continue
        dirs[:] = [
            d for d in sorted(dirs)
            if not is_excluded_secret_path(f"{rel}/{d}" if rel != "." else d)
            and not _is_regenerable_dir(root_path / d)
        ]
    return sorted(repos)


def _export_one_repo(repo_dir: Path, home_dir: Path, staging_dir: Path, index: int) -> RepoArchiveEntry:
    rel_path = repo_dir.relative_to(home_dir).as_posix()
    repo_stage = staging_dir / f"repo-{index:03d}"
    repo_stage.mkdir(parents=True, exist_ok=True)

    head_proc = _run_git(repo_dir, ["rev-parse", "HEAD"], check=False)
    head_sha = head_proc.stdout.strip() if head_proc.returncode == 0 else None
    branch_proc = _run_git(repo_dir, ["symbolic-ref", "--short", "HEAD"], check=False)
    branch = branch_proc.stdout.strip() if branch_proc.returncode == 0 else None

    remotes_proc = _run_git(repo_dir, ["remote"], check=False)
    remote_names = [r.strip() for r in remotes_proc.stdout.splitlines() if r.strip()]
    remote_name = "origin" if "origin" in remote_names else (remote_names[0] if remote_names else None)
    remote_url: str | None = None
    if remote_name:
        url_proc = _run_git(repo_dir, ["remote", "get-url", remote_name], check=False)
        if url_proc.returncode == 0:
            candidate = url_proc.stdout.strip()
            if _has_embedded_credentials(candidate):
                raise ArchiveError(
                    "repo_remote_has_credentials",
                    f"Refusing export: repository {rel_path} remote {remote_name} embeds credentials",
                )
            remote_url = candidate

    bundle_member: str | None = None
    unpushed_commits = 0
    if head_sha:
        upstream_proc = _run_git(repo_dir, ["rev-parse", "--abbrev-ref", "@{upstream}"], check=False)
        if upstream_proc.returncode == 0 and upstream_proc.stdout.strip():
            upstream = upstream_proc.stdout.strip()
            count_proc = _run_git(repo_dir, ["rev-list", "--count", f"{upstream}..HEAD"], check=False)
            unpushed_commits = int(count_proc.stdout.strip() or "0") if count_proc.returncode == 0 else 0
            if unpushed_commits > 0:
                bundle_file = repo_stage / "unpushed.bundle"
                _run_git(repo_dir, ["bundle", "create", str(bundle_file), f"{upstream}..HEAD"], binary=True)
                bundle_member = f"repos/repo-{index:03d}/unpushed.bundle"
        else:
            count_proc = _run_git(repo_dir, ["rev-list", "--count", "HEAD"], check=False)
            unpushed_commits = int(count_proc.stdout.strip() or "0") if count_proc.returncode == 0 else 0
            if unpushed_commits > 0:
                bundle_file = repo_stage / "full.bundle"
                _run_git(repo_dir, ["bundle", "create", str(bundle_file), "--all"], binary=True)
                bundle_member = f"repos/repo-{index:03d}/full.bundle"

    staged_proc = _run_git(repo_dir, ["diff", "--cached", "--binary"], binary=True)
    staged_patch_member: str | None = None
    if staged_proc.stdout:
        (repo_stage / "staged.patch").write_bytes(staged_proc.stdout)
        staged_patch_member = f"repos/repo-{index:03d}/staged.patch"

    unstaged_proc = _run_git(repo_dir, ["diff", "--binary"], binary=True)
    unstaged_patch_member: str | None = None
    if unstaged_proc.stdout:
        (repo_stage / "unstaged.patch").write_bytes(unstaged_proc.stdout)
        unstaged_patch_member = f"repos/repo-{index:03d}/unstaged.patch"

    untracked_proc = _run_git(repo_dir, ["ls-files", "--others", "--exclude-standard", "-z"], binary=True)
    untracked_raw = [p.decode("utf-8", errors="surrogateescape") for p in untracked_proc.stdout.split(b"\x00") if p]
    untracked_files: list[str] = []
    untracked_dir = repo_stage / "untracked"
    for rel_u in sorted(untracked_raw):
        if is_excluded_secret_path(rel_u):
            continue
        src = repo_dir / rel_u
        if any(_is_regenerable_dir(parent) for parent in src.parents if parent != repo_dir and repo_dir in parent.parents):
            continue
        if not src.exists() or src.is_symlink() or not src.is_file():
            continue
        if src.lstat().st_size > MAX_UNTRACKED_FILE_BYTES:
            raise ArchiveError("untracked_file_too_large", f"Untracked file exceeds cap: {rel_path}/{rel_u}")
        dst = untracked_dir / rel_u
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
        untracked_files.append(rel_u)

    omitted_dirs: list[str] = []
    for root, dirs, _files in os.walk(repo_dir, followlinks=False):
        root_p = Path(root)
        if ".git" in dirs:
            dirs.remove(".git")
        kept: list[str] = []
        for d in sorted(dirs):
            cand = root_p / d
            if _is_regenerable_dir(cand):
                omitted_dirs.append(cand.relative_to(home_dir).as_posix())
            else:
                kept.append(d)
        dirs[:] = kept

    manifests = [
        marker for marker in ("package.json", "pnpm-lock.yaml", "package-lock.json", "Cargo.toml", "pyproject.toml", "requirements.txt")
        if (repo_dir / marker).exists()
    ]
    return RepoArchiveEntry(
        rel_path=rel_path, remote_name=remote_name, remote_url=remote_url,
        branch=branch, head_sha=head_sha, bundle_member=bundle_member,
        unpushed_commits=unpushed_commits, staged_patch_member=staged_patch_member,
        unstaged_patch_member=unstaged_patch_member, untracked_files=untracked_files,
        omitted_regenerable_dirs=sorted(omitted_dirs), reinstall_manifests=sorted(manifests),
    )


def _collect_essential_files(home_dir: Path, repo_paths: set[Path]) -> tuple[list[tuple[Path, str, int]], list[str], list[str]]:
    files: list[tuple[Path, str, int]] = []
    excluded_secrets: list[str] = []
    omitted_regen: list[str] = []

    for root, dirs, filenames in os.walk(home_dir, followlinks=False):
        root_p = Path(root)
        if root_p in repo_paths:
            dirs[:] = []
            continue
        rel_root = root_p.relative_to(home_dir).as_posix()
        kept_dirs: list[str] = []
        for d in sorted(dirs):
            cand = root_p / d
            cand_rel = f"{rel_root}/{d}" if rel_root != "." else d
            if cand in repo_paths:
                continue
            if is_excluded_secret_path(cand_rel):
                excluded_secrets.append(cand_rel)
                continue
            if _is_regenerable_dir(cand):
                omitted_regen.append(cand_rel)
                continue
            kept_dirs.append(d)
        dirs[:] = kept_dirs

        for fname in sorted(filenames):
            fpath = root_p / fname
            rel_posix = f"{rel_root}/{fname}" if rel_root != "." else fname
            if is_excluded_secret_path(rel_posix):
                excluded_secrets.append(rel_posix)
                continue
            try:
                st = fpath.lstat()
            except OSError as exc:
                raise ArchiveError("unreadable_path", f"Cannot stat {rel_posix}: {exc}") from exc
            if stat.S_ISLNK(st.st_mode) or not stat.S_ISREG(st.st_mode):
                continue
            if not os.access(fpath, os.R_OK):
                raise ArchiveError("unreadable_path", f"Path is unreadable: {rel_posix}")
            files.append((fpath, rel_posix, st.st_size))

    return files, sorted(set(excluded_secrets)), sorted(set(omitted_regen))


def verify_archive(
    archive_path: Path,
    *,
    owner_key: bytes,
    expected_owner_uid: str,
    expected_machine_id: str,
    expected_generation: int,
    expected_ciphertext_sha256: str | None = None,
) -> dict[str, Any]:
    ct_bytes, pt_bytes, digest = decrypt_stream_file(
        Path(archive_path), None,
        owner_key=owner_key, owner_uid=expected_owner_uid,
        machine_id=expected_machine_id, generation=expected_generation,
        expected_sha256=expected_ciphertext_sha256,
    )
    return {"verified": True, "archive_bytes": ct_bytes, "compressed_payload_bytes": pt_bytes, "ciphertext_sha256": digest}


def export_essentials_archive(
    home_dir: Path,
    output_archive_path: Path,
    *,
    archive_id: str,
    owner_uid: str,
    machine_id: str,
    generation: int,
    owner_key: bytes,
    revision: int = 1,
    max_archive_bytes: int = DEFAULT_MAX_ARCHIVE_BYTES,
    chunk_size: int = DEFAULT_CHUNK_SIZE,
    generation_fence_check: Callable[[], bool] | None = None,
    now_s: Callable[[], int] = lambda: int(time.time()),
) -> ArchiveExportResult:
    _validate_owner_key(owner_key)
    if generation_fence_check is not None and not generation_fence_check():
        raise ArchiveError("stale_generation", "Competing wake or stale generation detected before export")
    home_dir = Path(home_dir).resolve()
    output_archive_path = Path(output_archive_path).resolve()
    t0 = time.monotonic()
    if not home_dir.is_dir():
        raise ArchiveError("invalid_home", f"Home directory does not exist: {home_dir}")

    t_inv0 = time.monotonic()
    repo_list = _discover_git_repos(home_dir)
    essential_files, excluded_secrets, omitted_regen = _collect_essential_files(home_dir, set(repo_list))
    inventory_ms = int((time.monotonic() - t_inv0) * 1000)

    with tempfile.TemporaryDirectory(prefix="domios-archive-export-") as tmp_str:
        tmp_dir = Path(tmp_str)
        repos_stage = tmp_dir / "repos"
        repos_stage.mkdir(parents=True, exist_ok=True)

        t_rep0 = time.monotonic()
        repo_entries: list[RepoArchiveEntry] = []
        for idx, rpath in enumerate(repo_list):
            entry = _export_one_repo(rpath, home_dir, repos_stage, idx)
            repo_entries.append(entry)
            omitted_regen.extend(entry.omitted_regenerable_dirs)
        repos_ms = int((time.monotonic() - t_rep0) * 1000)

        omitted_regen = sorted(set(omitted_regen))
        created_at = now_s()
        uncompressed_bytes = sum(sz for _, _, sz in essential_files) + sum(
            p.stat().st_size for p in repos_stage.rglob("*") if p.is_file()
        )
        manifest_obj: dict[str, Any] = {
            "format_version": ARCHIVE_FORMAT_VERSION,
            "crypto": {"scheme": "aes-256-gcm-chunked-v2", "chunk_size": chunk_size},
            "archive_id": archive_id, "owner_uid": owner_uid, "machine_id": machine_id,
            "generation": generation, "revision": revision, "created_at": created_at,
            "excluded_secrets_verified": True,
            "files": [{"rel_path": rel_posix, "size": size} for _, rel_posix, size in essential_files],
            "repos": [asdict(r) for r in repo_entries],
            "excluded_secret_paths": excluded_secrets,
            "omitted_regenerable_dirs": omitted_regen,
            "uncompressed_bytes": uncompressed_bytes,
        }
        manifest_bytes = json.dumps(manifest_obj, sort_keys=True, separators=(",", ":")).encode("utf-8")
        manifest_sha256 = hashlib.sha256(manifest_bytes).hexdigest()

        t_pack0 = time.monotonic()
        manifest_file = tmp_dir / "manifest.json"
        manifest_file.write_bytes(manifest_bytes)
        tar_gz_path = tmp_dir / "payload.tar.gz"
        with gzip.open(tar_gz_path, mode="wb", compresslevel=1) as gz_out:
            with tarfile.open(fileobj=gz_out, mode="w|") as tf:
                tf.add(manifest_file, arcname="manifest.json", recursive=False)
                for fpath, rel_posix, _ in essential_files:
                    if is_excluded_secret_path(rel_posix):
                        raise ArchiveError("secret_leak_guard", f"Secret file reached pack stage: {rel_posix}")
                    tf.add(fpath, arcname=f"files/{rel_posix}", recursive=False)
                for p in sorted(repos_stage.rglob("*")):
                    if p.is_file():
                        tf.add(p, arcname="repos/" + p.relative_to(repos_stage).as_posix(), recursive=False)
        pack_ms = int((time.monotonic() - t_pack0) * 1000)

        t_enc0 = time.monotonic()
        output_archive_path.parent.mkdir(parents=True, exist_ok=True)
        tmp_out = output_archive_path.with_suffix(output_archive_path.suffix + ".tmp")
        try:
            ct_len, ciphertext_sha256 = encrypt_stream_file(
                tar_gz_path, tmp_out,
                owner_key=owner_key, owner_uid=owner_uid, machine_id=machine_id,
                generation=generation, chunk_size=chunk_size, max_archive_bytes=max_archive_bytes,
            )
            os.replace(tmp_out, output_archive_path)
        finally:
            tmp_out.unlink(missing_ok=True)
        encrypt_ms = int((time.monotonic() - t_enc0) * 1000)

        t_ver0 = time.monotonic()
        verified_gz = tmp_dir / "verified.tar.gz"
        ver_ct_len, _, ver_digest = decrypt_stream_file(
            output_archive_path, verified_gz,
            owner_key=owner_key, owner_uid=owner_uid, machine_id=machine_id,
            generation=generation, expected_sha256=ciphertext_sha256,
        )
        if ver_ct_len != ct_len or ver_digest != ciphertext_sha256:
            raise ArchiveError("verify_failed", "Read-back verification digest/length mismatch")
        found_manifest = False
        with gzip.open(verified_gz, mode="rb") as gz_in:
            with tarfile.open(fileobj=gz_in, mode="r|") as tf:
                for member in tf:
                    if member.name == "manifest.json":
                        stream = tf.extractfile(member)
                        if stream is None or hashlib.sha256(stream.read()).hexdigest() != manifest_sha256:
                            raise ArchiveError("verify_failed", "manifest.json mismatch on verify")
                        found_manifest = True
                    elif member.name.startswith("files/") and is_excluded_secret_path(member.name[6:]):
                        raise ArchiveError("secret_leak_guard", f"Excluded secret detected during verify: {member.name[6:]}")
        if not found_manifest:
            raise ArchiveError("verify_failed", "Verified archive missing manifest.json")
        verify_ms = int((time.monotonic() - t_ver0) * 1000)

    return ArchiveExportResult(
        format_version=ARCHIVE_FORMAT_VERSION, archive_id=archive_id, owner_uid=owner_uid,
        machine_id=machine_id, generation=generation, revision=revision, created_at=created_at,
        archive_path=str(output_archive_path), archive_bytes=ct_len, uncompressed_bytes=uncompressed_bytes,
        manifest_sha256=manifest_sha256, ciphertext_sha256=ciphertext_sha256,
        excluded_secrets_verified=True, files_archived=len(essential_files),
        excluded_secret_paths=excluded_secrets, omitted_regenerable_dirs=omitted_regen,
        repos=repo_entries,
        timings_ms=ArchivePhaseTimingsMs(
            inventory_ms=inventory_ms, repos_ms=repos_ms, pack_ms=pack_ms,
            encrypt_ms=encrypt_ms, verify_ms=verify_ms, total_ms=int((time.monotonic() - t0) * 1000),
        ),
    )


def _safe_extract_target(base_dir: Path, rel_posix: str) -> Path:
    parts = [p for p in rel_posix.split("/") if p]
    if not parts or any(p in (".", "..") for p in parts) or rel_posix.startswith("/"):
        raise ArchiveError("unsafe_path", f"Refusing unsafe archive member path: {rel_posix}")
    target = (base_dir / Path(*parts)).resolve()
    base_resolved = base_dir.resolve()
    if base_resolved != target and base_resolved not in target.parents:
        raise ArchiveError("unsafe_path", f"Archive member escapes target directory: {rel_posix}")
    return target


def _restore_one_repo(
    repo_meta: dict[str, Any],
    target_home: Path,
    extracted_root: Path,
    missing_or_partial: list[str],
    deferred_installs: list[dict[str, Any]],
) -> bool:
    rel_path = repo_meta["rel_path"]
    dest = _safe_extract_target(target_home, rel_path)
    dest.parent.mkdir(parents=True, exist_ok=True)
    remote_url = repo_meta.get("remote_url")
    remote_name = repo_meta.get("remote_name") or "origin"
    branch = repo_meta.get("branch")
    head_sha = repo_meta.get("head_sha")
    bundle_member = repo_meta.get("bundle_member")

    if remote_url and _has_embedded_credentials(remote_url):
        raise ArchiveError("repo_remote_has_credentials", f"Refusing credential-bearing remote on restore: {rel_path}")

    cloned = False
    if remote_url:
        clone_proc = subprocess.run(["git", "clone", "--no-checkout", remote_url, str(dest)], capture_output=True, check=False)
        if clone_proc.returncode == 0:
            cloned = True
            if remote_name != "origin":
                _run_git(dest, ["remote", "rename", "origin", remote_name], check=False)
        else:
            missing_or_partial.append(f"repo_clone_failed:{rel_path}")

    if not cloned:
        dest.mkdir(parents=True, exist_ok=True)
        _run_git(dest, ["init"])
        if remote_url:
            _run_git(dest, ["remote", "add", remote_name, remote_url], check=False)

    if bundle_member:
        bundle_path = extracted_root / bundle_member
        if bundle_path.exists():
            ref_target = f"refs/domios-restore/{branch}" if branch else "refs/domios-restore/head"
            _run_git(dest, ["fetch", str(bundle_path), f"HEAD:{ref_target}"], check=False)
            _run_git(dest, ["fetch", str(bundle_path), "refs/heads/*:refs/domios-restore/heads/*"], check=False)
        else:
            missing_or_partial.append(f"repo_bundle_missing:{rel_path}")

    if head_sha:
        chk = _run_git(dest, ["checkout", "-B", branch, head_sha] if branch else ["checkout", "--detach", head_sha], check=False)
        if chk.returncode != 0:
            missing_or_partial.append(f"repo_head_missing:{rel_path}:{head_sha}")
            return False

    staged_member = repo_meta.get("staged_patch_member")
    if staged_member and (extracted_root / staged_member).exists():
        if _run_git(dest, ["apply", "--index", "--binary", str(extracted_root / staged_member)], check=False).returncode != 0:
            missing_or_partial.append(f"repo_staged_patch_failed:{rel_path}")

    unstaged_member = repo_meta.get("unstaged_patch_member")
    if unstaged_member and (extracted_root / unstaged_member).exists():
        if _run_git(dest, ["apply", "--binary", str(extracted_root / unstaged_member)], check=False).returncode != 0:
            missing_or_partial.append(f"repo_unstaged_patch_failed:{rel_path}")

    repo_prefix = Path(bundle_member or staged_member or unstaged_member or "").parent
    if not str(repo_prefix) or str(repo_prefix) == ".":
        for cand in sorted((extracted_root / "repos").glob("repo-*")) if (extracted_root / "repos").exists() else []:
            if (cand / "untracked").exists():
                repo_prefix = cand.relative_to(extracted_root)
                break
    untracked_src_root = extracted_root / repo_prefix / "untracked"
    for rel_u in repo_meta.get("untracked_files", []):
        if is_excluded_secret_path(rel_u):
            continue
        src = untracked_src_root / rel_u
        if src.exists() and src.is_file():
            dst = _safe_extract_target(dest, rel_u)
            dst.parent.mkdir(parents=True, exist_ok=True)
            try:
                shutil.copy2(src, dst)
            except OSError as exc:
                if exc.errno == errno.ENOSPC:
                    raise RestoreDiskFullError() from exc
                raise

    if repo_meta.get("omitted_regenerable_dirs"):
        deferred_installs.append({
            "repo": rel_path,
            "omitted_dirs": repo_meta["omitted_regenerable_dirs"],
            "manifests": repo_meta.get("reinstall_manifests", []),
            "status": "deferred_post_ready",
        })
    return True


def restore_essentials_archive(
    archive_path: Path,
    target_home: Path,
    *,
    owner_key: bytes,
    requester_uid: str,
    target_machine_id: str,
    expected_generation: int | None = None,
    expected_ciphertext_sha256: str | None = None,
    simulated_disk_full_after_bytes: int | None = None,
) -> ArchiveRestoreResult:
    _validate_owner_key(owner_key)
    archive_path = Path(archive_path).resolve()
    target_home = Path(target_home).resolve()
    target_home.mkdir(parents=True, exist_ok=True)
    t0 = time.monotonic()
    if not archive_path.is_file():
        raise ArchiveError("archive_missing", f"Archive file not found: {archive_path}")

    t_ver0 = time.monotonic()
    gen = expected_generation if expected_generation is not None else 1
    with tempfile.TemporaryDirectory(prefix="domios-archive-restore-") as tmp_str:
        tmp_dir = Path(tmp_str)
        decrypted_gz = tmp_dir / "decrypted.tar.gz"
        try:
            decrypt_stream_file(
                archive_path, decrypted_gz,
                owner_key=owner_key, owner_uid=requester_uid,
                machine_id=target_machine_id, generation=gen,
                expected_sha256=expected_ciphertext_sha256,
            )
        except OSError as exc:
            if exc.errno == errno.ENOSPC:
                raise RestoreDiskFullError() from exc
            raise
        verify_ms = int((time.monotonic() - t_ver0) * 1000)

        t_ext0 = time.monotonic()
        extracted_repos = tmp_dir / "extracted"
        extracted_repos.mkdir(parents=True, exist_ok=True)
        manifest: dict[str, Any] | None = None
        files_restored = 0
        bytes_restored = 0

        with gzip.open(decrypted_gz, mode="rb") as gz_in:
            with tarfile.open(fileobj=gz_in, mode="r|") as tf:
                for member in tf:
                    if not member.isfile():
                        continue
                    stream = tf.extractfile(member)
                    if stream is None:
                        continue
                    if member.name == "manifest.json":
                        manifest = json.loads(stream.read().decode("utf-8"))
                        if manifest.get("format_version") != ARCHIVE_FORMAT_VERSION:
                            raise ArchiveError("unsupported_format", "Unsupported archive format_version")
                        if manifest.get("owner_uid") != requester_uid:
                            raise ArchiveError("owner_mismatch", "Archive owner_uid does not match requester")
                        if manifest.get("machine_id") != target_machine_id:
                            raise ArchiveError("machine_mismatch", "Archive machine_id does not match target")
                        if expected_generation is not None and manifest.get("generation") != expected_generation:
                            raise ArchiveError("generation_mismatch", "Archive generation mismatch")
                        if manifest.get("excluded_secrets_verified") is not True:
                            raise ArchiveError("secrets_not_verified", "Archive lacks excluded_secrets_verified")
                    elif member.name.startswith("files/"):
                        rel_posix = member.name[6:]
                        if is_excluded_secret_path(rel_posix):
                            raise ArchiveError("secret_leak_guard", f"Refusing to restore excluded secret: {rel_posix}")
                        dst = _safe_extract_target(target_home, rel_posix)
                        dst.parent.mkdir(parents=True, exist_ok=True)
                        try:
                            with dst.open("wb") as out_f:
                                while True:
                                    chunk = stream.read(DEFAULT_CHUNK_SIZE)
                                    if not chunk:
                                        break
                                    bytes_restored += len(chunk)
                                    if simulated_disk_full_after_bytes is not None and bytes_restored > simulated_disk_full_after_bytes:
                                        raise RestoreDiskFullError("Simulated ENOSPC during archive restore")
                                    out_f.write(chunk)
                        except OSError as exc:
                            if exc.errno == errno.ENOSPC:
                                raise RestoreDiskFullError() from exc
                            raise
                        files_restored += 1
                    elif member.name.startswith("repos/"):
                        dst = _safe_extract_target(extracted_repos, member.name)
                        dst.parent.mkdir(parents=True, exist_ok=True)
                        try:
                            with dst.open("wb") as out_f:
                                shutil.copyfileobj(stream, out_f, length=DEFAULT_CHUNK_SIZE)
                        except OSError as exc:
                            if exc.errno == errno.ENOSPC:
                                raise RestoreDiskFullError() from exc
                            raise
        if manifest is None:
            raise ArchiveError("corrupt_archive", "Archive missing manifest.json")
        extract_ms = int((time.monotonic() - t_ext0) * 1000)

        t_rep0 = time.monotonic()
        missing_or_partial: list[str] = []
        deferred_installs: list[dict[str, Any]] = []
        repos_restored = sum(
            1 for repo_meta in manifest.get("repos", [])
            if _restore_one_repo(repo_meta, target_home, extracted_repos, missing_or_partial, deferred_installs)
        )
        repos_ms = int((time.monotonic() - t_rep0) * 1000)

        t_app0 = time.monotonic()
        if deferred_installs:
            marker_dir = target_home / ".domios-restore"
            marker_dir.mkdir(parents=True, exist_ok=True)
            (marker_dir / "deferred-installs.json").write_text(json.dumps(deferred_installs, indent=2), encoding="utf-8")
        apply_ms = int((time.monotonic() - t_app0) * 1000)

    return ArchiveRestoreResult(
        archive_id=manifest["archive_id"], owner_uid=manifest["owner_uid"],
        machine_id=manifest["machine_id"], generation=int(manifest["generation"]),
        files_restored=files_restored, bytes_restored=bytes_restored,
        repos_restored=repos_restored, deferred_installs=deferred_installs,
        missing_or_partial=missing_or_partial, recovery_ready=len(missing_or_partial) == 0,
        timings_ms=RestorePhaseTimingsMs(
            verify_ms=verify_ms, extract_ms=extract_ms, repos_ms=repos_ms,
            apply_ms=apply_ms, total_ms=int((time.monotonic() - t0) * 1000),
        ),
    )


def _cli_export_and_upload() -> int:
    raw = sys.stdin.read(65536)
    req = json.loads(raw) if raw.strip() else {}
    key_hex = os.environ.pop("DOMIOS_EXPORT_ARCHIVE_KEY_HEX", None)
    if not key_hex or len(key_hex.strip()) != 64:
        raise ArchiveError("missing_key", "DOMIOS_EXPORT_ARCHIVE_KEY_HEX must be 64 hex chars")
    owner_key = bytes.fromhex(key_hex.strip())
    home_dir = Path(req.get("home_dir") or os.environ.get("HOME") or "/var/lib/domios-cloud")
    archive_id = str(req["archive_id"])
    with tempfile.TemporaryDirectory(prefix="domios-export-run-") as tmp:
        out_path = Path(tmp) / f"{archive_id}.enc"
        result = export_essentials_archive(
            home_dir, out_path,
            archive_id=archive_id, owner_uid=str(req["owner_uid"]), machine_id=str(req["machine_id"]),
            generation=int(req.get("generation", 1)), owner_key=owner_key,
            revision=int(req.get("revision", 1)),
            max_archive_bytes=int(req.get("max_archive_bytes", DEFAULT_MAX_ARCHIVE_BYTES)),
        )
        upload_receipt = (
            upload_encrypted_archive(out_path, str(req["upload_url"]), extra_headers=req.get("upload_headers") or None)
            if req.get("upload_url") else None
        )
    payload = asdict(result)
    payload["archive_path"] = ""
    if upload_receipt is not None:
        payload["upload"] = upload_receipt
    print(json.dumps(payload, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        if sys.argv[1:] != ["export-and-upload"]:
            raise ArchiveError("invalid_args", "Supported subcommand: export-and-upload")
        sys.exit(_cli_export_and_upload())
    except ArchiveError as err:
        print(json.dumps({"ok": False, "code": err.code, "message": str(err)}), file=sys.stderr)
        sys.exit(2)
    except Exception as err:
        print(json.dumps({"ok": False, "code": "internal", "message": type(err).__name__}), file=sys.stderr)
        sys.exit(1)
