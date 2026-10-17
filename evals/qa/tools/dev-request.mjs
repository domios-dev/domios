import { readFileSync, writeFileSync } from 'node:fs';

const [method, path, bodyPath, out] = process.argv.slice(2);
const base = new URL(process.env.CODETERM_API_URL || '');
if (base.origin !== 'http://127.0.0.1:7686' || !process.env.CODETERM_API_TOKEN) {
  throw Error('DEV wrapper required');
}
const read = /^\/api\/(health|mux\/state|crons(?:\/[A-Za-z0-9_-]+(?:\/history)?)?|tabs(?:\/[A-Za-z0-9_-]+\/(?:chat|status))?)$/;
if (!(method === 'GET' && read.test(path)) && !(method === 'POST' && path === '/api/crons')) {
  throw Error('Route outside curated evidence scope');
}
if (!out || !bodyPath) throw Error('Usage: METHOD PATH BODY_FILE_OR_DASH OUTPUT_FILE');
const headers = { Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}` };
if (process.env.CODETERM_TAB_ID) headers['X-CodeTerm-Tab-Id'] = process.env.CODETERM_TAB_ID;
let body;
if (method === 'POST') {
  body = JSON.stringify(JSON.parse(readFileSync(bodyPath, 'utf8').replace(/^\uFEFF/, '')));
  headers['Content-Type'] = 'application/json';
}
const response = await fetch(new URL(path, base), {
  method, headers, body, signal: AbortSignal.timeout(30000),
});
const text = await response.text();
let data;
try { data = JSON.parse(text); } catch { data = { non_json: true }; }
function redact(value) {
  if (Array.isArray(value)) return value.map(redact);
  if (!value || typeof value !== 'object') return value;
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [
    key, /token|credential|password|secret|email|uid|verifier|challenge/i.test(key) ? '[REDACTED]' : redact(item),
  ]));
}
const evidence = { method, path, http_status: response.status, data: redact(data) };
writeFileSync(out, JSON.stringify(evidence, null, 2), { flag: 'wx' });
console.log(JSON.stringify({ http_status: response.status, out }));
if (!response.ok) process.exitCode = 1;
