import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm, utimes, chmod, symlink } from 'node:fs/promises';
import { createHash, createPublicKey, verify } from 'node:crypto';
import { gunzipSync } from 'node:zlib';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalManifest, createManifest, checksum, validateTag } from './manifest.mjs';
import { deterministicArchive } from './archive.mjs';
import { fixtureSign, verifySignature, productionSign } from './sign.mjs';
import { publishRelease } from './publish.mjs';
import { prepare, isDryRun } from './release.mjs';
const fixture = fileURLToPath(new URL('./fixtures/', import.meta.url));
const contract = JSON.parse(await readFile(new URL('../../frontend-release-contract.json', import.meta.url), 'utf8'));
const publicKey = await readFile(path.join(fixture, 'frontend-test.pub'), 'utf8');
const privateKey = await readFile(path.join(fixture, 'frontend-test.pem'), 'utf8');
const archive = await deterministicArchive(path.join(fixture, 'dist'));
const manifest = createManifest('frontend-v1.0.0', contract, archive);
const bytes = canonicalManifest(manifest);
const signature = fixtureSign(bytes, privateKey, publicKey);

test('frontend version is independent; malformed and mismatched tags fail', () => {
  assert.equal(validateTag('frontend-v1.0.0', contract), '1.0.0');
  for (const tag of ['v1.0.0','frontend-v01.0.0','frontend-v1.0.1','frontend-v1.0.0-rc.1','frontend-v1.0.0\n','frontend-vX.Y.Z']) assert.throws(() => validateTag(tag, contract));
});
test('canonical bytes sorted, compact, UTF-8 with exact six fields', () => {
  assert.deepEqual(canonicalManifest(Object.fromEntries(Object.entries(manifest).reverse())), bytes);
  assert.equal(bytes.toString(), JSON.stringify({ maxApiVersion: 2, minApiVersion: 2, releaseId: manifest.releaseId, sha256: checksum(archive), url: manifest.url, version: '1.0.0' }));
  assert.throws(() => canonicalManifest({ ...manifest, signature }));
});
test('API ranges, digest, identity and URL are validated', () => {
  for (const value of [{ minApiVersion: 0 },{ maxApiVersion: 1 },{ maxApiVersion: 2.5 },{ sha256: 'bad' },{ releaseId: '../x' },{ url: 'http://example.com/a' }]) assert.throws(() => canonicalManifest({ ...manifest, ...value }));
});
test('archive bytes reproduce despite order, permissions and timestamps', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'frontend-archive-test-'));
  try {
    await mkdir(path.join(root,'assets'));
    await writeFile(path.join(root,'assets/fixture.js'), await readFile(path.join(fixture,'dist/assets/fixture.js')));
    await writeFile(path.join(root,'index.html'), await readFile(path.join(fixture,'dist/index.html')));
    await utimes(path.join(root,'index.html'), 100, 9999);
    await chmod(path.join(root,'index.html'), 0o755);
    assert.deepEqual(await deterministicArchive(root), archive);
    assert.equal(checksum(archive), createHash('sha256').update(archive).digest('hex'));
    const tar = gunzipSync(archive);
    assert.equal(tar.subarray(0,100).toString().replace(/\0.*$/s,''), 'assets/fixture.js');
    assert.equal(parseInt(tar.subarray(124,136).toString(), 8), 57);
    assert.ok(tar.includes(Buffer.from('index.html')));
  } finally { await rm(root,{recursive:true,force:true}); }
});
test('archive refuses links and incomplete Vite output', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'frontend-links-test-'));
  try {
    await assert.rejects(deterministicArchive(root), /Missing Vite/);
    await symlink(path.join(fixture,'dist/index.html'), path.join(root,'link'));
    await assert.rejects(deterministicArchive(root), /symlinks/);
  } finally { await rm(root,{recursive:true,force:true}); }
});
test('prehashed minisign packet independently verifies using Ed25519 primitives', () => {
  const packet = Buffer.from(signature.split('\n')[1],'base64');
  const key = Buffer.from(publicKey.trim().split('\n').at(-1),'base64');
  const native = createPublicKey({key:Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),key.subarray(10)]),format:'der',type:'spki'});
  assert.equal(packet.subarray(0,2).toString(),'ED');
  assert.ok(verify(null,createHash('blake2b512').update(bytes).digest(),native,packet.subarray(10)));
  assert.ok(verifySignature(bytes,signature,publicKey));
});
test('signature binds every metadata field and archive digest', () => {
  for (const value of [{version:'1.0.1',releaseId:'frontend-v1.0.1'},{ minApiVersion: 1 },{ maxApiVersion: 3 },{ sha256: '0'.repeat(64) },{ url: 'https://example.com/other' }]) assert.throws(() => verifySignature(canonicalManifest({...manifest,...value}),signature,publicKey));
  assert.throws(() => verifySignature(bytes, signature.replace('frontend fixture contract 1','changed'),publicKey));
});
test('malformed signature and foreign identity fail closed', () => {
  const key = Buffer.from(publicKey.trim().split('\n').at(-1),'base64'); key[2] ^= 1;
  assert.throws(() => verifySignature(bytes,signature,key.toString('base64')));
  for (const value of ['',signature+'extra\n',signature.replace('\ntrusted comment: frontend','\ntrusted comment: bad')]) assert.throws(() => verifySignature(bytes,value,publicKey));
});
test('production requires independently provisioned key', async () => {
  await assert.rejects(productionSign(bytes,null,null),/not provisioned/);
});
function remote(fail) {
  const calls=[]; const assets=new Map();
  return {calls,createDraft:async tag=>{calls.push('draft');},upload:async(tag,name,data)=>{calls.push('upload:'+name); if(fail===name)throw Error('partial'); assets.set(name,data);},download:async(tag,name)=>{calls.push('verify:'+name);return fail==='corrupt'?Buffer.from('corrupt'):assets.get(name);},publish:async()=>{calls.push('publish');},discover:async()=>{calls.push('discover');}};
}
const input={manifest,signature,archive,publicKey,dryRun:false,fixtureKey:'different-test-key'};
test('all versioned assets complete and read back before publication and discovery',async()=>{
  const service=remote(); await publishRelease(input,service);
  assert.deepEqual(service.calls,['draft',`upload:${manifest.releaseId}.tar.gz`,'upload:frontend-manifest.json','upload:frontend-manifest.json.minisig',`verify:${manifest.releaseId}.tar.gz`,'verify:frontend-manifest.json','verify:frontend-manifest.json.minisig','publish','discover']);
});
test('partial upload or corrupt readback cannot publish or mutate discovery',async()=>{
  for(const fail of ['frontend-manifest.json','frontend-manifest.json.minisig','corrupt']) {
    const service=remote(fail); await assert.rejects(publishRelease(input,service));
    assert.ok(!service.calls.includes('publish')); assert.ok(!service.calls.includes('discover'));
  }
});
test('signature or digest refusal precedes any remote side effect',async()=>{
  for(const override of [{signature:''},{archive:Buffer.from('bad')}]){
    const service=remote();await assert.rejects(publishRelease({...input,...override},service));assert.deepEqual(service.calls,[]);
  }
});
test('dry run validates artifacts with zero remote calls; fixture production is refused',async()=>{
  const service=remote();assert.deepEqual(await publishRelease({...input,dryRun:true},service),{published:false});
  await assert.rejects(publishRelease({...input,publicKey:publicKey.trim().split('\n').at(-1),fixtureKey:publicKey},service),/Fixture/);assert.deepEqual(service.calls,[]);
});
test('dry run artifact has complete dist, archive, manifest, signature and reproducible provenance',async()=>{
  const root=await mkdtemp(path.join(os.tmpdir(),'frontend-prepare-test-'));
  try{
    await prepare({contract,tag:'frontend-v1.0.0',dist:path.join(fixture,'dist'),output:root,dryRun:true,sourceSha:'test-source'});
    assert.deepEqual(await readFile(path.join(root,'frontend-v1.0.0.tar.gz')),archive);
    const canonical=await readFile(path.join(root,'frontend-manifest.json'));
    verifySignature(canonical,await readFile(path.join(root,'frontend-manifest.json.minisig'),'utf8'),publicKey);
    assert.ok((await readFile(path.join(root,'dist/index.html'))).length);
    assert.ok((await readFile(path.join(root,'dist/assets/fixture.js'))).length);
    assert.equal(JSON.parse(await readFile(path.join(root,'provenance.json'))).contractVersion,1);
  }finally{await rm(root,{recursive:true,force:true});}
});
test('committed cross-language fixture is canonical, signed and matches archive',async()=>{
  const stored=await readFile(path.join(fixture,'frontend-manifest.json'));
  assert.deepEqual(stored,bytes);
  verifySignature(stored,await readFile(path.join(fixture,'frontend-manifest.json.minisig'),'utf8'),publicKey);
  assert.deepEqual(await readFile(path.join(fixture,'frontend-v1.0.0.tar.gz')),archive);
});

test('pull requests always force fixture dry run even when requested flag is false or missing', () => {
  assert.equal(isDryRun('pull_request', 'false'), true);
  assert.equal(isDryRun('pull_request', undefined), true);
  assert.equal(isDryRun('workflow_dispatch', 'true'), true);
  assert.equal(isDryRun('workflow_dispatch', 'false'), false);
  assert.equal(isDryRun('push', undefined), false);
});
