import { lstat, readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { gzipSync } from 'node:zlib';

function header(name, size) {
  const bytes = Buffer.alloc(512);
  const encoded = Buffer.from(name);
  if (encoded.length > 100) throw new Error(`Archive path exceeds USTAR name limit: ${name}`);
  encoded.copy(bytes);
  const octal = (value, offset, length) => bytes.write(value.toString(8).padStart(length - 1, '0') + '\0', offset, length, 'ascii');
  octal(0o644, 100, 8); octal(0, 108, 8); octal(0, 116, 8); octal(size, 124, 12); octal(0, 136, 12);
  bytes.fill(32, 148, 156); bytes[156] = 48;
  bytes.write('ustar\0', 257, 'ascii'); bytes.write('00', 263, 'ascii');
  const sum = bytes.reduce((total, byte) => total + byte, 0);
  bytes.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'ascii');
  return bytes;
}
export async function deterministicArchive(root) {
  const files = [];
  async function visit(relative) {
    const absolute = path.join(root, relative);
    const stat = await lstat(absolute);
    if (stat.isSymbolicLink()) throw new Error('Archive refuses symlinks');
    if (stat.isDirectory()) {
      for (const name of (await readdir(absolute)).sort()) await visit(relative ? `${relative}/${name}` : name);
    } else if (stat.isFile()) {
      if (relative.includes('\\') || relative.split('/').some(part => part === '..' || part === '.')) throw new Error('Unsafe archive path');
      files.push([relative, await readFile(absolute)]);
    } else throw new Error('Archive accepts regular files only');
  }
  await visit('');
  if (!files.some(([name]) => name === 'index.html') || !files.some(([name]) => name.startsWith('assets/'))) throw new Error('Missing Vite index/assets');
  files.sort(([a], [b]) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  const tar = Buffer.concat(files.flatMap(([name, data]) => [header(name, data.length), data, Buffer.alloc((512 - data.length % 512) % 512)]).concat(Buffer.alloc(1024)));
  const gzip = gzipSync(tar, { level: 9 });
  gzip[9] = 255;
  return gzip;
}
