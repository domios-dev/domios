import ctypes
from ctypes import wintypes
import socket
import struct


class TcpRow(ctypes.Structure):
    _fields_ = [(name, wintypes.DWORD) for name in (
        "state", "local_address", "local_port", "remote_address", "remote_port", "pid",
    )]


def connection_pid(connection):
    library = ctypes.WinDLL("iphlpapi", use_last_error=True)
    table = library.GetExtendedTcpTable
    table.argtypes = [ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), wintypes.BOOL,
                      wintypes.ULONG, ctypes.c_int, wintypes.ULONG]
    table.restype = wintypes.DWORD
    size = wintypes.DWORD()
    result = table(None, ctypes.byref(size), False, socket.AF_INET, 5, 0)
    if result != 122:
        raise OSError(result, "Cannot size the TCP owner table")
    storage = ctypes.create_string_buffer(size.value)
    result = table(storage, ctypes.byref(size), False, socket.AF_INET, 5, 0)
    if result:
        raise OSError(result, "Cannot read the TCP owner table")
    count = wintypes.DWORD.from_buffer(storage).value
    offset = ctypes.sizeof(wintypes.DWORD)
    if offset + count * ctypes.sizeof(TcpRow) > len(storage):
        raise RuntimeError("TCP owner table exceeds its returned buffer")
    client = connection.getsockname()
    server = connection.getpeername()
    address = lambda value: socket.inet_ntoa(struct.pack("=I", value))
    pids = set()
    for index in range(count):
        row = TcpRow.from_buffer(storage, offset + index * ctypes.sizeof(TcpRow))
        local = (address(row.local_address), socket.ntohs(row.local_port & 0xffff))
        remote = (address(row.remote_address), socket.ntohs(row.remote_port & 0xffff))
        if local == server and remote == client and row.pid:
            pids.add(row.pid)
    if len(pids) != 1:
        raise RuntimeError(f"Expected one owner of the held connection, found {sorted(pids)}")
    return pids.pop()


def process_start(pid):
    library = ctypes.WinDLL("kernel32", use_last_error=True)
    library.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    library.OpenProcess.restype = wintypes.HANDLE
    library.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    library.GetProcessTimes.restype = wintypes.BOOL
    library.CloseHandle.argtypes = [wintypes.HANDLE]
    library.CloseHandle.restype = wintypes.BOOL
    handle = library.OpenProcess(0x1000, False, pid)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        created, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
        if not library.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                       ctypes.byref(kernel), ctypes.byref(user)):
            raise ctypes.WinError(ctypes.get_last_error())
        ticks = (created.dwHighDateTime << 32) | created.dwLowDateTime
        if ticks <= 116444736000000000:
            raise RuntimeError("Invalid process creation time")
        return (ticks - 116444736000000000) // 10
    finally:
        library.CloseHandle(handle)


def identity(port):
    with socket.create_connection(("127.0.0.1", port), timeout=10) as connection:
        pid = connection_pid(connection)
        return {"pid": pid, "started_us": process_start(pid)}
