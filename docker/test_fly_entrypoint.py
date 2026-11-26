import importlib.util
import os
import pathlib
import tempfile
import types
import unittest
from unittest import mock

REPO = pathlib.Path(__file__).resolve().parent.parent


def load_container():
    spec = importlib.util.spec_from_file_location(
        "domios_container_under_test", REPO / "packaging/docker/container.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def close_fd_if_open(fd):
    try:
        os.close(fd)
    except OSError:
        pass


class FlyEntrypointTests(unittest.TestCase):
    def _run_fly(self, extra_env, profile_name="qa"):
        mod = load_container()
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            cloud_home = root / "domios-cloud"
            cloud_home.mkdir()
            profile = root / "domios-cloud-profile"
            profile.write_text(f"{profile_name}\n", encoding="utf-8")
            data_dir = cloud_home / (".domios-dev" if profile_name == "qa" else ".domios")
            data_dir.mkdir()
            (data_dir / "config.toml").write_bytes((REPO / "docker/cloud-config.toml").read_bytes())

            events = []
            uid_state = {"uid": 0, "gid": 0}
            fake_pw = types.SimpleNamespace(
                pw_name="node", pw_uid=1000, pw_gid=1000, pw_dir="/home/node"
            )
            fake_pwd = types.SimpleNamespace(getpwnam=lambda name: fake_pw)
            captured = {}

            def fake_chown(path_arg, u, g):
                events.append(("chown", str(path_arg), u, g))

            def fake_chmod(path_arg, mode):
                events.append(("chmod", str(path_arg), oct(mode)))

            def fake_initgroups(name, g):
                events.append(("initgroups", name, g))

            def fake_setgid(g):
                uid_state["gid"] = g
                events.append(("setgid", g))

            def fake_setuid(u):
                uid_state["uid"] = u
                events.append(("setuid", u))

            def patched_private(path_arg, mode, directory=False):
                events.append(("private", str(path_arg), oct(mode), directory, uid_state["uid"]))
                self.assertEqual(uid_state["uid"], 1000, "private() must run after dropping root")

            def fake_execve(path_arg, argv, env):
                captured["path"] = path_arg
                captured["argv"] = list(argv)
                captured["env"] = dict(env)
                if env.get("DOMIOS_CLOUD_BINDING_PIPE") == "9":
                    captured["fd9_inheritable"] = os.get_inheritable(9)
                    captured["fd9_bytes"] = os.read(9, 256)
                    os.close(9)
                raise SystemExit(42)

            base_env = {
                "DOMIOS_CLOUD_PLATFORM": "fly_machines",
                "DOMIOS_CLOUD_MACHINE": "1",
                "PATH": "/usr/local/bin:/usr/bin:/bin",
            }
            base_env.update(extra_env)

            try:
                with mock.patch.object(mod, "CLOUD_HOME", cloud_home), \
                     mock.patch.object(mod, "CLOUD_PROFILE", profile), \
                     mock.patch.object(mod, "pwd", fake_pwd), \
                     mock.patch.object(mod, "private", patched_private), \
                     mock.patch.dict(os.environ, base_env, clear=True), \
                     mock.patch.object(os, "geteuid", lambda: uid_state["uid"], create=True), \
                     mock.patch.object(os, "getuid", lambda: uid_state["uid"], create=True), \
                     mock.patch.object(os, "getegid", lambda: uid_state["gid"], create=True), \
                     mock.patch.object(os, "getgid", lambda: uid_state["gid"], create=True), \
                     mock.patch.object(os, "chown", fake_chown, create=True), \
                     mock.patch.object(os, "chmod", fake_chmod), \
                     mock.patch.object(os, "initgroups", fake_initgroups, create=True), \
                     mock.patch.object(os, "setgid", fake_setgid, create=True), \
                     mock.patch.object(os, "setuid", fake_setuid, create=True), \
                     mock.patch.object(os, "chdir", lambda p: events.append(("chdir", str(p)))), \
                     mock.patch.object(os, "execve", fake_execve):
                    with self.assertRaises(SystemExit) as cm:
                        mod.command([])
                    self.assertEqual(cm.exception.code, 42)
            finally:
                close_fd_if_open(9)

            return events, captured, cloud_home

    def test_execve_env_lacks_bootstrap_secrets_and_fd9_carries_binding(self):
        controller = "A" * 43
        nonce = "B" * 43
        events, captured, cloud_home = self._run_fly({
            "DOMIOS_BOOTSTRAP_CONTROLLER_KEY": controller,
            "DOMIOS_BOOTSTRAP_NONCE": nonce,
        })
        self.assertEqual(events[0], ("chown", str(cloud_home), 1000, 1000))
        self.assertEqual(events[1], ("chmod", str(cloud_home), "0o700"))
        self.assertEqual(events[2], ("initgroups", "node", 1000))
        self.assertEqual(events[3], ("setgid", 1000))
        self.assertEqual(events[4], ("setuid", 1000))
        self.assertEqual(captured["path"], "/usr/local/bin/domios-cloud-start")
        self.assertEqual(captured["argv"], ["/usr/local/bin/domios-cloud-start"])
        env = captured["env"]
        self.assertNotIn("DOMIOS_BOOTSTRAP_CONTROLLER_KEY", env)
        self.assertNotIn("DOMIOS_BOOTSTRAP_NONCE", env)
        self.assertNotIn(controller, env.values())
        self.assertNotIn(nonce, env.values())
        self.assertEqual(env["DOMIOS_CLOUD_BINDING_PIPE"], "9")
        self.assertTrue(captured["fd9_inheritable"])
        self.assertEqual(captured["fd9_bytes"], f"{controller}\n{nonce}\n".encode("ascii"))
        self.assertEqual(env["HOME"], str(cloud_home))
        self.assertEqual(env["XDG_CONFIG_HOME"], str(cloud_home / ".config"))
        self.assertEqual(env["XDG_DATA_HOME"], str(cloud_home / ".local/share"))
        self.assertEqual(env["XDG_CACHE_HOME"], str(cloud_home / ".cache"))
        self.assertEqual(env["XDG_STATE_HOME"], str(cloud_home / ".local/state"))

    def test_execve_env_lacks_malformed_secrets_and_fd9_carries_invalid_sentinel(self):
        bad_secret = "malformed-secret-must-never-leak"
        _, captured, _ = self._run_fly({
            "DOMIOS_BOOTSTRAP_CONTROLLER_KEY": bad_secret,
            "DOMIOS_BOOTSTRAP_NONCE": "short",
        })
        env = captured["env"]
        self.assertNotIn("DOMIOS_BOOTSTRAP_CONTROLLER_KEY", env)
        self.assertNotIn("DOMIOS_BOOTSTRAP_NONCE", env)
        self.assertNotIn(bad_secret, env.values())
        self.assertEqual(env["DOMIOS_CLOUD_BINDING_PIPE"], "9")
        self.assertTrue(captured["fd9_inheritable"])
        self.assertEqual(captured["fd9_bytes"], b"invalid\n")

    def test_execve_without_secrets_clears_stray_binding_pipe(self):
        _, captured, _ = self._run_fly({"DOMIOS_CLOUD_BINDING_PIPE": "42"})
        env = captured["env"]
        self.assertNotIn("DOMIOS_BOOTSTRAP_CONTROLLER_KEY", env)
        self.assertNotIn("DOMIOS_BOOTSTRAP_NONCE", env)
        self.assertNotIn("DOMIOS_CLOUD_BINDING_PIPE", env)
        self.assertNotIn("fd9_bytes", captured)

    def test_unknown_platform_and_fly_args_refused(self):
        mod = load_container()
        with mock.patch.dict(os.environ, {"DOMIOS_CLOUD_PLATFORM": "other"}, clear=False):
            with self.assertRaises(ValueError):
                mod.command([])
        with mock.patch.dict(os.environ, {"DOMIOS_CLOUD_PLATFORM": "fly_machines"}, clear=False):
            with self.assertRaises(ValueError):
                mod.command(["health"])

    def test_unmarked_container_skips_fly_branch(self):
        mod = load_container()
        with mock.patch.dict(os.environ, {}, clear=True):
            with mock.patch.object(mod, "run", return_value=0) as mocked_run:
                self.assertEqual(mod.command([]), 0)
                mocked_run.assert_called_once_with(False)

    def test_domios_cloud_start_passes_inherited_fd9_on_fly(self):
        script_src = (REPO / "docker/domios-cloud-start").read_text(encoding="utf-8")
        self.assertNotIn("\r\n", script_src)
        recorded = {}

        class FakeRuntime:
            def __init__(self, data=None, instance=None, pass_fds=()):
                recorded["data"] = data
                recorded["instance"] = instance
                recorded["pass_fds"] = pass_fds

        class FakeSupervisor:
            def __init__(self, runtime):
                recorded["runtime"] = runtime

            def run(self, prepare_data):
                prepare_data()
                return 0

        fake_container = types.SimpleNamespace(
            ProcessRuntime=FakeRuntime,
            Supervisor=FakeSupervisor,
            stage_cloud_binding_pipe=lambda *a, **kw: (_ for _ in ()).throw(
                AssertionError("must not restage when secrets already stripped")
            ),
        )
        fake_spec = types.SimpleNamespace(
            loader=types.SimpleNamespace(exec_module=lambda mod: None)
        )

        with tempfile.TemporaryDirectory() as tmp:
            fake_home = pathlib.Path(tmp)
            orig_read_text = pathlib.Path.read_text

            def fake_read_text(self_path, *args, **kwargs):
                if str(self_path).replace("\\", "/") == "/etc/domios-cloud-profile":
                    return "qa\n"
                return orig_read_text(self_path, *args, **kwargs)

            env = {
                "DOMIOS_CLOUD_PLATFORM": "fly_machines",
                "DOMIOS_CLOUD_BINDING_PIPE": "9",
                "HOME": str(fake_home),
            }
            with mock.patch.dict(os.environ, env, clear=True), \
                 mock.patch.object(pathlib.Path, "read_text", fake_read_text), \
                 mock.patch.object(pathlib.Path, "home", classmethod(lambda cls: fake_home)), \
                 mock.patch("importlib.util.spec_from_file_location", return_value=fake_spec), \
                 mock.patch("importlib.util.module_from_spec", return_value=fake_container):
                with self.assertRaises(SystemExit) as cm:
                    exec(compile(script_src, "domios-cloud-start", "exec"), {"__name__": "__main__"})
                self.assertEqual(cm.exception.code, 0)

        self.assertEqual(recorded["pass_fds"], (9,))
        self.assertEqual(recorded["instance"], {"kind": "development", "instance": None, "port": 7686})
        self.assertEqual(recorded["data"], fake_home / ".domios-dev")


if __name__ == "__main__":
    unittest.main(verbosity=2)
