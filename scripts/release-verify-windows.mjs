#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
const [directory, tag, platform] = process.argv.slice(2);
const installers = readdirSync(directory).filter(name => name.endsWith('-setup.exe'));
if (installers.length !== 1) throw new Error(`Expected one installer, found ${installers.length}`);
const bundle = installers[0];
const manifest = path.join(directory, 'signature-check.json');
writeFileSync(manifest, JSON.stringify({ version: tag.replace(/^v/, ''), platforms: { [platform]: {
  url: `https://github.com/rollacode/codeterm-releases/releases/download/${tag}/${bundle}`,
  signature: readFileSync(path.join(directory, `${bundle}.sig`), 'utf8').trim(),
} } }));
execFileSync(process.execPath, ['scripts/verify-updater-manifest.mjs', '--manifest', manifest, '--local', directory, '--require-version', tag.replace(/^v/, ''), '--require-platform', platform], { stdio: 'inherit' });
