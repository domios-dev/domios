import { cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { deterministicArchive } from '../../scripts/frontend-release/archive.mjs';
import { checksum, fields } from '../../scripts/frontend-release/manifest.mjs';
import { fixtureSign, verifySignature } from '../../scripts/frontend-release/sign.mjs';

export async function prepare(dist, output, origin) {
  const parsed = new URL(origin);
  if (parsed.protocol !== 'http:' || !['127.0.0.1', 'localhost', '[::1]'].includes(parsed.hostname)) throw new Error('DEV fixture requires loopback HTTP');
  if (path.resolve(dist) === path.resolve(output)) throw new Error('Output must differ from dist');
  const fixture = new URL('../../scripts/frontend-release/fixtures/', import.meta.url);
  const publicKey = await readFile(new URL('frontend-test.pub', fixture), 'utf8');
  const privateKey = await readFile(new URL('frontend-test.pem', fixture), 'utf8');
  await mkdir(output, { recursive: true });
  for (const [name, version, minimum, broken] of [['good', '1.0.1', 2, false], ['incompatible', '1.0.2', 3, false], ['broken', '1.0.3', 2, true]]) {
    const root = path.join(output, name, 'dist');
    await cp(dist, root, { recursive: true, errorOnExist: true, force: false });
    const html = await readFile(path.join(root, 'index.html'), 'utf8');
    const injection = broken ? '<script>throw new Error("frontend fixture boot failure")</script>' : `<meta name="frontend-fixture-version" content="${version}">`;
    const marker = `<output aria-label="Frontend fixture version">${version}</output>`;
    const observed = html.includes('</head>') ? html.replace('</head>', `${injection}</head>`) : html.replace(/^(<!doctype[^>]*>)/i, `$1${injection}`);
    await writeFile(path.join(root, 'index.html'), observed.includes('</body>') ? observed.replace('</body>', `${marker}</body>`) : observed + marker);
    const archive = await deterministicArchive(root);
    const manifest = { maxApiVersion: minimum, minApiVersion: minimum, releaseId: `frontend-v${version}`, sha256: checksum(archive), url: `${parsed.origin}/${name}/frontend-v${version}.tar.gz`, version };
    const canonical = Buffer.from(JSON.stringify(Object.fromEntries(fields.map(key => [key, manifest[key]]))));
    const signature = fixtureSign(canonical, privateKey, publicKey);
    verifySignature(canonical, signature, publicKey);
    await writeFile(path.join(output, name, `frontend-v${version}.tar.gz`), archive);
    await writeFile(path.join(output, name, 'frontend-manifest.json'), canonical);
    await writeFile(path.join(output, name, 'frontend-manifest.json.minisig'), signature);
  }
  await writeFile(path.join(output, 'frontend-test.pub'), publicKey);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.length !== 5) throw new Error('Usage: node prepare.mjs DIST OUTPUT http://127.0.0.1:PORT');
  await prepare(...process.argv.slice(2));
}
