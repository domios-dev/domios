import base64
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('signing', Path(__file__).parents[1] / 'release-signing-material.py')
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)


class SigningMaterialTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.env = patch.dict(os.environ, {
            'RUNNER_TEMP': str(self.root), 'GITHUB_ENV': str(self.root / 'job.env'),
            'TAURI_SIGNING_PRIVATE_KEY': base64.b64encode(b'untrusted comment: fixture\nsecret').decode(),
            'APPLE_CERTIFICATE': base64.b64encode(b'fixture p12').decode(),
            'APPLE_CERTIFICATE_PASSWORD': 'private-password',
        })
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.temp.cleanup()

    def material_path(self):
        lines = (self.root / 'job.env').read_text().splitlines()
        return Path(next(line.split('=', 1)[1] for line in lines if line.startswith('RELEASE_SIGNING_DIR=')))

    def test_updater_is_private_and_cleanup_removes_it(self):
        signing.setup(False)
        directory = self.material_path()
        key = directory / 'updater.key'
        self.assertEqual(key.stat().st_mode & 0o777, 0o600)
        self.assertEqual(key.read_text(), os.environ['TAURI_SIGNING_PRIVATE_KEY'])
        with patch.dict(os.environ, RELEASE_SIGNING_DIR=str(directory)):
            signing.cleanup()
        self.assertFalse(directory.exists())

    def test_failed_import_exposes_cleanup_path_and_erases_certificate(self):
        with patch.object(signing, 'security', side_effect=RuntimeError('import refused')):
            with self.assertRaises(RuntimeError):
                signing.setup(True)
        directory = self.material_path()
        self.assertFalse((directory / 'certificate.p12').exists())
        with patch.dict(os.environ, RELEASE_SIGNING_DIR=str(directory)):
            signing.cleanup()
        self.assertFalse(directory.exists())

    def test_import_targets_only_job_keychain_and_preserves_search_entries(self):
        commands = []
        with patch.object(signing, 'security', side_effect=lambda *args: commands.append(args)), \
                patch.object(signing, 'search_list', return_value=['existing.keychain', 'parallel.keychain']), \
                patch.object(signing, 'notary_profile') as notary:
            signing.setup(True)
        directory = self.material_path()
        keychain = str(directory / 'signing.keychain-db')
        imported = next(args for args in commands if args[0] == 'import')
        self.assertEqual(imported[imported.index('-k') + 1], keychain)
        self.assertIn('private-password', imported)
        self.assertIn(('list-keychains', '-d', 'user', '-s', keychain, 'existing.keychain', 'parallel.keychain'), commands)
        self.assertTrue(any(args[0] == 'set-key-partition-list' for args in commands))
        notary.assert_called_once_with(Path(keychain))
        self.assertFalse((directory / 'certificate.p12').exists())

    def test_tauri_signs_from_temp_key_and_verifies_artifact_bytes(self):
        repo = Path(__file__).resolve().parents[2]
        cli = repo / 'node_modules/.bin/tauri'
        key = self.root / 'fixture.key'
        result = subprocess.run([str(cli), 'signer', 'generate', '--ci', '-w', str(key)],
                                capture_output=True)
        self.assertEqual(result.returncode, 0)
        key.chmod(0o600)
        artifact = self.root / 'artifact.bin'
        artifact.write_bytes(b'artifact fixture')
        env = dict(os.environ)
        env.pop('TAURI_SIGNING_PRIVATE_KEY', None)
        env.update(TAURI_SIGNING_PRIVATE_KEY_PATH=str(key), TAURI_SIGNING_PRIVATE_KEY_PASSWORD='')
        signed = subprocess.run([str(cli), 'signer', 'sign', str(artifact)], env=env, capture_output=True)
        self.assertEqual(signed.returncode, 0)
        manifest = self.root / 'manifest.json'
        manifest.write_text(json.dumps({'version': '0.0.1', 'platforms': {'fixture': {
            'url': 'https://example.invalid/artifact.bin',
            'signature': (self.root / 'artifact.bin.sig').read_text(),
        }}}))
        command = ['node', str(repo / 'scripts/verify-updater-manifest.mjs'), '--manifest',
                   str(manifest), '--pubkey', '@' + str(key) + '.pub', '--local', str(self.root)]
        self.assertEqual(subprocess.run(command, capture_output=True).returncode, 0)
        artifact.write_bytes(b'tampered artifact')
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)

    def test_security_password_is_only_in_stdin(self):
        password = 'private"\\password'
        with patch.object(signing.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, '', '')) as run:
            signing.security('unlock-keychain', '-p', password, 'job.keychain')
        self.assertEqual(run.call_args.args[0], ['security', '-i'])
        self.assertIn('private\\"\\\\password', run.call_args.kwargs['input'])
        self.assertNotIn(password, str(run.call_args.args[0]))

    def test_cleanup_keeps_other_jobs_and_existing_keychains(self):
        directory = self.root / 'release-signing-fixture'
        directory.mkdir()
        keychain = directory / 'signing.keychain-db'
        keychain.touch()
        commands = []
        def security(*args):
            commands.append(args)
            if args[0] == 'delete-keychain':
                keychain.unlink()
        with patch.dict(os.environ, RELEASE_SIGNING_DIR=str(directory)), \
                patch.object(signing, 'security', side_effect=security), \
                patch.object(signing, 'search_list', return_value=['existing.keychain', str(keychain), 'parallel.keychain']):
            signing.cleanup()
        self.assertEqual(commands[0], ('list-keychains', '-d', 'user', '-s', 'existing.keychain', 'parallel.keychain'))
        self.assertFalse(directory.exists())

    @unittest.skipUnless(sys.platform == 'darwin', 'macOS keychain oracle')
    def test_native_keychain_and_notary_prompt_storage_without_network(self):
        directory = self.root / 'release-signing-native'
        directory.mkdir()
        keychain = directory / 'signing.keychain-db'
        password = 'fixture-password'
        real_exec = os.execvp
        def no_validation(file, args):
            real_exec(file, args + ['--no-validate'])
        try:
            signing.security('create-keychain', '-p', password, str(keychain))
            signing.security('unlock-keychain', '-p', password, str(keychain))
            with patch.dict(os.environ, APPLE_ID='fixture@example.invalid',
                            APPLE_PASSWORD='fixture-password', APPLE_TEAM_ID='FIXTURETEAM'), \
                    patch.object(signing.os, 'execvp', side_effect=no_validation):
                signing.notary_profile(keychain)
            # The profile persists in the job keychain; the password is never printed.
            self.assertTrue(keychain.exists())
            stored = subprocess.run(['security', 'dump-keychain', str(keychain)], capture_output=True)
            self.assertEqual(stored.returncode, 0)
            self.assertIn(b'release-job', stored.stdout)
        finally:
            with patch.dict(os.environ, RELEASE_SIGNING_DIR=str(directory)):
                signing.cleanup()
        self.assertFalse(keychain.exists())

    def test_invalid_updater_key_is_rejected(self):
        with patch.dict(os.environ, TAURI_SIGNING_PRIVATE_KEY=base64.b64encode(b'wrong key').decode()):
            with self.assertRaises(ValueError):
                signing.setup(False)
        with patch.dict(os.environ, RELEASE_SIGNING_DIR=str(self.material_path())):
            signing.cleanup()

    def test_notary_prompts_receive_env_credentials_without_argv(self):
        tool = self.root / 'xcrun'
        tool.write_text('''#!/usr/bin/env python3
import os, sys
expected = [('', 'Path to App Store Connect API private key:'),
            (os.environ['APPLE_ID'], 'Developer Apple ID:'),
            (os.environ['APPLE_PASSWORD'], 'App-specific password for fixture: '),
            (os.environ['APPLE_TEAM_ID'], 'Developer Team ID:')]
assert not any(os.environ[name] in sys.argv for name in ['APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID'])
for value, prompt in expected:
    print(prompt, flush=True)
    assert input() == value
''')
        tool.chmod(0o700)
        with patch.dict(os.environ, PATH=str(self.root) + os.pathsep + os.environ['PATH'],
                        APPLE_ID='fixture-id', APPLE_PASSWORD='fixture-password', APPLE_TEAM_ID='fixture-team'):
            signing.notary_profile(self.root / 'job.keychain')


if __name__ == '__main__':
    unittest.main()
