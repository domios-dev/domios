#!/usr/bin/env python3
"""Symbolicate a Windows minidump written by `crash::native`.

No symbolication tooling shipped with this repository — `minidump-writer` is a
writer only, and `scripts/` had nothing crash-related. This script is the reader:
it parses the minidump with the stdlib and drives `dbghelp.dll` (present on every
Windows install), so a crash artifact can be read with nothing installed.

    python scripts/symbolicate-minidump.py <dump.dmp> \
        --image <path-to-the-exact-codeterm.exe> [--all-threads] [--scan]

    # with Microsoft symbols for the system DLLs:
    python scripts/symbolicate-minidump.py <dump.dmp> --image <exe> \
        --symbol-path "<exe-dir>;srv*C:\\symbols*https://msdl.microsoft.com/download/symbols"

When the crashing binary has been relinked away, point the tool at the symbol file
`build.sh dev` retained for that build — the crash sidecar's `Symbols:` line names
the directory:

    python scripts/symbolicate-minidump.py <dump.dmp> --sidecar <crash-...-native.txt>
    python scripts/symbolicate-minidump.py <dump.dmp> --pdb <codeterm.pdb>
    python scripts/symbolicate-minidump.py <dump.dmp> --pdb-store ~/.codeterm-dev/pdb

A symbol file is accepted only when the GUID and age it records for itself match the
dump module's RSDS, and dbghelp then loads it as the module source. Frame NAMES come
back; x64 unwind data lives in the binary, so the frame ORDER does not — read `--scan`
as the authority in that mode.

Identity is checked per module: the RSDS (GUID + age) recorded in the dump is
compared against the RSDS in the on-disk file at the same path. A module whose
image no longer matches is attributed as `module+0xRVA` and its symbols are
WITHHELD — never guessed. A stale PDB produces confident, wrong function names,
which is worse than no names at all.

x64 unwinding needs the crashing image's own `.pdata`, so when the main module
fails verification `StackWalk64` is not trusted either. `--scan` falls back to a
raw stack scan: every code-range pointer in the thread's captured stack, in
address order. That is a superset with false positives and is labelled as such,
but it still names the modules that participated.

Other platforms use `minidump_stackwalk` (rust-minidump) with breakpad symbols
from `dump_syms`; neither is installed on the Windows box that produced these
artifacts, which is why this exists.
"""

from __future__ import annotations

import argparse
import ctypes
import struct
import sys
from ctypes import wintypes
from dataclasses import dataclass, field
from pathlib import Path

MINIDUMP_SIGNATURE = 0x504D444D

STREAM_THREAD_LIST = 3
STREAM_MODULE_LIST = 4
STREAM_MEMORY_LIST = 5
STREAM_EXCEPTION = 6
STREAM_SYSTEM_INFO = 7
STREAM_MEMORY64_LIST = 9
STREAM_THREAD_NAMES = 24

IMAGE_FILE_MACHINE_AMD64 = 0x8664

CTX_SIZE = 1232
CTX_RSP = 0x98
CTX_RBP = 0xA0
CTX_RIP = 0xF8

SYMOPT_UNDNAME = 0x00000002
SYMOPT_DEFERRED_LOADS = 0x00000004
SYMOPT_LOAD_LINES = 0x00000010
SYMOPT_FAIL_CRITICAL_ERRORS = 0x00000200
SYMOPT_EXACT_SYMBOLS = 0x00000400
SYMOPT_NO_PROMPTS = 0x00080000

SYM_TYPE_NAMES = {
    0: "SymNone", 1: "SymCoff", 2: "SymCv", 3: "SymPdb",
    4: "SymExport", 5: "SymDeferred", 6: "SymSym", 7: "SymDia", 8: "SymVirtual",
}

MAX_SYM_NAME = 2000
MAX_FRAMES = 512


class DumpError(Exception):
    pass


@dataclass
class Module:
    base: int
    size: int
    name: str
    cv_record: bytes
    verified: bool = False
    note: str = ""

    @property
    def short_name(self) -> str:
        return self.name.replace("\\", "/").rsplit("/", 1)[-1]

    def contains(self, addr: int) -> bool:
        return self.base <= addr < self.base + self.size


@dataclass
class Thread:
    thread_id: int
    stack_start: int
    stack_size: int
    context: bytes
    name: str | None = None

    def reg(self, offset: int) -> int:
        if len(self.context) < offset + 8:
            return 0
        return struct.unpack_from("<Q", self.context, offset)[0]


