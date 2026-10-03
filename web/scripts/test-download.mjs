import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, mkdir, writeFile, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
const temporary = await realpath(await mkdtemp(join(tmpdir(), 'domios-download-')));
const realFetch = globalThis.fetch;
const realTimeout = AbortSignal.timeout;
const deadline = AbortSignal.timeout(30_000);
deadline.addEventListener('abort', () => { console.error('Download fixture deadline exceeded'); process.exit(1); });
let assertions = 0;

const fixtures = [
  ['mac-arm64-dmg', 'mac', 'arm64', 'dmg', 'CodeTerm_1.2.3_aarch64.dmg'],
  ['mac-x64-dmg', 'mac', 'x64', 'dmg', 'CodeTerm_1.2.3_x64.dmg'],
  ['windows-x64-exe', 'windows', 'x64', 'exe', 'CodeTerm_1.2.3_x64-setup.exe'],
  ['windows-arm64-exe', 'windows', 'arm64', 'exe', 'CodeTerm_1.2.3_arm64-setup.exe'],
  ['linux-x64-deb', 'linux', 'x64', 'deb', 'CodeTerm_1.2.3_amd64.deb'],
  ['linux-arm64-deb', 'linux', 'arm64', 'deb', 'CodeTerm_1.2.3_arm64.deb'],
  ['linux-x64-appimage', 'linux', 'x64', 'appimage', 'CodeTerm_1.2.3_amd64.AppImage'],
  ['linux-arm64-appimage', 'linux', 'arm64', 'appimage', 'CodeTerm_1.2.3_aarch64.AppImage'],
];
const assetPrefix = 'https://github.com/rollacode/codeterm-releases/releases/download/v1.2.3/';
const windowsHref = 'https://github.com/rollacode/codeterm-releases/releases/download/v1.2.3/CodeTerm_1.2.3_x64-setup.exe';
const releasesPage = 'https://github.com/rollacode/codeterm-releases/releases/latest';
const release = { tag_name: 'v1.2.3', assets: fixtures.map(([, , , , name]) => ({ name, browser_download_url: assetPrefix + name })) };
const info = { version: 'v1.2.3', downloads: fixtures.map(([id, , , , asset]) => ({ id, href: assetPrefix + asset, available: true, asset })) };
const releaseCache = 'public, max-age=0, s-maxage=300, stale-while-revalidate=600';
const fallbackCache = 'public, max-age=0, s-maxage=60, stale-while-revalidate=300';
const json = value => new Response(JSON.stringify(value), { headers: { 'Content-Type': 'application/json' } });
const request = (path, method = 'GET') => new Request('https://domios.test' + path, { method });
const deferred = () => {
  let resolvePromise, reject;
  const promise = new Promise((resolve, fail) => { resolvePromise = resolve; reject = fail; });
  return { promise, resolve: resolvePromise, reject };
};
async function test(name, action) {
  await action();
  assertions++;
  console.log(`ok ${assertions} - ${name}`);
}

async function fixture(label, baseline = null) {
  const folder = join(temporary, label);
  await mkdir(folder, { recursive: true });
  await writeFile(join(folder, 'package.json'), JSON.stringify({ type: 'commonjs' }));
  const files = ['src/download/catalog.ts', 'src/download/detect.ts', 'src/download/release.ts', 'api/download.ts', 'api/download/info.ts'];
  if (!baseline) files.push('src/download/flow.ts');
  for (const file of files) {
    const source = baseline ? execFileSync('git', ['show', `${baseline}:web/${file}`], { cwd: root, encoding: 'utf8' })
      : await readFile(join(root, file), 'utf8');
    const output = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } });
    const target = join(folder, file.replace(/\.ts$/, '.js'));
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, output.outputText);
  }
  return () => {
    for (const path of Object.keys(require.cache)) if (path.startsWith(folder + sep)) delete require.cache[path];
    return {
      catalog: require(join(folder, 'src/download/catalog.js')),
      detect: require(join(folder, 'src/download/detect.js')).detectInstaller,
      release: require(join(folder, 'src/download/release.js')),
      download: require(join(folder, 'api/download.js')).default,
      metadata: require(join(folder, 'api/download/info.js')).default,
      flow: baseline ? null : require(join(folder, 'src/download/flow.js')),
    };
  };
}

