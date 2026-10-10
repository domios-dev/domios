import ctypes
import os
import struct

from jev_request_worker import WorkerError, require


MOUNT_POINT_TAG = 0xA0000003
SET_REPARSE = 0x900A4
GET_REPARSE = 0x900A8


def mount_point_buffer(target):
    print_name = str(target).encode("utf-16-le")
    substitute = ("\\??\\" + str(target)).encode("utf-16-le")
    paths = substitute + b"\0\0" + print_name + b"\0\0"
    require(len(paths) + 16 <= 16384, "windows_fixture_reparse_size")
    return struct.pack("<IHHHHHH", MOUNT_POINT_TAG, len(paths) + 8, 0,
                       0, len(substitute), len(substitute) + 2, len(print_name)) + paths


def create_junction(guard, path, target, descriptor):
    api = guard.api
    guard.check_time()
    guard.revalidate()
    api.create_directory(path, descriptor)
    handle = api.kernel.CreateFileW(str(path), 0x40020080, 1, None, 3, 0x02200000, None)
    if handle in (None, 0, ctypes.c_void_p(-1).value):
        raise WorkerError("windows_reparse_control_unavailable", reparse_creation_os={"winerror": ctypes.get_last_error()})
    owner = api.captured("file", handle)
    with api.retiring(owner):
        facts = api.inspect(handle)
        require(facts["directory"] and os.path.normcase(facts["final"]) == os.path.normcase(str(path)),
                "windows_fixture_identity")
        identity = facts["identity"]
        function = api.kernel.DeviceIoControl
        function.argtypes = (ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_uint32,
                             ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p)
        function.restype = ctypes.c_int32
        raw = mount_point_buffer(target)
        data, returned = ctypes.create_string_buffer(raw), ctypes.c_uint32()
        guard.check_time()
        if not function(handle, SET_REPARSE, data, len(raw), None, 0, ctypes.byref(returned), None):
            raise WorkerError("windows_reparse_control_unavailable", reparse_creation_os={"winerror": ctypes.get_last_error()})
        guard.check_time()
        output = ctypes.create_string_buffer(16384)
        if not function(handle, GET_REPARSE, None, 0, output, len(output), ctypes.byref(returned), None):
            raise WorkerError("windows_fixture_reparse_read", reparse_creation_os={"winerror": ctypes.get_last_error()})
        require(returned.value == len(raw) and output.raw[:returned.value] == raw, "windows_fixture_reparse_data")
        after = api.identify(handle)
        require(after["identity"] == identity and after["attributes"] & 0x400 and
                os.path.normcase(after["final"]) == os.path.normcase(str(path)), "windows_fixture_identity")
        guard.check_time()
    return {"kind": "directory_junction", "tag": MOUNT_POINT_TAG, "identity": identity.hex()}
