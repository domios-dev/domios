"""Nonblocking socket frames share one absolute deadline and cancellation handle."""

import errno
import select
import socket
import time

from native_layout import FixtureError, demand


class Cancelled(FixtureError):
    pass


class Deadline:
    def __init__(self, end, clock=time.monotonic):
        self.end, self.clock = end, clock

    def remaining(self):
        remaining = self.end - self.clock()
        demand(remaining > 0, "Absolute IO deadline exceeded")
        return remaining

    def reserve(self, seconds):
        return Deadline(self.end - seconds, self.clock)


def completed_worker(thread, finished, deadline):
    demand(finished.wait(deadline.remaining()), "IO worker completion unconfirmed")
    thread.join(timeout=deadline.remaining())
    demand(not thread.is_alive(), "IO worker thread termination unconfirmed")
    deadline.remaining()


class SocketIO:
    def __init__(self, stream, deadline, cancel=None, wake=None, wait=select.select):
        self.stream, self.deadline = stream, deadline
        self.cancel, self.wake, self.wait = cancel, wake, wait
        stream.setblocking(False)

    def check(self):
        self.deadline.remaining()
        if self.cancel is not None and self.cancel.is_set():
            raise Cancelled("Socket IO was cancelled")

    def ready(self, write=False):
        self.check()
        readers = [self.wake] if self.wake is not None else []
        writers = [self.stream] if write else []
        if not write:
            readers.append(self.stream)
        readable, writable, exceptional = self.wait(readers, writers, [self.stream], self.deadline.remaining())
        self.check()
        demand(self.stream not in exceptional, "Socket readiness reported failure")
        demand(self.stream in (writable if write else readable), "IO wait ended without readiness")

    def accept(self):
        while True:
            self.check()
            try:
                result = self.stream.accept()
            except BlockingIOError:
                self.ready()
                continue
            try:
                self.check()
            except BaseException:
                result[0].close()
                raise
            return result

    def connect(self, address):
        self.check()
        result = self.stream.connect_ex(address)
        self.check()
        if result:
            pending = {errno.EINPROGRESS, errno.EWOULDBLOCK, errno.EALREADY, errno.EINTR,
                       getattr(errno, "WSAEWOULDBLOCK", 10035)}
            demand(result in pending, "Socket connect failed")
            self.ready(write=True)
            demand(self.stream.getsockopt(socket.SOL_SOCKET, socket.SO_ERROR) == 0,
                   "Socket connect completion failed")
            self.check()

    def frame(self, limit=4096, chunk_limit=256):
        data = bytearray()
        while True:
            self.check()
            capacity = min(chunk_limit, limit + 1 - len(data))
            demand(capacity > 0, "Socket frame byte limit exceeded")
            try:
                chunk = self.stream.recv(capacity)
            except BlockingIOError:
                self.ready()
                continue
            self.check()
            demand(bool(chunk), "Socket frame closed before completion")
            demand(len(chunk) <= capacity, "Socket receive exceeded bounded chunk")
            data.extend(chunk)
            demand(len(data) <= limit, "Socket frame byte limit exceeded")
            newline = data.find(b"\n")
            if newline >= 0:
                demand(newline == len(data) - 1, "Socket frame has trailing bytes")
                return bytes(data)

    def send(self, data, limit=4096, chunk_limit=256):
        demand(bool(data) and len(data) <= limit and data.endswith(b"\n") and b"\n" not in data[:-1],
               "Invalid bounded socket frame")
        offset = 0
        while offset < len(data):
            self.check()
            chunk = data[offset:offset + chunk_limit]
            try:
                count = self.stream.send(chunk)
            except BlockingIOError:
                self.ready(write=True)
                continue
            self.check()
            demand(isinstance(count, int) and not isinstance(count, bool) and 0 < count <= len(chunk),
                   "Socket send did not complete a bounded chunk")
            offset += count
