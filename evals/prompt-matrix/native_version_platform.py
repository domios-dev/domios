"""Direct native --version ownership only; no descendant or process-group authority."""

import errno
import math
import os
import selectors
import signal
import subprocess
import sys
import threading
import time
import uuid

from native_layout import demand

READ_SIZE = 4096
UNCONFIRMED_OWNERS = []


def error_fact(error):
    return {"type": "OSError" if isinstance(error, OSError) else "Exception",
            "errno": error.errno if isinstance(error, OSError) and type(error.errno) is int and
            -2147483648 <= error.errno <= 2147483647 else None}


class ChildAuthority:
    def __init__(self, clock=time.monotonic):
        self.clock = clock
        self.lock = threading.Lock()
        self.child = None
        self.cancelled = False
        self.signal_attempted = False
        self.signalling = False
        self.observed = False
        self.collected = False
        self.reaping = False
        self.lost = False
        self.code = None
        self.error = None
        self.captured_at = self.observed_at = self.collected_at = self.cancelled_at = self.signal_finished_at = None
        self.signal_succeeded = None
        self.signal_finished = threading.Event()
        self.signal_finished.set()

    def capture(self, child):
        with self.lock:
            demand(self.child is None, "Native child already captured")
            self.child = child
            self.captured_at = self.clock()
            return self.cancelled

    def cancel(self, terminate):
        with self.lock:
            self.cancelled = True
            if self.cancelled_at is None:
                self.cancelled_at = self.clock()
            if self.child is None or self.observed or self.collected or self.reaping or self.lost or self.signal_attempted:
                return False
            self.signal_attempted, self.signalling = True, True
            self.signal_finished.clear()
            child = self.child
        try:
            terminate(child)
            with self.lock:
                self.signal_succeeded = True
            return True
        except BaseException as error:
            with self.lock:
                self.error = error_fact(error)
                self.signal_succeeded = False
            return False
        finally:
            with self.lock:
                self.signalling = False
                self.signal_finished_at = self.clock()
            self.signal_finished.set()

    def observe(self, code):
        with self.lock:
            self.code, self.observed = code, True
            self.observed_at = self.clock()

    def lose(self, error):
        with self.lock:
            self.lost, self.error = True, error_fact(error)

    def collect(self, reap):
        with self.lock:
            if self.collected:
                return
            demand(self.observed and not self.lost and not self.signalling and not self.reaping,
                   "Native child ownership unavailable for final collection")
            self.reaping = True
            child, expected = self.child, self.code
        try:
            actual = reap(child)
            demand(type(actual) is int and actual == expected, "Native final exit differs from observation")
            with self.lock:
                self.collected = True
                self.collected_at = self.clock()
        except BaseException as error:
            self.lose(error)
            raise
        finally:
            with self.lock:
                self.reaping = False

    def facts(self):
        with self.lock:
            return {"captured": self.child is not None, "exit_observed": self.observed,
                    "exit_collected": self.collected, "exit_code": self.code, "ownership_lost": self.lost,
                    "cancel_requested": self.cancelled, "signal_attempted": self.signal_attempted,
                    "error": self.error, "captured_at": self.captured_at, "exit_observed_at": self.observed_at,
                    "exit_collected_at": self.collected_at, "cancel_requested_at": self.cancelled_at,
                    "signal_succeeded": self.signal_succeeded, "signal_finished_at": self.signal_finished_at}


def unix_preflight(platform=sys.platform, version=None, api=os, handler=None):
    version = sys.version_info[:2] if version is None else version
    demand(platform in ("darwin", "linux") and version >= (3, 13), "Native waitid interpreter unsupported")
    demand(all(hasattr(api, name) for name in ("waitid", "waitpid", "waitstatus_to_exitcode", "P_PID", "WEXITED",
                                             "WNOWAIT", "CLD_EXITED", "kill", "set_blocking", "pipe")),
           "Native Unix ownership APIs unavailable")
    handler = signal.getsignal(signal.SIGCHLD) if handler is None else handler
    demand(handler == signal.SIG_DFL, "Native sole-reaper SIGCHLD authority unavailable")


