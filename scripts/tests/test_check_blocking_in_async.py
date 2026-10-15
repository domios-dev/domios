import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "gate", Path(__file__).resolve().parents[1] / "check-blocking-in-async.py")
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


class AsyncBlockingGateTests(unittest.TestCase):
    def patterns(self, source):
        return [pattern for _, pattern in gate.findings(source)]

    def test_every_ordered_lock_is_rejected(self):
        for field, methods in gate.LOCKS.items():
            for method in methods:
                with self.subTest(field=field, method=method):
                    self.assertEqual(
                        self.patterns(f"async fn f() {{ svc.{field}.{method}(); }}"),
                        [f".{field}.{method}()"])

    def test_multiline_chain_reports_lock_line(self):
        self.assertEqual(gate.findings(
            "async fn f() {\nsvc\n.mux\n.read();\n}"), [(3, ".mux.read()")])

    def test_sync_function_and_test_module_are_exempt(self):
        self.assertEqual(self.patterns("""
            fn f() { svc.mux.read(); }
            #[cfg(test)] mod tests {
                async fn test() { svc.mux.write(); }
            }
        """), [])

    def test_both_blocking_closure_shapes_are_exempt(self):
        self.assertEqual(self.patterns("""
            async fn f() {
                tokio::task::spawn_blocking(move || svc.mux.read().snapshot()).await;
                tauri::async_runtime::spawn_blocking({
                    let svc = svc.clone();
                    move || { svc.pty.lock(); }
                }).await;
                blocking_run(move || { svc.output_buf.lock(); Ok(()) }).await;
                on_blocking_pool(move |svc| { svc.screen_state.lock(); }).await;
            }
        """), [])

    def test_lock_after_blocking_closure_is_rejected(self):
        self.assertEqual(self.patterns("""
            async fn f() {
                spawn_blocking(move || { svc.mux.read(); }).await;
                svc.mux.write();
            }
        """), [".mux.write()"])

    def test_lock_evaluated_before_closure_is_rejected(self):
        self.assertEqual(self.patterns("""
            async fn f() {
                spawn_blocking({ let snapshot = svc.mux.read().snapshot();
                    move || snapshot }).await;
            }
        """), [".mux.read()"])

    def test_known_sync_helpers_are_rejected(self):
        for helper in gate.LOCKING_HELPERS:
            with self.subTest(helper=helper):
                self.assertEqual(
                    self.patterns(f"async fn f() {{ {helper}(&state, &id); }}"),
                    [f"{helper}()"])

    def test_comments_strings_and_lifetimes_do_not_change_scope(self):
        self.assertEqual(self.patterns(r'''
            async fn f(x: &'static str) {
                // svc.mux.read();
                let text = r##" } svc.pty.lock(); { "##;
                let escaped = "\"svc.output_buf.lock()\"";
                /* svc.screen_state.lock(); */
                svc.agent_tracker.lock();
            }
        '''), [".agent_tracker.lock()"])

    def test_async_blocks_streams_and_lock_aliases_are_rejected(self):
        self.assertEqual(self.patterns("""
            fn f() {
                spawn(async move { svc.mux.read(); });
                async_stream::stream! { output_buf.lock(); }
            }
        """), [".mux.read()", ".output_buf.lock()"])

    def test_legacy_blockers_remain_guarded(self):
        self.assertEqual(self.patterns("""
            use std::process::Command;
            async fn f() {
                std::process::Command::new("x");
                Command::new("x");
                std::thread::sleep(deadline);
                reqwest::blocking::get(url);
            }
        """), ["std::process::Command::new", "Command::new",
               "std::thread::sleep", "reqwest::blocking"])

    def test_repository_scope_and_empty_allowlist(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            roots = [repo / "src-tauri/crates" / name / "src" for name in (
                "codeterm-api", "codeterm-commands", "codeterm-socket",
                "codeterm-sentinel-host")]
            roots += [repo / "src-tauri/crates/codeterm-service-core/src" / name
                      for name in ("api_support", "startup")]
            for root in roots:
                root.mkdir(parents=True)
                (root / "handler.rs").write_text("async fn f() { svc.mux.read(); }")
                (root / "tests").mkdir()
                (root / "tests/ignored.rs").write_text("async fn f() { svc.mux.write(); }")
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(gate.main(repo), 1)
            self.assertEqual(len(output.getvalue().splitlines()), 6)
            (repo / "scripts").mkdir()
            allowlist = repo / "scripts/blocking-in-async-allowlist.txt"
            allowlist.write_text(output.getvalue())
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gate.main(repo), 0)
            allowlist.write_text("")
            for root in roots:
                (root / "handler.rs").write_text(
                    "async fn f() { spawn_blocking(move || svc.mux.read()); }")
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gate.main(repo), 0)


if __name__ == "__main__":
    unittest.main()
