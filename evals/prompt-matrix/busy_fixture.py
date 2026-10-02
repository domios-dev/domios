"""A real tool stays blocked until one nonce-bound local release acknowledgement."""

import argparse
import json
import os
import queue
import socket
import threading
import uuid

from oracles import require


class BusyGate:
    def __init__(self, dev, events):
        self.dev, self.events = dev, events
        self.nonce = uuid.uuid4().hex
        self.socket = socket.socket()
        self.socket.bind(("127.0.0.1", 0))
        self.socket.listen(1)
        self.socket.settimeout(dev.remaining())
        self.port = self.socket.getsockname()[1]
        self.connected = threading.Event()
        self.released = False
        self.connection = None
        self.receipt = None
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()

    def accept(self):
        try:
            connection, address = self.socket.accept()
            self.connection = connection
            connection.settimeout(self.dev.remaining())
            raw = connection.makefile("rb").readline(4097)
            require(address[0] == "127.0.0.1" and raw.endswith(b"\n") and len(raw) <= 4096,
                    "Busy tool signal escaped bounded protocol")
            receipt = json.loads(raw)
            require(receipt.get("nonce") == self.nonce and receipt.get("pid", 0) > 1 and
                    receipt.get("event") == "tool_started", "Busy tool signal is foreign")
            self.receipt = receipt
            self.dev.transcript.record("busy_tool_signal", receipt)
            self.connected.set()
            self.events.frames.put_nowait({"event_type": "busy-tool-started", "data": receipt})
        except Exception as error:
            self.events.error = error
            try:
                self.events.frames.put_nowait({"error": str(error)})
            except queue.Full:
                pass

    def release(self):
        require(self.connected.is_set() and self.connection is not None and not self.released,
                "Busy release has no exact blocked tool or was duplicated")
        self.connection.sendall(json.dumps({"nonce": self.nonce, "event": "release"}).encode() + b"\n")
        self.released = True
        self.dev.transcript.record("busy_release", {"nonce": self.nonce})

    def close(self):
        if self.connection:
            self.connection.close()
        self.socket.close()
        self.thread.join(timeout=2)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--nonce", required=True)
    args = parser.parse_args()
    with socket.create_connection(("127.0.0.1", args.port), timeout=150) as connection:
        connection.settimeout(150)
        connection.sendall(json.dumps({"event": "tool_started", "nonce": args.nonce,
                                       "pid": os.getpid()}).encode() + b"\n")
        raw = connection.makefile("rb").readline(4097)
        require(raw.endswith(b"\n") and len(raw) <= 4096 and
                json.loads(raw) == {"event": "release", "nonce": args.nonce},
                "Busy tool received no exact release acknowledgement")
    print(json.dumps({"event": "tool_completed", "nonce": args.nonce, "pid": os.getpid()}))


if __name__ == "__main__":
    main()