class UnixOwner:
    kind = "unix_waitid_sole_reaper"

    def __init__(self, request, environment, clock, factory=subprocess.Popen, api=os,
                 selector_factory=selectors.DefaultSelector, thread_factory=threading.Thread):
        self.request, self.environment, self.clock, self.factory, self.api = request, environment, clock, factory, api
        self.authority = ChildAuthority(clock)
        self.finished = threading.Event()
        self.joined, self.started = False, False
        self.joined_at = self.handles_closed_at = None
        self.abandoned = False
        self.streams, self.closed_streams = {}, set()
        self.selector, self.wake_read, self.wake_write = None, None, None
        self.wakes_closed, self.selector_closed = False, False
        try:
            self.selector = selector_factory()
            self.wake_read, self.wake_write = api.pipe()
            api.set_blocking(self.wake_read, False)
            api.set_blocking(self.wake_write, False)
            self.selector.register(self.wake_read, selectors.EVENT_READ, "wake")
            self.thread = thread_factory(target=self.launch_observe, daemon=False)
        except BaseException:
            UNCONFIRMED_OWNERS.append(self)
            self.close_wake()
            raise

    def notify(self):
        try:
            self.api.write(self.wake_write, b"x")
        except OSError as error:
            if error.errno not in (errno.EAGAIN, errno.EWOULDBLOCK):
                self.authority.lose(error)

    def terminate(self, child):
        self.api.kill(child.pid, signal.SIGKILL)

    def cancel(self):
        return self.authority.cancel(self.terminate)

    def reap(self, child):
        try:
            pid, status = self.api.waitpid(child.pid, 0)
            demand(pid == child.pid, "Native final reap is not the owned child")
            code = self.api.waitstatus_to_exitcode(status)
            child.returncode = code
            return code
        except BaseException:
            child.returncode = -255
            raise

    def launch_observe(self):
        try:
            demand(not self.authority.cancelled and self.clock() < self.request["operation_deadline"],
                   "Native cancelled before creation")
            child = self.factory(self.request["argv"], cwd=self.request["cwd"], env=self.environment,
                                 shell=False, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, close_fds=True, bufsize=0)
            cancelled = self.authority.capture(child)
            if cancelled or self.clock() >= self.request["operation_deadline"]:
                self.cancel()
            self.notify()
            value = self.api.waitid(self.api.P_PID, child.pid, self.api.WEXITED | self.api.WNOWAIT)
            demand(value is not None and value.si_pid == child.pid, "Native exit observation identity differs")
            code = value.si_status if value.si_code == self.api.CLD_EXITED else -value.si_status
            self.authority.observe(code)
            self.notify()
            self.authority.signal_finished.wait()
            self.authority.collect(self.reap)
        except BaseException as error:
            self.authority.lose(error)
            self.cancel()
            child = self.authority.child
            if child is not None and self.authority.lost:
                child.returncode = -255
        finally:
            if self.abandoned:
                self.late_cleanup()
            else:
                self.notify()
            self.finished.set()

    def start(self):
        try:
            self.thread.start()
            self.started = True
        except BaseException:
            self.cancel()
            self.close_capture()
            self.close_wake()
            raise

    def register_capture(self):
        child = self.authority.child
        if child is None:
            return
        for name in ("stdout", "stderr"):
            if name not in self.streams:
                stream = getattr(child, name)
                self.api.set_blocking(stream.fileno(), False)
                self.streams[name] = stream
                self.selector.register(stream.fileno(), selectors.EVENT_READ, name)

    def next(self, deadline, capacities):
        self.register_capture()
        ready = self.selector.select(deadline.remaining())
        demand(ready, "Native readiness deadline exceeded")
        events = []
        for key, _ in ready:
            name = key.data
            if name == "wake":
                self.api.read(self.wake_read, READ_SIZE)
                self.register_capture()
                events.append(("wake", None))
                continue
            try:
                data = self.api.read(key.fd, min(READ_SIZE, capacities[name]))
                if not data:
                    self.selector.unregister(key.fd)
                events.append((name, data))
            except BlockingIOError:
                continue
            except OSError as error:
                events.append((name, error_fact(error)))
        return events

    def close_capture(self):
        child = self.authority.child
        if child is None:
            return
        for name in ("stdout", "stderr"):
            stream = getattr(child, name)
            if stream.closed:
                self.closed_streams.add(name)
                continue
            if not self.selector_closed:
                try:
                    self.selector.unregister(stream.fileno())
                except KeyError:
                    pass
            stream.close()
            demand(stream.closed, "Native pipe close unconfirmed")
            self.closed_streams.add(name)

    def close_wake(self):
        if not self.selector_closed:
            if self.selector is not None:
                self.selector.close()
            self.selector_closed = True
        if not self.wakes_closed:
            for name in ("wake_read", "wake_write"):
                handle = getattr(self, name)
                if handle is not None:
                    self.api.close(handle)
                    setattr(self, name, None)
            self.wakes_closed = True

    def late_cleanup(self):
        try:
            if self.authority.observed and not self.authority.collected:
                self.authority.collect(self.reap)
            self.close_capture()
            self.close_wake()
        except BaseException as error:
            self.authority.lose(error)

    def finish(self, hard, cancel=False):
        if cancel:
            self.cancel()
        try:
            if self.started and not self.finished.is_set():
                demand(self.finished.wait(hard.remaining()), "Native observation completion unconfirmed")
            if self.started:
                self.thread.join(timeout=max(0, hard.end - hard.clock()))
            demand(not self.thread.is_alive(), "Native observer join unconfirmed")
            self.joined = self.started
            if self.joined:
                self.joined_at = self.clock()
            if self.authority.child is not None:
                self.authority.collect(self.reap)
            self.close_capture()
            self.close_wake()
            self.handles_closed_at = self.clock()
            hard.remaining()
        except BaseException:
            self.abandoned = True
            UNCONFIRMED_OWNERS.append(self)
            self.cancel()
            if self.finished.is_set() and not self.thread.is_alive():
                self.late_cleanup()
            raise

    def facts(self):
        return self.authority.facts() | {"kind": self.kind, "reaped": self.authority.collected,
                "waiter_finished": self.finished.is_set(), "waiter_joined": self.joined,
                "capture_completed": len(self.closed_streams) == 2, "handles_closed":
                len(self.closed_streams) == 2 and self.wakes_closed and self.selector_closed,
                "waiter_joined_at": self.joined_at, "handles_closed_at": self.handles_closed_at}


