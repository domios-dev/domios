"""Separate canonical-only assembly and executable qualification commands."""

import argparse
import json
from pathlib import Path
import platform
import re
import shutil
import sys
import time

from fixtures import HERE, canonical, fixture_root, write_json
from native_archive import acquire, demand, extract, sha256, tree_receipt, verify_archive, within
from native_layout import recipe


def manifest_path(root, provider, version):
    demand(provider in ("claude", "codex", "opencode"), "Unsupported native provider")
    demand(re.fullmatch(r"\d+\.\d+\.\d+", version) is not None, "Exact version literal required")
    return within(root, root / "native" / provider / version / "manifest.json")


def load_native(root, provider, version, qualified=True, deadline=None):
    def check():
        if deadline is not None:
            deadline.remaining()
    check()
    path = manifest_path(root, provider, version)
    demand(path.is_file(), "Verified fixture native manifest is absent")
    manifest = json.loads(path.read_text())
    demand(manifest["provider"] == provider and manifest["version"] == version,
           "Native manifest provider or literal differs")
    pins = json.loads((HERE / "versions.json").read_text())
    selected = recipe(pins, provider, version, manifest["platform"], manifest["arch"],
                      manifest["libc"], manifest["baseline"])
    check()
    demand(manifest["native_package"] == selected["native_package"], "Native package selection differs")
    directory = path.parent
    for name in ("wrapper", "native"):
        check()
        demand(verify_archive(directory / (name + ".tgz"), selected[name]) == manifest[name + "_bytes"],
               "Retained archive receipt changed")
        check()
    demand(manifest["assembly_status"] == "assembled" and manifest["wrapper_bytes"]["sha512"] ==
           selected["wrapper"]["integrity"] and manifest["native_bytes"]["sha512"] ==
           selected["native"]["integrity"], "Native assembly or pinned integrity is unavailable")
    demand(tree_receipt(directory / "wrapper") == manifest["wrapper_tree"] and
           tree_receipt(directory / "native") == manifest["native_tree"], "Assembled tree changed")
    check()
    executable = within(directory, directory / manifest["executable"])
    demand(executable.is_file() and sha256(executable) == manifest["executable_sha256"],
           "Selected native executable changed or is absent")
    expected_executable = ("native/" + selected["native_executable"] if provider == "codex"
                           else "wrapper/" + selected["assembled_executable"])
    demand(manifest["executable"] == expected_executable, "Selected executable differs from pinned layout")
    if qualified:
        from native_version_runner import file_identity, validate_sidecar
        receipt = validate_sidecar(directory, manifest["version_receipt"], manifest)
        qualify_receipt(receipt, version, executable, manifest["executable_sha256"])
        request = receipt["request"]
        demand(request["root"] == str(root) and request["directory"] == str(directory) and
               request["provider"] == provider and request["platform"] == manifest["platform"] and
               request["arch"] == manifest["arch"] and request["file_identity"] == file_identity(executable),
               "Native qualification input identity changed")
        identity = json.loads((root / "instance.json").read_text())
        demand(request["instance"] == identity["instance"] and request["port"] == identity["port"],
               "Native qualification fixture identity changed")
        demand(manifest["platform"] == sys.platform and manifest["arch"] == host_arch(),
               "Native executable is for a different host")
    check()
    return manifest, executable


def qualify_receipt(receipt, version, executable, executable_sha256):
    from native_layout import FixtureError
    from native_version_runner import validate_receipt
    try:
        validate_receipt(receipt, version, executable, executable_sha256)
    except (FixtureError, KeyError, TypeError, ValueError, UnicodeError) as error:
        raise FixtureError("Native executable version remains unqualified") from error


def require_native(root, provider, version):
    from native_layout import FixtureError
    from transport import Pending
    try:
        return load_native(root, provider, version)
    except FixtureError as error:
        raise Pending(f"Fixture native qualification pending: {error}") from error


def host_arch():
    value = platform.machine().lower()
    demand(value in ("arm64", "aarch64", "x86_64", "amd64"), "Unsupported host architecture")
    return "arm64" if value in ("arm64", "aarch64") else "x64"


def assemble(root, selected):
    path = manifest_path(root, selected["provider"], selected["version"])
    directory = path.parent
    demand(not directory.exists(), "Native fixture already exists; retain or review partial evidence")
    directory.mkdir(parents=True, mode=0o700)
    deadline = time.monotonic() + 120
    receipt = {key: selected[key] for key in ("provider", "version", "platform", "arch", "libc",
                                             "baseline", "native_package")}
    receipt.update(assembly_status="incomplete", version_receipt={"status": "NOT RUN"})
    write_json(path, receipt)
    for name in ("wrapper", "native"):
        archive = directory / (name + ".tgz")
        receipt[name + "_bytes"] = acquire(selected[name], archive, deadline)
        receipt[name + "_tree"] = extract(archive, directory / name, deadline)
        package = json.loads((directory / name / "package.json").read_text())
        expected_version = selected["version"] if name == "wrapper" else selected["native"]["version"]
        expected_name = selected["package"] if name == "wrapper" else selected["native_package"]
        if name == "native" and selected["provider"] == "codex":
            expected_name = "@openai/codex"
        demand(package.get("name") == expected_name and package.get("version") == expected_version,
               "Archive package identity differs from metadata")
    source = selected["wrapper"]["installer_source"]
    installer = within(directory / "wrapper", directory / "wrapper" / source["path"])
    demand(installer.is_file() and sha256(installer) == source["sha256"],
           "Published wrapper installer source differs from reviewed source")
    native = within(directory / "native", directory / "native" / selected["native_executable"])
    demand(native.is_file() and native.stat().st_size > 0, "Selected native binary is absent")
    native.chmod(0o700)
    if selected["provider"] == "codex":
        executable = native
    else:
        executable = within(directory / "wrapper", directory / "wrapper" / selected["assembled_executable"])
        executable.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(native, executable)
        executable.chmod(0o700)
    receipt.update(assembly_status="assembled", executable=executable.relative_to(directory).as_posix(),
                   executable_sha256=sha256(executable),
                   wrapper_tree=tree_receipt(directory / "wrapper"),
                   native_tree=tree_receipt(directory / "native"))
    write_json(path, receipt)
    return receipt


def verify_version(root, provider, version):
    from native_version_runner import qualify_native
    return qualify_native(root, provider, version, load_native, manifest_path, host_arch)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--provider", choices=("claude", "codex", "opencode"), required=True)
    parser.add_argument("--version", required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--assemble", action="store_true")
    mode.add_argument("--verify-version", action="store_true")
    parser.add_argument("--platform", choices=("darwin", "linux", "win32"))
    parser.add_argument("--arch", choices=("arm64", "x64"))
    parser.add_argument("--libc", choices=("none", "glibc", "musl"), default="none")
    parser.add_argument("--baseline", action="store_true")
    options = parser.parse_args()
    canonical()
    root = fixture_root(options.instance, options.port)
    identity = json.loads((root / "instance.json").read_text())
    demand(identity["port"] == options.port and identity["instance"] == options.instance, "Wrong fixture identity")
    from transport import InstanceLock
    with InstanceLock(root):
        if options.assemble:
            pins = json.loads((HERE / "versions.json").read_text())
            receipt = assemble(root, recipe(pins, options.provider, options.version, options.platform,
                                            options.arch, options.libc, options.baseline))
        else:
            receipt = verify_version(root, options.provider, options.version)
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
