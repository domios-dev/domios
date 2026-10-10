"""A real tool waits for one nonce-bound release under the case's absolute deadline."""

import argparse
import json
import os
import queue
import socket
import threading
import uuid

from deadline_io import Cancelled, Deadline, SocketIO, completed_worker
from oracles import require


def frame(value):
    return json.dumps(value).encode() + b"\n"


def close_socket(stream):
    try:
        stream.shutdown(socket.SHUT_RDWR)
    except OSError:
        pass
    stream.close()


class BusyGate:
    def __init__(self, dev, events):
        self.dev, self.events = dev, events
        self.deadline = Deadline(dev.deadline)
        self.io_deadline = self.deadline.reserve(2)
        self.io_deadline.remaining()
        self.nonce = uuid.uuid4().hex
        self.cancel = threading.Event()
        self.finished = threading.Event()
        self.completed = threading.Event()
        self.wake_reader, self.wake_writer = socket.socketpair()
        self.wake_reader.setblocking(False)
        self.wake_writer.setblocking(False)
        self.socket = socket.socket()
        self.socket.bind(("127.0.0.1", 0))
        self.socket.listen(1)
        self.port = self.socket.getsockname()[1]
        self.connected = threading.Event()
        self.released = False
        self.closed = False
        self.connection = None
        self.receipt = None
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()

    def io(self, stream):
        return SocketIO(stream, self.io_deadline, self.cancel, self.wake_reader)

    def accept(self):
        try:
            connection, address = self.io(self.socket).accept()
            self.connection = connection
            io = self.io(connection)
            receipt = json.loads(io.frame())
            require(address[0] == "127.0.0.1" and receipt.get("nonce") == self.nonce and
                    receipt.get("pid", 0) > 1 and receipt.get("event") == "tool_started",
                    "Busy tool signal is foreign")
            io.check()
            self.receipt = receipt
            self.dev.transcript.record("busy_tool_signal", receipt)
            self.connected.set()
            self.events.frames.put_nowait({"event_type": "busy-tool-started", "data": receipt})
            acknowledgement = json.loads(io.frame())
            require(acknowledgement == {"event": "tool_completed", "nonce": self.nonce, "pid": receipt["pid"]},
                    "Busy completion acknowledgement is foreign")
            io.check()
            self.dev.transcript.record("busy_tool_acknowledgement", acknowledgement)
            self.completed.set()
            self.events.frames.put_nowait({"event_type": "busy-tool-acknowledgement", "data": acknowledgement})
        except Cancelled:
            pass
        except Exception as error:
            self.events.error = error
            try:
                self.events.frames.put_nowait({"error": str(error)})
            except queue.Full:
                pass
        finally:
            self.finished.set()

    def release(self):
        require(self.connected.is_set() and self.connection is not None and not self.released,
                "Busy release has no exact blocked tool or was duplicated")
        self.io(self.connection).send(frame({"nonce": self.nonce, "event": "release"}))
        require(self.completed.wait(self.io_deadline.remaining()), "Busy tool release acknowledgement unconfirmed")
        self.io_deadline.remaining()
        self.released = True
        self.dev.transcript.record("busy_release", {"nonce": self.nonce})

    def close(self):
        if self.closed:
            return
        self.cancel.set()
        try:
            require(self.wake_writer.send(b"x") == 1, "Busy cancellation wakeup unconfirmed")
            completed_worker(self.thread, self.finished, self.deadline)
        finally:
            failures = []
            for stream in (self.connection, self.socket, self.wake_reader, self.wake_writer):
                if stream is not None:
                    try:
                        close_socket(stream)
                    except OSError as error:
                        failures.append(str(error))
            require(not failures, "Busy socket cleanup unconfirmed: " + "; ".join(failures))
        self.deadline.remaining()
        self.closed = True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--nonce", required=True)
    parser.add_argument("--deadline", type=float, required=True)
    args = parser.parse_args()
    hard_deadline = Deadline(args.deadline)
    deadline = hard_deadline.reserve(2)
    connection = socket.socket()
    try:
        io = SocketIO(connection, deadline)
        io.connect(("127.0.0.1", args.port))
        io.send(frame({"event": "tool_started", "nonce": args.nonce, "pid": os.getpid()}))
        require(json.loads(io.frame()) == {"event": "release", "nonce": args.nonce},
                "Busy tool received no exact release acknowledgement")
        receipt = {"event": "tool_completed", "nonce": args.nonce, "pid": os.getpid()}
        io.send(frame(receipt))
        deadline.remaining()
    finally:
        close_socket(connection)
    hard_deadline.remaining()
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
