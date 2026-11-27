"""Streaming chunked AES-256-GCM AEAD encryption and single-object signed-URL HTTP transfer
for Domios portable essentials archives (`domios.cloud-archive/1`).

Wire format (`DMCA2\\x00`):
- Header (20 bytes): magic(6) | flags(2, 0x0001=AES-256-GCM) | chunk_size(4) | nonce_prefix(8)
- Per chunk: ciphertext_len(4) | is_final(1) | ciphertext_with_16B_gcm_tag
- Per-chunk 96-bit nonce: `nonce_prefix (8B) || uint32_be(chunk_index)`
- Per-chunk AAD: `domios.cloud-archive.v2|{owner_uid}|{machine_id}|{generation}|{nonce_prefix}|{chunk_index}|{is_final}`
"""

from __future__ import annotations

import errno
import hashlib
import http.client
import os
from pathlib import Path
import struct
import tempfile
from typing import Any
import urllib.parse

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

WIRE_MAGIC = b"DMCA2\x00"
WIRE_FLAGS_AES_256_GCM = 0x0001
WIRE_HEADER_STRUCT = struct.Struct(">6sHI8s")
WIRE_CHUNK_STRUCT = struct.Struct(">IB")
DEFAULT_CHUNK_SIZE = 1024 * 1024  # 1 MiB streaming chunks
DEFAULT_MAX_ARCHIVE_BYTES = 512 * 1024 * 1024  # 512 MiB
GCM_TAG_BYTES = 16


class ArchiveError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


class RestoreDiskFullError(ArchiveError):
    """Raised on ENOSPC during restore; signals caller to retain encrypted archive intact."""

    def __init__(self, message: str = "Insufficient disk space during cold restore"):
        super().__init__("disk_full_local", message)


def redact_url(url: str) -> str:
    try:
        parsed = urllib.parse.urlsplit(url)
        host = parsed.hostname or ""
        port = f":{parsed.port}" if parsed.port else ""
        return f"{parsed.scheme}://{host}{port}{parsed.path}"
    except Exception:
        return "[REDACTED URL]"


def _chunk_aad(
    owner_uid: str,
    machine_id: str,
    generation: int,
    nonce_prefix: bytes,
    chunk_index: int,
    is_final: bool,
) -> bytes:
    return (
        f"domios.cloud-archive.v2|{owner_uid}|{machine_id}|{generation}|".encode("utf-8")
        + nonce_prefix
        + b"|"
        + struct.pack(">IB", chunk_index, 1 if is_final else 0)
    )


def _chunk_nonce(nonce_prefix: bytes, chunk_index: int) -> bytes:
    if len(nonce_prefix) != 8:
        raise ArchiveError("invalid_header", "nonce_prefix must be 8 bytes")
    return nonce_prefix + struct.pack(">I", chunk_index)


def validate_owner_key(owner_key: bytes) -> bytes:
    if not isinstance(owner_key, (bytes, bytearray)) or len(owner_key) != 32:
        raise ArchiveError("invalid_key", "Archive key must be exactly 32 bytes (AES-256-GCM)")
    return bytes(owner_key)


def encrypt_stream_file(
    plaintext_path: Path,
    ciphertext_path: Path,
    *,
    owner_key: bytes,
    owner_uid: str,
    machine_id: str,
    generation: int,
    chunk_size: int = DEFAULT_CHUNK_SIZE,
    max_archive_bytes: int = DEFAULT_MAX_ARCHIVE_BYTES,
) -> tuple[int, str]:
    """Streams `plaintext_path` into chunked AES-256-GCM `ciphertext_path`, returning (bytes, sha256)."""
    key = validate_owner_key(owner_key)
    if chunk_size < 4096 or chunk_size > 16 * 1024 * 1024:
        raise ArchiveError("invalid_chunk_size", f"Unsupported chunk_size: {chunk_size}")
    aead = AESGCM(key)
    nonce_prefix = os.urandom(8)
    header = WIRE_HEADER_STRUCT.pack(WIRE_MAGIC, WIRE_FLAGS_AES_256_GCM, chunk_size, nonce_prefix)
    sha = hashlib.sha256()
    total_written = 0

    with plaintext_path.open("rb") as src, ciphertext_path.open("wb") as dst:
        dst.write(header)
        sha.update(header)
        total_written += len(header)

        chunk_index = 0
        pending = src.read(chunk_size)
        while True:
            nxt = src.read(chunk_size)
            is_final = len(nxt) == 0
            nonce = _chunk_nonce(nonce_prefix, chunk_index)
            aad = _chunk_aad(owner_uid, machine_id, generation, nonce_prefix, chunk_index, is_final)
            ct = aead.encrypt(nonce, pending, aad)
            frame_hdr = WIRE_CHUNK_STRUCT.pack(len(ct), 1 if is_final else 0)
            total_written += len(frame_hdr) + len(ct)
            if total_written > max_archive_bytes:
                raise ArchiveError(
                    "archive_size_exceeded",
                    f"Encrypted archive exceeds limit {max_archive_bytes} bytes",
                )
            dst.write(frame_hdr)
            dst.write(ct)
            sha.update(frame_hdr)
            sha.update(ct)
            if is_final:
                break
            pending = nxt
            chunk_index += 1
        dst.flush()
        os.fsync(dst.fileno())
    return total_written, sha.hexdigest()


