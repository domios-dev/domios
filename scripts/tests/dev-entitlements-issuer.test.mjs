import assert from 'node:assert/strict';
import { generateKeyPairSync, sign, verify } from 'node:crypto';
import { once } from 'node:events';
import test from 'node:test';
import { checkRefusedModes, createIssuer, tokenUid, verdict, verifiesWith } from '../dev-entitlements-issuer.mjs';

test('local DEV bearer requires the local uid and matching installation', () => {
  const bearer = 'Bearer domios-local-dev.uid=local-dev-user&installation_id=fixture-installation';
  assert.equal(tokenUid(bearer, 'fixture-installation'), 'local-dev-user');
  assert.throws(() => tokenUid(bearer, 'foreign-installation'));
  assert.throws(() => tokenUid(bearer));
  assert.throws(() => tokenUid(bearer.replace('local-dev-user', 'foreign-user'), 'fixture-installation'));
});

test('loopback issuer signs a verifiable Sponsor verdict for local-dev-user', async () => {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const server = createIssuer(privateKey, 'fixture-qa-project', () => 'sponsor');
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/getEntitlements`, {
      method: 'POST',
      headers: { authorization: 'Bearer domios-local-dev.uid=local-dev-user&installation_id=fixture-installation' },
      body: JSON.stringify({ data: { installation_id: 'fixture-installation' } }),
    });
    assert.equal(response.status, 200);
    const { result } = await response.json();
    const payload = Buffer.from(result.grant.payload, 'base64url');
    assert(verify(null, payload, publicKey, Buffer.from(result.grant.signature, 'base64url')));
    const claims = JSON.parse(payload);
    assert.equal(claims.sub, 'local-dev-user');
    assert.equal(claims.inst, 'fixture-installation');
    assert.deepEqual(claims.aud, { env: 'test', project: 'fixture-qa-project' });
    assert.equal(claims.perks.sponsor, true);
    assert.equal(result.grant.key_id, 'test-2026-10');
  } finally {
    server.closeAllConnections();
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
});

test('tampered and unsigned serve the paid payload but fail verification while pro passes', () => {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const issue = (mode) => verdict(privateKey, 'fixture-qa-project', mode, 'local-dev-user', 'fixture-installation');
  const pro = issue('pro').grant;
  assert(verifiesWith(pro, publicKey));
  for (const mode of ['tampered', 'unsigned']) {
    const { grant, billing } = issue(mode);
    assert.equal(billing.plan, 'pro');
    assert.equal(JSON.parse(Buffer.from(grant.payload, 'base64url')).authority, 'billing');
    assert.equal(verifiesWith(grant, publicKey), false, mode);
  }
  const tampered = issue('tampered').grant;
  const genuine = sign(null, Buffer.from(tampered.payload, 'base64url'), privateKey);
  const served = Buffer.from(tampered.signature, 'base64url');
  assert.equal(served.length, genuine.length);
  assert.equal(genuine.filter((byte, index) => byte !== served[index]).length, 1);
  assert.equal(issue('unsigned').grant.signature, '');
  assert.deepEqual(checkRefusedModes(privateKey, publicKey, 'fixture-qa-project', 'local-dev-user', 'fixture-installation'), ['pro', 'tampered', 'unsigned']);
  assert.throws(() => checkRefusedModes(privateKey, generateKeyPairSync('ed25519').publicKey, 'p', 'u', 'i'), /pro verdict failed/);
});