@dataclass
class Minidump:
    data: bytes
    modules: list[Module] = field(default_factory=list)
    threads: list[Thread] = field(default_factory=list)
    ranges: list[tuple[int, int, int]] = field(default_factory=list)
    exception_thread_id: int | None = None
    exception_code: int | None = None
    exception_address: int | None = None
    exception_context: bytes | None = None
    architecture: int | None = None

    def module_for(self, addr: int) -> Module | None:
        return next((m for m in self.modules if m.contains(addr)), None)

    def read_memory(self, addr: int, size: int) -> bytes | None:
        for start, length, offset in self.ranges:
            if start <= addr and addr + size <= start + length:
                delta = addr - start
                return self.data[offset + delta : offset + delta + size]
        return None


def _loc(data: bytes, off: int) -> tuple[int, int]:
    return struct.unpack_from("<II", data, off)


def _mdstring(data: bytes, rva: int) -> str:
    if rva == 0 or rva + 4 > len(data):
        return ""
    (length,) = struct.unpack_from("<I", data, rva)
    return data[rva + 4 : rva + 4 + length].decode("utf-16-le", "replace")


def parse(path: Path) -> Minidump:
    data = path.read_bytes()
    if len(data) == 0:
        raise DumpError(
            f"{path} is zero bytes. The dump write never completed, so there are no frames "
            "to recover. Look for the sidecar .txt; if there is none either, the reporter "
            "died inside write_minidump before it could record the attempt."
        )
    if len(data) < 32:
        raise DumpError(f"{path} is {len(data)} bytes — too short to be a minidump.")

    signature, _version, n_streams, dir_rva = struct.unpack_from("<IIII", data, 0)
    if signature != MINIDUMP_SIGNATURE:
        raise DumpError(f"{path} is not a minidump (signature 0x{signature:08x}).")

    dump = Minidump(data=data)
    streams: dict[int, tuple[int, int]] = {}
    for i in range(n_streams):
        stream_type, size, rva = struct.unpack_from("<III", data, dir_rva + i * 12)
        streams[stream_type] = (size, rva)

    if STREAM_SYSTEM_INFO in streams:
        (dump.architecture,) = struct.unpack_from("<H", data, streams[STREAM_SYSTEM_INFO][1])

    if STREAM_MODULE_LIST in streams:
        rva = streams[STREAM_MODULE_LIST][1]
        (count,) = struct.unpack_from("<I", data, rva)
        for i in range(count):
            off = rva + 4 + i * 108
            base, size = struct.unpack_from("<QI", data, off)
            (name_rva,) = struct.unpack_from("<I", data, off + 20)
            cv_size, cv_rva = _loc(data, off + 76)
            dump.modules.append(Module(
                base=base, size=size, name=_mdstring(data, name_rva),
                cv_record=data[cv_rva : cv_rva + cv_size] if cv_rva else b"",
            ))

    if STREAM_THREAD_LIST in streams:
        rva = streams[STREAM_THREAD_LIST][1]
        (count,) = struct.unpack_from("<I", data, rva)
        for i in range(count):
            off = rva + 4 + i * 48
            (tid,) = struct.unpack_from("<I", data, off)
            (stack_start,) = struct.unpack_from("<Q", data, off + 24)
            stack_size, _ = _loc(data, off + 32)
            ctx_size, ctx_rva = _loc(data, off + 40)
            dump.threads.append(Thread(
                thread_id=tid, stack_start=stack_start, stack_size=stack_size,
                context=data[ctx_rva : ctx_rva + ctx_size],
            ))

    if STREAM_THREAD_NAMES in streams:
        rva = streams[STREAM_THREAD_NAMES][1]
        (count,) = struct.unpack_from("<I", data, rva)
        names = {}
        for i in range(count):
            off = rva + 4 + i * 16
            (tid,) = struct.unpack_from("<I", data, off)
            (name_rva,) = struct.unpack_from("<Q", data, off + 8)
            names[tid] = _mdstring(data, name_rva)
        for thread in dump.threads:
            thread.name = names.get(thread.thread_id)

    if STREAM_MEMORY_LIST in streams:
        rva = streams[STREAM_MEMORY_LIST][1]
        (count,) = struct.unpack_from("<I", data, rva)
        for i in range(count):
            off = rva + 4 + i * 16
            (start,) = struct.unpack_from("<Q", data, off)
            size, mem_rva = _loc(data, off + 8)
            dump.ranges.append((start, size, mem_rva))

    if STREAM_MEMORY64_LIST in streams:
        rva = streams[STREAM_MEMORY64_LIST][1]
        count, base_rva = struct.unpack_from("<QQ", data, rva)
        cursor = base_rva
        for i in range(count):
            start, size = struct.unpack_from("<QQ", data, rva + 16 + i * 16)
            dump.ranges.append((start, size, cursor))
            cursor += size

    if STREAM_EXCEPTION in streams:
        rva = streams[STREAM_EXCEPTION][1]
        (dump.exception_thread_id,) = struct.unpack_from("<I", data, rva)
        code, _flags, _rec, address = struct.unpack_from("<IIQQ", data, rva + 8)
        dump.exception_code = code
        dump.exception_address = address
        ctx_size, ctx_rva = _loc(data, rva + 160)
        dump.exception_context = data[ctx_rva : ctx_rva + ctx_size]

    return dump


