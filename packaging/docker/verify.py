import hashlib
import base64
import json
import pathlib
import re
import sys
import subprocess
import tarfile
import tempfile
from urllib.parse import urlsplit

PUBLIC_KEY = pathlib.Path(__file__).with_name("release.pub")


def trusted_digest(source, name, version, platform, mode, allow_smoke):
    if mode == "signed-release" and allow_smoke == "0":
        feed = source / f"domios-headless-linux-{platform}.json"
        if feed.stat().st_size > 1024 * 1024:
            raise ValueError("Signed feed exceeds its bound")
        envelope = json.loads(feed.read_bytes())
        if set(envelope) != {"manifest", "signature"}:
            raise ValueError("Invalid signed envelope fields")
        manifest_bytes = base64.b64decode(envelope["manifest"], validate=True)
        signature = base64.b64decode(envelope["signature"], validate=True)
        with tempfile.TemporaryDirectory() as temporary:
            manifest_path = pathlib.Path(temporary) / "manifest.json"
            signature_path = pathlib.Path(temporary) / "manifest.minisig"
            manifest_path.write_bytes(manifest_bytes)
            signature_path.write_bytes(signature)
            subprocess.run(["minisign", "-Vm", str(manifest_path), "-x", str(signature_path),
                            "-p", str(PUBLIC_KEY)], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        manifest = json.loads(manifest_bytes)
        fields = {"schema", "platform", "version", "archive", "url", "sha256", "size", "allow_downgrade"}
        if (set(manifest) != fields or manifest["schema"] != 1
                or manifest["platform"] != f"linux-{platform}"
                or manifest["version"] != version or manifest["archive"] != name
                or urlsplit(manifest["url"]).scheme != "https"
                or type(manifest["size"]) is not int
                or not 0 < manifest["size"] <= 1024**3
                or type(manifest["allow_downgrade"]) is not bool):
            raise ValueError("Signed release identity or size mismatch")
        return manifest["sha256"], manifest["size"]
    if mode == "ci-smoke" and allow_smoke == "1":
        checksum = (source / (name + ".sha256")).read_text().split()
        if len(checksum) != 2 or checksum[1] != name:
            raise ValueError("Expected one CI archive SHA-256 and its exact name")
        return checksum[0], None
    raise ValueError("Unsigned input requires ARCHIVE_SOURCE=ci-smoke and ALLOW_CI_SMOKE=1")


def extract(source, destination, version, architecture, mode, allow_smoke):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("VERSION must identify a stable production release")
    platform = {"amd64": "x86_64", "arm64": "aarch64"}[architecture]
    name = f"domios-headless-v{version}-linux-{platform}.tar.gz"
    archive = source / name
    checksum, size = trusted_digest(source, name, version, platform, mode, allow_smoke)
    if not re.fullmatch(r"[a-f0-9]{64}", checksum):
        raise ValueError("Invalid release SHA-256")
    if archive.stat().st_size > 1024**3 or (size is not None and archive.stat().st_size != size):
        raise ValueError("Compressed archive exceeds 1 GiB")
    with archive.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != checksum:
            raise ValueError("Release archive SHA-256 mismatch")
    destination.mkdir()
    with tarfile.open(archive, "r:gz") as bundle:
        seen = set()
        total = 0
        for count, member in enumerate(bundle, 1):
            path = pathlib.PurePosixPath(member.name)
            if (count > 100000 or path.is_absolute() or ".." in path.parts
                    or "\\" in member.name or ":" in member.name
                    or member.name in seen or not (member.isfile() or member.isdir())
                    or member.mode & 0o7000):
                raise ValueError("Unsafe release archive member")
            seen.add(member.name)
            total += member.size
            if total > 4 * 1024**3:
                raise ValueError("Expanded archive exceeds 4 GiB")
            bundle.extract(member, destination, filter="data")
    for required in ("domios-server/domios-server", "cli/codeterm",
                     "server-component.json", "dist/index.html", "LICENSE", "NOTICE"):
        if not (destination / required).is_file():
            raise ValueError("Incomplete production headless archive: " + required)
    for executable in ("domios-server/domios-server", "cli/codeterm"):
        (destination / executable).chmod(0o755)


if __name__ == "__main__":
    extract(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), *sys.argv[3:])
