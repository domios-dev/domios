import socket
import sys


def serve(timeout: float) -> None:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.bind(("127.0.0.1", 0))
        server.listen(2)
        server.settimeout(timeout)
        print(server.getsockname()[1], flush=True)
        connection, _ = server.accept()
        with connection:
            print("CONNECTED", flush=True)
            release_connection, _ = server.accept()
            with release_connection:
                release = release_connection.recv(1)
            if release != b"R":
                raise SystemExit("gate server received an invalid release")
            connection.sendall(b"R")
            print("RELEASED", flush=True)


def client(port: int, timeout: float) -> None:
    with socket.create_connection(("127.0.0.1", port), timeout=timeout) as connection:
        print("CODETERM_LONG_PASTE_GATE_CONNECTED", flush=True)
        connection.settimeout(None)
        if connection.recv(1) != b"R":
            raise SystemExit("gate client was not released")


def release(port: int, timeout: float) -> None:
    with socket.create_connection(("127.0.0.1", port), timeout=timeout) as connection:
        connection.sendall(b"R")


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "server":
        serve(float(sys.argv[2]))
    elif mode == "client":
        client(int(sys.argv[2]), float(sys.argv[3]))
    elif mode == "release":
        release(int(sys.argv[2]), float(sys.argv[3]))
    else:
        raise SystemExit(f"unknown mode: {mode}")
