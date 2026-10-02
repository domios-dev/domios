"""Bounded archive acquisition and contained extraction; never run package scripts."""

import base64
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile
import time
from urllib.parse import urlsplit

from native_layout import demand
from deadline_io import Deadline

MAX_ARCHIVE = 512 * 1024 * 1024
MAX_EXPANDED = 1024 * 1024 * 1024
MAX_MEMBERS = 4096
RESERVED = {"con", "prn", "aux", "nul", *(f"com{i}" for i in range(1, 10)),
            *(f"lpt{i}" for i in range(1, 10))}


def sha256(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def within(root, path):
    root, path = Path(root).absolute(), Path(path).absolute()
    demand(path != root and path.is_relative_to(root), "Path escaped fixture root")
    for part in (root, *(root / Path(*path.relative_to(root).parts[:i])
                         for i in range(1, len(path.relative_to(root).parts) + 1))):
        demand(not part.is_symlink(), "Symlink in fixture path")
    demand(path.resolve().is_relative_to(root.resolve()), "Resolved path escaped fixture root")
    return path


def member_path(name):
    demand("\\" not in name and ":" not in name and not name.startswith("/"), "Unsafe archive path")
    parts = PurePosixPath(name).parts
    demand(parts and parts[0] == "package" and ".." not in parts, "Archive traversal or wrong root")
    demand(all(part not in ("", ".", "..") and not part.endswith((" ", ".")) and
               part.split(".", 1)[0].lower() not in RESERVED for part in parts),
           "Unsafe archive component")
    return Path(*parts[1:])


def verify_archive(path, metadata):
    demand(Path(path).stat().st_size <= MAX_ARCHIVE, "Archive byte limit exceeded")
    with Path(path).open("rb") as source:
        digest = hashlib.file_digest(source, "sha512").digest()
    demand(metadata["integrity"] == "sha512-" + base64.b64encode(digest).decode(),
           "Registry SHA512 integrity differs")
    observed = sha256(path)
    demand(not metadata.get("tarball_sha256") or observed == metadata["tarball_sha256"],
           "Reviewed archive SHA256 differs")
    return {"sha512": metadata["integrity"], "sha256": observed,
            "bytes": Path(path).stat().st_size, "source": metadata["tarball"]}


def registry_url(url):
    parsed = urlsplit(url)
    demand(parsed.scheme == "https" and parsed.hostname == "registry.npmjs.org" and
           not parsed.username and not parsed.password and parsed.port in (None, 443),
           "Archive URL is outside pinned HTTPS registry")


def acquire(metadata, destination, deadline, transport=None, clock=time.monotonic):
    registry_url(metadata["tarball"])
    if transport is None:
        from acquisition_transport import acquire_owned
        return acquire_owned(metadata, destination, deadline, clock=clock)
    hard = Deadline(deadline, clock)
    operation = hard.reserve(2)
    failed = True
    try:
        operation.remaining()
        stream = transport.open(metadata["tarball"], operation)
        operation.remaining()
        registry_url(stream.url)
        demand(stream.content_length is None or 0 <= stream.content_length <= MAX_ARCHIVE,
               "Archive byte limit exceeded")
        count = 0
        with Path(destination).open("xb") as output:
            while True:
                operation.remaining()
                chunk = stream.read_chunk(64 * 1024, operation)
                operation.remaining()
                demand(isinstance(chunk, bytes) and len(chunk) <= 64 * 1024,
                       "Archive transport exceeded bounded chunk")
                if not chunk:
                    break
                count += len(chunk)
                demand(count <= MAX_ARCHIVE, "Archive byte limit exceeded")
                output.write(chunk)
                operation.remaining()
        operation.remaining()
        demand(stream.content_length is None or count == stream.content_length,
               "Archive body ended before declared completion")
        failed = False
    finally:
        receipt = transport.finish(cancel=failed, deadline=hard)
        demand(isinstance(receipt, dict) and receipt.get("completed") is True,
               "Acquisition transport completion unconfirmed")
        hard.remaining()
    result = verify_archive(destination, metadata)
    hard.remaining()
    return result


class ArchiveComponents:
    def __init__(self):
        self.paths = {}

    def add(self, relative, directory):
        parts = relative.parts
        for index in range(1, len(parts) + 1):
            spelling = parts[:index]
            key = tuple(part.casefold() for part in spelling)
            leaf = index == len(parts)
            kind = "directory" if not leaf or directory else "file"
            existing = self.paths.get(key)
            if existing is None:
                self.paths[key] = {"spelling": spelling, "kind": kind, "declared": leaf}
                continue
            demand(existing["spelling"] == spelling, "Archive case-colliding component")
            demand(existing["kind"] == kind, "Archive file/directory component conflict")
            if leaf:
                demand(kind == "directory" and not existing["declared"], "Duplicate archive member")
                existing["declared"] = True


def extract(archive, destination, deadline):
    destination = Path(destination)
    demand(not destination.exists(), "Extraction destination already exists")
    destination.mkdir(parents=True, mode=0o700)
    count, size, components = 0, 0, ArchiveComponents()
    with tarfile.open(archive, "r:gz") as source:
        for member in source:
            demand(time.monotonic() < deadline, "Extraction deadline exceeded")
            count += 1
            size += member.size
            demand(count <= MAX_MEMBERS and size <= MAX_EXPANDED, "Archive expansion limit exceeded")
            demand(member.isdir() or member.isfile(), "Archive links or special files are forbidden")
            relative = member_path(member.name)
            if not relative.parts:
                demand(member.isdir(), "Archive root is not a directory")
                continue
            components.add(relative, member.isdir())
            path = within(destination, destination / relative)
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True, mode=0o700)
                continue
            path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            stream = source.extractfile(member)
            demand(stream is not None and member.size >= 0, "Invalid regular archive file")
            left = member.size
            with stream, path.open("xb") as output:
                while left:
                    demand(time.monotonic() < deadline, "Extraction deadline exceeded")
                    chunk = stream.read(min(left, 1024 * 1024))
                    demand(bool(chunk), "Truncated archive member")
                    output.write(chunk)
                    left -= len(chunk)
            path.chmod(0o700 if member.mode & 0o111 else 0o600)
    return tree_receipt(destination)


def tree_receipt(root):
    rows = []
    for path in sorted(Path(root).rglob("*")):
        within(root, path)
        if path.is_dir():
            continue
        demand(path.is_file(), "Nonregular assembly member")
        rows.append({"path": path.relative_to(root).as_posix(), "bytes": path.stat().st_size,
                     "sha256": sha256(path), "mode": path.stat().st_mode & 0o777})
    digest = hashlib.sha256(json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return {"sha256": digest, "files": rows}
