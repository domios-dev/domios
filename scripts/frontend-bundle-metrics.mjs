import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { init, parse } from 'es-module-lexer';

export function staticChunkClosure(entries, graph) {
  const visited = new Set();
  const pending = [...entries];
  while (pending.length) {
    const file = pending.pop();
    if (visited.has(file)) continue;
    const chunk = graph.get(file);
    if (!chunk) throw new Error(`Missing chunk: ${file}`);
    visited.add(file);
    pending.push(...chunk.imports);
  }
  return [...visited].sort();
}

export async function bundleMetrics(directory) {
  await init;
  const root = path.resolve(directory);
  const graph = new Map();
  const visit = (relative) => {
    if (graph.has(relative)) return;
    const absolute = path.resolve(root, relative);
    if (!absolute.startsWith(root + path.sep)) throw new Error(`Chunk outside bundle: ${relative}`);
    const source = fs.readFileSync(absolute, 'utf8');
    const [imports] = parse(source);
    const dependencies = imports.filter(item => item.d === -1 && item.n?.startsWith('.'))
      .map(item => path.posix.normalize(path.posix.join(path.posix.dirname(relative), item.n)));
    graph.set(relative, { bytes: Buffer.byteLength(source), imports: dependencies });
    dependencies.forEach(visit);
  };
  const html = fs.readFileSync(path.join(root, 'index.html'), 'utf8');
  const entries = [...html.matchAll(/<script\b[^>]*\bsrc="([^\"]+\.js)"/g)].map(match => match[1].replace(/^\.\//, ''));
  if (!entries.length) throw new Error('No JavaScript entry found');
  const assets = fs.readdirSync(path.join(root, 'assets')).filter(file => file.endsWith('.js')).map(file => `assets/${file}`);
  assets.forEach(visit);
  const initialChunks = staticChunkClosure(entries, graph);
  const bytes = files => files.reduce((sum, file) => sum + graph.get(file).bytes, 0);
  return { entries, entryBytes: bytes(entries), initialBytes: bytes(initialChunks), initialChunks,
    javascriptBytes: bytes([...graph.keys()]), largestChunkBytes: Math.max(...[...graph.values()].map(chunk => chunk.bytes)) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log(JSON.stringify(await bundleMetrics(process.argv[2] || 'dist'), null, 2));
}
