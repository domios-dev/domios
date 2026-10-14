#!/usr/bin/env python3
import base64
import os
from pathlib import Path
import pty
import re
import secrets
import select
import signal
import subprocess
import sys
import time


def required(name):
    value = os.environ.get(name)
    if not value:
        raise ValueError(f"Missing {name}")
    return value


def security(*args):
    # Interactive security commands keep passwords out of process arguments.
    def quote(value):
        if '\n' in value or '\r' in value:
            raise ValueError('Multiline security argument')
        return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'
    result = subprocess.run(['security', '-i'], input=' '.join(map(quote, args)) + '\n',
                            text=True, capture_output=True, timeout=30)
    if result.returncode or re.search(r'security:|SecKeychain.*:', result.stderr):
        raise RuntimeError(f"security {args[0]} failed")


def search_list():
    result = subprocess.run(['security', 'list-keychains', '-d', 'user'],
                            check=True, text=True, capture_output=True)
    return re.findall(r'"([^"\n]+)"', result.stdout)


def notary_profile(keychain):
    responses = [
        (b'Path to App Store Connect API private key:', ''),
        (b'Developer Apple ID:', required('APPLE_ID')),
        (b'App-specific password for', required('APPLE_PASSWORD')),
        (b'Developer Team ID:', required('APPLE_TEAM_ID')),
    ]
    pid, descriptor = pty.fork()
    if pid == 0:
        os.execvp('xcrun', ['xcrun', 'notarytool', 'store-credentials', 'release-job',
                           '--keychain', str(keychain)])
    deadline = time.monotonic() + 120
    pending = b''
    try:
        for prompt, value in responses:
            while prompt not in pending:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([descriptor], [], [], remaining)[0]:
                    raise RuntimeError('Notary credential prompt timed out')
                pending += os.read(descriptor, 4096)
            pending = b''
            os.write(descriptor, (value + '\n').encode())
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([descriptor], [], [], remaining)[0]:
                raise RuntimeError('Notary credential validation timed out')
            try:
                if not os.read(descriptor, 4096):
                    break
            except OSError:
                break
        _, status = os.waitpid(pid, 0)
        pid = None
        if os.waitstatus_to_exitcode(status):
            raise RuntimeError('Apple notary rejected credentials; check password and Developer agreement')
    finally:
        if pid is not None:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        os.close(descriptor)


def updater(directory):
    key = required('TAURI_SIGNING_PRIVATE_KEY')
    if not base64.b64decode(key).startswith(b'untrusted comment:'):
        raise ValueError('Updater secret is not a base64 minisign key')
    path = directory / 'updater.key'
    path.write_text(key)
    path.chmod(0o600)
    return path


def setup(mac):
    directory = Path(required('RUNNER_TEMP')) / ('release-signing-' + secrets.token_hex(16))
    directory.mkdir(mode=0o700)
    # Publish the cleanup path before provisioning anything that can fail.
    with open(required('GITHUB_ENV'), 'a') as output:
        output.write(f'RELEASE_SIGNING_DIR={directory}\n')
        output.write(f'TAURI_SIGNING_PRIVATE_KEY_PATH={directory / "updater.key"}\n')
        if mac:
            output.write(f'RELEASE_KEYCHAIN={directory / "signing.keychain-db"}\n')
            output.write('APPLE_KEYCHAIN_PROFILE=release-job\n')
    updater(directory)
    if not mac:
        return
    keychain = directory / 'signing.keychain-db'
    password = secrets.token_hex(32)
    certificate = directory / 'certificate.p12'
    certificate.write_bytes(base64.b64decode(required('APPLE_CERTIFICATE'), validate=True))
    certificate.chmod(0o600)
    try:
        security('create-keychain', '-p', password, str(keychain))
        security('set-keychain-settings', '-lut', '21600', str(keychain))
        security('unlock-keychain', '-p', password, str(keychain))
        security('import', str(certificate), '-k', str(keychain), '-P',
                 required('APPLE_CERTIFICATE_PASSWORD'), '-T', '/usr/bin/codesign', '-T', '/usr/bin/security')
        security('set-key-partition-list', '-S', 'apple-tool:,apple:,codesign:', '-s', '-k', password, str(keychain))
        security('list-keychains', '-d', 'user', '-s', str(keychain),
                 *[path for path in search_list() if path != str(keychain)])
        notary_profile(keychain)
    finally:
        certificate.unlink(missing_ok=True)


def cleanup():
    directory = os.environ.get('RELEASE_SIGNING_DIR')
    if not directory:
        return
    path = Path(directory)
    if path.parent != Path(required('RUNNER_TEMP')) or not path.name.startswith('release-signing-'):
        raise ValueError('Invalid cleanup directory')
    keychain = path / 'signing.keychain-db'
    try:
        if keychain.exists():
            try:
                security('list-keychains', '-d', 'user', '-s',
                         *[item for item in search_list() if item != str(keychain)])
            finally:
                security('delete-keychain', str(keychain))
    finally:
        for name in ['updater.key', 'certificate.p12', 'notary-preflight.log']:
            (path / name).unlink(missing_ok=True)
    path.rmdir()


if __name__ == '__main__':
    os.umask(0o077)
    try:
        if sys.argv[1] == 'cleanup':
            cleanup()
        else:
            setup(sys.argv[1] == 'macos')
    except Exception as error:
        print(f'::error::{type(error).__name__}: release signing material operation failed', file=sys.stderr)
        sys.exit(1)