class WinAPI:
    def __init__(self):
        import ctypes as c
        from ctypes import wintypes as w
        demand(hasattr(c, "WinDLL"), "Native Win32 public API loader unavailable")
        self.c, self.w = c, w
        class Security(c.Structure):
            _fields_ = [("length", w.DWORD), ("descriptor", c.c_void_p), ("inherit", w.BOOL)]
        class Overlapped(c.Structure):
            _fields_ = [("internal", c.c_size_t), ("internal_high", c.c_size_t),
                        ("offset", w.DWORD), ("offset_high", w.DWORD), ("event", w.HANDLE)]
        class Startup(c.Structure):
            _fields_ = [("cb", w.DWORD), ("reserved", w.LPWSTR), ("desktop", w.LPWSTR), ("title", w.LPWSTR),
                        ("x", w.DWORD), ("y", w.DWORD), ("xsize", w.DWORD), ("ysize", w.DWORD),
                        ("xchars", w.DWORD), ("ychars", w.DWORD), ("fill", w.DWORD), ("flags", w.DWORD),
                        ("show", w.WORD), ("reserved_size", w.WORD), ("reserved_bytes", c.POINTER(c.c_byte)),
                        ("stdin", w.HANDLE), ("stdout", w.HANDLE), ("stderr", w.HANDLE)]
        class StartupEx(c.Structure):
            _fields_ = [("startup", Startup), ("attributes", c.c_void_p)]
        class Process(c.Structure):
            _fields_ = [("process", w.HANDLE), ("thread", w.HANDLE), ("pid", w.DWORD), ("tid", w.DWORD)]
        self.Security, self.Overlapped, self.StartupEx, self.Process = Security, Overlapped, StartupEx, Process
        self.kernel = c.WinDLL("kernel32", use_last_error=True)
        specifications = {
            "CreateEventW": (w.HANDLE, [c.c_void_p, w.BOOL, w.BOOL, w.LPCWSTR]),
            "SetEvent": (w.BOOL, [w.HANDLE]), "ResetEvent": (w.BOOL, [w.HANDLE]),
            "CloseHandle": (w.BOOL, [w.HANDLE]),
            "CreateNamedPipeW": (w.HANDLE, [w.LPCWSTR, w.DWORD, w.DWORD, w.DWORD, w.DWORD, w.DWORD, w.DWORD, c.c_void_p]),
            "CreateFileW": (w.HANDLE, [w.LPCWSTR, w.DWORD, w.DWORD, c.c_void_p, w.DWORD, w.DWORD, w.HANDLE]),
            "ConnectNamedPipe": (w.BOOL, [w.HANDLE, c.c_void_p]),
            "ReadFile": (w.BOOL, [w.HANDLE, c.c_void_p, w.DWORD, c.c_void_p, c.c_void_p]),
            "CancelIoEx": (w.BOOL, [w.HANDLE, c.c_void_p]),
            "GetOverlappedResult": (w.BOOL, [w.HANDLE, c.c_void_p, c.POINTER(w.DWORD), w.BOOL]),
            "WaitForMultipleObjects": (w.DWORD, [w.DWORD, c.POINTER(w.HANDLE), w.BOOL, w.DWORD]),
            "TerminateProcess": (w.BOOL, [w.HANDLE, w.UINT]),
            "GetExitCodeProcess": (w.BOOL, [w.HANDLE, c.POINTER(w.DWORD)]),
            "InitializeProcThreadAttributeList": (w.BOOL, [c.c_void_p, w.DWORD, w.DWORD, c.POINTER(c.c_size_t)]),
            "UpdateProcThreadAttribute": (w.BOOL, [c.c_void_p, w.DWORD, c.c_size_t, c.c_void_p,
                                                c.c_size_t, c.c_void_p, c.c_void_p]),
            "DeleteProcThreadAttributeList": (None, [c.c_void_p]),
            "CreateProcessW": (w.BOOL, [w.LPCWSTR, w.LPWSTR, c.c_void_p, c.c_void_p, w.BOOL,
                                       w.DWORD, c.c_void_p, w.LPCWSTR, c.c_void_p, c.c_void_p])}
        for name, (result, args) in specifications.items():
            function = getattr(self.kernel, name)
            function.restype, function.argtypes = result, args

    def check(self, value):
        if not value:
            raise OSError(self.c.get_last_error(), "Native Win32 operation failed")
        return value

    def handle(self, value):
        demand(value not in (None, 0, self.c.c_void_p(-1).value), "Native Win32 handle unavailable")
        return value

    def event(self):
        return self.handle(self.kernel.CreateEventW(None, True, False, None))

    def close(self, handle):
        self.check(self.kernel.CloseHandle(handle))

    def notify(self, event):
        self.check(self.kernel.SetEvent(event))

    def reset(self, event):
        self.check(self.kernel.ResetEvent(event))

    def wait(self, handles, timeout):
        milliseconds = 0xFFFFFFFF if timeout is None else max(0, min(0xFFFFFFFE, math.ceil(timeout * 1000)))
        array = (self.w.HANDLE * len(handles))(*handles)
        result = self.kernel.WaitForMultipleObjects(len(handles), array, False, milliseconds)
        if result == 258:
            return None
        demand(0 <= result < len(handles), "Native Windows event wait failed")
        return result

    def pipe(self):
        name = "\\\\.\\pipe\\codeterm-native-version-" + uuid.uuid4().hex
        reader = self.handle(self.kernel.CreateNamedPipeW(name, 1 | 0x40000000 | 0x00080000, 8,
                                                        1, READ_SIZE, READ_SIZE, 0, None))
        writer = event = None
        try:
            security = self.Security(self.c.sizeof(self.Security), None, True)
            writer = self.handle(self.kernel.CreateFileW(name, 0x40000000, 0, self.c.byref(security), 3, 0, None))
            event = self.event()
            overlap = self.Overlapped()
            overlap.event = event
            connected = self.kernel.ConnectNamedPipe(reader, self.c.byref(overlap))
            error = self.c.get_last_error()
            demand(connected or error in (535, 997), "Native local pipe connection unconfirmed")
            return reader, writer, event, overlap if not connected and error == 997 else None
        except BaseException:
            for handle in (writer, event, reader):
                if handle is not None:
                    self.close(handle)
            raise

    def spawn(self, request, environment, slots, capture, auxiliary):
        security = self.Security(self.c.sizeof(self.Security), None, True)
        null = self.handle(self.kernel.CreateFileW(os.devnull, 0x80000000, 3, self.c.byref(security), 3, 0, None))
        auxiliary.append(null)
        size, attributes, initialized = self.c.c_size_t(), None, False
        process = self.Process()
        try:
            self.kernel.InitializeProcThreadAttributeList(None, 1, 0, self.c.byref(size))
            demand(size.value > 0 and size.value <= 65536, "Native handle-list allocation invalid")
            attributes = self.c.create_string_buffer(size.value)
            self.check(self.kernel.InitializeProcThreadAttributeList(attributes, 1, 0, self.c.byref(size)))
            initialized = True
            inherited = (self.w.HANDLE * 3)(null, slots["stdout"].writer, slots["stderr"].writer)
            self.check(self.kernel.UpdateProcThreadAttribute(attributes, 0, 0x20002, inherited,
                                                           self.c.sizeof(inherited), None, None))
            startup = self.StartupEx()
            startup.startup.cb = self.c.sizeof(startup)
            startup.startup.flags = 0x100
            startup.startup.stdin, startup.startup.stdout, startup.startup.stderr = inherited
            startup.attributes = self.c.cast(attributes, self.c.c_void_p)
            command = self.c.create_unicode_buffer(subprocess.list2cmdline(request["argv"]))
            env = self.c.create_unicode_buffer("\0".join(key + "=" + value for key, value in sorted(environment.items())) + "\0\0")
            self.check(self.kernel.CreateProcessW(request["executable"], command, None, None, True,
                       0x08000000 | 0x400 | 0x80000, env, request["cwd"], self.c.byref(startup), self.c.byref(process)))
            auxiliary.append(process.thread)
            capture(process.process)
            return process.process
        finally:
            if initialized:
                self.kernel.DeleteProcThreadAttributeList(attributes)
            errors = []
            for handle in tuple(auxiliary):
                try:
                    self.close(handle)
                    auxiliary.remove(handle)
                except BaseException as error:
                    errors.append(error)
            for slot in slots.values():
                if slot.writer is not None:
                    try:
                        self.close(slot.writer)
                        slot.writer = None
                    except BaseException as error:
                        errors.append(error)
            if errors:
                raise errors[0]

    def observe(self, process):
        demand(self.wait([process], None) == 0, "Native Windows exit wait unconfirmed")
        code = self.w.DWORD()
        self.check(self.kernel.GetExitCodeProcess(process, self.c.byref(code)))
        return code.value

    def terminate(self, process):
        self.check(self.kernel.TerminateProcess(process, 1))

    def read_start(self, slot, size):
        if slot.connection is not None:
            return self.read_finish(slot)
        slot.buffer = self.c.create_string_buffer(size)
        slot.overlap = self.Overlapped()
        slot.overlap.event = slot.event
        self.reset(slot.event)
        result = self.kernel.ReadFile(slot.reader, slot.buffer, size, None, self.c.byref(slot.overlap))
        if result or self.c.get_last_error() == 997:
            slot.pending = True
            return self.read_finish(slot)
        error = self.c.get_last_error()
        return ("eof", b"") if error in (109, 38) else ("error", {"type": "OSError", "errno": error})

    def read_finish(self, slot):
        count = self.w.DWORD()
        overlap = slot.connection if slot.connection is not None else slot.overlap
        if self.kernel.GetOverlappedResult(slot.reader, self.c.byref(overlap), self.c.byref(count), False):
            slot.pending = False
            if slot.connection is not None:
                slot.connection = None
                return "connected", None
            return "data" if count.value else "eof", slot.buffer.raw[:count.value]
        error = self.c.get_last_error()
        if error == 996:
            return "pending", None
        slot.pending = False
        slot.connection = None
        if error == 995:
            return "cancelled", None
        return ("eof", b"") if error in (109, 38) else ("error", {"type": "OSError", "errno": error})

    def read_cancel(self, slot):
        overlap = slot.connection if slot.connection is not None else slot.overlap
        result = self.kernel.CancelIoEx(slot.reader, self.c.byref(overlap))
        demand(result or self.c.get_last_error() == 1168, "Native Windows IO cancellation request failed")


