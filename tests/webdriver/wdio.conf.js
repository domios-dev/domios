/**
 * WebdriverIO config for CodeTerm UI tests.
 * Connects to running app with --features webdriver (port 4445).
 *
 * Run: npm test (or: npx wdio run wdio.conf.js)
 * Start app: ./build.sh webdriver
 */

import { resolve } from 'path';
import { readFileSync, existsSync } from 'fs';
import http from 'http';

const projectRoot = resolve(import.meta.dirname, '..', '..');

// --- API helper for cleanup ---
function readToken() {
  const p = resolve(process.env.HOME, '.codeterm-dev', 'config.toml');
  if (existsSync(p)) {
    const m = readFileSync(p, 'utf8').match(/token\s*=\s*"([^"]+)"/);
    return m?.[1] || '';
  }
  return '';
}

function apiCall(method, path, body = null) {
  const token = readToken();
  return new Promise((resolve, reject) => {
    const req = http.request({
      hostname: '127.0.0.1', port: 7686,
      path: `/api${path}`, method,
      headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
    }, res => {
      let d = '';
      res.on('data', c => d += c);
      res.on('end', () => { try { resolve(JSON.parse(d)); } catch { resolve(d); } });
    });
    req.on('error', reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

export const config = {
  runner: 'local',
  port: 4445,
  specs: ['./specs/**/*.js'],
  exclude: [],
  maxInstances: 1,

  capabilities: [{
    browserName: 'chrome',
  }],

  hostname: '127.0.0.1',
  path: '/',

  reporters: ['spec'],
  framework: 'mocha',

  mochaOpts: {
    timeout: 120000,
  },

  // Clean state before each spec file — close spawned panes, keep only Terminal
  before: async function () {
    try {
      const panes = await apiCall('GET', '/panes');
      if (!Array.isArray(panes) || panes.length === 0) {
        console.log('[setup] No panes');
        return;
      }

      // Keep first non-agent pane (the default Terminal tab)
      const keep = panes.find(p => p.pty_id && p.title === 'Terminal' && !p.agent_type)
                || panes.find(p => p.pty_id && !p.agent_type)
                || panes[0];

      let closed = 0;
      for (const p of panes) {
        if (p.id === keep.id) continue;
        // Clear prompts before closing
        if (p.pending_prompt) {
          try { await apiCall('POST', `/panes/${p.id}/prompt/clear`); } catch {}
        }
        // Close extra panes (spawned Claude instances from previous runs)
        try { await apiCall('POST', `/panes/${p.id}/close`); closed++; } catch {}
      }

      // Reset agent_type on the kept pane
      if (keep.agent_type) {
        try { await apiCall('POST', `/panes/${keep.id}/agent-type`, { agent_type: null }); } catch {}
      }
      if (keep.pending_prompt) {
        try { await apiCall('POST', `/panes/${keep.id}/prompt/clear`); } catch {}
      }

      await new Promise(r => setTimeout(r, 1000));
      console.log(`[setup] Closed ${closed} pane(s), kept ${keep.id} (${keep.title})`);
    } catch (e) {
      console.log(`[setup] Cleanup error: ${e.message}`);
    }
  },
};
