/**
 * WebdriverIO config for CodeTerm E2E UI tests.
 *
 * Uses tauri-driver (Windows/Linux only — macOS WKWebView not supported).
 * Requires:
 *   - cargo install tauri-driver
 *   - Built debug binary: cargo build --manifest-path src-tauri/Cargo.toml
 *
 * Run: npm test (or: npx wdio run wdio.conf.js)
 */

import { spawn } from 'child_process';
import { join, resolve } from 'path';
import { existsSync } from 'fs';

const projectRoot = resolve(import.meta.dirname, '..', '..');

// Find the debug binary — cross-platform
const binaryName = process.platform === 'win32' ? 'codeterm.exe' : 'codeterm';
const binaryPath = join(projectRoot, 'src-tauri', 'target', 'debug', binaryName);

let tauriDriver;

export const config = {
  runner: 'local',
  port: 4444,
  specs: ['./specs/**/*.js'],
  maxInstances: 1,

  capabilities: [{
    'tauri:options': {
      application: binaryPath,
    },
  }],

  reporters: ['spec'],
  framework: 'mocha',

  mochaOpts: {
    timeout: 120000,  // 2 min per test (app startup can be slow)
  },

  // Start tauri-driver before tests
  beforeSession: async () => {
    if (!existsSync(binaryPath)) {
      throw new Error(
        `Debug binary not found at ${binaryPath}. ` +
        `Run: cargo build --manifest-path src-tauri/Cargo.toml`
      );
    }

    tauriDriver = spawn('tauri-driver', [], {
      stdio: [null, process.stdout, process.stderr],
    });

    // Give tauri-driver time to start
    await new Promise(resolve => setTimeout(resolve, 3000));
  },

  afterSession: () => {
    if (tauriDriver) {
      tauriDriver.kill();
      tauriDriver = null;
    }
  },
};