class WinRead:
    def __init__(self, api):
        self.reader, self.writer, self.event, self.connection = api.pipe()
        self.buffer, self.overlap, self.pending = None, None, self.connection is not None
        self.retired, self.closed, self.cancel_requested = False, False, False

    def cancel(self, api):
        self.cancel_requested = True
        if self.pending:
            api.read_cancel(self)

    def close(self, api):
        demand(not self.pending, "Native Windows IO completion unconfirmed")
        for name in ("writer", "reader", "event"):
            handle = getattr(self, name)
            if handle is not None:
                api.close(handle)
                setattr(self, name, None)
        self.buffer = self.overlap = self.connection = None
        self.closed = True


def windows_preflight(version=None, api_factory=WinAPI):
    version = sys.version_info[:2] if version is None else version
    demand(version >= (3, 13), "Native Windows interpreter unsupported")
    return api_factory()


class WindowsOwner:
    kind = "win32_captured_process_handle"

    def __init__(self, request, environment, clock, api=None, thread_factory=threading.Thread):
        self.request, self.environment, self.clock = request, environment, clock
        self.api = WinAPI() if api is None else api
        self.authority, self.finished = ChildAuthority(clock), threading.Event()
        self.joined, self.started = False, False
        self.joined_at = self.handles_closed_at = None
        self.wake = self.api.event()
        self.slots = {}
        try:
            for name in ("stdout", "stderr"):
                self.slots[name] = WinRead(self.api)
            self.thread = thread_factory(target=self.launch_observe, daemon=False)
        except BaseException:
            UNCONFIRMED_OWNERS.append(self)
            for slot in self.slots.values():
                if slot.pending:
                    slot.cancel(self.api)
                    self.api.read_finish(slot)
                if not slot.pending:
                    slot.close(self.api)
            self.api.close(self.wake)
            raise
        self.process_closed, self.wake_closed, self.abandoned = False, False, False
        self.auxiliary = []

    def cancel(self):
        return self.authority.cancel(self.api.terminate)

    def capture(self, process):
        cancelled = self.authority.capture(process)
        if cancelled or self.clock() >= self.request["operation_deadline"]:
            self.cancel()
        self.api.notify(self.wake)

    def launch_observe(self):
        try:
            demand(not self.authority.cancelled and self.clock() < self.request["operation_deadline"],
                   "Native cancelled before creation")
            process = self.api.spawn(self.request, self.environment, self.slots, self.capture, self.auxiliary)
            self.authority.observe(self.api.observe(process))
            self.authority.signal_finished.wait()
            self.authority.collect(lambda owned: self.authority.code)
        except BaseException as error:
            self.cancel()
            self.authority.lose(error)
        finally:
            try:
                self.api.notify(self.wake)
            except BaseException as error:
                self.authority.lose(error)
            self.finished.set()

    def start(self):
        try:
            self.thread.start()
            self.started = True
        except BaseException:
            self.cancel()
            for slot in self.slots.values():
                slot.close(self.api)
            self.api.close(self.wake)
            self.wake_closed = True
            raise

    def next(self, deadline, capacities):
        events, waits = [], [self.wake]
        names = ["wake"]
        for name, slot in self.slots.items():
            if slot.retired or self.authority.child is None:
                continue
            state, value = self.api.read_finish(slot) if slot.pending else self.api.read_start(slot, min(READ_SIZE, capacities[name]))
            if state == "pending":
                waits.append(slot.event)
                names.append(name)
            else:
                if state == "connected":
                    events.append(("wake", None))
                    continue
                if state in ("eof", "error", "cancelled"):
                    slot.retired = True
                events.append((name, value if state in ("data", "error") else b"" if state == "eof" else
                               {"type": "cancelled", "errno": None}))
        if events:
            return events
        index = self.api.wait(waits, deadline.remaining())
        demand(index is not None, "Native Windows readiness deadline exceeded")
        if names[index] == "wake":
            self.api.reset(self.wake)
            return [("wake", None)]
        slot = self.slots[names[index]]
        state, value = self.api.read_finish(slot)
        demand(state != "pending", "Native Windows IO event without completion")
        if state == "connected":
            return [("wake", None)]
        if state in ("eof", "error", "cancelled"):
            slot.retired = True
        return [(names[index], value if state in ("data", "error") else b"" if state == "eof" else
                 {"type": "cancelled", "errno": None})]

    def finish(self, hard, cancel=False):
        if cancel:
            self.cancel()
        try:
            errors = []
            for slot in self.slots.values():
                if slot.pending:
                    try:
                        slot.cancel(self.api)
                    except BaseException as error:
                        errors.append(error)
            if errors:
                raise errors[0]
            if self.started and not self.finished.is_set():
                demand(self.finished.wait(hard.remaining()), "Native process waiter completion unconfirmed")
            if self.started:
                self.thread.join(timeout=max(0, hard.end - hard.clock()))
            demand(not self.thread.is_alive(), "Native Windows waiter join unconfirmed")
            self.joined = self.started
            if self.joined:
                self.joined_at = self.clock()
            for slot in self.slots.values():
                if slot.pending:
                    state, _ = self.api.read_finish(slot)
                    if state == "pending":
                        demand(self.api.wait([slot.event], hard.remaining()) == 0, "Native IO cancellation completion unconfirmed")
                        state, _ = self.api.read_finish(slot)
                    demand(state != "pending", "Native IO cancellation completion unconfirmed")
                slot.close(self.api)
            for handle in tuple(self.auxiliary):
                self.api.close(handle)
                self.auxiliary.remove(handle)
            if self.authority.child is not None:
                demand(self.authority.collected, "Native Windows exit collection unconfirmed")
                self.api.close(self.authority.child)
            self.process_closed = True
            self.api.close(self.wake)
            self.wake_closed = True
            self.handles_closed_at = self.clock()
            hard.remaining()
        except BaseException:
            self.abandoned = True
            UNCONFIRMED_OWNERS.append(self)
            self.cancel()
            raise

    def facts(self):
        return self.authority.facts() | {"kind": self.kind, "reaped": None,
                "waiter_finished": self.finished.is_set(), "waiter_joined": self.joined,
                "capture_completed": all(not slot.pending and slot.closed for slot in self.slots.values()),
                "handles_closed": self.process_closed and self.wake_closed and not self.auxiliary and
                all(slot.closed for slot in self.slots.values()), "waiter_joined_at": self.joined_at,
                "handles_closed_at": self.handles_closed_at}


def preflight(platform=sys.platform):
    demand(sys.implementation.name == "cpython" and (3, 13) <= sys.version_info[:2] <= (3, 14),
           "Native version CPython runtime unsupported")
    if platform in ("darwin", "linux"):
        unix_preflight(platform)
    elif platform == "win32":
        windows_preflight()
    else:
        demand(False, "Native version platform unsupported")


def owner_for(request, environment, clock):
    return WindowsOwner(request, environment, clock) if request["platform"] == "win32" else UnixOwner(request, environment, clock)
