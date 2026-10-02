"""Standalone isolated registry reader; no executable hooks, subprocesses or threads."""

import base64
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import re
import sys
import time
import urllib.request
from urllib.parse import urljoin, urlsplit

MAX_ARCHIVE = 512 * 1024 * 1024
MAX_REQUEST = 16 * 1024
MAX_RECEIPT = 8 * 1024
CHUNK = 64 * 1024
MAX_REDIRECTS = 5
MAX_HEADER_LINE = 8192
MAX_HEADERS = 64
MAX_HEADER_BYTES = 32768


class AcquisitionError(RuntimeError):
    pass


def require(value, reason):
    if not value:
        raise AcquisitionError(reason)


def remaining(end, clock=time.monotonic):
    value = end - clock()
    require(math.isfinite(end) and value > 0, "Acquisition operation deadline exceeded")
    return value


def registry_url(url):
    require(isinstance(url, str) and len(url) <= 4096, "Registry URL exceeds bound")
    parsed = urlsplit(url)
    require(parsed.scheme == "https" and parsed.hostname == "registry.npmjs.org" and
            parsed.port in (None, 443) and not parsed.username and not parsed.password and
            not parsed.fragment, "Registry URL escaped HTTPS confinement")
    return url


def metadata_input(metadata):
    url = registry_url(metadata["tarball"])
    integrity = metadata["integrity"]
    require(isinstance(integrity, str) and integrity.startswith("sha512-"), "Pinned SHA512 absent")
    try:
        digest = base64.b64decode(integrity[7:], validate=True)
    except (ValueError, TypeError) as error:
        raise AcquisitionError("Pinned SHA512 invalid") from error
    require(len(digest) == 64, "Pinned SHA512 length differs")
    sha256 = metadata.get("tarball_sha256")
    require(sha256 is None or isinstance(sha256, str) and re.fullmatch(r"[0-9a-f]{64}", sha256),
            "Pinned SHA256 invalid")
    return {"tarball": url, "integrity": integrity, "tarball_sha256": sha256}


def contained(root, path):
    root, path = Path(root).absolute(), Path(path).absolute()
    require(path != root and path.is_relative_to(root), "Acquisition path escaped fixture")
    require(not root.is_symlink() and root.resolve() == root, "Acquisition root is redirected")
    current = root
    for part in path.relative_to(root).parts:
        current = current / part
        require(not current.is_symlink(), "Acquisition path contains symlink")
    require(path.resolve().is_relative_to(root), "Acquisition path resolves outside fixture")
    return path


def bounded_json(path, limit):
    with Path(path).open("rb") as stream:
        raw = stream.read(limit + 1)
    require(len(raw) <= limit, "Acquisition JSON exceeds bound")
    return raw, json.loads(raw)


def exclusive_json(path, value):
    raw = json.dumps(value, sort_keys=True, separators=(",", ":")).encode() + b"\n"
    require(len(raw) <= MAX_RECEIPT, "Acquisition receipt exceeds bound")
    with exclusive_file(path) as output:
        require(output.write(raw) == len(raw), "Acquisition receipt write incomplete")


def exclusive_file(path):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
    descriptor = os.open(path, flags, 0o600)
    try:
        return os.fdopen(descriptor, "wb")
    except BaseException:
        os.close(descriptor)
        raise


def file_digest(path, end, clock=time.monotonic):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while True:
            remaining(end, clock)
            chunk = stream.read(CHUNK)
            if not chunk:
                break
            digest.update(chunk)
    remaining(end, clock)
    return digest.hexdigest()


def response_headers(response):
    headers = list(response.headers.items())
    require(len(headers) <= MAX_HEADERS and sum(len(k) + len(v) + 4 for k, v in headers) <= MAX_HEADER_BYTES,
            "Registry response headers exceed bound")
    require(all(len(k) + len(v) + 4 <= MAX_HEADER_LINE for k, v in headers), "Registry header line exceeds bound")
    lengths = response.headers.get_all("Content-Length", [])
    require(len(lengths) <= 1, "Ambiguous archive length")
    require(not lengths or re.fullmatch(r"[0-9]{1,10}", lengths[0]), "Invalid archive length")
    length = int(lengths[0]) if lengths else None
    require(length is None or length <= MAX_ARCHIVE, "Archive byte limit exceeded")
    require(not response.headers.get_all("Content-Encoding", []), "Encoded archive body unsupported")
    return length


class RegistryRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self, deadline, clock=time.monotonic):
        self.deadline, self.clock, self.hops = deadline, clock, []

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        remaining(self.deadline, self.clock)
        response_headers(type("Headers", (), {"headers": headers})())
        registry_url(newurl)
        require(len(self.hops) < MAX_REDIRECTS, "Registry redirect limit exceeded")
        self.hops.append(newurl)
        require(req is not None and req.get_method() == "GET", "Registry redirect method unsupported")
        return urllib.request.Request(newurl, method="GET")

    def http_error_302(self, req, fp, code, msg, headers):
        try:
            locations = headers.get_all("Location", [])
            require(len(locations) == 1 and len(locations[0]) <= 4096, "Registry redirect location ambiguous")
            request = self.redirect_request(req, fp, code, msg, headers, urljoin(req.full_url, locations[0]))
        finally:
            fp.close()
        return self.parent.open(request, timeout=min(10, remaining(self.deadline, self.clock)))

    http_error_301 = http_error_303 = http_error_307 = http_error_308 = http_error_302


def download(request, opener=None, redirect=None, clock=time.monotonic):
    end = request["operation_deadline"]
    remaining(end, clock)
    metadata = metadata_input(request["metadata"])
    root = Path(request["fixture_root"])
    archive = contained(root, request["archive"])
    require(not archive.exists(), "Archive destination already exists")
    if opener is None:
        http.client._MAXLINE = MAX_HEADER_LINE
        http.client._MAXHEADERS = MAX_HEADERS
        redirect = RegistryRedirect(end, clock)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), redirect)
    digest512, digest256, count = hashlib.sha512(), hashlib.sha256(), 0
    with opener.open(metadata["tarball"], timeout=min(10, remaining(end, clock))) as response:
        remaining(end, clock)
        final_url = registry_url(response.url)
        require(response.status == 200, "Registry response is not a complete archive")
        length = response_headers(response)
        with exclusive_file(archive) as output:
            while True:
                remaining(end, clock)
                chunk = response.read(CHUNK)
                remaining(end, clock)
                require(isinstance(chunk, bytes) and len(chunk) <= CHUNK, "Archive read exceeds bounded chunk")
                if not chunk:
                    break
                count += len(chunk)
                require(count <= MAX_ARCHIVE, "Archive byte limit exceeded")
                require(output.write(chunk) == len(chunk), "Archive write incomplete")
                digest512.update(chunk)
                digest256.update(chunk)
                remaining(end, clock)
        require(length is None or length == count, "Archive body ended before declared completion")
    remaining(end, clock)
    integrity = "sha512-" + base64.b64encode(digest512.digest()).decode()
    sha256 = digest256.hexdigest()
    require(integrity == metadata["integrity"], "Registry SHA512 integrity differs")
    require(metadata["tarball_sha256"] is None or sha256 == metadata["tarball_sha256"],
            "Reviewed archive SHA256 differs")
    return {"sha512": integrity, "sha256": sha256, "bytes": count, "source": metadata["tarball"],
            "final_url": final_url, "redirects": list(redirect.hops) if redirect else []}


def main():
    require(len(sys.argv) == 3 and sys.flags.isolated and sys.flags.no_site and sys.flags.dont_write_bytecode,
            "Helper requires isolated Python startup")
    request_path, request_hash = Path(sys.argv[1]).absolute(), sys.argv[2]
    base = (Path(__file__).resolve().parents[2] / "target-check/evals/prompt-matrix").resolve()
    contained(base, request_path)
    require(request_path.name == "request.json", "Helper request path differs")
    raw, request = bounded_json(request_path, MAX_REQUEST)
    require(isinstance(request, dict) and set(request) == {"version", "metadata", "fixture_root", "archive", "receipt",
            "operation_deadline", "worker_sha256", "interpreter", "interpreter_sha256"} and
            type(request["version"]) is int and request["version"] == 1, "Helper request schema differs")
    require(hashlib.sha256(raw).hexdigest() == request_hash, "Acquisition request changed")
    end = request["operation_deadline"]
    remaining(end)
    root = Path(request["fixture_root"])
    require(root.parent == base and re.fullmatch(r"[a-z][a-z0-9-]{2,39}", root.name), "Helper fixture root differs")
    contained(root, request_path)
    receipt_path = contained(root, request["receipt"])
    archive = contained(root, request["archive"])
    require(archive.parent == request_path.parent.parent and
            request_path.parent.name == archive.name + ".acquisition", "Acquisition archive/envelope join differs")
    require(request_path.parent == receipt_path.parent and request_path.name == "request.json" and
            receipt_path.name == "receipt.json", "Acquisition envelope paths differ")
    require(file_digest(__file__, end) == request["worker_sha256"] and
            Path(sys.executable).resolve() == Path(request["interpreter"]) and
            file_digest(sys.executable, end) == request["interpreter_sha256"], "Trusted acquisition inputs changed")
    try:
        receipt = download(request)
        remaining(end)
        exclusive_json(receipt_path, {"version": 1, "status": "completed", "request_sha256": request_hash,
                                      "archive": request["archive"], "result": receipt})
        remaining(end)
        return 0
    except Exception as error:
        exclusive_json(receipt_path, {"version": 1, "status": "failed", "request_sha256": request_hash,
                                      "error_type": type(error).__name__[:80]})
        return 1


if __name__ == "__main__":
    sys.exit(main())
