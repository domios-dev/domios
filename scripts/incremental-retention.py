import contextlib
import os
from pathlib import Path
import re
import time


def snapshot(directory, safe_path):
    newest = directory.stat().st_mtime
    size = 0
    for base, directories, names in os.walk(directory, followlinks=False):
        for name in [*directories, *names]:
            entry = safe_path(Path(base) / name)
            stat = entry.stat()
            newest = max(newest, stat.st_mtime)
            if entry.is_file():
                size += stat.st_size
    return newest, size


def trim_incremental(directory, safe_path, cargo_lock, dry_run=False, idle_hours=12, now=None):
    if not 0 < idle_hours < float('inf'):
        raise ValueError('invalid CODETERM_INCREMENTAL_IDLE_HOURS')
    cutoff = (time.time() if now is None else now) - idle_hours * 3600
    reclaimed = 0
    skipped = 0
    # Target triples insert one directory between the cache root and profile.
    roots = sorted(set(directory.glob('*/incremental')) | set(directory.glob('*/*/incremental')))
    with contextlib.ExitStack() as locks:
        try:
            for root in roots:
                safe_path(root)
                lock = safe_path(root.parent / '.cargo-lock')
                with lock.open('ab'):
                    pass
                locks.enter_context(cargo_lock(lock))
            groups = []
            for root in roots:
                crates = {}
                for entry in root.iterdir():
                    safe_path(entry)
                    match = re.fullmatch(r'(.+)-[a-z0-9]+', entry.name)
                    if match and entry.is_dir():
                        state = snapshot(entry, safe_path)
                        crates.setdefault(match[1], []).append((entry, state))
                for variants in crates.values():
                    groups.append(sorted(variants, key=lambda item: (-item[1][0], str(item[0]))))
        except OSError as error:
            return {'status': 'protected', 'bytes': 0, 'reason': f'Cargo lock unavailable: {error}'}
        for variants in groups:
            for index, (variant, state) in enumerate(variants):
                try:
                    with contextlib.ExitStack() as sessions:
                        for lock in sorted(variant.glob('*.lock')):
                            sessions.enter_context(cargo_lock(safe_path(lock)))
                        if index and state[0] < cutoff:
                            targets = [(variant, state)]
                        else:
                            entries = [(entry, snapshot(safe_path(entry), safe_path))
                                       for entry in variant.iterdir()
                                       if entry.name.startswith('s-') and entry.is_dir()]
                            entries.sort(key=lambda item: (-item[1][0], str(item[0])))
                            targets = [item for item in entries[1:] if item[1][0] < cutoff]
                        for target, prior in targets:
                            if snapshot(target, safe_path) != prior:
                                skipped += 1
                                continue
                            if not dry_run:
                                for base, directories, names in os.walk(target, topdown=False):
                                    for name in names:
                                        safe_path(Path(base) / name).unlink()
                                    for name in directories:
                                        safe_path(Path(base) / name).rmdir()
                                target.rmdir()
                            reclaimed += prior[1]
                except OSError:
                    skipped += 1
    return {'status': 'would trim incremental' if dry_run else 'trimmed incremental',
            'bytes': reclaimed, 'skipped': skipped}
