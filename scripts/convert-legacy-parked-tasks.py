#!/usr/bin/env python3
"""Explicit, auditable conversion of legacy task sidecars from parked to paused."""

import argparse
import ctypes
import errno
import hashlib
import json
import os
import pathlib
import re
import secrets
import socket
import stat
import subprocess
import sys
import tempfile
import time
import uuid
from dataclasses import dataclass
from typing import Callable

import yaml
try:
    from scripts.legacy_parked_model import (
        AUDIT_MARKER,
        Conversion,
        load_candidate,
        load_yaml,
        new_audit_id,
        prepare_conversion,
        validate_already_converted,
    )
except ModuleNotFoundError:
    from legacy_parked_model import (
        AUDIT_MARKER,
        Conversion,
        load_candidate,
        load_yaml,
        new_audit_id,
        prepare_conversion,
        validate_already_converted,
    )



class ConversionWriteError(OSError):
    def __init__(self, message: str, journal_path: pathlib.Path):
        super().__init__(f"{message}; recovery journal: {journal_path}")
        self.journal_path = journal_path


class DurabilityError(OSError):
    pass


class ReplaceLayoutError(OSError):
    def __init__(self, message: str, layout: dict[str, str | None], repair_required: bool):
        super().__init__(message)
        self.layout = layout
        self.repair_required = repair_required


@dataclass
class HeldLock:
    path: pathlib.Path
    descriptor: int
    record: dict


@dataclass
class SourceHandle:
    path: pathlib.Path
    descriptor: int




def windows_volume(path: pathlib.Path) -> tuple[str, int]:
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.GetVolumePathNameW.argtypes = [
        ctypes.c_wchar_p,
        ctypes.c_wchar_p,
        ctypes.c_uint32,
    ]
    kernel32.GetVolumePathNameW.restype = ctypes.c_int
    kernel32.GetDriveTypeW.argtypes = [ctypes.c_wchar_p]
    kernel32.GetDriveTypeW.restype = ctypes.c_uint32
    kernel32.GetVolumeInformationW.argtypes = [
        ctypes.c_wchar_p,
        ctypes.c_wchar_p,
        ctypes.c_uint32,
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.c_wchar_p,
        ctypes.c_uint32,
    ]
    kernel32.GetVolumeInformationW.restype = ctypes.c_int
    root = ctypes.create_unicode_buffer(32768)
    if not kernel32.GetVolumePathNameW(str(path.resolve()), root, len(root)):
        code = ctypes.get_last_error()
        raise DurabilityError(code, f"cannot resolve volume for {path}")
    if kernel32.GetDriveTypeW(root.value) != 3:
        raise DurabilityError(f"conversion requires a local fixed volume: {root.value}")
    serial = ctypes.c_uint32()
    if not kernel32.GetVolumeInformationW(
        root.value, None, 0, ctypes.byref(serial), None, None, None, 0
    ):
        code = ctypes.get_last_error()
        raise DurabilityError(code, f"cannot identify volume for {path}")
    return root.value, serial.value


def assert_local_same_volume(paths: list[pathlib.Path]) -> None:
    if os.name == "nt":
        volumes = {windows_volume(path)[1] for path in paths}
    else:
        volumes = {path.resolve().stat().st_dev for path in paths}
        command = ["stat", "-f", "-c", "%T", str(paths[0])]
        if sys.platform == "darwin":
            command = ["stat", "-f", "%T", str(paths[0])]
        filesystem = subprocess.check_output(command, text=True).strip().lower()
        remote = ("nfs", "smb", "cifs", "fuse.sshfs", "9p", "afs")
        if any(marker in filesystem for marker in remote):
            raise DurabilityError(f"conversion requires a local filesystem, got {filesystem}")
    if len(volumes) != 1:
        raise DurabilityError("transaction paths must be on one volume")


def sync_windows_volume(path: pathlib.Path) -> None:
    root, _serial = windows_volume(path)
    if not re.fullmatch(r"[A-Za-z]:\\", root):
        raise DurabilityError(f"unsupported Windows volume root: {root}")
    volume = f"\\\\.\\{root[:2]}"
    descriptor = windows_open(volume, 0x40000000, 0x00000001 | 0x00000002, 3)
    try:
        import msvcrt

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.FlushFileBuffers.argtypes = [ctypes.c_void_p]
        kernel32.FlushFileBuffers.restype = ctypes.c_int
        handle = msvcrt.get_osfhandle(descriptor)
        if not kernel32.FlushFileBuffers(handle):
            code = ctypes.get_last_error()
            raise DurabilityError(code, f"volume namespace barrier failed for {root}")
    finally:
        os.close(descriptor)


def full_sync_descriptor(descriptor: int) -> None:
    try:
        if sys.platform == "darwin":
            import fcntl

            if fcntl.fcntl(descriptor, 51) != 0:
                raise DurabilityError("F_FULLFSYNC failed")
            return
        os.fsync(descriptor)
    except OSError as error:
        raise DurabilityError(str(error)) from error