async function serverContracts(label, fresh) {
  const { detect, catalog, release: resolver } = fresh();
  const detections = [
    ['Windows NT 10.0; Win64; x64', 'Win32', '', false, 'windows-x64-exe'],
    ['Windows NT 10.0', 'Windows', 'arm', false, 'windows-arm64-exe'],
    ['Windows NT 10.0; ARM64', 'Win32', '', false, 'windows-arm64-exe'],
    ['Macintosh; Intel Mac OS X', 'macOS', 'arm', false, 'mac-arm64-dmg'],
    ['Macintosh; Intel Mac OS X', 'macOS', 'x86', false, 'mac-x64-dmg'],
    ['Macintosh; Intel Mac OS X', 'MacIntel', '', false, 'mac-arm64-dmg'],
    ['X11; Linux x86_64', 'Linux', '', false, 'linux-x64-deb'],
    ['X11; Linux aarch64', 'Linux', '', false, 'linux-arm64-deb'],
    ['iPhone; CPU iPhone OS', '', '', false, null],
    ['Android; Linux arm', 'Linux', '', false, null],
    ['Macintosh', 'MacIntel', '', true, null],
    ['Unknown desktop', 'Unknown', '', false, null],
  ];
  for (const [ua, platform, arch, mobile, id] of detections) await test(`${label} detect ${id ?? ua}`, () => {
    assert.equal(detect(ua, platform, arch, mobile)?.id ?? null, id);
  });
  await test(`${label} all eight release assets and explicit alternatives`, () => {
    assert.deepEqual(resolver.releaseInfo(release), info);
    assert.equal(catalog.installers.length, 8);
    fixtures.forEach(([id, os, arch, format]) => {
      const selection = catalog.installers.find(entry => entry.id === id);
      assert.equal(catalog.downloadHref(selection), `/api/download?os=${os}&arch=${arch}&format=${format}`);
    });
    assert.equal(resolver.releaseInfo(null).downloads.every(entry => !entry.available && entry.href === releasesPage), true);
  });
  await test(`${label} info GET/HEAD share headers and empty HEAD; POST refuses`, async () => {
    const runtime = fresh();
    let calls = 0;
    globalThis.fetch = async (url, options) => {
      calls++;
      assert.equal(String(url), 'https://api.github.com/repos/rollacode/codeterm-releases/releases/latest');
      assert.equal(options.headers.Accept, 'application/vnd.github+json');
      assert.ok(options.signal instanceof AbortSignal);
      return json(release);
    };
    const get = await runtime.metadata(request('/api/download/info'));
    const head = await runtime.metadata(request('/api/download/info', 'HEAD'));
    const post = await runtime.metadata(request('/api/download/info', 'POST'));
    assert.equal(get.status, 200);
    assert.deepEqual(await get.json(), info);
    assert.deepEqual([...get.headers], [...head.headers]);
    assert.equal(head.headers.get('Cache-Control'), releaseCache);
    assert.equal(head.headers.get('Content-Type'), 'application/json; charset=utf-8');
    assert.equal(await head.text(), '');
    assert.equal(post.status, 405);
    assert.equal(post.headers.get('Allow'), 'GET, HEAD');
    assert.equal(calls, 1);
  });
  for (const [id, os, arch, format, asset] of fixtures) for (const method of ['GET', 'HEAD']) {
    await test(`${label} ${method} resolver ${id}`, async () => {
      const runtime = fresh();
      let calls = 0;
      globalThis.fetch = async (url, options) => {
        calls++;
        assert.equal(String(url), 'https://domios.test/api/download/info');
        assert.ok(options.signal instanceof AbortSignal);
        return json(info);
      };
      const result = await runtime.download(request(`/api/download?os=${os}&arch=${arch}&format=${format}`, method));
      assert.equal(result.status, 302);
      assert.equal(result.headers.get('Location'), assetPrefix + asset);
      assert.equal(result.headers.get('Cache-Control'), releaseCache);
      assert.equal(await result.text(), '');
      assert.equal(calls, 1);
    });
  }
  const failures = [
    ['unavailable', () => json({ ...info, downloads: [] })],
    ['unsafe URL', () => json({ ...info, downloads: [{ ...info.downloads[2], href: 'https://evil.test/setup.exe' }] })],
    ['wrong shape', () => json({ downloads: {} })],
    ['invalid JSON', () => new Response('{')],
    ['503', () => new Response(null, { status: 503 })],
    ['network rejection', () => { throw new TypeError('offline'); }],
    ['aborted fetch', () => { throw new DOMException('Aborted', 'AbortError'); }],
  ];
  for (const [name, response] of failures) for (const method of ['GET', 'HEAD']) await test(`${label} ${method} resolver fallback ${name}`, async () => {
    const runtime = fresh();
    globalThis.fetch = async () => response();
    const result = await runtime.download(request('/api/download?os=windows&arch=x64&format=exe', method));
    assert.equal(result.status, 302);
    assert.equal(result.headers.get('Location'), releasesPage);
    assert.equal(result.headers.get('Cache-Control'), fallbackCache);
  });
  for (const method of ['GET', 'HEAD', 'POST']) await test(`${label} invalid selection/method ${method}`, async () => {
    const runtime = fresh();
    let calls = 0;
    globalThis.fetch = () => { calls++; throw new Error('Invalid selection must not fetch'); };
    const result = await runtime.download(request('/api/download?os=unknown&arch=x64&format=exe', method));
    assert.equal(result.status, method === 'POST' ? 405 : 302);
    if (method === 'POST') assert.equal(result.headers.get('Allow'), 'GET, HEAD');
    else {
      assert.equal(result.headers.get('Location'), releasesPage);
      assert.equal(result.headers.get('Cache-Control'), 'no-store');
    }
    assert.equal(calls, 0);
  });
  await test(`${label} POST valid selection refuses before fetch`, async () => {
    const runtime = fresh();
    let calls = 0;
    globalThis.fetch = () => { calls++; throw new Error('POST must not fetch'); };
    const result = await runtime.download(request('/api/download?os=windows&arch=x64&format=exe', 'POST'));
    assert.equal(result.status, 405);
    assert.equal(result.headers.get('Allow'), 'GET, HEAD');
    assert.equal(calls, 0);
  });
  for (const [name, fetcher] of [
    ['malformed JSON', () => new Response('{')],
    ['503', () => new Response(null, { status: 503 })],
    ['rejection', () => { throw new TypeError('offline'); }],
    ['aborted deadline', (url, options) => { options.signal.throwIfAborted(); throw new Error('Expected aborted signal'); }],
  ]) await test(`${label} info fallback ${name}`, async () => {
    const runtime = fresh();
    let deadlineObserved = false;
    globalThis.fetch = async (...args) => {
      if (name === 'aborted deadline') deadlineObserved = args[1]?.signal?.aborted === true;
      return fetcher(...args);
    };
    if (name === 'aborted deadline') AbortSignal.timeout = () => AbortSignal.abort(new DOMException('Expired', 'TimeoutError'));
    try {
      const result = await runtime.metadata(request('/api/download/info'));
      assert.equal(result.status, 200);
      assert.deepEqual(await result.json(), { version: null, downloads: fixtures.map(([id]) => ({ id, href: releasesPage, available: false, asset: null })) });
      assert.equal(result.headers.get('Cache-Control'), fallbackCache);
      if (name === 'aborted deadline') assert.equal(deadlineObserved, true);
    } finally { AbortSignal.timeout = realTimeout; }
  });
}

