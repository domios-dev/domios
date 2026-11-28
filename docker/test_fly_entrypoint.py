import hashlib
import http.server
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import threading
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
            runtime_dir = root / "run-domios-cloud"
            profile = root / "domios-cloud-profile"
            profile.write_text(f"{profile_name}\n", encoding="utf-8")
            data_dir = cloud_home / (".domios-dev" if profile_name == "qa" else ".domios")
            data_dir.mkdir()
            (data_dir / "config.toml").write_bytes((REPO / "docker/cloud-config.toml").read_bytes())

            events = []
            uid_state = {"uid": 0, "gid": 0}
            fake_pw = types.SimpleNamespace(
                pw_name="node", pw_uid=1000, pw_gid=1000, pw_dir="/opt/domios-home"
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
                     mock.patch.object(mod, "CLOUD_RUNTIME_DIR", runtime_dir), \
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
        self.assertEqual(events[2][0], "chown")
        self.assertEqual(events[3][0], "chmod")
        self.assertEqual(events[4], ("initgroups", "node", 1000))
        self.assertEqual(events[5], ("setgid", 1000))
        self.assertEqual(events[6], ("setuid", 1000))
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

    def test_export_and_restore_secrets_never_reach_daemon_or_children_and_export_key_is_one_shot(self):
        mod = load_container()
        spec_arc = importlib.util.spec_from_file_location(
            "domios_cloud_archive_ep_test", REPO / "packaging/docker/cloud_archive.py"
        )
        arc_mod = importlib.util.module_from_spec(spec_arc)
        sys.modules[spec_arc.name] = arc_mod
        spec_arc.loader.exec_module(arc_mod)

        export_key = os.urandom(32).hex()
        restore_key = os.urandom(32).hex()

        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            # Prepare a real encrypted archive for pre-start restore testing too.
            src_home = root / "src-home"
            (src_home / ".domios").mkdir(parents=True)
            (src_home / ".domios" / "desk.json").write_text('{"tabs":1}', encoding="utf-8")
            (src_home / "sentinel.txt").write_text("boot-secret-isolation-proof\n", encoding="utf-8")
            restore_enc = root / "restore.enc"
            exp_seed = arc_mod.export_essentials_archive(
                src_home,
                restore_enc,
                archive_id="arc_boot_seed",
                owner_uid="uid-boot-1",
                machine_id="cm_boot_1",
                generation=1,
                owner_key=bytes.fromhex(restore_key),
            )

            cloud_home = root / "domios-cloud"
            cloud_home.mkdir()
            runtime_dir = root / "run-domios-cloud"
            profile = root / "domios-cloud-profile"
            profile.write_text("qa\n", encoding="utf-8")
            data_dir = cloud_home / ".domios-dev"
            data_dir.mkdir()
            (data_dir / "config.toml").write_bytes((REPO / "docker/cloud-config.toml").read_bytes())

            uid_state = {"uid": 0, "gid": 0}
            fake_pw = types.SimpleNamespace(pw_name="node", pw_uid=1000, pw_gid=1000, pw_dir="/opt/domios-home")
            fake_pwd = types.SimpleNamespace(getpwnam=lambda name: fake_pw)
            captured = {}
            chmod_modes = []

            def fake_execve(path_arg, argv, env):
                captured["env"] = dict(env)
                raise SystemExit(42)

            def record_chmod(p, m):
                chmod_modes.append((str(p), oct(m)))

            boot_env = {
                "DOMIOS_CLOUD_PLATFORM": "fly_machines",
                "DOMIOS_CLOUD_MACHINE": "1",
                "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
                # Ephemeral export secrets delivered via Fly secret before wake:
                "DOMIOS_EXPORT_ARCHIVE_KEY_HEX": export_key,
                "DOMIOS_EXPORT_ARCHIVE_UPLOAD_URL": "https://storage.example.invalid/signed-put?sig=secret",
                # Ephemeral restore secrets delivered via Fly secret before cold wake:
                "DOMIOS_RESTORE_ARCHIVE_PATH": str(restore_enc),
                "DOMIOS_RESTORE_ARCHIVE_SHA256": exp_seed.ciphertext_sha256,
                "DOMIOS_RESTORE_ARCHIVE_KEY_HEX": restore_key,
                "DOMIOS_RESTORE_OWNER_UID": "uid-boot-1",
                "DOMIOS_RESTORE_MACHINE_ID": "cm_boot_1",
                "DOMIOS_RESTORE_GENERATION": "1",
            }
            if "SYSTEMROOT" in os.environ:
                boot_env["SYSTEMROOT"] = os.environ["SYSTEMROOT"]

            with mock.patch.object(mod, "CLOUD_HOME", cloud_home), \
                 mock.patch.object(mod, "CLOUD_RUNTIME_DIR", runtime_dir), \
                 mock.patch.object(mod, "CLOUD_PROFILE", profile), \
                 mock.patch.object(mod, "pwd", fake_pwd), \
                 mock.patch.object(mod, "private", lambda *a, **kw: None), \
                 mock.patch.dict(os.environ, boot_env, clear=True), \
                 mock.patch.object(os, "geteuid", lambda: uid_state["uid"], create=True), \
                 mock.patch.object(os, "getuid", lambda: uid_state["uid"], create=True), \
                 mock.patch.object(os, "getegid", lambda: uid_state["gid"], create=True), \
                 mock.patch.object(os, "getgid", lambda: uid_state["gid"], create=True), \
                 mock.patch.object(os, "chown", lambda *a, **kw: None, create=True), \
                 mock.patch.object(os, "chmod", record_chmod), \
                 mock.patch.object(os, "initgroups", lambda *a, **kw: None, create=True), \
                 mock.patch.object(os, "setgid", lambda g: uid_state.__setitem__("gid", g), create=True), \
                 mock.patch.object(os, "setuid", lambda u: uid_state.__setitem__("uid", u), create=True), \
                 mock.patch.object(os, "chdir", lambda p: None), \
                 mock.patch.object(os, "execve", fake_execve):
                with self.assertRaises(SystemExit) as cm:
                    mod.command([])
                self.assertEqual(cm.exception.code, 42)

            # 1. Confirm restore ran before execve AND neither DOMIOS_EXPORT_* nor DOMIOS_RESTORE_* reached daemon env!
            self.assertEqual((cloud_home / "sentinel.txt").read_text(encoding="utf-8"), "boot-secret-isolation-proof\n")
            daemon_env = captured["env"]
            for k, v in daemon_env.items():
                self.assertFalse(k.startswith("DOMIOS_EXPORT_"), f"Leaked export env var to daemon: {k}")
                self.assertFalse(k.startswith("DOMIOS_RESTORE_"), f"Leaked restore env var to daemon: {k}")
                self.assertNotIn(export_key, v, f"Leaked export key in daemon env var: {k}")
                self.assertNotIn(restore_key, v, f"Leaked restore key in daemon env var: {k}")

            # 2. Spawn a real child process with the daemon env (modeling PTY/agent child) -> child env has NO secrets!
            child_proc = subprocess.run(
                [sys.executable, "-c", "import json, os; print(json.dumps(dict(os.environ)))"],
                env=daemon_env,
                capture_output=True,
                text=True,
                check=True,
            )
            child_env = json.loads(child_proc.stdout)
            for k, v in child_env.items():
                self.assertFalse(k.startswith("DOMIOS_EXPORT_"), f"Leaked export env var to child: {k}")
                self.assertFalse(k.startswith("DOMIOS_RESTORE_"), f"Leaked restore env var to child: {k}")
                self.assertNotIn(export_key, v)
                self.assertNotIn(restore_key, v)

            # 3. Export key was staged into 0600 file inside runtime_dir; consume ONCE (read + unlink) and export via stdin!
            key_file = runtime_dir / mod.EXPORT_KEY_FILENAME
            self.assertTrue(key_file.is_file())
            self.assertIn((str(key_file), "0o600"), chmod_modes)
            consumed_key = key_file.read_text(encoding="ascii").strip()
            key_file.unlink()
            self.assertEqual(consumed_key, export_key)
            self.assertFalse(key_file.exists())

            uploaded: dict[str, bytes] = {}

            class Handler(http.server.BaseHTTPRequestHandler):
                def log_message(self, fmt, *args):
                    return

                def do_PUT(self):
                    n = int(self.headers.get("Content-Length", "0"))
                    uploaded[self.path] = self.rfile.read(n)
                    self.send_response(200)
                    self.end_headers()

            srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            port = srv.server_address[1]
            t = threading.Thread(target=srv.serve_forever, daemon=True)
            t.start()
            try:
                upload_url = f"http://127.0.0.1:{port}/arc-boot-export.enc"
                # First export succeeds with key delivered on stdin (daemon_env has NO export key!).
                first = subprocess.run(
                    [sys.executable, str(REPO / "packaging/docker/cloud_archive.py"), "export-and-upload"],
                    input=json.dumps({
                        "home_dir": str(cloud_home),
                        "archive_id": "arc_ep_1",
                        "owner_uid": "uid-boot-1",
                        "machine_id": "cm_boot_1",
                        "generation": 2,
                        "upload_url": upload_url,
                        "owner_key_hex": consumed_key,
                    }),
                    env=daemon_env,
                    capture_output=True,
                    text=True,
                    check=True,
                )
                receipt = json.loads(first.stdout)
                self.assertTrue(receipt["excluded_secrets_verified"])
                self.assertEqual(
                    hashlib.sha256(uploaded["/arc-boot-export.enc"]).hexdigest(),
                    receipt["ciphertext_sha256"],
                )

                # 4. Second export without a new key delivery MUST fail with `missing_key`!
                self.assertFalse(key_file.exists(), "One-shot key file must remain unlinked")
                second = subprocess.run(
                    [sys.executable, str(REPO / "packaging/docker/cloud_archive.py"), "export-and-upload"],
                    input=json.dumps({
                        "home_dir": str(cloud_home),
                        "archive_id": "arc_ep_2",
                        "owner_uid": "uid-boot-1",
                        "machine_id": "cm_boot_1",
                        "generation": 2,
                        "upload_url": upload_url,
                    }),
                    # Even if someone tries to pass it via env, stdin-only contract refuses with missing_key!
                    env={**daemon_env, "DOMIOS_EXPORT_ARCHIVE_KEY_HEX": export_key},
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertNotEqual(second.returncode, 0)
                err_json = json.loads(second.stderr)
                self.assertEqual(err_json["code"], "missing_key")
            finally:
                srv.shutdown()
                srv.server_close()


if __name__ == "__main__":
    unittest.main(verbosity=2)