def fsync_parent(path: pathlib.Path) -> None:
    if os.name == "nt":
        raise DurabilityError("Windows has no documented directory namespace barrier")
    try:
        descriptor = os.open(path.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    except OSError as error:
        raise DurabilityError(str(error)) from error


def sync_namespace(path: pathlib.Path) -> None:
    if os.name == "nt":
        sync_windows_volume(path)
        return
    fsync_parent(path)


def durable_write(path: pathlib.Path, content: bytes) -> None:
    with path.open("wb") as stream:
        stream.write(content)
        stream.flush()
        full_sync_descriptor(stream.fileno())


def write_journal(
    path: pathlib.Path,
    journal: dict,
    namespace_barrier: Callable[[pathlib.Path], None],
    require_namespace: bool = True,
) -> None:
    handle, name = tempfile.mkstemp(dir=path.parent, prefix=".journal.", suffix=".tmp")
    temporary = pathlib.Path(name)
    try:
        with os.fdopen(handle, "wb") as stream:
            stream.write((json.dumps(journal, indent=2) + "\n").encode("utf-8"))
            stream.flush()
            full_sync_descriptor(stream.fileno())
        os.replace(temporary, path)
        if require_namespace:
            namespace_barrier(path)
    finally:
        temporary.unlink(missing_ok=True)


def content_hash(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def file_hash(path: pathlib.Path) -> str:
    return content_hash(path.read_bytes())


def process_start_token(pid: int) -> str | None:
    if sys.platform.startswith("linux"):
        stat_path = pathlib.Path(f"/proc/{pid}/stat")
        try:
            fields = stat_path.read_text(encoding="utf-8").rsplit(")", 1)[1].split()
            return f"linux:{fields[19]}"
        except FileNotFoundError:
            return None
        except (OSError, IndexError) as error:
            raise OSError(f"cannot prove process start for pid {pid}: {error}") from error
    if os.name == "nt":
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        kernel32.OpenProcess.restype = ctypes.c_void_p
        kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        kernel32.CloseHandle.restype = ctypes.c_int

        class FileTime(ctypes.Structure):
            _fields_ = [("low", ctypes.c_uint32), ("high", ctypes.c_uint32)]

        kernel32.GetProcessTimes.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(FileTime),
            ctypes.POINTER(FileTime),
            ctypes.POINTER(FileTime),
            ctypes.POINTER(FileTime),
        ]
        kernel32.GetProcessTimes.restype = ctypes.c_int
        handle = kernel32.OpenProcess(0x1000, False, pid)
        if not handle:
            code = ctypes.get_last_error()
            if code == 87:
                return None
            raise OSError(code, f"cannot prove process start for pid {pid}")
        creation = FileTime()
        exit_time = FileTime()
        kernel_time = FileTime()
        user_time = FileTime()
        try:
            if not kernel32.GetProcessTimes(
                handle,
                ctypes.byref(creation),
                ctypes.byref(exit_time),
                ctypes.byref(kernel_time),
                ctypes.byref(user_time),
            ):
                code = ctypes.get_last_error()
                raise OSError(code, f"cannot prove process start for pid {pid}")
            return f"windows:{(creation.high << 32) | creation.low}"
        finally:
            kernel32.CloseHandle(handle)
    try:
        output = subprocess.check_output(
            ["ps", "-o", "lstart=", "-p", str(pid)],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except subprocess.CalledProcessError:
        return None
    if not output:
        return None
    return f"unix:{output}"


def owner_record(transaction_id: str, journal_path: pathlib.Path) -> dict:
    token = process_start_token(os.getpid())
    if token is None:
        raise OSError("cannot determine current process start token")
    return {
        "version": 1,
        "transaction_id": transaction_id,
        "pid": os.getpid(),
        "process_start": token,
        "host": socket.gethostname(),
        "journal": str(journal_path.resolve()),
    }


def write_descriptor_json(descriptor: int, value: dict) -> None:
    payload = (json.dumps(value, indent=2) + "\n").encode("utf-8")
    os.lseek(descriptor, 0, os.SEEK_SET)
    os.ftruncate(descriptor, 0)
    remaining = memoryview(payload)
    while remaining:
        written = os.write(descriptor, remaining)
        if written == 0:
            raise OSError("short write while persisting migration owner")
        remaining = remaining[written:]
    full_sync_descriptor(descriptor)


def read_descriptor_json(descriptor: int) -> dict:
    os.lseek(descriptor, 0, os.SEEK_SET)
    with os.fdopen(os.dup(descriptor), "rb") as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise ValueError("migration lock must contain a JSON object")
    return value


def windows_open(
    path: pathlib.Path, access: int, share: int, disposition: int
) -> int:
    import msvcrt

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateFileW.argtypes = [
        ctypes.c_wchar_p,
        ctypes.c_uint32,
        ctypes.c_uint32,
        ctypes.c_void_p,
        ctypes.c_uint32,
        ctypes.c_uint32,
        ctypes.c_void_p,
    ]
    kernel32.CreateFileW.restype = ctypes.c_void_p
    kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
    kernel32.CloseHandle.restype = ctypes.c_int
    handle = kernel32.CreateFileW(
        str(path), access, share, None, disposition, 0x80, None
    )
    if handle == ctypes.c_void_p(-1).value:
        code = ctypes.get_last_error()
        raise OSError(code, f"cannot open {path}")
    try:
        return msvcrt.open_osfhandle(handle, os.O_RDWR if access & 0x40000000 else os.O_RDONLY)
    except BaseException:
        kernel32.CloseHandle(handle)
        raise


def open_global_lock(path: pathlib.Path, record: dict, create: bool) -> HeldLock:
    if os.name == "nt":
        descriptor = windows_open(
            path,
            0x80000000 | 0x40000000,
            0x00000001 | 0x00000004,
            1 if create else 3,
        )
    else:
        flags = os.O_RDWR | (os.O_CREAT | os.O_EXCL if create else 0)
        descriptor = os.open(path, flags, 0o600)
        try:
            import fcntl

            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BaseException:
            os.close(descriptor)
            raise
    try:
        if create:
            write_descriptor_json(descriptor, record)
        return HeldLock(path, descriptor, record)
    except BaseException:
        os.close(descriptor)
        raise


def release_global_lock(
    lock: HeldLock,
    remove: bool,
    namespace_barrier: Callable[[pathlib.Path], None] = sync_namespace,
) -> None:
    try:
        if remove:
            lock.path.unlink(missing_ok=True)
            namespace_barrier(lock.path)
    finally:
        os.close(lock.descriptor)


def remove_global_lock(lock: HeldLock, namespace_barrier: Callable[[pathlib.Path], None]) -> None:
    lock.path.unlink()
    namespace_barrier(lock.path)


def retain_global_lock_record(lock: HeldLock) -> HeldLock:
    if lock.path.exists():
        return lock
    os.close(lock.descriptor)
    return open_global_lock(lock.path, lock.record, create=True)


def assert_owner_stale(record: dict) -> None:
    required = {"pid", "process_start", "host", "journal", "transaction_id"}
    if not required.issubset(record):
        raise ValueError("migration lock owner record is incomplete")
    if record["host"] != socket.gethostname():
        raise OSError("cannot prove stale migration owner from another host")
    if not isinstance(record["pid"], int) or not isinstance(record["process_start"], str):
        raise ValueError("migration lock owner identity is malformed")
    current = process_start_token(record["pid"])
    if current == record["process_start"]:
        raise OSError("migration owner is still running")


def acquire_source_handles(paths: list[pathlib.Path]) -> list[SourceHandle]:
    handles: list[SourceHandle] = []
    try:
        for path in paths:
            if os.name == "nt":
                descriptor = windows_open(
                    path,
                    0x80000000,
                    0x00000001 | 0x00000004,
                    3,
                )
            else:
                descriptor = os.open(path, os.O_RDONLY)
                import fcntl

                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            handles.append(SourceHandle(path, descriptor))
        return handles
    except BaseException:
        release_source_handles(handles)
        raise


def release_source_handles(handles: list[SourceHandle]) -> None:
    for handle in reversed(handles):
        os.close(handle.descriptor)


def reacquire_windows_destination(
    handles: list[SourceHandle], index: int, path: pathlib.Path
) -> None:
    if os.name != "nt" or not handles:
        return
    descriptor = windows_open(path, 0x80000000 | 0x40000000, 0x00000001, 3)
    os.close(handles[index].descriptor)
    handles[index] = SourceHandle(path, descriptor)


def descriptor_hash(descriptor: int) -> str:
    os.lseek(descriptor, 0, os.SEEK_SET)
    digest = hashlib.sha256()
    with os.fdopen(os.dup(descriptor), "rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_installed_under_locks(
    entries: list[dict], current_handles: list[SourceHandle]
) -> None:
    for entry, handle in zip(entries, current_handles):
        expected = entry["installed_sha256"]
        if descriptor_hash(handle.descriptor) != expected or file_hash(handle.path) != expected:
            raise OSError(f"final installed hash mismatch for {handle.path}")


def verify_sources_unchanged(entries: list[dict], start: int = 0) -> None:
    for entry in entries[start:]:
        destination = pathlib.Path(entry["path"])
        if file_hash(destination) != entry["original_sha256"]:
            raise OSError(f"source changed after staging: {destination}")


def unix_exchange(source: pathlib.Path, destination: pathlib.Path) -> bool:
    libc = ctypes.CDLL(None, use_errno=True)
    operation = None
    args = (os.fsencode(source), os.fsencode(destination), 2)
    if sys.platform == "darwin" and hasattr(libc, "renamex_np"):
        operation = libc.renamex_np
    elif sys.platform.startswith("linux") and hasattr(libc, "renameat2"):
        operation = lambda src, dst, flag: libc.renameat2(-100, src, -100, dst, flag)
    if operation is None:
        return False
    if operation(*args) == 0:
        return True
    code = ctypes.get_errno()
    if code in {errno.ENOSYS, errno.EINVAL, errno.ENOTSUP, errno.EOPNOTSUPP}:
        return False
    raise OSError(code, os.strerror(code))


def path_hash(path: pathlib.Path) -> str | None:
    try:
        return file_hash(path)
    except FileNotFoundError:
        return None


def windows_replace_layout(
    source: pathlib.Path, destination: pathlib.Path, backup: pathlib.Path
) -> dict[str, str | None]:
    return {
        "destination": path_hash(destination),
        "replacement": path_hash(source),
        "replace_backup": path_hash(backup),
    }


def classify_windows_replace_error(
    code: int,
    layout: dict[str, str | None],
    original_hash: str,
    installed_hash: str,
) -> tuple[str, bool]:
    names = {
        1175: "ERROR_UNABLE_TO_REMOVE_REPLACED",
        1176: "ERROR_UNABLE_TO_MOVE_REPLACEMENT",
        1177: "ERROR_UNABLE_TO_MOVE_REPLACEMENT_2",
    }
    unchanged = layout == {
        "destination": original_hash,
        "replacement": installed_hash,
        "replace_backup": None,
    }
    safe_unchanged = unchanged and code in {1175, 1176}
    return names.get(code, f"WIN32_ERROR_{code}"), not safe_unchanged


def atomic_replace(
    source: pathlib.Path, destination: pathlib.Path, backup: pathlib.Path
) -> None:
    if os.name == "nt":
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.ReplaceFileW.argtypes = [
            ctypes.c_wchar_p,
            ctypes.c_wchar_p,
            ctypes.c_wchar_p,
            ctypes.c_uint32,
            ctypes.c_void_p,
            ctypes.c_void_p,
        ]
        kernel32.ReplaceFileW.restype = ctypes.c_int
        if backup.exists():
            raise OSError(f"ReplaceFileW backup already exists: {backup}")
        if windows_volume(source)[1] != windows_volume(destination)[1]:
            raise OSError("ReplaceFileW paths are not on the same volume")
        if windows_volume(backup.parent)[1] != windows_volume(destination)[1]:
            raise OSError("ReplaceFileW backup is not on the destination volume")
        before_destination = file_hash(destination)
        before_source = file_hash(source)
        if not kernel32.ReplaceFileW(
            str(destination), str(source), str(backup), 0, None, None
        ):
            code = ctypes.get_last_error()
            layout = windows_replace_layout(source, destination, backup)
            detail, repair_required = classify_windows_replace_error(
                code, layout, before_destination, before_source
            )
            raise ReplaceLayoutError(
                f"ReplaceFileW {detail} for {destination}",
                layout,
                repair_required=repair_required,
            )
        return
    if not unix_exchange(source, destination):
        os.replace(source, destination)


def sync_installed(path: pathlib.Path) -> None:
    try:
        descriptor = os.open(path, os.O_RDWR)
        try:
            full_sync_descriptor(descriptor)
        finally:
            os.close(descriptor)
    except OSError as error:
        raise DurabilityError(str(error)) from error


def install_and_verify(
    source: pathlib.Path,
    destination: pathlib.Path,
    expected_hash: str,
    replacement_backup: pathlib.Path,
    replace_file: Callable[[pathlib.Path, pathlib.Path, pathlib.Path], None],
    sync_destination: Callable[[pathlib.Path], None],
) -> None:
    replace_file(source, destination, replacement_backup)
    sync_destination(destination)
    actual = file_hash(destination)
    if actual != expected_hash:
        raise OSError(f"installed hash mismatch for {destination}: {actual}")


def classify_destination(entry: dict) -> str:
    try:
        actual = file_hash(pathlib.Path(entry["path"]))
    except FileNotFoundError:
        entry["actual_sha256"] = None
        return "missing"
    except OSError as error:
        entry["actual_sha256"] = None
        entry["classification_error"] = str(error)
        return "conflict"
    entry["actual_sha256"] = actual
    if actual == entry["original_sha256"]:
        return "original"
    if actual == entry["installed_sha256"]:
        return "installed"
    return "conflict"


def restore_entry(entry: dict) -> str | None:
    destination = pathlib.Path(entry["path"])
    classification = classify_destination(entry)
    entry["recovery_classification"] = classification
    if classification == "original":
        return None
    if classification == "conflict":
        actual = entry.get("actual_sha256") or entry.get("classification_error")
        return f"{destination}: preserved conflict {actual}"
    layout_paths = {
        "destination": destination,
        "replacement": pathlib.Path(entry["staged"]),
        "replace_backup": pathlib.Path(entry["replace_backup"]),
    }
    layout = {name: path_hash(path) for name, path in layout_paths.items()}
    entry["rollback_layout"] = layout
    known = {None, entry["original_sha256"], entry["installed_sha256"]}
    if any(value not in known for value in layout.values()):
        entry["recovery_classification"] = "conflict"
        return f"{destination}: incomplete replacement layout {layout}"
    if classification == "installed" and layout["destination"] != entry["installed_sha256"]:
        entry["recovery_classification"] = "conflict"
        return f"{destination}: changed before guarded rollback"
    if classification == "missing" and layout["replace_backup"] != entry["original_sha256"]:
        entry["recovery_classification"] = "conflict"
        return f"{destination}: missing without a classified original replace backup"
    handle, name = tempfile.mkstemp(
        dir=destination.parent,
        prefix=f".{destination.name}.rollback.",
        suffix=".tmp",
    )
    temporary = pathlib.Path(name)
    try:
        with os.fdopen(handle, "wb") as stream:
            stream.write(pathlib.Path(entry["backup"]).read_bytes())
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, entry["mode"])
        install_and_verify(
            temporary,
            destination,
            entry["original_sha256"],
            pathlib.Path(entry["replace_backup"]).with_suffix(".rollback"),
            atomic_replace,
            sync_installed,
        ) if destination.exists() else install_missing_and_verify(
            temporary, destination, entry["original_sha256"], sync_installed
        )
        entry["recovery_classification"] = "original"
        return None
    except OSError as error:
        return f"{destination}: {error}"
    finally:
        temporary.unlink(missing_ok=True)


def install_missing_and_verify(
    source: pathlib.Path,
    destination: pathlib.Path,
    expected_hash: str,
    sync_destination: Callable[[pathlib.Path], None],
) -> None:
    os.replace(source, destination)
    sync_destination(destination)
    if file_hash(destination) != expected_hash:
        raise OSError(f"restored hash mismatch for {destination}")


def rollback_entries(entries: list[dict]) -> list[str]:
    return [error for entry in entries if (error := restore_entry(entry)) is not None]


def validate_journal(journal_path: pathlib.Path, journal: dict) -> list[dict]:
    if journal.get("version") != 3 or not isinstance(journal.get("transaction_id"), str):
        raise ValueError("unsupported or malformed conversion journal")
    if journal.get("logical_state") not in {
        "prepared",
        "applying",
        "committed",
        "rolled_back",
        "rollback_failed",
    }:
        raise ValueError("conversion journal logical state is malformed")
    if journal.get("durability") not in {
        "pending",
        "verified",
        "durability_unverified",
        "repair_required",
    }:
        raise ValueError("conversion journal durability is malformed")
    owner = journal.get("owner")
    if not isinstance(owner, dict) or owner.get("transaction_id") != journal["transaction_id"]:
        raise ValueError("conversion journal owner is malformed")
    if pathlib.Path(owner.get("journal", "")).resolve() != journal_path.resolve():
        raise ValueError("conversion journal owner path is malformed")
    entries = journal.get("entries")
    if not isinstance(entries, list) or not entries:
        raise ValueError("conversion journal has no entries")
    transaction_dir = journal_path.resolve().parent
    parents: set[pathlib.Path] = set()
    seen: set[pathlib.Path] = set()
    hashes = re.compile(r"^[0-9a-f]{64}$")
    for entry in entries:
        if not isinstance(entry, dict):
            raise ValueError("conversion journal entry is not an object")
        if entry.get("state") not in {"prepared", "replace_intent", "installed"}:
            raise ValueError("conversion journal entry state is malformed")
        path = pathlib.Path(entry.get("path", "")).resolve()
        backup = pathlib.Path(entry.get("backup", "")).resolve()
        staged = pathlib.Path(entry.get("staged", "")).resolve()
        replace_backup = pathlib.Path(entry.get("replace_backup", "")).resolve()
        if path in seen:
            raise ValueError("conversion journal contains duplicate paths")
        seen.add(path)
        parents.add(path.parent)
        if (
            backup.parent != transaction_dir
            or replace_backup.parent != transaction_dir
            or staged.parent != path.parent
        ):
            raise ValueError("conversion journal path escapes transaction ownership")
        for key in (
            "original_sha256",
            "installed_sha256",
            "backup_sha256",
            "staged_sha256",
        ):
            if not isinstance(entry.get(key), str) or not hashes.fullmatch(entry[key]):
                raise ValueError(f"conversion journal has malformed {key}")
        if entry["backup_sha256"] != entry["original_sha256"]:
            raise ValueError("backup hash does not match original hash")
        if entry["staged_sha256"] != entry["installed_sha256"]:
            raise ValueError("staged hash does not match installed hash")
        if not backup.is_file() or file_hash(backup) != entry["backup_sha256"]:
            raise ValueError(f"conversion backup hash mismatch: {backup}")
        if staged.exists() and file_hash(staged) not in {
            entry["staged_sha256"],
            entry["original_sha256"],
        }:
            raise ValueError(f"conversion staged hash mismatch: {staged}")
        if not isinstance(entry.get("mode"), int):
            raise ValueError("conversion journal mode is malformed")
    if len(parents) != 1 or transaction_dir.parent not in parents:
        raise ValueError("conversion journal is not transaction-wide for one task store")
    return entries


def write_all(
    conversions: list[Conversion],
    replace_file: Callable[
        [pathlib.Path, pathlib.Path, pathlib.Path], None
    ] = atomic_replace,
    sync_destination: Callable[[pathlib.Path], None] = sync_installed,
    namespace_barrier: Callable[[pathlib.Path], None] | None = None,
    after_staging: Callable[[], None] | None = None,
    before_replace: Callable[[int], None] | None = None,
    after_install: Callable[[int], None] | None = None,
    source_handle_factory: Callable[
        [list[pathlib.Path]], list[SourceHandle]
    ] = acquire_source_handles,
) -> pathlib.Path | None:
    if not conversions:
        return None
    destinations = [conversion.path.resolve(strict=True) for conversion in conversions]
    assert_local_same_volume(destinations)
    namespace_barrier = namespace_barrier or sync_namespace
    parents = {destination.parent for destination in destinations}
    if len(parents) != 1:
        raise OSError("all converted sidecars must share one task-store directory")
    transaction_id = str(uuid.uuid4())
    task_store = next(iter(parents))
    transaction_dir = task_store / f".legacy-parked-conversion-{transaction_id}"
    transaction_dir.mkdir(mode=0o700)
    journal_path = transaction_dir / "journal.json"
    lock_path = task_store / ".legacy-parked-conversion.lock"
    owner = owner_record(transaction_id, journal_path)
    entries: list[dict] = []
    staged: list[pathlib.Path] = []
    source_handles: list[SourceHandle] = []
    staged_handles: list[SourceHandle] = []
    lock: HeldLock | None = None
    journal: dict = {}
    terminal = False
    lock_removed = False
    commit_reached = False
    destination_mutated = False
    durability_failure = False
    try:
        lock = open_global_lock(lock_path, owner, create=True)
        source_handles = source_handle_factory(destinations)
        for index, (conversion, destination) in enumerate(zip(conversions, destinations)):
            if destination.read_bytes() != conversion.original:
                raise OSError(f"source changed after validation: {destination}")
            mode = stat.S_IMODE(destination.stat().st_mode)
            backup = transaction_dir / f"{index:03d}-{destination.name}.backup"
            replace_backup = transaction_dir / f"{index:03d}-{destination.name}.replaced"
            durable_write(backup, conversion.original)
            handle, name = tempfile.mkstemp(
                dir=destination.parent,
                prefix=f".{destination.name}.conversion.",
                suffix=".tmp",
            )
            temporary = pathlib.Path(name)
            staged.append(temporary)
            with os.fdopen(handle, "wb") as stream:
                stream.write(conversion.text.encode("utf-8"))
                stream.flush()
                os.fsync(stream.fileno())
            os.chmod(temporary, mode)
            staged_value = load_yaml(temporary.read_text(encoding="utf-8"))
            if staged_value != load_yaml(conversion.text):
                raise OSError(f"staged conversion validation failed: {destination}")
            if not validate_already_converted(staged_value):
                raise OSError(f"staged conversion is not audited paused state: {destination}")
            original_hash = content_hash(conversion.original)
            installed_hash = content_hash(conversion.text.encode("utf-8"))
            entries.append(
                {
                    "path": str(destination),
                    "backup": str(backup),
                    "staged": str(temporary),
                    "replace_backup": str(replace_backup),
                    "mode": mode,
                    "audit_id": conversion.audit_id,
                    "timestamp_ms": conversion.timestamp_ms,
                    "state": "prepared",
                    "original_sha256": original_hash,
                    "installed_sha256": installed_hash,
                    "backup_sha256": file_hash(backup),
                    "staged_sha256": file_hash(temporary),
                }
            )
        journal = {
            "version": 3,
            "transaction_id": transaction_id,
            "owner": owner,
            "logical_state": "prepared",
            "durability": "pending",
            "entries": entries,
        }
        validate_journal(journal_path, journal)
        if os.name != "nt":
            staged_handles = acquire_source_handles(
                [pathlib.Path(entry["staged"]) for entry in entries]
            )
        try:
            write_journal(journal_path, journal, namespace_barrier)
        except DurabilityError:
            durability_failure = True
            journal["durability"] = "durability_unverified"
            write_journal(journal_path, journal, namespace_barrier, False)
            raise
        if after_staging is not None:
            after_staging()
        verify_sources_unchanged(entries)
        for index, entry in enumerate(entries):
            if before_replace is not None:
                before_replace(index)
            verify_sources_unchanged(entries, index)
            journal["logical_state"] = "applying"
            entry["state"] = "replace_intent"
            try:
                write_journal(journal_path, journal, namespace_barrier)
            except DurabilityError:
                durability_failure = True
                journal["durability"] = "durability_unverified"
                write_journal(journal_path, journal, namespace_barrier, False)
                raise
            try:
                replace_file(
                    pathlib.Path(entry["staged"]),
                    pathlib.Path(entry["path"]),
                    pathlib.Path(entry["replace_backup"]),
                )
                destination_mutated = True
                if os.name == "nt" and replace_file is atomic_replace:
                    layout = windows_replace_layout(
                        pathlib.Path(entry["staged"]),
                        pathlib.Path(entry["path"]),
                        pathlib.Path(entry["replace_backup"]),
                    )
                    entry["installed_layout"] = layout
                    if layout != {
                        "destination": entry["installed_sha256"],
                        "replacement": None,
                        "replace_backup": entry["original_sha256"],
                    }:
                        raise ReplaceLayoutError(
                            f"ReplaceFileW returned an incomplete layout for {entry['path']}",
                            layout,
                            repair_required=True,
                        )
            except ReplaceLayoutError as replace_error:
                entry["replace_error_layout"] = replace_error.layout
                destination_mutated = destination_mutated or replace_error.repair_required
                durability_failure = durability_failure or replace_error.repair_required
                raise
            try:
                sync_destination(pathlib.Path(entry["path"]))
                reacquire_windows_destination(
                    source_handles, index, pathlib.Path(entry["path"])
                )
                namespace_barrier(pathlib.Path(entry["path"]))
            except DurabilityError:
                durability_failure = True
                raise
            if file_hash(pathlib.Path(entry["path"])) != entry["installed_sha256"]:
                raise OSError(f"installed hash mismatch for {entry['path']}")
            entry["state"] = "installed"
            entry["actual_sha256"] = entry["installed_sha256"]
            journal["replaced_count"] = index + 1
            write_journal(journal_path, journal, namespace_barrier)
            if after_install is not None:
                after_install(index)
        current_handles = source_handles if os.name == "nt" else staged_handles
        verify_installed_under_locks(entries, current_handles)
        journal["logical_state"] = "committed"
        journal["durability"] = "verified"
        write_journal(journal_path, journal, namespace_barrier)
        commit_reached = True
        try:
            remove_global_lock(lock, namespace_barrier)
        except DurabilityError:
            lock = retain_global_lock_record(lock)
            durability_failure = True
            raise
        lock_removed = True
        terminal = True
        return journal_path
    except (OSError, ValueError, yaml.YAMLError) as error:
        release_source_handles(source_handles)
        source_handles = []
        release_source_handles(staged_handles)
        staged_handles = []
        rollback_errors = [] if commit_reached else rollback_entries(entries)
        durability = "repair_required" if destination_mutated and durability_failure else "verified"
        logical_state = (
            "committed"
            if commit_reached
            else "rollback_failed" if rollback_errors else "rolled_back"
        )
        if not destination_mutated and durability_failure:
            logical_state = "prepared"
            durability = "durability_unverified"
        journal.update(
            {
                "version": 3,
                "transaction_id": transaction_id,
                "owner": owner,
                "logical_state": logical_state,
                "durability": durability,
                "error": str(error),
                "rollback_errors": rollback_errors,
                "entries": entries,
            }
        )
        try:
            write_journal(
                journal_path,
                journal,
                namespace_barrier,
                require_namespace=durability not in {
                    "durability_unverified",
                    "repair_required",
                },
            )
        except OSError as journal_error:
            rollback_errors.append(f"journal: {journal_error}")
            journal["durability"] = "repair_required" if destination_mutated else "durability_unverified"
        terminal = not rollback_errors and journal["durability"] == "verified"
        detail = f"transaction failed and all files were restored: {error}"
        if journal["durability"] != "verified":
            detail = f"transaction durability requires recovery: {journal['durability']}: {error}"
        elif rollback_errors:
            detail = f"transaction failed; rollback requires recovery: {'; '.join(rollback_errors)}"
        raise ConversionWriteError(detail, journal_path) from error
    finally:
        release_source_handles(staged_handles)
        release_source_handles(source_handles)
        if terminal:
            for temporary in staged:
                temporary.unlink(missing_ok=True)
        if lock is not None:
            release_global_lock(
                lock,
                remove=terminal and not lock_removed,
                namespace_barrier=namespace_barrier,
            )


def recover_transaction(journal_path: pathlib.Path) -> str:
    journal_path = journal_path.resolve(strict=True)
    journal = json.loads(journal_path.read_text(encoding="utf-8"))
    entries = validate_journal(journal_path, journal)
    task_store = pathlib.Path(entries[0]["path"]).parent
    lock_path = task_store / ".legacy-parked-conversion.lock"
    lock = open_global_lock(lock_path, {}, create=False)
    source_handles: list[SourceHandle] = []
    terminal = False
    lock_removed = False
    namespace_barrier = sync_namespace
    try:
        prior_owner = read_descriptor_json(lock.descriptor)
        if pathlib.Path(prior_owner.get("journal", "")).resolve() != journal_path:
            raise ValueError("migration lock names a different recovery journal")
        if prior_owner.get("transaction_id") != journal.get("transaction_id"):
            raise ValueError("migration lock transaction does not match journal")
        assert_owner_stale(prior_owner)
        recovery_owner = owner_record(journal["transaction_id"], journal_path)
        recovery_owner["recovery_of"] = prior_owner
        write_descriptor_json(lock.descriptor, recovery_owner)
        lock.record = recovery_owner
        journal = json.loads(journal_path.read_text(encoding="utf-8"))
        entries = validate_journal(journal_path, journal)
        owned_paths = []
        for entry in entries:
            owned_paths.extend(
                pathlib.Path(entry[key])
                for key in ("path", "staged", "replace_backup")
                if pathlib.Path(entry[key]).exists()
            )
        source_handles = acquire_source_handles(owned_paths)
        classifications = [classify_destination(entry) for entry in entries]
        if journal.get("logical_state") == "committed" and all(
            classification == "installed" for classification in classifications
        ):
            if journal.get("durability") != "verified":
                raise ConversionWriteError(
                    "committed layout has unverified durability; manual repair required",
                    journal_path,
                )
            journal["recovery"] = {
                "owner": recovery_owner,
                "classifications": classifications,
                "result": "committed",
            }
            write_journal(journal_path, journal, namespace_barrier)
            try:
                remove_global_lock(lock, namespace_barrier)
            except DurabilityError as error:
                lock = retain_global_lock_record(lock)
                journal["durability"] = "repair_required"
                write_journal(journal_path, journal, namespace_barrier, False)
                raise ConversionWriteError(
                    f"commit lock removal durability failed: {error}", journal_path
                ) from error
            lock_removed = True
            terminal = True
            return "committed"
        errors = rollback_entries(entries)
        journal["logical_state"] = "rollback_failed" if errors else "rolled_back"
        journal["durability"] = "repair_required" if errors else "verified"
        journal["rollback_errors"] = errors
        journal["recovery"] = {
            "owner": recovery_owner,
            "classifications": [
                entry.get("recovery_classification") for entry in entries
            ],
            "result": journal["logical_state"],
        }
        try:
            for entry in entries:
                namespace_barrier(pathlib.Path(entry["path"]))
            write_journal(journal_path, journal, namespace_barrier)
        except DurabilityError as error:
            journal["durability"] = "repair_required"
            write_journal(journal_path, journal, namespace_barrier, False)
            errors.append(str(error))
        terminal = not errors and journal["durability"] == "verified"
        if errors:
            raise ConversionWriteError(
                f"recovery preserved conflicts: {'; '.join(errors)}", journal_path
            )
        if terminal:
            try:
                remove_global_lock(lock, namespace_barrier)
            except DurabilityError as error:
                lock = retain_global_lock_record(lock)
                journal["durability"] = "repair_required"
                write_journal(journal_path, journal, namespace_barrier, False)
                terminal = False
                raise ConversionWriteError(
                    f"rollback lock removal durability failed: {error}", journal_path
                ) from error
            lock_removed = True
        return "rolled_back"
    finally:
        release_source_handles(source_handles)
        release_global_lock(
            lock,
            remove=terminal and not lock_removed,
            namespace_barrier=namespace_barrier,
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=pathlib.Path)
    parser.add_argument("--reason", help="audit reason for this explicit conversion")
    parser.add_argument("--apply", action="store_true", help="write changes; default is audit only")
    parser.add_argument(
        "--recover",
        type=pathlib.Path,
        help="recover a stale interrupted transaction; never resumes conversion",
    )
    args = parser.parse_args()
    if args.recover is not None:
        if args.paths or args.reason is not None or args.apply:
            parser.error("--recover cannot be combined with paths, --reason, or --apply")
        try:
            result = recover_transaction(args.recover)
        except (OSError, ValueError, json.JSONDecodeError) as error:
            print(f"conversion recovery failed: {error}", file=sys.stderr)
            return 2
        print(f"conversion recovery {result} {args.recover}")
        return 0
    if not args.paths:
        parser.error("at least one sidecar path is required")
    reason = (args.reason or "").strip()
    if not reason:
        parser.error("--reason must not be blank")
    candidates: list[pathlib.Path] = []
    invalid: list[str] = []
    reserved_ids: set[str] = set()
    seen_paths: set[pathlib.Path] = set()
    latest_timestamp = time.time_ns() // 1_000_000 - 1
    for path in args.paths:
        try:
            resolved = path.resolve(strict=True)
            if resolved in seen_paths:
                raise ValueError("duplicate input path")
            seen_paths.add(resolved)
            value, ids, timestamp = load_candidate(path)
            if validate_already_converted(value):
                print(f"already converted {path}")
            else:
                candidates.append(path)
                reserved_ids.update(ids)
                latest_timestamp = max(latest_timestamp, timestamp)
        except (OSError, ValueError, yaml.YAMLError) as error:
            invalid.append(f"{path}: {error}")
    if invalid:
        print("refusing conversion:\n" + "\n".join(invalid), file=sys.stderr)
        return 2
    conversions: list[Conversion] = []
    for path in candidates:
        latest_timestamp += 1
        audit_id = new_audit_id(latest_timestamp, reserved_ids)
        reserved_ids.add(audit_id)
        try:
            conversion = prepare_conversion(path, reason, audit_id, latest_timestamp)
        except (OSError, ValueError, yaml.YAMLError) as error:
            print(f"refusing conversion:\n{path}: {error}", file=sys.stderr)
            return 2
        if conversion is not None:
            conversions.append(conversion)
    if args.apply:
        try:
            journal_path = write_all(conversions)
        except OSError as error:
            print(f"conversion write failed: {error}", file=sys.stderr)
            return 2
        if journal_path is not None:
            print(f"conversion journal {journal_path}")
    for conversion in conversions:
        if args.apply:
            print(
                f"converted {conversion.path} audit_id={conversion.audit_id} "
                f"timestamp_ms={conversion.timestamp_ms}"
            )
        else:
            print(
                f"would convert {conversion.path} audit_id={conversion.audit_id} "
                f"timestamp_ms={conversion.timestamp_ms}"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
