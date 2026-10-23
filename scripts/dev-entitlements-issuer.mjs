#!/usr/bin/env node
// DEV-only QA issuer: serves getEntitlements on loopback with verdicts signed by the TEST key.
// Release builds pin the PRODUCTION key and endpoint, so nothing it signs is accepted there.
import { createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const KEY_ID = 'test-2026-10';
const SPKI = 'MCowBQYDK2VwAyEA2nQy+tOBmjHHrGnun9ZlMCKQ9fdd7HlkpF5NrykMl7A=';
const EPOCH = 'test-v1';
const SECRET = 'entitlements/test-signing-key';
const LEASE_SECS = 86_400;
const REFRESH_SECS = 3_600;
const MODES = ['free', 'pro', 'sponsor', 'expired', 'offline'];
const MODE_FILE = join(tmpdir(), 'domios-dev-issuer-mode');

const usage = `usage:
  node scripts/dev-entitlements-issuer.mjs serve [--port 15200]
  node scripts/dev-entitlements-issuer.mjs mode <${MODES.join('|')}>
  node scripts/dev-entitlements-issuer.mjs mode`;

const currentMode = () => {
  try {
    const mode = readFileSync(MODE_FILE, 'utf8').trim();
    return MODES.includes(mode) ? mode : 'free';
  } catch {
    return 'free';
  }
};

const canonical = (value) => {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
};

const secret = (name) =>
  execFileSync('domios', ['mem', 'secret', 'get', '--name', name], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'inherit'],
  }).trim();

const loadKey = () => {
  const der = secret(SECRET);
  const key = createPrivateKey({ key: Buffer.from(der, 'base64'), format: 'der', type: 'pkcs8' });
  const spki = createPublicKey(key).export({ format: 'der', type: 'spki' }).toString('base64');
  if (spki !== SPKI) throw new Error(`${SECRET} does not match the pinned TEST SPKI`);
  return key;
};

const tokenUid = (header) => {
  const token = /^Bearer (.+)$/.exec(header ?? '')?.[1];
  const claims = token && JSON.parse(Buffer.from(token.split('.')[1] ?? '', 'base64url').toString() || 'null');
  const uid = claims?.user_id ?? claims?.sub;
  if (typeof uid !== 'string' || !uid) throw new Error('missing ID token subject');
  return uid;
};

const PLANS = {
  free: { label: 'free', authority: 'free', limit: 2, mem: 1000, sponsor: false },
  pro: { label: 'pro', authority: 'billing', limit: null, mem: null, sponsor: false },
  sponsor: { label: 'sponsor', authority: 'billing', limit: null, mem: null, sponsor: true },
};

const verdict = (key, project, mode, uid, inst) => {
  const now = Math.floor(Date.now() / 1000);
  const plan = PLANS[mode === 'expired' ? 'pro' : mode];
  const issuedAt = mode === 'expired' ? now - LEASE_SECS - 60 : now;
  const periodEnd = plan.authority === 'free' ? null : now + 30 * 86_400;
  const payload = {
    aud: { env: 'test', project },
    authority: plan.authority,
    capabilities: [],
    enforcement: { enabled: true, rollback: false },
    expires_at: issuedAt + LEASE_SECS,
    grace_until: issuedAt + LEASE_SECS,
    inst,
    iss: 'domios-entitlements',
    issued_at: issuedAt,
    label: plan.label,
    limits: {
      'mem.items': { max: plan.mem, scope: 'installation' },
      'mesh.machines': { max: plan.limit, scope: 'account' },
      workspaces: { max: plan.limit, scope: 'installation' },
    },
    not_before: issuedAt,
    period_end: periodEnd,
    perks: { priority_support: plan.sponsor, sponsor: plan.sponsor },
    policy_epoch: EPOCH,
    policy_version: 1,
    refresh_after: issuedAt + REFRESH_SECS,
    revision: Date.now(),
    schema_version: 1,
    sub: uid,
  };
  const bytes = Buffer.from(canonical(payload));
  const caps = {
    max_mesh_machines: plan.limit,
    max_workspaces: plan.limit,
    sponsor: plan.sponsor,
    priority_support: plan.sponsor,
  };
  return {
    schema_version: 2,
    billing: { schema_version: 1, plan: plan.label, capabilities: caps, period_end: periodEnd },
    billing_inventory: 'verified',
    application_access: { schema_version: 1, authority: plan.authority, full_access: false, ...caps },
    grant: {
      payload: bytes.toString('base64url'),
      signature: sign(null, bytes, key).toString('base64url'),
      key_id: KEY_ID,
      alg: 'Ed25519',
    },
  };
};

const serve = (port) => {
  const key = loadKey();
  const project = secret('DOMIOS_QA_PROJECT_ID');
  if (!project) throw new Error('DOMIOS_QA_PROJECT_ID is not in the secret store');
  createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => (body += chunk));
    req.on('end', () => {
      const reply = (status, value) => {
        res.writeHead(status, { 'content-type': 'application/json' });
        res.end(JSON.stringify(value));
      };
      const mode = currentMode();
      if (req.method !== 'POST' || !req.url?.endsWith('/getEntitlements')) {
        return reply(404, { error: { status: 'NOT_FOUND', message: 'DEV issuer serves getEntitlements only' } });
      }
      if (mode === 'offline') return reply(503, { error: { status: 'UNAVAILABLE', message: 'offline mode' } });
      try {
        const uid = tokenUid(req.headers.authorization);
        const inst = JSON.parse(body).data?.installation_id;
        if (typeof inst !== 'string' || !inst) throw new Error('missing installation_id');
        console.log(`${new Date().toISOString()} ${mode} uid=${uid} inst=${inst}`);
        reply(200, { result: verdict(key, project, mode, uid, inst) });
      } catch (error) {
        reply(400, { error: { status: 'INVALID_ARGUMENT', message: String(error.message ?? error) } });
      }
    });
  }).listen(port, '127.0.0.1', () => {
    console.log(`DEV issuer on http://127.0.0.1:${port}/ (mode ${currentMode()}, key ${KEY_ID})`);
  });
};

const [command, ...rest] = process.argv.slice(2);
if (command === 'serve') {
  const flag = rest.indexOf('--port');
  serve(flag >= 0 ? Number(rest[flag + 1]) : 15200);
} else if (command === 'mode' && rest.length === 0) {
  console.log(currentMode());
} else if (command === 'mode' && MODES.includes(rest[0])) {
  writeFileSync(MODE_FILE, rest[0]);
  console.log(`mode ${rest[0]}`);
} else {
  console.error(usage);
  process.exit(2);
}
