"""Select exact native package layouts without executing vendor installers."""

from pathlib import PurePosixPath


class FixtureError(RuntimeError):
    pass


def demand(condition, reason):
    if not condition:
        raise FixtureError(reason)


def recipe(pins, provider, version, platform, arch, libc="none", baseline=False):
    demand(provider in ("claude", "codex", "opencode"), "Unsupported native provider")
    pin = pins["providers"][provider]
    demand(version in (pin["older"], pin["target"]), "Version differs from reviewed literal")
    demand(platform in ("darwin", "linux", "win32") and arch in ("x64", "arm64"),
           "Unsupported platform or architecture")
    demand(libc in (("glibc", "musl") if platform == "linux" else ("none",)),
           "Explicit compatible libc is required")
    demand(not baseline or (provider == "opencode" and arch == "x64"),
           "Baseline selection is supported only for OpenCode x64")
    wrapper = pin["versions"][version]
    suffix = ".exe" if platform == "win32" else ""
    if provider == "claude":
        package = f"@anthropic-ai/claude-code-{platform}-{arch}"
        if libc == "musl":
            package += "-musl"
        executable = "claude" + suffix
    elif provider == "codex":
        package = f"@openai/codex-{platform}-{arch}"
        cpu = "x86_64" if arch == "x64" else "aarch64"
        target = {"darwin": "apple-darwin", "linux": "unknown-linux-musl",
                  "win32": "pc-windows-msvc"}[platform]
        executable = f"vendor/{cpu}-{target}/bin/codex{suffix}"
    else:
        package = f"opencode-{'windows' if platform == 'win32' else platform}-{arch}"
        if baseline:
            package += "-baseline"
        if libc == "musl":
            package += "-musl"
        executable = "bin/opencode" + suffix
    demand(package in wrapper["optional_dependencies"], "Compatible optional dependency absent")
    native = wrapper["native_distribution_metadata"].get(package)
    demand(native is not None, "Native metadata absent")
    demand(platform in native["os"] and arch in native["cpu"], "Native metadata platform differs")
    expected = wrapper["optional_dependencies"][package]
    expected = expected.rsplit("@", 1)[-1] if expected.startswith("npm:") else expected
    demand(native["version"] == expected, "Native alias version differs")
    return {"provider": provider, "version": version, "platform": platform, "arch": arch,
            "libc": libc, "baseline": baseline, "package": pin["package"],
            "native_package": package, "wrapper": wrapper, "native": native,
            "native_executable": str(PurePosixPath(executable)),
            "assembled_executable": executable if provider == "codex" else f"bin/{provider}.exe",
            "layout": f"{platform}-{arch}-{libc}-{'baseline' if baseline else 'normal'}"}
