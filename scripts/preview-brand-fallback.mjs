#!/usr/bin/env node
// Renders the static brand fallback pages from their Rust template for instant visual review (no client rebuild).
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dir = path.join(root, 'src-tauri/crates/domios-service-core/src/startup');
const template = readFileSync(path.join(dir, 'brand_fallback.html'), 'utf8');
const source = readFileSync(path.join(dir, 'brand_fallback.rs'), 'utf8');

function constant(name) {
  const match = source.match(new RegExp(`const ${name}: &str = r#"([\\s\\S]*?)"#;`));
  if (!match) throw new Error(`missing ${name} in brand_fallback.rs`);
  return match[1];
}

function arm(kind) {
  const start = source.indexOf(`BrandFallback::${kind} => (`);
  const body = source.slice(start, source.indexOf('),', start));
  return [...body.matchAll(/"((?:[^"\\]|\\.)*)"|r#"([\s\S]*?)"#|([A-Z_]{4,})/g)]
    .map(match => match[3] ? constant(match[3]) : (match[1] ?? match[2]).replace(/\\"/g, '"'));
}

const out = path.join(root, 'target', 'brand-fallback-preview');
mkdirSync(out, { recursive: true });
for (const kind of ['Reconnecting', 'InterfaceMissing']) {
  const [title, heading, body, role, pulse, chrome, canvas, action, script] = arm(kind);
  const page = template
    .replaceAll('{{TITLE}}', title).replaceAll('{{HEADING}}', heading).replaceAll('{{BODY}}', body)
    .replaceAll('{{ROLE}}', role).replaceAll('{{PULSE}}', pulse).replaceAll('{{CHROME}}', chrome)
    .replaceAll('{{CANVAS}}', canvas).replaceAll('{{ACTION}}', action).replaceAll('{{SCRIPT}}', script);
  const file = path.join(out, `${kind}.html`);
  writeFileSync(file, page);
  console.log(file);
}
