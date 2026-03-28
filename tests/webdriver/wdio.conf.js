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
  port: 4445,
  specs: ['./specs/**/*.js'],
  maxInstances: 1,

  capabilities: [{
    browserName: 'chrome',  // Required by WebdriverIO but ignored by tauri-plugin-webdriver
  }],

  // Connect directly to the tauri-plugin-webdriver server (no tauri-driver needed)
  hostname: '127.0.0.1',
  path: '/',

  reporters: ['spec'],
  framework: 'mocha',

  mochaOpts: {
    timeout: 120000,  // 2 min per test (app startup can be slow)
  },

  // No beforeSession needed — tauri-plugin-webdriver runs embedded in the app.
  // Just ensure the app is running with --features webdriver before tests.
};
