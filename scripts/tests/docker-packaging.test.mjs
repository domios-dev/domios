import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';
import { selectProductionArtifact } from '../docker/package-ci.mjs';

function rows(overrides = {}) {
  return [{ reason: 'compiler-artifact', target: { name: 'domios-server', kind: ['bin'], src_path: path.resolve('server.rs') },
    executable: '/release/domios-server', profile: { test: false, debug_assertions: false, opt_level: '3' }, ...overrides },
  { reason: 'build-finished', success: true }];
}

function python(body, module = 'verify') {
  const executable = process.platform === 'win32' ? 'python' : 'python3';
  const prefix = `import importlib.util, pathlib, sys, tempfile\nsys.dont_write_bytecode = True\nspec = importlib.util.spec_from_file_location('subject', 'packaging/docker/${module}.py')\nsubject = importlib.util.module_from_spec(spec)\nspec.loader.exec_module(subject)\n`;
  execFileSync(executable, ['-'], { input: prefix + body, stdio: ['pipe', 'pipe', 'pipe'] });
}

test('CI packager accepts exactly the successful production artifact', () => {
  assert.equal(selectProductionArtifact(rows(), 'domios-server', 'server.rs').executable, '/release/domios-server');
});

test('CI packager refuses development, test, foreign, ambiguous and failed artifacts', () => {
  for (const profile of [{ test: false, debug_assertions: true, opt_level: '3' }, { test: false, debug_assertions: false, opt_level: '0' }, { test: true }, {}]) {
    assert.throws(() => selectProductionArtifact(rows({ profile }), 'domios-server', 'server.rs'));
  }
  assert.throws(() => selectProductionArtifact(rows(), 'domios-server', 'foreign.rs'));
  const duplicate = rows();
  duplicate.push(duplicate[0]);
  assert.throws(() => selectProductionArtifact(duplicate, 'domios-server', 'server.rs'));
  assert.throws(() => selectProductionArtifact(rows().slice(0, 1), 'domios-server', 'server.rs'));
});

test('unsigned archive requires both explicit smoke flags; production never falls back to its checksum', () => {
  python(`
with tempfile.TemporaryDirectory() as temporary:
    root = pathlib.Path(temporary)
    name = 'domios-headless-v1.2.3-linux-x86_64.tar.gz'
    digest = 'a' * 64
    (root / (name + '.sha256')).write_text(digest + '  ' + name + '\\n')
    assert subject.trusted_digest(root, name, '1.2.3', 'x86_64', 'ci-smoke', '1') == (digest, None)
    for mode, flag in [('signed-release', '0'), ('signed-release', '1'), ('ci-smoke', '0'), ('unknown', '1')]:
        try:
            subject.trusted_digest(root, name, '1.2.3', 'x86_64', mode, flag)
        except (ValueError, FileNotFoundError):
            pass
        else:
            raise AssertionError('Unsigned path was admitted implicitly')
`);
});

test('archive extraction rejects a checksum mismatch before writing payloads', () => {
  python(`
with tempfile.TemporaryDirectory() as temporary:
    root = pathlib.Path(temporary)
    name = 'domios-headless-v1.2.3-linux-x86_64.tar.gz'
    (root / name).write_bytes(b'corrupt')
    (root / (name + '.sha256')).write_text('a' * 64 + '  ' + name)
    try:
        subject.extract(root, root / 'output', '1.2.3', 'amd64', 'ci-smoke', '1')
    except ValueError as error:
        assert 'SHA-256 mismatch' in str(error)
    else:
        raise AssertionError('Corrupt archive accepted')
    assert not (root / 'output').exists()
`);
});