def decrypt_stream_file(
    ciphertext_path: Path,
    plaintext_path: Path | None,
    *,
    owner_key: bytes,
    owner_uid: str,
    machine_id: str,
    generation: int,
    expected_sha256: str | None = None,
) -> tuple[int, int, str]:
    """Streams and verifies chunked AES-256-GCM ciphertext.

    When `plaintext_path` is None, runs in verification-only mode (no plaintext written to disk).
    Returns `(ciphertext_bytes, plaintext_bytes, ciphertext_sha256)`.
    """
    key = validate_owner_key(owner_key)
    aead = AESGCM(key)
    sha = hashlib.sha256()
    ct_total = 0
    pt_total = 0

    def _read_exact(stream, n: int) -> bytes:
        buf = bytearray()
        while len(buf) < n:
            piece = stream.read(n - len(buf))
            if not piece:
                break
            buf.extend(piece)
        return bytes(buf)

    dst = plaintext_path.open("wb") if plaintext_path is not None else None
    try:
        with ciphertext_path.open("rb") as src:
            hdr = _read_exact(src, WIRE_HEADER_STRUCT.size)
            if len(hdr) != WIRE_HEADER_STRUCT.size:
                raise ArchiveError("corrupt_archive", "Truncated archive header")
            magic, flags, chunk_size, nonce_prefix = WIRE_HEADER_STRUCT.unpack(hdr)
            if magic != WIRE_MAGIC or flags != WIRE_FLAGS_AES_256_GCM:
                raise ArchiveError("corrupt_archive", "Unsupported archive magic or crypto flags")
            if chunk_size < 4096 or chunk_size > 16 * 1024 * 1024:
                raise ArchiveError("corrupt_archive", "Invalid archive chunk size")
            sha.update(hdr)
            ct_total += len(hdr)

            chunk_index = 0
            saw_final = False
            while True:
                fhdr = _read_exact(src, WIRE_CHUNK_STRUCT.size)
                if not fhdr:
                    break
                if len(fhdr) != WIRE_CHUNK_STRUCT.size:
                    raise ArchiveError("corrupt_archive", "Truncated chunk frame header")
                if saw_final:
                    raise ArchiveError("corrupt_archive", "Trailing bytes after final archive chunk")
                ct_len, final_flag = WIRE_CHUNK_STRUCT.unpack(fhdr)
                if final_flag not in (0, 1) or ct_len < GCM_TAG_BYTES or ct_len > chunk_size + GCM_TAG_BYTES:
                    raise ArchiveError("corrupt_archive", "Invalid chunk frame metadata")
                ct = _read_exact(src, ct_len)
                if len(ct) != ct_len:
                    raise ArchiveError("corrupt_archive", "Truncated chunk ciphertext")
                is_final = final_flag == 1
                nonce = _chunk_nonce(nonce_prefix, chunk_index)
                aad = _chunk_aad(owner_uid, machine_id, generation, nonce_prefix, chunk_index, is_final)
                try:
                    pt = aead.decrypt(nonce, ct, aad)
                except InvalidTag as exc:
                    raise ArchiveError(
                        "auth_failed",
                        "Archive decryption failed (wrong key, owner/machine/generation AAD mismatch, or tampered chunk)",
                    ) from exc
                sha.update(fhdr)
                sha.update(ct)
                ct_total += len(fhdr) + len(ct)
                pt_total += len(pt)
                if dst is not None and pt:
                    try:
                        dst.write(pt)
                    except OSError as exc:
                        if exc.errno == errno.ENOSPC:
                            raise RestoreDiskFullError() from exc
                        raise
                if is_final:
                    saw_final = True
                chunk_index += 1

            if not saw_final:
                raise ArchiveError("corrupt_archive", "Archive missing authenticated final chunk (truncated)")
        if dst is not None:
            dst.flush()
    finally:
        if dst is not None:
            dst.close()

    digest = sha.hexdigest()
    if expected_sha256 is not None and digest != expected_sha256:
        raise ArchiveError("digest_mismatch", "Archive ciphertext SHA-256 mismatch")
    return ct_total, pt_total, digest


def encrypt_archive_bytes(
    plaintext: bytes,
    owner_key: bytes,
    owner_uid: str,
    machine_id: str,
    generation: int,
    *,
    chunk_size: int = DEFAULT_CHUNK_SIZE,
) -> bytes:
    with tempfile.TemporaryDirectory(prefix="domios-enc-") as tmp:
        pt_p = Path(tmp) / "in.bin"
        ct_p = Path(tmp) / "out.enc"
        pt_p.write_bytes(plaintext)
        encrypt_stream_file(
            pt_p, ct_p,
            owner_key=owner_key, owner_uid=owner_uid, machine_id=machine_id,
            generation=generation, chunk_size=chunk_size,
        )
        return ct_p.read_bytes()


