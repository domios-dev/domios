import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

import yaml

SCRIPTS_DIR = pathlib.Path(__file__).parents[1]
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

SCRIPT = SCRIPTS_DIR / "convert-legacy-parked-tasks.py"
SPEC = importlib.util.spec_from_file_location("convert", SCRIPT)
CONVERT = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CONVERT
SPEC.loader.exec_module(CONVERT)
FIXTURE = pathlib.Path(__file__).parents[2] / "tests/fixtures/plan-state/legacy-parked-task.yaml"
INLINE_FIXTURE = (
    pathlib.Path(__file__).parents[2]
    / "tests/fixtures/plan-state/legacy-parked-inline-milestones.yaml"
)


class ConvertLegacyParkedTaskTests(unittest.TestCase):
    def setUp(self):
        self.real_namespace_barrier = CONVERT.sync_namespace
        CONVERT.sync_namespace = lambda _path: None

    def tearDown(self):
        CONVERT.sync_namespace = self.real_namespace_barrier

    def copy_fixture(self):
        directory = tempfile.TemporaryDirectory()
        path = pathlib.Path(directory.name) / "task.yaml"
        shutil.copy(FIXTURE, path)
        return directory, path

    def make_batch(self, count=3):
        directory = tempfile.TemporaryDirectory()
        paths = []
        conversions = []
        originals = []
        for index in range(count):
            path = pathlib.Path(directory.name) / f"task-{index}.yaml"
            shutil.copy(FIXTURE, path)
            paths.append(path)
            originals.append(path.read_bytes())
            conversions.append(
                CONVERT.prepare_conversion(
                    path,
                    "transaction test",
                    f"ms_transaction_{index}",
                    123456 + index,
                )
            )
        return directory, paths, conversions, originals

    def assert_rolled_back(self, error, paths, originals):
        for path, original in zip(paths, originals):
            self.assertEqual(path.read_bytes(), original)
        journal = json.loads(error.exception.journal_path.read_text())
        self.assertEqual(journal["logical_state"], "rolled_back")
        self.assertEqual(journal["durability"], "verified")
        self.assertEqual(len(journal["entries"]), len(paths))
        for entry, original in zip(journal["entries"], originals):
            self.assertEqual(pathlib.Path(entry["backup"]).read_bytes(), original)

    def assert_repair_required(self, error, paths, originals):
        self.assertEqual([path.read_bytes() for path in paths], originals)
        journal = json.loads(error.exception.journal_path.read_text())
        self.assertEqual(journal["logical_state"], "rolled_back")
        self.assertEqual(journal["durability"], "repair_required")
        self.assertTrue((paths[0].parent / ".legacy-parked-conversion.lock").exists())

    def assert_drift_rollback(self, error, paths, originals, drift_index, drifted):
        expected = list(originals)
        expected[drift_index] = drifted
        for path, content in zip(paths, expected):
            self.assertEqual(path.read_bytes(), content)
        journal = json.loads(error.exception.journal_path.read_text())
        self.assertEqual(journal["logical_state"], "rollback_failed")
        self.assertIn("source changed after staging", journal["error"])
        self.assertEqual(
            journal["entries"][drift_index]["recovery_classification"], "conflict"
        )

    @staticmethod
    def no_source_handles(_paths):
        return []

    def test_dry_run_is_non_mutating(self):
        directory, path = self.copy_fixture()
        before = path.read_text()
        result = subprocess.run(
            [sys.executable, SCRIPT, "--reason", "operator hold", path],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn("would convert", result.stdout)
        self.assertEqual(path.read_text(), before)
        directory.cleanup()

    def test_apply_preserves_structure_and_closes_assignments(self):
        directory, path = self.copy_fixture()
        before = path.read_text()
        conversion = CONVERT.prepare_conversion(path, "operator hold", "ms_audit", 123456)
        self.assertIsNotNone(conversion)
        path.write_text(conversion.text, newline="")
        after = path.read_text()
        value = yaml.safe_load(after)
        self.assertEqual(value["status"], "paused")
        self.assertEqual(value["pause_reason"], "Legacy operator hold")
        self.assertIsNone(value["assigned_pane_id"])
        self.assertIsNone(value["lane"]["pane_id"])
        self.assertEqual(value["execution_attempts"][0]["closed_at"], 123456)
        self.assertEqual(value["execution_attempts"][1]["closed_at"], 75)
        self.assertEqual(
            value["execution_attempts"][0]["trace"]["phases"][0]["commands"],
            ["rg", "cargo"],
        )
        self.assertEqual(value["milestones"][-1]["id"], "ms_audit")
        self.assertIn("audit reason: operator hold", value["milestones"][-1]["text"])
        self.assertEqual(value["unknown_field"], yaml.safe_load(before)["unknown_field"])
        self.assertIn("# preserve this comment", after)
        self.assertIn("body: |\n  Original formatting stays intact.\n", after)
        self.assertIn("# nested structures must remain byte-for-byte stable", after)
        self.assertIn("closed_at: 75 # preserve existing closure", after)
        self.assertEqual(
            before.split("    trace:", 1)[1].split("    closed_at:", 1)[0],
            after.split("    trace:", 1)[1].split("    closed_at:", 1)[0],
        )
        self.assertIn("name: Legacy parked task", before)
        directory.cleanup()

    def test_inline_milestone_comment_and_nested_yaml_are_preserved(self):
        directory = tempfile.TemporaryDirectory()
        path = pathlib.Path(directory.name) / "task.yaml"
        shutil.copy(INLINE_FIXTURE, path)
        before = path.read_text()
        conversion = CONVERT.prepare_conversion(path, "cutover hold", "ms_inline", 123456)
        self.assertIsNotNone(conversion)
        after = conversion.text
        value = yaml.safe_load(after)
        self.assertEqual(value["pause_reason"], "cutover hold")
        self.assertEqual(value["execution_attempts"][0]["closed_at"], 123456)
        self.assertEqual(
            value["execution_attempts"][0]["details"]["checkpoints"][1]["evidence"][
                "nested"
            ],
            ["alpha", "beta"],
        )
        self.assertEqual(value["unknown"], yaml.safe_load(before)["unknown"])
        self.assertEqual(
            before.split("    details:", 1)[1].split("    closed_at:", 1)[0],
            after.split("    details:", 1)[1].split("    closed_at:", 1)[0],
        )
        self.assertEqual(
            before.split("unknown:", 1)[1],
            after.split("unknown:", 1)[1].split("- id: ms_inline", 1)[0],
        )
        self.assertIn("milestones: # keep this audit-list comment", after)
        self.assertIn(
            "pause_reason: \"cutover hold\" # retain the legacy field comment on rename",
            after,
        )
        directory.cleanup()

    def test_apply_is_idempotent(self):
        directory, path = self.copy_fixture()
        conversion = CONVERT.prepare_conversion(path, "hold", "ms_audit", 123456)
        path.write_text(conversion.text, newline="")
        once = path.read_text()
        self.assertIsNone(CONVERT.prepare_conversion(path, "hold", "ms_other", 999999))
        self.assertEqual(path.read_text(), once)
        directory.cleanup()

    def test_mixed_input_is_rejected_before_any_write(self):
        first_directory, first = self.copy_fixture()
        second_directory = tempfile.TemporaryDirectory()
        second = pathlib.Path(second_directory.name) / "task.yaml"
        second.write_text("id: task_pending\nstatus: pending\n")
        before = first.read_text()
        result = subprocess.run(
            [sys.executable, SCRIPT, "--reason", "hold", "--apply", first, second],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertEqual(first.read_text(), before)
        first_directory.cleanup()
        second_directory.cleanup()

    def test_duplicate_input_is_rejected_before_write(self):
        directory, path = self.copy_fixture()
        before = path.read_text()
        result = subprocess.run(
            [sys.executable, SCRIPT, "--reason", "hold", "--apply", path, path],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("duplicate input path", result.stderr)
        self.assertEqual(path.read_text(), before)
        directory.cleanup()

    def test_blank_reason_is_rejected(self):
        directory, path = self.copy_fixture()
        result = subprocess.run(
            [sys.executable, SCRIPT, "--reason", "   ", path],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        directory.cleanup()

    def test_batch_audit_metadata_is_unique_and_strictly_monotonic(self):
        directory = tempfile.TemporaryDirectory()
        first = pathlib.Path(directory.name) / "first.yaml"
        second = pathlib.Path(directory.name) / "second.yaml"
        shutil.copy(FIXTURE, first)
        shutil.copy(FIXTURE, second)
        conversions = []
        for index, path in enumerate((first, second)):
            conversions.append(
                CONVERT.prepare_conversion(
                    path, "cutover", f"ms_cutover_{index}", 123456 + index
                )
            )
        CONVERT.write_all(conversions)
        first_value = yaml.safe_load(first.read_text())
        second_value = yaml.safe_load(second.read_text())
        first_audit = first_value["milestones"][-1]
        second_audit = second_value["milestones"][-1]
        self.assertNotEqual(first_audit["id"], second_audit["id"])
        self.assertLess(first_audit["ts"], second_audit["ts"])
        directory.cleanup()

    def test_existing_audit_id_and_timestamp_are_rejected(self):
        directory, path = self.copy_fixture()
        value = yaml.safe_load(path.read_text())
        value["milestones"][0]["ts"] = 500
        path.write_text(yaml.safe_dump(value, sort_keys=False))
        with self.assertRaisesRegex(ValueError, "audit id already exists"):
            CONVERT.prepare_conversion(path, "hold", "ms_existing", 501)
        with self.assertRaisesRegex(ValueError, "must be newer"):
            CONVERT.prepare_conversion(path, "hold", "ms_new", 500)
        directory.cleanup()

    def test_duplicate_lifecycle_keys_are_rejected_before_classification(self):
        mutations = {
            "status": lambda text: text.replace(
                "status: parked # legacy state requiring explicit conversion",
                "status: parked\nstatus: paused",
            ),
            "park_reason": lambda text: text.replace(
                "park_reason: Legacy operator hold",
                "park_reason: first\npark_reason: second",
            ),
            "pause_reason": lambda text: text
            + "pause_reason: first\npause_reason: second\n",
        }
        for key, mutate in mutations.items():
            with self.subTest(key=key):
                directory, path = self.copy_fixture()
                path.write_text(mutate(path.read_text()), newline="")
                with self.assertRaisesRegex(ValueError, f"duplicate lifecycle key '{key}'"):
                    CONVERT.load_candidate(path)
                directory.cleanup()

    def test_paused_idempotency_requires_one_well_formed_matching_audit(self):
        mutations = {
            "blank reason": lambda value: value.update(pause_reason="   "),
            "legacy reason retained": lambda value: value.update(park_reason="old"),
            "missing source": lambda value: value["milestones"][-1].pop("from"),
            "mismatched reason": lambda value: value["milestones"][-1].update(
                text=value["milestones"][-1]["text"].replace(
                    "Legacy operator hold", "different hold"
                )
            ),
            "blank audit reason": lambda value: value["milestones"][-1].update(
                text=(
                    "paused: Legacy operator hold; audit reason:  "
                    f"({CONVERT.AUDIT_MARKER})"
                )
            ),
            "duplicate audit id": lambda value: value["milestones"][0].update(
                id="ms_audit"
            ),
            "duplicate audit": lambda value: value["milestones"].append(
                dict(value["milestones"][-1])
            ),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                directory, path = self.copy_fixture()
                conversion = CONVERT.prepare_conversion(path, "hold", "ms_audit", 123456)
                value = yaml.safe_load(conversion.text)
                mutate(value)
                path.write_text(yaml.safe_dump(value, sort_keys=False))
                with self.assertRaises(ValueError):
                    CONVERT.prepare_conversion(path, "hold")
                directory.cleanup()

    def test_replace_failure_at_each_position_restores_the_whole_batch(self):
        for failure_position in (1, 2, 3):
            with self.subTest(failure_position=failure_position):
                directory, paths, conversions, originals = self.make_batch()
                calls = 0

                def replace_with_failure(source, destination, backup):
                    nonlocal calls
                    calls += 1
                    if calls == failure_position:
                        raise OSError("injected replace failure")
                    CONVERT.atomic_replace(source, destination, backup)

                with self.assertRaises(CONVERT.ConversionWriteError) as error:
                    CONVERT.write_all(conversions, replace_file=replace_with_failure)
                self.assert_rolled_back(error, paths, originals)
                journal = json.loads(error.exception.journal_path.read_text())
                states = [entry["state"] for entry in journal["entries"]]
                self.assertEqual(states[failure_position - 1], "replace_intent")
                directory.cleanup()

    def test_fsync_failure_at_each_position_restores_the_whole_batch(self):
        for failure_position in (1, 2, 3):
            with self.subTest(failure_position=failure_position):
                directory, paths, conversions, originals = self.make_batch()
                calls = 0

                def sync_with_failure(path):
                    nonlocal calls
                    calls += 1
                    if calls == failure_position:
                        raise CONVERT.DurabilityError("injected fsync failure")

                with self.assertRaises(CONVERT.ConversionWriteError) as error:
                    CONVERT.write_all(conversions, sync_destination=sync_with_failure)
                self.assert_repair_required(error, paths, originals)
                directory.cleanup()

    def test_drift_after_staging_aborts_before_any_replacement(self):
        for drift_position in (1, 2, 3):
            with self.subTest(drift_position=drift_position):
                directory, paths, conversions, originals = self.make_batch()
                drift_index = drift_position - 1
                drifted = originals[drift_index] + b"# concurrent edit\n"

                def mutate_after_staging():
                    paths[drift_index].write_bytes(drifted)

                with self.assertRaises(CONVERT.ConversionWriteError) as error:
                    CONVERT.write_all(
                        conversions,
                        after_staging=mutate_after_staging,
                        source_handle_factory=self.no_source_handles,
                    )
                self.assert_drift_rollback(
                    error, paths, originals, drift_index, drifted
                )
                directory.cleanup()

    def test_drift_at_each_replacement_boundary_is_preserved(self):
        for drift_position in (1, 2, 3):
            with self.subTest(drift_position=drift_position):
                directory, paths, conversions, originals = self.make_batch()
                drift_index = drift_position - 1
                drifted = originals[drift_index] + b"# boundary edit\n"

                def mutate_before_replace(index):
                    if index == drift_index:
                        paths[drift_index].write_bytes(drifted)

                with self.assertRaises(CONVERT.ConversionWriteError) as error:
                    CONVERT.write_all(
                        conversions,
                        before_replace=mutate_before_replace,
                        source_handle_factory=self.no_source_handles,
                    )
                self.assert_drift_rollback(
                    error, paths, originals, drift_index, drifted
                )
                directory.cleanup()

    def test_preflight_runs_with_prepared_journal_and_lock(self):
        directory, paths, conversions, originals = self.make_batch()
        lock = paths[0].parent / ".legacy-parked-conversion.lock"

        def inspect_preflight_state():
            journals = list(
                paths[0].parent.glob(".legacy-parked-conversion-*/journal.json")
            )
            self.assertEqual(len(journals), 1)
            journal = json.loads(journals[0].read_text())
            self.assertEqual(journal["logical_state"], "prepared")
            lock_record = json.loads(lock.read_text())
            self.assertEqual(lock_record["journal"], str(journals[0]))
            self.assertEqual(lock_record["pid"], os.getpid())
            self.assertTrue(lock_record["process_start"])
            self.assertTrue(lock_record["host"])
            self.assertEqual(
                [entry["original_sha256"] for entry in journal["entries"]],
                [CONVERT.content_hash(original) for original in originals],
            )

        CONVERT.write_all(conversions, after_staging=inspect_preflight_state)
        self.assertFalse(lock.exists())
        directory.cleanup()

    def test_source_handles_are_exclusive_for_the_transaction(self):
        directory, path = self.copy_fixture()
        handles = CONVERT.acquire_source_handles([path])
        try:
            with self.assertRaises(OSError):
                with path.open("r+b"):
                    pass
        finally:
            CONVERT.release_source_handles(handles)
        directory.cleanup()

    def test_successful_batch_retains_committed_journal_and_exact_backups(self):
        directory, paths, conversions, originals = self.make_batch()
        journal_path = CONVERT.write_all(conversions)
        journal = json.loads(journal_path.read_text())
        self.assertEqual(journal["logical_state"], "committed")
        self.assertEqual(journal["durability"], "verified")
        self.assertEqual(journal["replaced_count"], 3)
        for entry, original, path in zip(journal["entries"], originals, paths):
            self.assertEqual(pathlib.Path(entry["backup"]).read_bytes(), original)
            self.assertEqual(entry["state"], "installed")
            self.assertEqual(entry["original_sha256"], entry["backup_sha256"])
            self.assertEqual(entry["installed_sha256"], entry["staged_sha256"])
            self.assertEqual(CONVERT.file_hash(path), entry["installed_sha256"])
            self.assertEqual(yaml.safe_load(path.read_text())["status"], "paused")
        directory.cleanup()

    def test_preflight_barrier_failure_is_durability_unverified_without_mutation(self):
        directory, paths, conversions, originals = self.make_batch()

        def unavailable(_path):
            raise CONVERT.DurabilityError("namespace barrier unavailable")

        with self.assertRaises(CONVERT.ConversionWriteError) as error:
            CONVERT.write_all(conversions, namespace_barrier=unavailable)
        self.assertEqual([path.read_bytes() for path in paths], originals)
        journal = json.loads(error.exception.journal_path.read_text())
        self.assertEqual(journal["logical_state"], "prepared")
        self.assertEqual(journal["durability"], "durability_unverified")
        self.assertTrue((paths[0].parent / ".legacy-parked-conversion.lock").exists())
        directory.cleanup()

    def test_post_replace_barrier_failure_requires_repair(self):
        directory, paths, conversions, originals = self.make_batch()
        calls = 0

        def fail_after_first_replace(_path):
            nonlocal calls
            calls += 1
            if calls == 3:
                raise CONVERT.DurabilityError("post-replace barrier failed")

        with self.assertRaises(CONVERT.ConversionWriteError) as error:
            CONVERT.write_all(conversions, namespace_barrier=fail_after_first_replace)
        self.assert_repair_required(error, paths, originals)
        directory.cleanup()

    def test_commit_lock_barrier_failure_is_not_reported_committed(self):
        directory, paths, conversions, _originals = self.make_batch()

        def fail_lock_removal(path):
            if path.name == ".legacy-parked-conversion.lock":
                raise CONVERT.DurabilityError("lock namespace barrier failed")

        with self.assertRaises(CONVERT.ConversionWriteError) as error:
            CONVERT.write_all(conversions, namespace_barrier=fail_lock_removal)
        journal = json.loads(error.exception.journal_path.read_text())
        self.assertEqual(journal["logical_state"], "committed")
        self.assertEqual(journal["durability"], "repair_required")
        self.assertTrue((paths[0].parent / ".legacy-parked-conversion.lock").exists())
        for path in paths:
            self.assertEqual(yaml.safe_load(path.read_text())["status"], "paused")
        directory.cleanup()

    def test_windows_replace_error_codes_are_layout_classified(self):
        original = "a" * 64
        installed = "b" * 64
        unchanged = {
            "destination": original,
            "replacement": installed,
            "replace_backup": None,
        }
        partial = dict(unchanged, destination=None, replace_backup=original)
        for code, name in (
            (1175, "ERROR_UNABLE_TO_REMOVE_REPLACED"),
            (1176, "ERROR_UNABLE_TO_MOVE_REPLACEMENT"),
            (1177, "ERROR_UNABLE_TO_MOVE_REPLACEMENT_2"),
        ):
            with self.subTest(code=code):
                self.assertEqual(
                    CONVERT.classify_windows_replace_error(
                        code, unchanged, original, installed
                    ),
                    (name, code == 1177),
                )
                self.assertEqual(
                    CONVERT.classify_windows_replace_error(
                        code, partial, original, installed
                    ),
                    (name, True),
                )

    @unittest.skipUnless(os.name == "nt", "Windows deny-write handles")
    def test_windows_reacquires_deny_write_handle_after_each_replace(self):
        directory, paths, conversions, _originals = self.make_batch()

        def assert_locked(index):
            with self.assertRaises(OSError):
                with paths[index].open("r+b"):
                    pass

        CONVERT.write_all(conversions, after_install=assert_locked)
        directory.cleanup()

    def test_malformed_paused_batch_is_rejected_before_any_write(self):
        first_directory, first = self.copy_fixture()
        second_directory, second = self.copy_fixture()
        converted = CONVERT.prepare_conversion(second, "hold", "ms_audit", 123456)
        malformed = yaml.safe_load(converted.text)
        malformed["pause_reason"] = "different hold"
        second.write_text(yaml.safe_dump(malformed, sort_keys=False))
        before = first.read_bytes()
        result = subprocess.run(
            [sys.executable, SCRIPT, "--reason", "hold", "--apply", first, second],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("malformed or mismatched", result.stderr)
        self.assertEqual(first.read_bytes(), before)
        first_directory.cleanup()
        second_directory.cleanup()

    def leave_interrupted_transaction(self, crash_after_install=None):
        directory, paths, conversions, originals = self.make_batch()

        class SimulatedCrash(BaseException):
            pass

        def crash_after_staging():
            if crash_after_install is None:
                raise SimulatedCrash()

        def crash_after_entry(index):
            if index == crash_after_install:
                raise SimulatedCrash()

        with self.assertRaises(SimulatedCrash):
            CONVERT.write_all(
                conversions,
                after_staging=crash_after_staging,
                after_install=crash_after_entry,
            )
        journals = list(
            paths[0].parent.glob(".legacy-parked-conversion-*/journal.json")
        )
        self.assertEqual(len(journals), 1)
        return directory, paths, originals, journals[0]

    def mark_owner_stale(self, journal_path, host=None):
        journal = json.loads(journal_path.read_text())
        task_store = pathlib.Path(journal["entries"][0]["path"]).parent
        lock_path = task_store / ".legacy-parked-conversion.lock"
        record = json.loads(lock_path.read_text())
        record["pid"] = 999_999_999
        record["process_start"] = "dead:test-owner"
        if host is not None:
            record["host"] = host
        lock_path.write_text(json.dumps(record) + "\n")
        return lock_path

    def test_recovery_refuses_a_live_owner(self):
        directory, paths, originals, journal_path = self.leave_interrupted_transaction()
        with self.assertRaisesRegex(OSError, "owner is still running"):
            CONVERT.recover_transaction(journal_path)
        self.assertEqual([path.read_bytes() for path in paths], originals)
        directory.cleanup()

    def test_recovery_refuses_an_unprovable_remote_owner(self):
        directory, paths, originals, journal_path = self.leave_interrupted_transaction()
        self.mark_owner_stale(journal_path, host="different-host")
        with self.assertRaisesRegex(OSError, "another host"):
            CONVERT.recover_transaction(journal_path)
        self.assertEqual([path.read_bytes() for path in paths], originals)
        directory.cleanup()

    def test_prepared_recovery_never_resumes_forward(self):
        directory, paths, originals, journal_path = self.leave_interrupted_transaction()
        lock_path = self.mark_owner_stale(journal_path)
        self.assertEqual(CONVERT.recover_transaction(journal_path), "rolled_back")
        self.assertEqual([path.read_bytes() for path in paths], originals)
        self.assertFalse(lock_path.exists())
        journal = json.loads(journal_path.read_text())
        self.assertEqual(journal["logical_state"], "rolled_back")
        self.assertEqual(
            journal["recovery"]["classifications"],
            ["original", "original", "original"],
        )
        directory.cleanup()

    @unittest.skipIf(os.name == "nt", "CLI recovery requires an elevated volume barrier")
    def test_recovery_cli_reports_the_terminal_classification(self):
        directory, paths, originals, journal_path = self.leave_interrupted_transaction()
        lock_path = self.mark_owner_stale(journal_path)
        result = subprocess.run(
            [sys.executable, SCRIPT, "--recover", journal_path],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("conversion recovery rolled_back", result.stdout)
        self.assertEqual([path.read_bytes() for path in paths], originals)
        self.assertFalse(lock_path.exists())
        directory.cleanup()

    def test_applying_recovery_rolls_back_every_installed_prefix(self):
        for crash_position in (1, 2, 3):
            with self.subTest(crash_position=crash_position):
                directory, paths, originals, journal_path = (
                    self.leave_interrupted_transaction(crash_position - 1)
                )
                lock_path = self.mark_owner_stale(journal_path)
                self.assertEqual(
                    CONVERT.recover_transaction(journal_path), "rolled_back"
                )
                self.assertEqual([path.read_bytes() for path in paths], originals)
                self.assertFalse(lock_path.exists())
                journal = json.loads(journal_path.read_text())
                self.assertEqual(journal["logical_state"], "rolled_back")
                directory.cleanup()

    def test_recovery_restores_missing_destination_only_from_complete_layout(self):
        directory, paths, originals, journal_path = self.leave_interrupted_transaction(0)
        journal = json.loads(journal_path.read_text())
        paths[0].unlink()
        lock_path = self.mark_owner_stale(journal_path)
        self.assertEqual(CONVERT.recover_transaction(journal_path), "rolled_back")
        self.assertEqual([path.read_bytes() for path in paths], originals)
        self.assertFalse(lock_path.exists())
        self.assertEqual(
            journal["entries"][0]["original_sha256"],
            CONVERT.content_hash(originals[0]),
        )
        directory.cleanup()

    def test_recovery_refuses_missing_destination_with_partial_layout(self):
        directory, paths, _originals, journal_path = self.leave_interrupted_transaction(0)
        journal = json.loads(journal_path.read_text())
        paths[0].unlink()
        pathlib.Path(journal["entries"][0]["replace_backup"]).unlink()
        lock_path = self.mark_owner_stale(journal_path)
        with self.assertRaisesRegex(CONVERT.ConversionWriteError, "preserved conflicts"):
            CONVERT.recover_transaction(journal_path)
        self.assertFalse(paths[0].exists())
        self.assertTrue(lock_path.exists())
        recovered = json.loads(journal_path.read_text())
        self.assertEqual(recovered["logical_state"], "rollback_failed")
        self.assertEqual(recovered["durability"], "repair_required")
        directory.cleanup()

    def test_recovery_lock_barrier_failure_is_not_reported_rolled_back(self):
        directory, paths, originals, journal_path = self.leave_interrupted_transaction()
        lock_path = self.mark_owner_stale(journal_path)

        def fail_lock_removal(path):
            if path.name == ".legacy-parked-conversion.lock":
                raise CONVERT.DurabilityError("lock namespace barrier failed")

        CONVERT.sync_namespace = fail_lock_removal
        with self.assertRaises(CONVERT.ConversionWriteError):
            CONVERT.recover_transaction(journal_path)
        self.assertEqual([path.read_bytes() for path in paths], originals)
        self.assertTrue(lock_path.exists())
        journal = json.loads(journal_path.read_text())
        self.assertEqual(journal["logical_state"], "rolled_back")
        self.assertEqual(journal["durability"], "repair_required")
        directory.cleanup()

    def test_recovery_preserves_a_third_hash_and_stays_locked(self):
        directory, paths, conversions, originals = self.make_batch()
        drifted = originals[1] + b"# external conflict\n"

        def mutate_after_staging():
            paths[1].write_bytes(drifted)

        with self.assertRaises(CONVERT.ConversionWriteError) as error:
            CONVERT.write_all(
                conversions,
                after_staging=mutate_after_staging,
                source_handle_factory=self.no_source_handles,
            )
        journal_path = error.exception.journal_path
        lock_path = self.mark_owner_stale(journal_path)
        with self.assertRaisesRegex(CONVERT.ConversionWriteError, "preserved conflicts"):
            CONVERT.recover_transaction(journal_path)
        self.assertEqual(paths[1].read_bytes(), drifted)
        self.assertTrue(lock_path.exists())
        journal = json.loads(journal_path.read_text())
        self.assertEqual(journal["logical_state"], "rollback_failed")
        self.assertEqual(
            journal["entries"][1]["recovery_classification"], "conflict"
        )
        directory.cleanup()

    def test_committed_recovery_validates_without_rolling_back(self):
        directory, paths, conversions, _originals = self.make_batch()
        journal_path = CONVERT.write_all(conversions)
        installed = [path.read_bytes() for path in paths]
        journal = json.loads(journal_path.read_text())
        task_store = paths[0].parent
        lock_path = task_store / ".legacy-parked-conversion.lock"
        stale = dict(journal["owner"])
        stale["pid"] = 999_999_999
        stale["process_start"] = "dead:committed-owner"
        held = CONVERT.open_global_lock(lock_path, stale, create=True)
        CONVERT.release_global_lock(held, remove=False)
        self.assertEqual(CONVERT.recover_transaction(journal_path), "committed")
        self.assertEqual([path.read_bytes() for path in paths], installed)
        self.assertFalse(lock_path.exists())
        directory.cleanup()


if __name__ == "__main__":
    unittest.main()
