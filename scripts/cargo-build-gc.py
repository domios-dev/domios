# Caller: scripts/cargo-build-gc.mjs, scripts/cargo-build-gc.test.mjs
import contextlib
import ctypes
import json
import os
from pathlib import Path
import sys
import re


def safe_path(value):
    if os.name == 'nt' and not hasattr(Path, 'is_junction'):
        raise ValueError('Python 3.12+ is required for junction protection')
    value = Path(value).absolute()
    for part in [value, *value.parents]:
        if part.is_symlink() or (hasattr(part, 'is_junction') and part.is_junction()):
            raise ValueError(f'unsafe link/junction: {part}')
    return value


@contextlib.contextmanager
def cargo_lock(file):
    if os.name != 'nt':
        import fcntl
        with open(file, 'r+b') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield
        return
    from ctypes import wintypes
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)

    class Overlapped(ctypes.Structure):
        _fields_ = [('internal', ctypes.c_size_t), ('internal_high', ctypes.c_size_t),
                    ('offset', wintypes.DWORD), ('offset_high', wintypes.DWORD), ('event', wintypes.HANDLE)]

    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                  ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    kernel.LockFileEx.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD,
                                 wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(Overlapped)]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel.CreateFileW(str(file), 0xC0000000, 7, None, 3, 0x80, None)
    if handle == wintypes.HANDLE(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        overlap = Overlapped()
        if not kernel.LockFileEx(handle, 3, 0, 0xFFFFFFFF, 0xFFFFFFFF, ctypes.byref(overlap)):
            raise ctypes.WinError(ctypes.get_last_error())
        yield
    finally:
        kernel.CloseHandle(handle)


def inventory(directory):
    files = []
    for base, directories, names in os.walk(directory, followlinks=False):
        for name in [*directories, *names]:
            safe_path(Path(base) / name)
        files.extend(Path(base) / name for name in names)
    return files


def collect(root, directory, dry_run=False):
    root, directory = safe_path(root), safe_path(directory)
    if directory.parent.parent != root or not re.fullmatch('[a-f0-9]{14}', directory.name) or not re.fullmatch('[a-f0-9]{2}', directory.parent.name):
        raise ValueError('candidate is outside the two-level Cargo hash root')
    files = inventory(directory)
    with contextlib.ExitStack() as stack:
        try:
            for file in files:
                if file.name == '.cargo-lock':
                    stack.enter_context(cargo_lock(file))
        except OSError as error:
            return {'status': 'protected', 'bytes': 0, 'reason': f'Cargo lock unavailable: {error}'}
        size = sum(file.stat().st_size for file in files)
        if dry_run:
            return {'status': 'would remove orphan', 'bytes': size}
        # Windows refuses directory renames with open descendant handles, even with delete sharing.
        if set(inventory(directory)) != set(files):
            raise ValueError('directory contents changed while acquiring Cargo locks')
        for file in files:
            if file.name != '.cargo-lock':
                file.unlink()
        for file in files:
            if file.name == '.cargo-lock':
                file.unlink()
        for base, _directories, _names in os.walk(directory, topdown=False):
            Path(base).rmdir()
        return {'status': 'removed orphan', 'bytes': size}


def trim(root, directory, paths, dry_run=False):
    root, directory = safe_path(root), safe_path(directory)
    if directory.parent.parent != root or not re.fullmatch('[a-f0-9]{14}', directory.name) or not re.fullmatch('[a-f0-9]{2}', directory.parent.name):
        raise ValueError('trim target is outside the two-level Cargo hash root')
    targets = [safe_path(value) for value in paths]
    for target in targets:
        if directory not in target.parents or target.name in ('.cargo-lock', 'CACHEDIR.TAG', '.rustc_info.json'):
            raise ValueError(f'refusing trim path: {target}')
    with contextlib.ExitStack() as stack:
        try:
            for lock in sorted(directory.glob('*/.cargo-lock')):
                stack.enter_context(cargo_lock(lock))
        except OSError as error:
            return {'status': 'protected', 'bytes': 0, 'reason': f'Cargo lock unavailable: {error}'}
        size = 0
        for target in targets:
            if target.is_dir():
                files = inventory(target)
                size += sum(file.stat().st_size for file in files)
                if not dry_run:
                    for file in files:
                        file.unlink()
                    for base, _directories, _names in os.walk(target, topdown=False):
                        Path(base).rmdir()
            elif target.exists():
                size += target.stat().st_size
                if not dry_run:
                    target.unlink()
        return {'status': 'would trim' if dry_run else 'trimmed', 'bytes': size}


if __name__ == '__main__':
    try:
        if sys.argv[1] == '--trim':
            with open(sys.argv[4], encoding='utf-8') as handle:
                listed = json.load(handle)
            print(json.dumps(trim(sys.argv[2], sys.argv[3], listed, '--dry-run' in sys.argv[5:])))
            sys.exit(0)
        print(json.dumps(collect(sys.argv[1], sys.argv[2], '--dry-run' in sys.argv[3:])))
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(2)
