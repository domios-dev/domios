import assert from 'node:assert/strict';
import { generateKeyPairSync, verify } from 'node:crypto';
import { once } from 'node:events';
import test from 'node:test';
import { createIssuer, tokenUid } from '../dev-entitlements-issuer.mjs';

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