def pe_cv_record(image: Path) -> bytes:
    """Read the RSDS debug record straight out of an on-disk PE."""
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


def describe_cv(record: bytes) -> str:
    if len(record) < 24 or record[:4] != b"RSDS":
        return "<no RSDS record>"
    d1, d2, d3 = struct.unpack_from("<IHH", record, 4)
    d4 = record[12:20]
    (age,) = struct.unpack_from("<I", record, 20)
    pdb = record[24:].split(b"\x00", 1)[0].decode("utf-8", "replace")
    guid = f"{d1:08X}-{d2:04X}-{d3:04X}-{d4[:2].hex().upper()}-{d4[2:].hex().upper()}"
    return f"{guid} age={age} pdb={pdb.replace(chr(92), '/').rsplit('/', 1)[-1]}"


MSF_MAGIC = b"Microsoft C/C++ MSF 7.00" + bytes([13, 10, 26]) + b"DS" + bytes([0, 0, 0])


def pdb_identity(path: Path) -> tuple[str, int] | None:
    """(GUID, age) a PDB claims for itself, read from its MSF PDB-info stream.

    This is the half a symbol file can be checked by when the binary it was linked
    with is gone: a PDB that does not carry the dump's GUID names other functions.
    """
    try:
        with path.open("rb") as handle:
            header = handle.read(56)
            if not header.startswith(MSF_MAGIC):
                return None
            block_size, _free_map, _blocks, dir_bytes, _unknown, block_map = struct.unpack_from(
                "<IIIIII", header, 32)
            if block_size == 0:
                return None

            def read_block(index: int) -> bytes:
                handle.seek(index * block_size)
                return handle.read(block_size)

            n_dir_blocks = -(-dir_bytes // block_size)
            map_bytes = read_block(block_map)
            dir_blocks = struct.unpack_from(f"<{n_dir_blocks}I", map_bytes, 0)
            directory = b"".join(read_block(b) for b in dir_blocks)[:dir_bytes]

            (n_streams,) = struct.unpack_from("<I", directory, 0)
            sizes = struct.unpack_from(f"<{n_streams}I", directory, 4)
            cursor = 4 + n_streams * 4
            # Stream 1 is the PDB info stream; walk past stream 0's block list.
            for index in range(2):
                size = sizes[index] if sizes[index] != 0xFFFFFFFF else 0
                count = -(-size // block_size)
                if index == 1:
                    blocks = struct.unpack_from(f"<{count}I", directory, cursor)
                    info = b"".join(read_block(b) for b in blocks)[:size]
                    if len(info) < 28:
                        return None
                    _version, _signature, age = struct.unpack_from("<III", info, 0)
                    d1, d2, d3 = struct.unpack_from("<IHH", info, 12)
                    d4 = info[20:28]
                    guid = (f"{d1:08X}-{d2:04X}-{d3:04X}-"
                            f"{d4[:2].hex().upper()}-{d4[2:].hex().upper()}")
                    return guid, age
                cursor += count * 4
    except (OSError, struct.error, IndexError):
        return None
    return None


def module_identity(record: bytes) -> tuple[str, int] | None:
    """(GUID, age) a dump's module recorded, in the same shape as pdb_identity."""
    if len(record) < 24 or record[:4] != b"RSDS":
        return None
    d1, d2, d3 = struct.unpack_from("<IHH", record, 4)
    d4 = record[12:20]
    (age,) = struct.unpack_from("<I", record, 20)
    return (f"{d1:08X}-{d2:04X}-{d3:04X}-"
            f"{d4[:2].hex().upper()}-{d4[2:].hex().upper()}"), age


def sidecar_symbols_dir(sidecar: Path) -> Path | None:
    """The `Symbols:` line crash::report writes — where build.sh retained the PDB."""
    try:
        for line in sidecar.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("Symbols:"):
                return Path(line.split(":", 1)[1].strip())
            if line.startswith("Recent log tail"):
                break
    except OSError:
        return None
    return None


def find_pdb(store: Path, wanted: tuple[str, int]) -> Path | None:
    """The PDB under `store` whose own identity matches the dump's module."""
    if not store.is_dir():
        return None
    for candidate in sorted(store.rglob("*.pdb")):
        if pdb_identity(candidate) == wanted:
            return candidate
    return None


class ADDRESS64(ctypes.Structure):
    _fields_ = [("Offset", ctypes.c_uint64), ("Segment", ctypes.c_uint16),
                ("Mode", ctypes.c_uint32)]


class STACKFRAME64(ctypes.Structure):
    _fields_ = [
        ("AddrPC", ADDRESS64), ("AddrReturn", ADDRESS64), ("AddrFrame", ADDRESS64),
        ("AddrStack", ADDRESS64), ("AddrBStore", ADDRESS64),
        ("FuncTableEntry", ctypes.c_void_p), ("Params", ctypes.c_uint64 * 4),
        ("Far", ctypes.c_int), ("Virtual", ctypes.c_int),
        ("Reserved", ctypes.c_uint64 * 3), ("KdHelp", ctypes.c_uint64 * 32),
    ]


class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [
        ("SizeOfStruct", ctypes.c_uint32), ("TypeIndex", ctypes.c_uint32),
        ("Reserved", ctypes.c_uint64 * 2), ("Index", ctypes.c_uint32),
        ("Size", ctypes.c_uint32), ("ModBase", ctypes.c_uint64),
        ("Flags", ctypes.c_uint32), ("Value", ctypes.c_uint64),
        ("Address", ctypes.c_uint64), ("Register", ctypes.c_uint32),
        ("Scope", ctypes.c_uint32), ("Tag", ctypes.c_uint32),
        ("NameLen", ctypes.c_uint32), ("MaxNameLen", ctypes.c_uint32),
        ("Name", ctypes.c_char * MAX_SYM_NAME),
    ]


SYMBOL_INFO_HEADER = 88


class IMAGEHLP_LINE64(ctypes.Structure):
    _fields_ = [("SizeOfStruct", ctypes.c_uint32), ("Key", ctypes.c_void_p),
                ("LineNumber", ctypes.c_uint32), ("FileName", ctypes.c_char_p),
                ("Address", ctypes.c_uint64)]


class GUID(ctypes.Structure):
    _fields_ = [("Data1", ctypes.c_uint32), ("Data2", ctypes.c_uint16),
                ("Data3", ctypes.c_uint16), ("Data4", ctypes.c_ubyte * 8)]


class IMAGEHLP_MODULE64(ctypes.Structure):
    _fields_ = [
        ("SizeOfStruct", ctypes.c_uint32), ("BaseOfImage", ctypes.c_uint64),
        ("ImageSize", ctypes.c_uint32), ("TimeDateStamp", ctypes.c_uint32),
        ("CheckSum", ctypes.c_uint32), ("NumSyms", ctypes.c_uint32),
        ("SymType", ctypes.c_uint32), ("ModuleName", ctypes.c_char * 32),
        ("ImageName", ctypes.c_char * 256), ("LoadedImageName", ctypes.c_char * 256),
        ("LoadedPdbName", ctypes.c_char * 256), ("CVSig", ctypes.c_uint32),
        ("CVData", ctypes.c_char * (260 * 3)), ("PdbSig", ctypes.c_uint32),
        ("PdbSig70", GUID), ("PdbAge", ctypes.c_uint32),
        ("PdbUnmatched", ctypes.c_int), ("DbgUnmatched", ctypes.c_int),
        ("LineNumbers", ctypes.c_int), ("GlobalSymbols", ctypes.c_int),
        ("TypeInfo", ctypes.c_int), ("SourceIndexed", ctypes.c_int),
        ("Publics", ctypes.c_int), ("MachineType", ctypes.c_uint32),
        ("Reserved", ctypes.c_uint32),
    ]


READ_MEMORY = ctypes.WINFUNCTYPE(
    ctypes.c_int, wintypes.HANDLE, ctypes.c_uint64, ctypes.c_void_p,
    ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32))
FUNCTION_TABLE_ACCESS = ctypes.WINFUNCTYPE(ctypes.c_void_p, wintypes.HANDLE, ctypes.c_uint64)
GET_MODULE_BASE = ctypes.WINFUNCTYPE(ctypes.c_uint64, wintypes.HANDLE, ctypes.c_uint64)


class Symbolizer:
    """dbghelp driven against a dump instead of a live process."""

    FAKE_PROCESS = 0x0DEFACED

    def __init__(self, dump: Minidump, symbol_path: str, image_override: dict[int, Path],
                 symbol_files: dict[int, Path] | None = None):
        self.dump = dump
        self.dbghelp = ctypes.WinDLL("dbghelp.dll")
        self._prototypes()
        self.handle = wintypes.HANDLE(self.FAKE_PROCESS)
        self.dbghelp.SymSetOptions(
            SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES
            | SYMOPT_EXACT_SYMBOLS | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS)
        if not self.dbghelp.SymInitialize(self.handle, symbol_path.encode(), False):
            raise DumpError(f"SymInitialize failed: {ctypes.GetLastError()}")

        symbol_files = symbol_files or {}
        for module in dump.modules:
            # A retained PDB stands in for an image that is gone: dbghelp accepts the
            # symbol file itself as the module source, which recovers names even
            # though the unwind info in the binary is not available.
            source = symbol_files.get(module.base)
            if source is None:
                if not module.verified:
                    continue
                source = image_override.get(module.base, Path(module.name))
            self.dbghelp.SymLoadModuleEx(
                self.handle, None, str(source).encode(), None,
                module.base, module.size, None, 0)

        self._read_cb = READ_MEMORY(self._read_memory)
        self._func_cb = FUNCTION_TABLE_ACCESS(self._function_table_access)
        self._base_cb = GET_MODULE_BASE(self._module_base)

    def _prototypes(self) -> None:
        d = self.dbghelp
        d.SymSetOptions.restype = ctypes.c_uint32
        d.SymInitialize.argtypes = [wintypes.HANDLE, ctypes.c_char_p, ctypes.c_int]
        d.SymLoadModuleEx.restype = ctypes.c_uint64
        d.SymLoadModuleEx.argtypes = [
            wintypes.HANDLE, wintypes.HANDLE, ctypes.c_char_p, ctypes.c_char_p,
            ctypes.c_uint64, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_uint32]
        d.SymFunctionTableAccess64.restype = ctypes.c_void_p
        d.SymFunctionTableAccess64.argtypes = [wintypes.HANDLE, ctypes.c_uint64]
        d.SymGetModuleBase64.restype = ctypes.c_uint64
        d.SymGetModuleBase64.argtypes = [wintypes.HANDLE, ctypes.c_uint64]
        d.SymFromAddr.argtypes = [wintypes.HANDLE, ctypes.c_uint64,
                                  ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(SYMBOL_INFO)]
        d.SymGetLineFromAddr64.argtypes = [wintypes.HANDLE, ctypes.c_uint64,
                                           ctypes.POINTER(ctypes.c_uint32),
                                           ctypes.POINTER(IMAGEHLP_LINE64)]
        d.SymGetModuleInfo64.argtypes = [wintypes.HANDLE, ctypes.c_uint64,
                                         ctypes.POINTER(IMAGEHLP_MODULE64)]
        d.StackWalk64.argtypes = [
            ctypes.c_uint32, wintypes.HANDLE, wintypes.HANDLE,
            ctypes.POINTER(STACKFRAME64), ctypes.c_void_p,
            READ_MEMORY, FUNCTION_TABLE_ACCESS, GET_MODULE_BASE, ctypes.c_void_p]

    def _read_memory(self, _handle, address, buffer, size, read_out):
        chunk = self.dump.read_memory(address, size)
        if chunk is None:
            if read_out:
                read_out[0] = 0
            return 0
        ctypes.memmove(buffer, chunk, size)
        if read_out:
            read_out[0] = size
        return 1

    def _function_table_access(self, _handle, address):
        return self.dbghelp.SymFunctionTableAccess64(self.handle, address)

    def _module_base(self, _handle, address):
        base = self.dbghelp.SymGetModuleBase64(self.handle, address)
        if base:
            return base
        module = self.dump.module_for(address)
        return module.base if module else 0

    def module_info(self, base: int) -> IMAGEHLP_MODULE64 | None:
        info = IMAGEHLP_MODULE64()
        info.SizeOfStruct = ctypes.sizeof(IMAGEHLP_MODULE64)
        if not self.dbghelp.SymGetModuleInfo64(self.handle, base, ctypes.byref(info)):
            return None
        return info

    def describe(self, address: int) -> str:
        module = self.dump.module_for(address)
        if module is None:
            return f"0x{address:016x}  <address in no loaded module>"
        located = f"{module.short_name}+0x{address - module.base:x}"
        if not module.verified:
            return f"0x{address:016x}  {located}  [SYMBOLS WITHHELD: {module.note}]"

        sym = SYMBOL_INFO()
        sym.SizeOfStruct = SYMBOL_INFO_HEADER
        sym.MaxNameLen = MAX_SYM_NAME - 1
        displacement = ctypes.c_uint64(0)
        if not self.dbghelp.SymFromAddr(self.handle, address,
                                        ctypes.byref(displacement), ctypes.byref(sym)):
            return f"0x{address:016x}  {located}  <no symbol for this address>"

        text = f"0x{address:016x}  {sym.Name.decode('utf-8', 'replace')}+0x{displacement.value:x}"
        line = IMAGEHLP_LINE64()
        line.SizeOfStruct = ctypes.sizeof(IMAGEHLP_LINE64)
        line_disp = ctypes.c_uint32(0)
        if self.dbghelp.SymGetLineFromAddr64(self.handle, address,
                                             ctypes.byref(line_disp), ctypes.byref(line)):
            file_name = (line.FileName or b"").decode("utf-8", "replace")
            text += f"\n{'':>28}at {file_name}:{line.LineNumber}"
        return text

    def walk(self, context: bytes) -> list[int]:
        if len(context) < CTX_SIZE:
            return []
        ctx = ctypes.create_string_buffer(context, len(context))
        frame = STACKFRAME64()
        frame.AddrPC.Offset = struct.unpack_from("<Q", context, CTX_RIP)[0]
        frame.AddrStack.Offset = struct.unpack_from("<Q", context, CTX_RSP)[0]
        frame.AddrFrame.Offset = struct.unpack_from("<Q", context, CTX_RBP)[0]
        frame.AddrPC.Mode = frame.AddrStack.Mode = frame.AddrFrame.Mode = 3  # AddrModeFlat

        seen: set[int] = set()
        addresses: list[int] = []
        for _ in range(MAX_FRAMES):
            ok = self.dbghelp.StackWalk64(
                IMAGE_FILE_MACHINE_AMD64, self.handle, wintypes.HANDLE(0),
                ctypes.byref(frame), ctypes.cast(ctx, ctypes.c_void_p),
                self._read_cb, self._func_cb, self._base_cb, None)
            if not ok or frame.AddrPC.Offset == 0:
                break
            key = (frame.AddrPC.Offset, frame.AddrStack.Offset)
            if key in seen:
                break
            seen.add(key)
            addresses.append(frame.AddrPC.Offset)
            if frame.AddrReturn.Offset == 0:
                break
        return addresses

    def scan(self, thread: Thread) -> list[tuple[int, int]]:
        """Every code-range pointer on the captured stack, low address first."""
        rsp = thread.reg(CTX_RSP) or thread.stack_start
        top = thread.stack_start + thread.stack_size
        hits: list[tuple[int, int]] = []
        cursor = rsp & ~7
        while cursor < top:
            chunk = self.dump.read_memory(cursor, 8)
            if chunk is None or len(chunk) < 8:
                cursor += 8
                continue
            (value,) = struct.unpack("<Q", chunk)
            if self.dump.module_for(value) is not None:
                hits.append((cursor, value))
            cursor += 8
        return hits


MS_SYMBOL_SERVER = "https://msdl.microsoft.com/download/symbols"


def cv_key(record: bytes) -> tuple[str, str] | None:
    """(pdb file name, GUID+age key) as the Microsoft symbol server indexes it."""
    if len(record) < 24 or record[:4] != b"RSDS":
        return None
    d1, d2, d3 = struct.unpack_from("<IHH", record, 4)
    d4 = record[12:20]
    (age,) = struct.unpack_from("<I", record, 20)
    name = record[24:].split(b"\x00", 1)[0].decode("utf-8", "replace")
    name = name.replace("\\", "/").rsplit("/", 1)[-1]
    return name, f"{d1:08X}{d2:04X}{d3:04X}{d4.hex().upper()}{age:X}"


def fetch_symbols(dump: Minidump, destination: Path) -> None:
    """Pull public PDBs for the verified modules. System32's dbghelp ships without
    symsrv.dll, so the symbol-server request is made directly."""
    import urllib.error
    import urllib.request

    destination.mkdir(parents=True, exist_ok=True)
    for module in dump.modules:
        key = cv_key(module.cv_record)
        if not module.verified or key is None:
            continue
        pdb_name, identifier = key
        target = destination / pdb_name
        if target.exists():
            continue
        url = f"{MS_SYMBOL_SERVER}/{pdb_name}/{identifier}/{pdb_name}"
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                target.write_bytes(response.read())
            print(f"  fetched {pdb_name}")
        except (urllib.error.URLError, OSError) as exc:
            print(f"  no public symbols for {pdb_name}: {exc}")
    print()


def verify_modules(dump: Minidump, image_override: dict[int, Path]) -> None:
    for module in dump.modules:
        path = image_override.get(module.base, Path(module.name))
        disk = pe_cv_record(path)
        if not module.cv_record:
            module.note = "no RSDS record in the dump"
        elif not disk:
            module.note = f"image not readable at {path}"
        elif disk[:24] != module.cv_record[:24]:
            module.note = "on-disk image is a DIFFERENT BUILD"
        else:
            module.verified = True


def resolve_symbol_file(args, crash_module: Module | None) -> tuple[dict[int, Path], str]:
    """Pick a symbol file for the crashing module and prove it belongs to this dump.

    A PDB is only accepted when its own recorded GUID and age match the module's
    RSDS. That is the whole point of retaining one: the binary may be gone, but a
    symbol file that cannot prove its identity names other functions.
    """
    if crash_module is None:
        return {}, "no crashing module identified"

    wanted = module_identity(crash_module.cv_record)
    if wanted is None:
        return {}, "the dump records no RSDS for the crashing module"

    candidates: list[Path] = []
    if args.pdb:
        candidates.append(args.pdb)
    stores: list[Path] = []
    if args.sidecar:
        from_sidecar = sidecar_symbols_dir(args.sidecar)
        if from_sidecar is None:
            return {}, f"{args.sidecar} carries no Symbols: line"
        stores.append(from_sidecar)
    if args.pdb_store:
        stores.append(args.pdb_store)
    for store in stores:
        found = find_pdb(store, wanted)
        if found is not None:
            candidates.append(found)
        else:
            return {}, f"no symbol file in {store} matches {wanted[0]} age={wanted[1]}"

    for candidate in candidates:
        identity = pdb_identity(candidate)
        if identity is None:
            return {}, f"{candidate} is not a readable PDB"
        if identity != wanted:
            return {}, (f"{candidate} is {identity[0]} age={identity[1]}, the dump wants "
                        f"{wanted[0]} age={wanted[1]} - REFUSED")
        return {crash_module.base: candidate}, f"{candidate} (identity verified)"

    return {}, "none supplied"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("dump", type=Path)
    parser.add_argument("--image", type=Path, default=None,
                        help="the exact main binary that crashed, if it is not at its recorded path")
    parser.add_argument("--symbol-path", default=None, help="dbghelp symbol path")
    parser.add_argument("--pdb", type=Path, default=None,
                        help="symbol file for the crashing module, used when its binary is gone")
    parser.add_argument("--pdb-store", type=Path, default=None,
                        help="directory of retained symbol files to search by identity")
    parser.add_argument("--sidecar", type=Path, default=None,
                        help="crash .txt whose Symbols: line names the retained symbol directory")
    parser.add_argument("--all-threads", action="store_true")
    parser.add_argument("--scan", action="store_true",
                        help="raw stack scan (use when the main module fails verification)")
    parser.add_argument("--modules", action="store_true", help="print the module identity table")
    parser.add_argument("--fetch-symbols", type=Path, default=None,
                        help="download public PDBs for the verified modules into this directory")
    args = parser.parse_args()

    if sys.platform != "win32":
        print("This reader drives dbghelp.dll and only runs on Windows.", file=sys.stderr)
        return 2

    try:
        dump = parse(args.dump)
    except DumpError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    override: dict[int, Path] = {}
    if args.image:
        target = next((m for m in dump.modules
                       if m.short_name.lower() == args.image.name.lower()), None)
        if target is None:
            print(f"ERROR: {args.image.name} is not among the dump's modules.", file=sys.stderr)
            return 1
        override[target.base] = args.image

    verify_modules(dump, override)

    captured_rip = (
        struct.unpack_from("<Q", dump.exception_context, CTX_RIP)[0]
        if dump.exception_context and len(dump.exception_context) >= CTX_SIZE
        else 0
    )
    crash_module = dump.module_for(captured_rip)
    if crash_module is None and override:
        crash_module = next(m for m in dump.modules if m.base in override)

    symbol_files, symbol_note = resolve_symbol_file(args, crash_module)
    symbols_without_image = bool(symbol_files) and crash_module is not None         and not crash_module.verified
    if symbols_without_image:
        crash_module.verified = True

    print(f"Dump         : {args.dump}  ({args.dump.stat().st_size} bytes)")
    print(f"Threads      : {len(dump.threads)}    Modules: {len(dump.modules)}"
          f"    Verified: {sum(1 for m in dump.modules if m.verified)}")
    if dump.exception_code is not None:
        print(f"Exception    : 0x{dump.exception_code:08x} "
              f"ExceptionAddress=0x{dump.exception_address:016x}")
        print(f"Faulting tid : {dump.exception_thread_id}")
    if dump.exception_context and len(dump.exception_context) >= CTX_SIZE:
        rip = struct.unpack_from("<Q", dump.exception_context, CTX_RIP)[0]
        print(f"Captured RIP : 0x{rip:016x}")
    print()

    if args.modules:
        for module in sorted(dump.modules, key=lambda m: m.base):
            flag = "OK " if module.verified else "!! "
            print(f"{flag}0x{module.base:016x} +0x{module.size:08x} {module.short_name:<32} "
                  f"{describe_cv(module.cv_record)}"
                  f"{'' if module.verified else '   <- ' + module.note}")
        print()

    if args.fetch_symbols:
        print(f"Fetching public symbols into {args.fetch_symbols} ...")
        fetch_symbols(dump, args.fetch_symbols)

    search = [str(args.image.parent)] if args.image else []
    if args.fetch_symbols:
        search.append(str(args.fetch_symbols))
    symbol_path = args.symbol_path or ";".join(search) or "."
    print(f"Symbol file  : {symbol_note}")
    sym = Symbolizer(dump, symbol_path, override, symbol_files)

    main_module = crash_module
    if main_module is not None:
        info = sym.module_info(main_module.base) if main_module.verified else None
        state = (f"{SYM_TYPE_NAMES.get(info.SymType, info.SymType)} "
                 f"pdb={info.LoadedPdbName.decode('utf-8', 'replace') or '<none>'} "
                 f"unmatched={bool(info.PdbUnmatched)}") if info else "NOT LOADED"
        print(f"Crash module : {main_module.short_name}  symbols={state}")
        if not main_module.verified:
            print()
            print("!" * 78)
            print(f"! {main_module.short_name} FAILED IDENTITY VERIFICATION: {main_module.note}.")
            print("! Its frames are reported as module+RVA only. x64 unwinding also needs this")
            print("! image's .pdata, so StackWalk64 through it is not trustworthy either -")
            print("! use --scan and read the result as a superset.")
            print("! A symbol file retained for this build recovers the names: pass --pdb,")
            print("! --pdb-store, or --sidecar <the crash .txt>.")
            print("!" * 78)
        elif symbols_without_image:
            print()
            print("-" * 78)
            print(f"- Names come from the retained symbol file; {main_module.short_name} itself")
            print("- is gone, so x64 unwind data is not available. Frame NAMES are trustworthy,")
            print("- the StackWalk64 frame ORDER is not. Read --scan as the authority.")
            print("-" * 78)
        print()

    faulting = [t for t in dump.threads if t.thread_id == dump.exception_thread_id]
    others = [t for t in dump.threads if t.thread_id != dump.exception_thread_id]

    for thread in faulting:
        context = dump.exception_context or thread.context
        print("=" * 78)
        print(f"FAULTING THREAD {thread.thread_id}"
              f"{f' ({thread.name})' if thread.name else ''}   "
              f"stack=0x{thread.stack_start:x}+0x{thread.stack_size:x}")
        print("=" * 78)
        frames = sym.walk(context)
        if frames:
            print("-- StackWalk64 --")
            for i, addr in enumerate(frames):
                print(f"  #{i:<3} {sym.describe(addr)}")
        if args.scan:
            print("-- raw stack scan (superset: includes stale frames and data) --")
            for slot, addr in sym.scan(thread):
                print(f"  [0x{slot:012x}] {sym.describe(addr)}")
        print()

    if args.all_threads:
        for thread in others:
            frames = sym.walk(thread.context)
            print("-" * 78)
            print(f"thread {thread.thread_id}"
                  f"{f' ({thread.name})' if thread.name else ''}")
            for i, addr in enumerate(frames):
                print(f"  #{i:<3} {sym.describe(addr)}")
            print()

    return 0


if __name__ == "__main__":
    sys.exit(main())
