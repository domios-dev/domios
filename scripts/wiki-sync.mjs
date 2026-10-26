#!/usr/bin/env node
// Renders the stable essences into GitHub Wiki pages; essences stay the only source.
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { sourceRepository } from './source-repository.mjs';

const ESSENCE_ORDER = ['vision', 'capabilities', 'core', 'orchestration', 'memory', 'permissions',
  'supporter-agent', 'sentinels', 'interface', 'runtime-event-path', 'begin_here'];
const HEADING_ID = /^(#{1,6} .*?)\s*\{#([^}\s]+)\}\s*$/;
const ESSENCE_LINK = /\]\(([a-z0-9_-]+)\.md(#[^)\s]*)?\)/g;
const FENCE = /^\s*(```|~~~)/;

export function pageName(title) {
  return title.replace(/[^A-Za-z0-9 -]/g, '').trim().replace(/\s+/g, '-');
}

function essenceTitle(slug, source) {
  const heading = source.split(/\r?\n/).find(line => line.startsWith('# '));
  if (!heading) throw new Error(`essences/${slug}.md has no top-level heading`);
  return heading.slice(2).trim();
}

function orderedSlugs(slugs) {
  const known = ESSENCE_ORDER.filter(slug => slugs.includes(slug));
  return [...known, ...slugs.filter(slug => !known.includes(slug)).sort()];
}

export function renderPage({ slug, source, pages, repository }) {
  const lines = source.split(/\r?\n/);
  const out = [];
  let fenced = false;
  let droppedTitle = false;
  for (const line of lines) {
    if (FENCE.test(line)) fenced = !fenced;
    if (fenced || FENCE.test(line)) {
      out.push(line);
      continue;
    }
    if (!droppedTitle && line.startsWith('# ')) {
      droppedTitle = true;
      continue;
    }
    const heading = HEADING_ID.exec(line);
    const text = heading ? heading[1] : line;
    if (heading) out.push(`<a name="${heading[2]}"></a>`, '');
    out.push(text.replace(ESSENCE_LINK, (match, target, fragment = '') =>
      pages[target] ? `](${pages[target]}${fragment})` : match));
  }
  const sourceUrl = `https://github.com/${repository}/blob/master/essences/${slug}.md`;
  const banner = `> Generated from [essences/${slug}.md](${sourceUrl}). Edit the source; changes made here are overwritten.`;
  return `${banner}\n\n${out.join('\n').replace(/^\n+/, '').trimEnd()}\n`;
}

export function renderWiki(essences, repository) {
  const slugs = orderedSlugs(Object.keys(essences));
  const titles = Object.fromEntries(slugs.map(slug => [slug, essenceTitle(slug, essences[slug])]));
  const pages = Object.fromEntries(slugs.map(slug => [slug, pageName(titles[slug])]));
  const contents = slugs.map(slug => `- [${titles[slug]}](${pages[slug]})`).join('\n');
  const files = {
    'Home.md': `Domios is an open-source OS for AI agents. These pages are its essences: the stable product contracts that say what Domios does and why, generated from [essences/](https://github.com/${repository}/tree/master/essences).\n\nInstall and build instructions are in the [README](https://github.com/${repository}#readme). Inside Domios, run \`domios docs\` for the help that ships with your version.\n\n${contents}\n`,
    '_Sidebar.md': `**[Home](Home)**\n\n${contents}\n`,
  };
  for (const slug of slugs) {
    files[`${pages[slug]}.md`] = renderPage({ slug, source: essences[slug], pages, repository });
  }
  return files;
}

function readEssences(directory) {
  return Object.fromEntries(readdirSync(directory).filter(file => file.endsWith('.md'))
    .map(file => [file.slice(0, -3), readFileSync(path.join(directory, file), 'utf8')]));
}

function writeWiki(outDir, files) {
  mkdirSync(outDir, { recursive: true });
  for (const file of readdirSync(outDir).filter(file => file.endsWith('.md'))) rmSync(path.join(outDir, file));
  for (const [file, content] of Object.entries(files)) writeFileSync(path.join(outDir, file), content);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const outDir = process.argv[2];
  if (!outDir) {
    console.error('usage: node scripts/wiki-sync.mjs <wiki-checkout-dir>');
    process.exit(2);
  }
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const files = renderWiki(readEssences(path.join(root, 'essences')), sourceRepository());
  writeWiki(path.resolve(outDir), files);
  console.log(`wrote ${Object.keys(files).length} wiki pages to ${outDir}`);
}
