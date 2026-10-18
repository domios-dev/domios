import http from 'node:http';
import { writeFileSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

const args = process.argv.slice(2);
function option(name) {
  const index = args.indexOf(name);
  return index < 0 ? undefined : args[index + 1];
}
const out = option('--out');
const port = Number(option('--port') || 0);
if (!out || !Number.isInteger(port) || port < 0 || port > 65535 || port === 7685 || port === 7686) {
  throw Error('Usage: --out DIR [--port UNUSED_FIXTURE_PORT]');
}
mkdirSync(out, { recursive: true });
const pending = new Set();
const observers = new Set();
const records = [];
const html = body => '<!doctype html><html lang="en"><meta charset="utf-8"><title>Joy Browser fixture</title><body>' + body + '</body></html>';
const main = html(`
<h1>Joy Browser QA</h1>
<form action="/submitted">
  <label>Required name<input name="name" required></label>
  <label>Disabled field<input disabled value="fixed"></label>
  <label>Readonly field<input readonly value="constant"></label>
  <label>Choice<select name="choice"><option>Alpha</option><option>Beta</option></select></label>
  <button>Submit fixture</button>
</form>
<p id="selection">Joy selection alpha beta gamma.</p>
<a href="/held">Held navigation</a>
<a href="/popup" target="_blank">Open own popup</a>
<button id="spa" onclick="history.pushState({},'', '/spa');document.querySelector('h1').textContent='SPA destination';document.querySelector('#spa').remove()">SPA navigate</button>
<iframe title="QA frame" src="/frame"></iframe>`);
const server = http.createServer((req, res) => {
  const path = new URL(req.url, 'http://127.0.0.1').pathname;
  records.push({ method: req.method, path });
  if (records.length > 512) records.shift();
  if (path === '/held') {
    pending.add(res);
    res.on('close', () => pending.delete(res));
    for (const observer of observers) observer.end('held admitted');
    observers.clear();
    return;
  }
  if (path === '/gate') {
    if (pending.size) res.end('held admitted');
    else {
      observers.add(res);
      res.on('close', () => observers.delete(res));
    }
    return;
  }
  if (path === '/release') {
    for (const response of pending) {
      response.setHeader('Content-Type', 'text/html');
      response.end(html('<h1>Held destination complete</h1>'));
    }
    pending.clear();
    res.end('released');
    return;
  }
  if (path === '/records') {
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify(records));
    return;
  }
  res.setHeader('Content-Type', 'text/html');
  if (path === '/frame') {
    res.end(html('<label>Frame name<input></label><button onclick="document.body.append(\'Frame acknowledged\')">Frame action</button>'));
  } else if (path === '/popup') res.end(html('<h1>Owned popup ready</h1>'));
  else if (path === '/submitted') res.end(html('<h1>Fixture submitted</h1>'));
  else res.end(main);
});
server.on('error', error => { console.error(error.code); process.exitCode = 1; });
server.listen(port, '127.0.0.1', () => {
  const address = server.address();
  const ready = { origin: `http://127.0.0.1:${address.port}`, pid: process.pid };
  writeFileSync(join(resolve(out), 'ready.json'), JSON.stringify(ready), { flag: 'wx' });
  console.log(JSON.stringify(ready));
});
