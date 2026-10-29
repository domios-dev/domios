import ctypes
import json
import os
import select
import signal
import subprocess
import sys


class BsdInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint32) for name in (
        'flags', 'status', 'xstatus', 'pid', 'ppid', 'uid', 'gid', 'ruid',
        'rgid', 'svuid', 'svgid', 'rfu')]
    _fields_ += [('comm', ctypes.c_char * 16), ('name', ctypes.c_char * 32)]
    _fields_ += [(name, ctypes.c_uint32) for name in (
        'nfiles', 'pgid', 'pjobc', 'tdev', 'tpgid', 'nice')]
    _fields_ += [('seconds', ctypes.c_uint64), ('microseconds', ctypes.c_uint64)]


def observe(pid):
    if sys.platform == 'linux':
        with open(f'/proc/{pid}/stat') as source:
            fields = source.read().rsplit(')', 1)[1].split()
        return dict(pid=pid, parent=int(fields[1]), created=fields[19],
                    executable=os.path.realpath(os.readlink(f'/proc/{pid}/exe')))
    library = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
    library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
    library.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
    info = BsdInfo()
    image = ctypes.create_string_buffer(4096)
    if library.proc_pidinfo(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info)) != ctypes.sizeof(info):
        raise ProcessLookupError(pid)
    if library.proc_pidpath(pid, image, len(image)) <= 0:
        raise ProcessLookupError(pid)
    return dict(pid=pid, parent=info.ppid, created=f'{info.seconds}:{info.microseconds}',
                executable=os.path.realpath(os.fsdecode(image.value)))


def processes():
    if sys.platform == 'linux':
        ids = [int(name) for name in os.listdir('/proc') if name.isdigit()]
    else:
        ids = [int(value) for value in subprocess.check_output(['ps', '-axo', 'pid='], text=True).split()]
    rows = []
    for pid in ids:
        try:
            rows.append(observe(pid))
        except (OSError, ValueError):
            pass
    return rows


def production_listeners():
    if sys.platform != 'linux':
        result = subprocess.run(['lsof', '-nP', '-tiTCP:7685', '-sTCP:LISTEN'], capture_output=True, text=True)
        if result.returncode not in (0, 1):
            raise RuntimeError('Production listener identity unavailable')
        return [dict(pid=int(pid), port=7685) for pid in result.stdout.split()]
    inodes = set()
    for source in ('/proc/net/tcp', '/proc/net/tcp6'):
        with open(source) as table:
            for line in table.readlines()[1:]:
                fields = line.split()
                if int(fields[1].split(':')[1], 16) == 7685 and fields[3] == '0A':
                    inodes.add(fields[9])
    owners = []
    if not inodes:
        return owners
    for row in processes():
        try:
            for descriptor in os.scandir(f'/proc/{row["pid"]}/fd'):
                try:
                    if os.readlink(descriptor.path) in {f'socket:[{inode}]' for inode in inodes}:
                        owners.append(dict(pid=row['pid'], port=7685))
                except OSError:
                    pass
        except OSError:
            pass
    return owners


def stop_tree(request):
    root = os.path.realpath(request['root']) + '/'
    pinned = []
    try:
        for row in request['processes']:
            pid = row['pid']
            try:
                if sys.platform == 'linux':
                    handle = os.pidfd_open(pid)
                else:
                    handle = select.kqueue()
                    event = select.kevent(pid, filter=select.KQ_FILTER_PROC,
                                          flags=select.KQ_EV_ADD | select.KQ_EV_ENABLE,
                                          fflags=select.KQ_NOTE_EXIT)
                    try:
                        handle.control([event], 0, 0)
                    except OSError:
                        handle.close()
                        raise
            except (ProcessLookupError, FileNotFoundError):
                continue
            pinned.append((handle, row))
            try:
                current = observe(pid)
            except (ProcessLookupError, FileNotFoundError):
                continue
            if not same_process(current, row) or not current['executable'].startswith(root):
                raise RuntimeError('DEV instance image or creation identity changed; refusing cleanup')
        ids = {row['pid'] for _, row in pinned}
        if any(row['pid'] in ids for row in production_listeners()):
            raise RuntimeError('DEV instance image owns production port 7685; refusing cleanup')
        for handle, row in pinned:
            if sys.platform == 'linux':
                try:
                    signal.pidfd_send_signal(handle, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            else:
                try:
                    current = observe(row['pid'])
                except (ProcessLookupError, FileNotFoundError):
                    continue
                if not same_process(current, row):
                    raise RuntimeError('DEV process creation identity changed')
                try:
                    os.kill(row['pid'], signal.SIGKILL)
                except ProcessLookupError:
                    pass
        for handle, _ in pinned:
            if sys.platform == 'linux':
                events = select.poll()
                events.register(handle, select.POLLIN)
                if not events.poll(10000):
                    raise RuntimeError('DEV pinned process exit deadline; refusing staging')
            elif not handle.control(None, 1, 10):
                raise RuntimeError('DEV pinned process exit deadline; refusing staging')
        if any(row['executable'].startswith(root) for row in processes()):
            raise RuntimeError('DEV instance tree still has processes; refusing staging')
        return dict(stopped=[row for _, row in pinned])
    finally:
        for handle, _ in pinned:
            if sys.platform == 'linux':
                os.close(handle)
            else:
                handle.close()


def same_process(current, expected):
    return all(current[key] == expected[key] for key in ('pid', 'created', 'executable'))


def main():
    request = json.load(sys.stdin)
    if sys.argv[1] == 'Inspect':
        result = dict(processes=processes(), listeners=production_listeners())
    elif sys.argv[1] == 'StopTree' and 0 < request['port'] < 65536 and request['port'] != 7685:
        result = stop_tree(request)
    else:
        raise RuntimeError('Invalid DEV cleanup operation or production port')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