async function clientContracts(fresh) {
  const { flow, catalog } = fresh();
  const { parseDownloadInfo, selectedAsset, lookupDownloadInfo, createDownloadFlow, ordinaryDownloadClick } = flow;
  const windows = catalog.installers.find(entry => entry.id === 'windows-x64-exe');
  const mac = catalog.installers.find(entry => entry.id === 'mac-arm64-dmg');
  await test('client selected asset retains exact Windows identity', () => {
    assert.deepEqual(selectedAsset(parseDownloadInfo(info), windows), {
      id: 'windows-x64-exe', href: windowsHref, available: true, asset: 'CodeTerm_1.2.3_x64-setup.exe',
    });
    catalog.installers.forEach(installer => assert.equal(selectedAsset(parseDownloadInfo(info), installer).id, installer.id));
  });
  const unsafe = ['javascript:alert(1)', 'http://github.com/rollacode/codeterm-releases/releases/download/v1/a.exe',
    'https://github.com.evil.test/rollacode/codeterm-releases/releases/download/v1/a.exe',
    'https://user:pass@github.com/rollacode/codeterm-releases/releases/download/v1/a.exe',
    `${windowsHref}?redirect=evil`, `${windowsHref}#hash`, `${assetPrefix}nested/file.exe`];
  for (const href of unsafe) await test(`client refuses unsafe URL ${href}`, () => {
    assert.equal(flow.isDirectAssetUrl(href), false);
    assert.throws(() => parseDownloadInfo({ ...info, downloads: [{ ...info.downloads[2], href }] }));
  });
  for (const payload of [null, {}, { version: 3, downloads: [] }, { version: null, downloads: {} },
    { ...info, downloads: [info.downloads[2], info.downloads[2]] },
    { ...info, downloads: [{ ...info.downloads[2], available: 'true' }] },
    { ...info, downloads: [{ ...info.downloads[2], asset: 'Other.exe' }] }]) {
    await test('client refuses malformed metadata', () => assert.throws(() => parseDownloadInfo(payload)));
  }
  await test('client missing/unavailable/wrong architecture cannot start', () => {
    assert.throws(() => selectedAsset(parseDownloadInfo({ ...info, downloads: [] }), windows));
    assert.throws(() => selectedAsset(parseDownloadInfo({ ...info, downloads: [{ ...info.downloads[2], available: false }] }), windows));
    assert.throws(() => selectedAsset(parseDownloadInfo({ ...info, downloads: [{ ...info.downloads[0], id: windows.id }] }), windows));
  });
  await test('bounded lookup requests current metadata', async () => {
    let calls = 0;
    const result = await lookupDownloadInfo(new AbortController().signal, async (url, options) => {
      calls++;
      assert.equal(url, '/api/download/info');
      assert.equal(options.cache, 'no-cache');
      assert.ok(options.signal instanceof AbortSignal);
      return json(info);
    });
    assert.deepEqual(result, info);
    assert.equal(calls, 1);
  });
  for (const [name, response] of [
    ['invalid JSON', () => new Response('{')], ['shape', () => json({})],
    ['503', () => new Response(null, { status: 503 })], ['rejection', () => { throw new TypeError('offline'); }],
  ]) await test(`lookup rejects ${name}`, async () => {
    await assert.rejects(lookupDownloadInfo(new AbortController().signal, async () => response()));
  });
  await test('deadline rejects an uncooperative fetch without a sleep', async () => {
    const failureDeadline = new AbortController();
    const called = deferred();
    const result = lookupDownloadInfo(new AbortController().signal, () => { called.resolve(); return new Promise(() => {}); }, failureDeadline.signal);
    await called.promise;
    failureDeadline.abort(new DOMException('Expired', 'TimeoutError'));
    await assert.rejects(result, /timed out/);
  });
  await test('deadline bounds a stalled response body', async () => {
    const failureDeadline = new AbortController();
    const reading = deferred();
    const result = lookupDownloadInfo(new AbortController().signal, async () => ({ ok: true, json: () => { reading.resolve(); return new Promise(() => {}); } }), failureDeadline.signal);
    await reading.promise;
    failureDeadline.abort(new DOMException('Expired', 'TimeoutError'));
    await assert.rejects(result, /timed out/);
  });
  await test('cancelled lookup rejects without a request', async () => {
    await assert.rejects(lookupDownloadInfo(AbortSignal.abort(), () => { assert.fail('Cancelled lookup fetched'); }));
  });
  await test('modified and middle clicks preserve anchor navigation', () => {
    const plain = { button: 0, defaultPrevented: false, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false };
    assert.equal(ordinaryDownloadClick(plain), true);
    for (const key of ['ctrlKey', 'metaKey', 'shiftKey', 'altKey', 'defaultPrevented']) assert.equal(ordinaryDownloadClick({ ...plain, [key]: true }), false);
    assert.equal(ordinaryDownloadClick({ ...plain, button: 1 }), false);
  });
  await test('initiation precedes starting; duplicate pending/starting clicks ignored', async () => {
    const lookup = deferred();
    const events = [];
    let calls = 0;
    const session = createDownloadFlow({ lookup: () => { calls++; return lookup.promise; },
      initiate: asset => { assert.equal(asset.href, windowsHref); events.push('request'); }, change: state => events.push(state.status) });
    const first = session.start(windows);
    await session.start(windows);
    assert.deepEqual(events, ['pending']);
    lookup.resolve(info);
    await first;
    await session.start(windows);
    assert.deepEqual(events, ['pending', 'request', 'starting']);
    assert.equal(calls, 1);
  });
  await test('rejected metadata can be retried and succeed', async () => {
    const events = [];
    let calls = 0;
    const session = createDownloadFlow({ lookup: async () => { if (++calls === 1) throw new TypeError('offline'); return info; },
      initiate: () => events.push('request'), change: state => events.push(state.status) });
    await session.start(windows);
    await session.start(windows);
    assert.deepEqual(events, ['pending', 'error', 'pending', 'request', 'starting']);
    assert.equal(calls, 2);
  });
  await test('older completion cannot initiate or replace current selection', async () => {
    const older = deferred(), newer = deferred();
    const states = [], assets = [], signals = [];
    const session = createDownloadFlow({ lookup: signal => { signals.push(signal); return signals.length === 1 ? older.promise : newer.promise; },
      initiate: asset => assets.push(asset.id), change: state => states.push([state.status, state.installer?.id]) });
    const first = session.start(windows), second = session.start(mac);
    assert.equal(signals[0].aborted, true);
    newer.resolve(info);
    await second;
    older.resolve(info);
    await first;
    assert.deepEqual(assets, ['mac-arm64-dmg']);
    assert.deepEqual(states, [['pending', 'windows-x64-exe'], ['pending', 'mac-arm64-dmg'], ['starting', 'mac-arm64-dmg']]);
  });
  await test('dismiss/unmount invalidates pending work; another session works', async () => {
    const pending = deferred();
    const events = [];
    let signal;
    const session = createDownloadFlow({ lookup: input => { signal = input; return pending.promise; },
      initiate: () => events.push('request'), change: state => events.push(state.status) });
    const first = session.start(windows);
    session.close();
    assert.equal(signal.aborted, true);
    pending.resolve(info);
    await first;
    assert.deepEqual(events, ['pending', 'idle']);
    await session.start(windows);
    assert.deepEqual(events, ['pending', 'idle', 'pending', 'request', 'starting']);
  });
  await test('unsupported platform never looks up or initiates; explicit desktop choice works', async () => {
    const events = [];
    const session = createDownloadFlow({ lookup: async () => { events.push('lookup'); return info; },
      initiate: () => events.push('request'), change: state => events.push(state.status) });
    await session.start(null);
    assert.deepEqual(events, ['unsupported']);
    await session.start(windows);
    assert.deepEqual(events, ['unsupported', 'pending', 'lookup', 'request', 'starting']);
  });
  for (const value of [{}, { ...info, downloads: [] }, { ...info, downloads: [{ ...info.downloads[2], href: 'javascript:alert(1)' }] }]) {
    await test('flow invalid/unavailable/unsafe metadata is an error with no initiation', async () => {
      const events = [];
      const session = createDownloadFlow({ lookup: async () => value, initiate: () => assert.fail('Unsafe initiation'), change: state => events.push(state.status) });
      await session.start(windows);
      assert.deepEqual(events, ['pending', 'error']);
    });
  }
  await test('failed navigation initiation never publishes starting', async () => {
    const events = [];
    const session = createDownloadFlow({ lookup: async () => info, initiate: () => { throw new Error('Navigation refused'); }, change: state => events.push(state.status) });
    await session.start(windows);
    assert.deepEqual(events, ['pending', 'error']);
  });
}

try {
  const original = await fixture('original', 'c926a144a');
  await serverContracts('original c926a144a', original);
  const current = await fixture('current');
  await serverContracts('current', current);
  await clientContracts(current);
  console.log(`Download fixtures passed: ${assertions} cases; actual original/current handlers and current client helpers; no live requests.`);
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  globalThis.fetch = realFetch;
  AbortSignal.timeout = realTimeout;
  await rm(temporary, { recursive: true, force: true });
}
