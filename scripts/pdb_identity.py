#!/usr/bin/env python3
"""Debug identity of a Windows symbol file or binary, and the key it is stored under.

    python scripts/pdb_identity.py <codeterm.pdb | codeterm.exe>   -> 06BBF58EADAC4DCDAE38084A168534501

The key is the canonical Windows symbol-store form: the GUID's 32 hex digits
followed by the age in hex, no separators. It is the same string whether it is read
from a PDB's own MSF info stream or from a binary's RSDS debug record, which is what
lets `build.sh` file a retained PDB under the directory the crash sidecar names.

Imported by `symbolicate-minidump.py`; run directly by `build.sh retain-pdb`.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

MSF_MAGIC = b"Microsoft C/C++ MSF 7.00" + bytes([13, 10, 26]) + b"DS" + bytes([0, 0, 0])


def identity_key(identity: tuple[str, int] | None) -> str | None:
    """(GUID, age) -> the symbol-store key. One definition, used by every caller."""
    if identity is None:
        return None
    guid, age = identity
    return f"{guid.replace('-', '').upper()}{age:X}"


def guid_from_fields(d1: int, d2: int, d3: int, d4: bytes) -> str:
    return (f"{d1:08X}-{d2:04X}-{d3:04X}-"
            f"{d4[:2].hex().upper()}-{d4[2:].hex().upper()}")


def cv_identity(record: bytes) -> tuple[str, int] | None:
    """(GUID, age) from an RSDS CodeView record, as a binary and a minidump record it."""
    if len(record) < 24 or record[:4] != b"RSDS":
        return None
    d1, d2, d3 = struct.unpack_from("<IHH", record, 4)
    (age,) = struct.unpack_from("<I", record, 20)
    return guid_from_fields(d1, d2, d3, record[12:20]), age


def describe_cv(record: bytes) -> str:
    """Human form: `GUID age=N pdb=NAME`, matching what the crash sidecar prints."""
    identity = cv_identity(record)
    if identity is None:
        return "<no RSDS record>"
    guid, age = identity
    name = record[24:].split(b"\x00", 1)[0].decode("utf-8", "replace")
    name = name.replace("\\", "/").rsplit("/", 1)[-1]
    return f"{guid} age={age} pdb={name}"


def pe_cv_record(image: Path) -> bytes:
    """The RSDS debug record from an on-disk PE."""
    try:
        data = image.read_bytes()
    except OSError:
        return b""
    try:
        (e_lfanew,) = struct.unpack_from("<I", data, 0x3C)
        (n_sections,) = struct.unpack_from("<H", data, e_lfanew + 6)
        (opt_size,) = struct.unpack_from("<H", data, e_lfanew + 20)
        opt_off = e_lfanew + 24
        (magic,) = struct.unpack_from("<H", data, opt_off)
        dir_off = opt_off + (112 if magic == 0x20B else 96)
        debug_rva, debug_size = struct.unpack_from("<II", data, dir_off + 6 * 8)
        if debug_rva == 0:
            return b""

        sections = []
        sec_off = opt_off + opt_size
        for i in range(n_sections):
            off = sec_off + i * 40
            virt_size, virt_addr, raw_size, raw_ptr = struct.unpack_from("<IIII", data, off + 8)
            sections.append((virt_addr, max(virt_size, raw_size), raw_ptr))

        def to_offset(rva: int) -> int | None:
            for virt_addr, size, raw_ptr in sections:
                if virt_addr <= rva < virt_addr + size:
                    return raw_ptr + (rva - virt_addr)
            return None

        table = to_offset(debug_rva)
        if table is None:
            return b""
        for i in range(debug_size // 28):
            entry = table + i * 28
            (entry_type,) = struct.unpack_from("<I", data, entry + 12)
            size_of_data, _rva, ptr = struct.unpack_from("<III", data, entry + 16)
            if entry_type == 2:  # IMAGE_DEBUG_TYPE_CODEVIEW
                return data[ptr : ptr + size_of_data]
    except (struct.error, IndexError):
        return b""
    return b""


def pdb_identity(path: Path) -> tuple[str, int] | None:
    """(GUID, age) a PDB claims for itself, from its MSF PDB-info stream.

    This is the half a symbol file can be checked by when the binary it was linked
    with is gone: a PDB that does not carry the dump's GUID names other functions.
    """
    try:
        with path.open("rb") as handle:
            header = handle.read(56)
            if not header.startswith(MSF_MAGIC):
                return None
            block_size, _free, _blocks, dir_bytes, _unknown, block_map = struct.unpack_from(
                "<IIIIII", header, 32)
            if block_size == 0:
                return None

            def read_block(index: int) -> bytes:
                handle.seek(index * block_size)
                return handle.read(block_size)

            n_dir_blocks = -(-dir_bytes // block_size)
            dir_blocks = struct.unpack_from(f"<{n_dir_blocks}I", read_block(block_map), 0)
            directory = b"".join(read_block(b) for b in dir_blocks)[:dir_bytes]

            (n_streams,) = struct.unpack_from("<I", directory, 0)
            if n_streams < 2:
                return None
            sizes = struct.unpack_from(f"<{n_streams}I", directory, 4)
            cursor = 4 + n_streams * 4
            for index in range(2):
                size = 0 if sizes[index] == 0xFFFFFFFF else sizes[index]
                count = -(-size // block_size)
                if index == 1:
                    blocks = struct.unpack_from(f"<{count}I", directory, cursor)
                    info = b"".join(read_block(b) for b in blocks)[:size]
                    if len(info) < 28:
                        return None
                    _version, _signature, age = struct.unpack_from("<III", info, 0)
                    d1, d2, d3 = struct.unpack_from("<IHH", info, 12)
                    return guid_from_fields(d1, d2, d3, info[20:28]), age
                cursor += count * 4
    except (OSError, struct.error, IndexError):
        return None
    return None


def identity_of(path: Path) -> tuple[str, int] | None:
    """Identity of either a PDB or a PE, whichever the file turns out to be."""
    return pdb_identity(path) or cv_identity(pe_cv_record(path))


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    key = identity_key(identity_of(Path(argv[1])))
    if key is None:
        print(f"no debug identity in {argv[1]}", file=sys.stderr)
        return 1
    print(key)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