test('archive extraction refuses traversal, symlinks and special modes even with a matching checksum', () => {
  python(`
import hashlib, io, tarfile
for name_in_tar, kind, mode in [('../escape', tarfile.REGTYPE, 0o644), ('link', tarfile.SYMTYPE, 0o644), ('special', tarfile.REGTYPE, 0o4755)]:
    with tempfile.TemporaryDirectory() as temporary:
        root = pathlib.Path(temporary)
        name = 'domios-headless-v1.2.3-linux-x86_64.tar.gz'
        archive = root / name
        with tarfile.open(archive, 'w:gz') as bundle:
            entry = tarfile.TarInfo(name_in_tar)
            entry.type, entry.mode, entry.size = kind, mode, 0
            entry.linkname = 'outside'
            bundle.addfile(entry, io.BytesIO(b''))
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        (root / (name + '.sha256')).write_text(digest + '  ' + name)
        try:
            subject.extract(root, root / 'output', '1.2.3', 'amd64', 'ci-smoke', '1')
        except ValueError:
            pass
        else:
            raise AssertionError('Unsafe archive accepted')
        assert not (root / 'escape').exists()
`);
});

test('volume policy rejects wrong owner, mode, links and type', () => {
  python(`
import stat, types
subject.os.getuid = lambda: 10001
class Entry:
    def __init__(self, owner=10001, mode=stat.S_IFREG | 0o600, links=1):
        self.info = types.SimpleNamespace(st_uid=owner, st_mode=mode, st_nlink=links)
    def lstat(self):
        return self.info
subject.private(Entry(), 0o600)
subject.private(Entry(mode=stat.S_IFDIR | 0o700), 0o700, directory=True)
for entry in [Entry(owner=0), Entry(mode=stat.S_IFREG | 0o644), Entry(links=2), Entry(mode=stat.S_IFLNK | 0o600), Entry(mode=stat.S_IFDIR | 0o600)]:
    try:
        subject.private(entry, 0o600)
    except RuntimeError:
        pass
    else:
        raise AssertionError('Unsafe volume entry accepted')
`, 'container');
});

test('offline backend selection adds an omitted setting without changing nested embedder config', () => {
  python(`
subject.prepare = lambda: None
with tempfile.TemporaryDirectory() as temporary:
    subject.DATA = pathlib.Path(temporary)
    config = subject.DATA / 'config.toml'
    config.write_text('[memory.embedder]\\nprovider = "local"\\n')
    assert subject.command(['select-memory', '--to', 'postgres']) == 0
    assert subject.tomllib.loads(config.read_text()) == {'memory': {'backend': 'postgres', 'embedder': {'provider': 'local'}}}
    assert list(subject.DATA.iterdir()) == [config]
`, 'container');
});

test('offline selection refuses ambiguous TOML without rewriting the original file', () => {
  python(`
subject.prepare = lambda: None
with tempfile.TemporaryDirectory() as temporary:
    subject.DATA = pathlib.Path(temporary)
    config = subject.DATA / 'config.toml'
    original = 'memory = { backend = "sqlite" }\\n'
    config.write_text(original)
    try:
        subject.command(['select-memory', '--to', 'postgres'])
    except subject.tomllib.TOMLDecodeError:
        pass
    else:
        raise AssertionError('Ambiguous configuration was rewritten')
    assert config.read_text() == original
`, 'container');
});

test('signed production feed verifies and tampered signature refuses', {
  skip: spawnSync('minisign', ['-v'], { encoding: 'utf8' }).error ? 'minisign is installed in Linux smoke CI; unavailable on this host' : false,
}, () => {
  python(`
import base64, shutil, subprocess
fixtures = pathlib.Path('src-tauri/crates/codeterm-update/src/headless/fixtures')
with tempfile.TemporaryDirectory() as temporary:
    root = pathlib.Path(temporary)
    key = root / 'test.pub'
    key.write_bytes(base64.b64decode((fixtures / 'test-public-key.txt').read_bytes()))
    subject.PUBLIC_KEY = key
    feed = root / 'domios-headless-linux-x86_64.json'
    shutil.copyfile(fixtures / 'valid.json', feed)
    digest, size = subject.trusted_digest(root, 'domios-headless-v9.0.0-linux-x86_64.tar.gz', '9.0.0', 'x86_64', 'signed-release', '0')
    assert len(digest) == 64 and size > 0
    shutil.copyfile(fixtures / 'tampered.json', feed)
    try:
        subject.trusted_digest(root, 'domios-headless-v9.0.0-linux-x86_64.tar.gz', '9.0.0', 'x86_64', 'signed-release', '0')
    except subprocess.CalledProcessError:
        pass
    else:
        raise AssertionError('Tampered production signature accepted')
`);
});