def decrypt_archive_bytes(
    ciphertext: bytes,
    owner_key: bytes,
    owner_uid: str,
    machine_id: str,
    generation: int,
) -> bytes:
    with tempfile.TemporaryDirectory(prefix="domios-dec-") as tmp:
        ct_p = Path(tmp) / "in.enc"
        pt_p = Path(tmp) / "out.bin"
        ct_p.write_bytes(ciphertext)
        decrypt_stream_file(
            ct_p, pt_p,
            owner_key=owner_key, owner_uid=owner_uid, machine_id=machine_id,
            generation=generation,
        )
        return pt_p.read_bytes()


def upload_encrypted_archive(
    archive_path: Path,
    upload_url: str,
    *,
    content_type: str = "application/octet-stream",
    extra_headers: dict[str, str] | None = None,
    timeout_s: float = 300.0,
) -> dict[str, Any]:
    """Streams encrypted archive via HTTP(S) PUT to a narrowly scoped short-lived signed URL."""
    parsed = urllib.parse.urlsplit(upload_url)
    if parsed.scheme not in ("http", "https") or not parsed.hostname:
        raise ArchiveError("invalid_upload_url", "Upload URL must be http(s) with a valid host")
    size = archive_path.stat().st_size
    conn_cls = http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
    port = parsed.port or (443 if parsed.scheme == "https" else 80)
    target = parsed.path or "/"
    if parsed.query:
        target = f"{target}?{parsed.query}"

    headers = {
        "Content-Type": content_type,
        "Content-Length": str(size),
        **(extra_headers or {}),
    }
    conn = conn_cls(parsed.hostname, port, timeout=timeout_s)
    try:
        conn.putrequest("PUT", target)
        for k, v in headers.items():
            conn.putheader(k, v)
        conn.endheaders()
        with archive_path.open("rb") as src:
            while True:
                buf = src.read(DEFAULT_CHUNK_SIZE)
                if not buf:
                    break
                conn.send(buf)
        resp = conn.getresponse()
        body = resp.read(8192)
        if resp.status < 200 or resp.status >= 300:
            raise ArchiveError(
                "upload_failed",
                f"Archive upload to {redact_url(upload_url)} failed with HTTP {resp.status}",
            )
        return {"status": resp.status, "bytes_uploaded": size, "etag": resp.getheader("ETag"), "body_len": len(body)}
    except ArchiveError:
        raise
    except OSError as exc:
        raise ArchiveError("upload_failed", f"Archive upload to {redact_url(upload_url)} failed: {type(exc).__name__}") from exc
    finally:
        conn.close()


def download_encrypted_archive(
    download_url: str,
    dest_path: Path,
    *,
    expected_sha256: str | None = None,
    max_bytes: int = DEFAULT_MAX_ARCHIVE_BYTES,
    timeout_s: float = 300.0,
) -> tuple[int, str]:
    """Streams encrypted archive via HTTP(S) GET from a narrowly scoped signed URL to `dest_path`."""
    parsed = urllib.parse.urlsplit(download_url)
    if parsed.scheme not in ("http", "https") or not parsed.hostname:
        raise ArchiveError("invalid_download_url", "Download URL must be http(s) with a valid host")
    conn_cls = http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
    port = parsed.port or (443 if parsed.scheme == "https" else 80)
    target = parsed.path or "/"
    if parsed.query:
        target = f"{target}?{parsed.query}"

    conn = conn_cls(parsed.hostname, port, timeout=timeout_s)
    sha = hashlib.sha256()
    total = 0
    try:
        conn.request("GET", target)
        resp = conn.getresponse()
        if resp.status < 200 or resp.status >= 300:
            raise ArchiveError(
                "download_failed",
                f"Archive download from {redact_url(download_url)} failed with HTTP {resp.status}",
            )
        dest_path.parent.mkdir(parents=True, exist_ok=True)
        with dest_path.open("wb") as dst:
            while True:
                buf = resp.read(DEFAULT_CHUNK_SIZE)
                if not buf:
                    break
                total += len(buf)
                if total > max_bytes:
                    raise ArchiveError("archive_size_exceeded", "Downloaded archive exceeds size cap")
                try:
                    dst.write(buf)
                except OSError as exc:
                    if exc.errno == errno.ENOSPC:
                        raise RestoreDiskFullError() from exc
                    raise
                sha.update(buf)
            dst.flush()
    except ArchiveError:
        dest_path.unlink(missing_ok=True)
        raise
    except OSError as exc:
        dest_path.unlink(missing_ok=True)
        if exc.errno == errno.ENOSPC:
            raise RestoreDiskFullError() from exc
        raise ArchiveError(
            "download_failed",
            f"Archive download from {redact_url(download_url)} failed: {type(exc).__name__}",
        ) from exc
    finally:
        conn.close()

    digest = sha.hexdigest()
    if expected_sha256 and digest != expected_sha256:
        dest_path.unlink(missing_ok=True)
        raise ArchiveError("digest_mismatch", "Downloaded archive SHA-256 mismatch")
    return total, digest
