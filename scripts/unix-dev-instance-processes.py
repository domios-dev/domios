import ctypes
import json
import os
import select
import signal
import shlex
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


def production_listeners(ports=(7685,)):
    if sys.platform != 'linux':
        owners = []
        for port in ports:
            result = subprocess.run(['lsof', '-nP', f'-tiTCP:{port}', '-sTCP:LISTEN'], capture_output=True, text=True)
            if result.returncode not in (0, 1):
                raise RuntimeError('Production listener identity unavailable')
            owners += [dict(pid=int(pid), port=port) for pid in set(result.stdout.split())]
        return owners
    inodes = {}
    for source in ('/proc/net/tcp', '/proc/net/tcp6'):
        with open(source) as table:
            for line in table.readlines()[1:]:
                fields = line.split()
                port = int(fields[1].split(':')[1], 16)
                if port in ports and fields[3] == '0A':
                    inodes[f'socket:[{fields[9]}]'] = port
    owners = []
    if not inodes:
        return owners
    for row in processes():
        try:
            for descriptor in os.scandir(f'/proc/{row["pid"]}/fd'):
                try:
                    port = inodes.get(os.readlink(descriptor.path))
                    if port is not None:
                        owners.append(dict(pid=row['pid'], port=port))
                except OSError:
                    pass
        except OSError:
            pass
    return owners


def stop_tree(request):
    root = os.path.join(os.path.realpath(request['root']), '')
    images = set(request.get('images') or [])
    selected = lambda row: row['executable'].startswith(root) and (not images or row['executable'] in images)
    pinned = []
    exited = set()
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
            if not same_process(current, row) or not selected(current):
                raise RuntimeError('DEV instance image or creation identity changed; refusing cleanup')
        ids = {row['pid'] for _, row in pinned}
        if any(row['pid'] in ids for row in production_listeners()):
            raise RuntimeError('DEV instance image owns production port 7685; refusing cleanup')
        for handle, row in pinned:
            if row['pid'] in request.get('gracefulPids', []) and wait_exit(handle, 10000):
                exited.add(row['pid'])
        for handle, row in pinned:
            if row['pid'] in exited:
                continue
            if sys.platform == 'linux':
                try:
                    signal.pidfd_send_signal(handle, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            else:
                if handle.control(None, 1, 0):
                    exited.add(row['pid'])
                    continue
                try:
                    current = observe(row['pid'])
                except (ProcessLookupError, FileNotFoundError):
                    continue
                if not same_process(current, row):
                    if handle.control(None, 1, 0):
                        exited.add(row['pid'])
                        continue
                    raise RuntimeError('DEV process creation identity changed')
                if handle.control(None, 1, 0):
                    exited.add(row['pid'])
                    continue
                try:
                    os.kill(row['pid'], signal.SIGKILL)
                except ProcessLookupError:
                    pass
        for handle, row in pinned:
            if row['pid'] not in exited and not wait_exit(handle, 10000):
                raise RuntimeError('DEV pinned process exit deadline; refusing staging')
        if any(selected(row) and (not request.get('onlyPinned') or any(same_process(row, expected)
               for expected in request['processes'])) for row in processes()):
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


def wait_exit(handle, milliseconds):
    if sys.platform == 'linux':
        events = select.poll()
        events.register(handle, select.POLLIN)
        return bool(events.poll(milliseconds))
    return bool(handle.control(None, 1, milliseconds / 1000))


def stop_frontend(request):
    repo = os.path.realpath(request['repo'])
    port = request['port']
    result = subprocess.run(['lsof', '-nP', f'-tiTCP:{port}', '-sTCP:LISTEN'], capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise RuntimeError('DEV frontend listener identity unavailable')
    rows = []
    for pid in {int(value) for value in result.stdout.split()}:
        try:
            row = observe(pid)
            if sys.platform == 'linux':
                cwd = os.path.realpath(f'/proc/{pid}/cwd')
                with open(f'/proc/{pid}/cmdline', 'rb') as source:
                    args = [os.fsdecode(value) for value in source.read().split(b'\0') if value]
            else:
                cwd_result = subprocess.check_output(['lsof', '-a', '-p', str(pid), '-d', 'cwd', '-Fn'], text=True)
                cwd = next(line[1:] for line in cwd_result.splitlines() if line.startswith('n'))
                args = shlex.split(subprocess.check_output(['ps', '-p', str(pid), '-o', 'command='], text=True))
            script = os.path.join(repo, 'node_modules/vite/bin/vite.js')
            if os.path.realpath(cwd) != repo or not any(os.path.realpath(os.path.join(cwd, value)) == script for value in args[1:]):
                continue
            selected_port = any(value == f'--port={port}' or
                                (value == '--port' and index + 1 < len(args) and args[index + 1] == str(port))
                                for index, value in enumerate(args))
            if selected_port:
                rows.append(row)
        except (OSError, ValueError, StopIteration):
            continue
    return stop_tree(dict(root='/', port=port, processes=rows, images=[row['executable'] for row in rows], onlyPinned=True))


def main():
    request = json.load(sys.stdin)
    if sys.argv[1] == 'Inspect':
        ports = {7685, *(port for port in request.get('ports', []) if isinstance(port, int) and 0 < port < 65536)}
        result = dict(processes=processes(), listeners=production_listeners(tuple(sorted(ports))))
    elif sys.argv[1] == 'StopTree' and 0 < request['port'] < 65536 and request['port'] != 7685:
        result = stop_tree(request)
    elif sys.argv[1] == 'StopFrontend' and 0 < request['port'] < 65536 and request['port'] != 7685:
        result = stop_frontend(request)
    else:
        raise RuntimeError('Invalid DEV cleanup operation or production port')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
